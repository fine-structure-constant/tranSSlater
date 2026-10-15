//! 有道翻译驱动。
//!
//! 端点：`https://aidemo.youdao.com/trans`
//!
//! 特点（2026-10 实测）：
//! - **无需 key**、**国内直连**（强制不走代理，保证翻墙配置坏掉时仍可用）
//! - 单词与整句都能翻
//! - 返回 `speakUrl` / `tSpeakUrl`，是可直接播放的 mp3，比系统 TTS 稳定

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;

use super::driver::{Driver, EngineInfo};
use super::error::TranslateError;
use super::model::{Segment, TranslateResult};

pub const ID: &str = "youdaoTrans";
const ENDPOINT: &str = "https://aidemo.youdao.com/trans";

pub const USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                              (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// 内部语言代码 → 有道语言代码。`None` 表示有道不支持该语言。
pub fn to_youdao_lang(code: &str) -> Option<&'static str> {
    Some(match code {
        "auto" => "auto",
        "en" => "en",
        "zh-CN" | "zh" => "zh-CHS",
        "zh-TW" => "zh-CHT",
        "ja" => "ja",
        "ko" => "ko",
        "fr" => "fr",
        "de" => "de",
        "ru" => "ru",
        "es" => "es",
        "pt" => "pt",
        "it" => "it",
        "ar" => "ar",
        "th" => "th",
        "vi" => "vi",
        "nl" => "nl",
        "pl" => "pl",
        "tr" => "tr",
        _ => return None,
    })
}

/// 有道语言代码 → 内部代码。
fn from_youdao_lang(code: &str) -> String {
    match code {
        "zh-CHS" | "zh" => "zh-CN".into(),
        "zh-CHT" => "zh-TW".into(),
        other => other.to_ascii_lowercase(),
    }
}

/// aidemo 不返回词典数据，只能按形状判断是不是「单词」。
///
/// 前端用它决定要不要自动朗读、要不要展开词条视图，所以宁缺勿滥：
/// 带空格（短语以上）或带句读标点的一律当句子。
fn looks_like_word(text: &str) -> bool {
    const PUNCT: [char; 13] = [
        '。', '，', '、', '；', '：', '！', '？', '.', ',', ';', ':', '!', '?',
    ];
    let t = text.trim();
    !t.is_empty()
        && t.chars().count() <= 32
        && !t.chars().any(char::is_whitespace)
        && !t.contains(PUNCT)
}

/// 建一个直连（不走代理）的 HTTP 客户端。
pub fn direct_client(timeout_secs: u64) -> Result<reqwest::Client, TranslateError> {
    Ok(reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(timeout_secs))
        .user_agent(USER_AGENT)
        .build()?)
}

pub struct YoudaoTransClient {
    http: reqwest::Client,
}

impl YoudaoTransClient {
    pub fn new() -> Result<Self, TranslateError> {
        Ok(Self {
            http: direct_client(20)?,
        })
    }

    /// 只取原始 JSON，供词典驱动做整句兜底。
    pub async fn raw(&self, text: &str, from: &str, to: &str) -> Result<Value, TranslateError> {
        let from = to_youdao_lang(from)
            .ok_or_else(|| TranslateError::Invalid(format!("有道翻译不支持源语言 {from}")))?;
        let to = to_youdao_lang(to)
            .ok_or_else(|| TranslateError::Invalid(format!("有道翻译不支持目标语言 {to}")))?;

        let resp = self
            .http
            .get(ENDPOINT)
            .query(&[("q", text), ("from", from), ("to", to)])
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(TranslateError::Status { status });
        }
        let body: Value = resp.json().await?;
        let code = body
            .get("errorCode")
            .and_then(|v| v.as_str())
            .unwrap_or("0");
        if code != "0" {
            let msg = body
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("有道翻译返回错误");
            return Err(TranslateError::Parse(format!("{msg}（错误码 {code}）")));
        }
        Ok(body)
    }

    /// 从 aidemo 响应里抽出译文与发音地址。
    pub fn extract(body: &Value, query: &str, to: &str) -> TranslateResult {
        let paragraphs: Vec<String> = body
            .get("translation")
            .and_then(|v| v.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str())
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let translation = paragraphs.join("\n");
        // aidemo 返回的 l 形如 "en2zh-CHS"，左半是识别出的源语言。
        let source_lang = body
            .get("l")
            .and_then(|v| v.as_str())
            .and_then(|l| l.split("2").next())
            .map(from_youdao_lang)
            .unwrap_or_else(|| "auto".into());

        let segments = paragraphs
            .iter()
            .map(|p| Segment {
                text: p.clone(),
                source: String::new(),
            })
            .collect::<Vec<_>>();
        let segments = if segments.is_empty() {
            vec![Segment {
                text: translation.clone(),
                source: query.to_string(),
            }]
        } else {
            segments
        };

        TranslateResult {
            query: query.to_string(),
            source_lang,
            target_lang: to.to_string(),
            engine: ID.to_string(),
            translation,
            segments,
            romanization: None,
            phonetic: None,
            phonetic_uk: None,
            speak_url: body
                .get("speakUrl")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            target_speak_url: body
                .get("tSpeakUrl")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            dictionary: Vec::new(),
            alternatives: Vec::new(),
            synonyms: Vec::new(),
            definitions: Vec::new(),
            examples: Vec::new(),
            phrases: Vec::new(),
            related: Vec::new(),
            sections: Vec::new(),
            is_word: looks_like_word(query),
            cached: false,
            elapsed_ms: 0,
        }
    }
}

pub struct YoudaoTransDriver {
    client: YoudaoTransClient,
}

impl YoudaoTransDriver {
    pub const ID: &'static str = ID;

    pub fn new() -> Result<Self, TranslateError> {
        Ok(Self {
            client: YoudaoTransClient::new()?,
        })
    }
}

#[async_trait]
impl Driver for YoudaoTransDriver {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            id: ID.into(),
            name: "有道翻译".into(),
            note: "国内直连，无需代理；单词与整句都能翻，附带发音音频".into(),
            needs_proxy: false,
            supports_sentence: true,
            supports_dictionary: false,
            supports_audio: true,
        }
    }

    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        if text.trim().is_empty() {
            return Err(TranslateError::Empty);
        }
        let started = Instant::now();
        let body = self.client.raw(text, from, to).await?;
        let mut result = YoudaoTransClient::extract(&body, text, to);
        result.elapsed_ms = started.elapsed().as_millis() as u64;
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn maps_language_codes() {
        assert_eq!(to_youdao_lang("zh-CN"), Some("zh-CHS"));
        assert_eq!(to_youdao_lang("auto"), Some("auto"));
        assert_eq!(to_youdao_lang("en"), Some("en"));
        assert_eq!(to_youdao_lang("kl"), None);
    }

    #[test]
    fn extracts_translation_and_audio() {
        let body = json!({
            "errorCode": "0",
            "query": "coercion",
            "translation": ["强迫"],
            "l": "en2zh-CHS",
            "speakUrl": "https://example.com/en.mp3",
            "tSpeakUrl": "https://example.com/zh.mp3"
        });
        let r = YoudaoTransClient::extract(&body, "coercion", "zh-CN");
        assert_eq!(r.translation, "强迫");
        assert_eq!(r.source_lang, "en");
        assert_eq!(r.engine, "youdaoTrans");
        assert_eq!(r.speak_url.as_deref(), Some("https://example.com/en.mp3"));
        assert_eq!(r.target_speak_url.as_deref(), Some("https://example.com/zh.mp3"));
        assert_eq!(r.segments.len(), 1);
        assert!(r.is_word, "单个词应标记为单词，前端据此自动朗读");
    }

    #[test]
    fn word_shape_heuristic() {
        assert!(looks_like_word("coercion"));
        assert!(looks_like_word("  get  "));
        assert!(looks_like_word("强迫"));

        assert!(!looks_like_word("hello world"), "带空格按句子处理");
        assert!(!looks_like_word("今天天气不错，我们出去走走吧。"), "带句读按句子处理");
        assert!(!looks_like_word("What time is it?"));
        assert!(!looks_like_word(""));
        assert!(!looks_like_word(&"a".repeat(33)), "超长一律按句子处理");
    }

    #[test]
    fn multi_paragraph_translation_keeps_lines() {
        let body = json!({
            "errorCode": "0",
            "translation": ["第一段。", "第二段。"],
            "l": "en2zh-CHS"
        });
        let r = YoudaoTransClient::extract(&body, "one\ntwo", "zh-CN");
        assert_eq!(r.translation, "第一段。\n第二段。");
        assert_eq!(r.segments.len(), 2);
    }
}
