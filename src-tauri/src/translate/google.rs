//! Google 翻译驱动。
//!
//! 端点：`https://translate.googleapis.com/translate_a/single`
//!
//! 实测（2026-10）确认：
//! - `client=gtx` 可用；`client=webapp` 返回 403
//! - 短时间连续请求会返回 429，需要退避重试
//! - **大陆网络下必须走代理**，因此这是唯一接受代理配置的引擎
//! - 响应是一个异构数组，各下标含义见 [`index`]
//!
//! 我们固定请求 `dt=t,bd,rm,md,ss,ex,at,ld`，因此下标是稳定的。

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;
use url::Url;

use super::driver::{Driver, EngineInfo};
use super::error::TranslateError;
use super::model::*;

pub const ID: &str = "google";
const ENDPOINT: &str = "https://translate.googleapis.com/translate_a/single";

/// 固定请求的 dt 参数。改动这里必须同步复核 [`index`]。
const DT_PARAMS: [&str; 8] = ["t", "bd", "rm", "md", "ss", "ex", "at", "ld"];

/// 响应数组的下标含义（与 [`DT_PARAMS`] 绑定）。
mod index {
    /// 译文分段；每个元素是 `[译文, 原文, ...]`，末尾可能有一行罗马化数据
    pub const TRANSLATION: usize = 0;
    /// 词典：词性 + 译词 + 反查（dt=bd）
    pub const DICTIONARY: usize = 1;
    /// 检测到的源语言（字符串）
    pub const DETECTED_LANG: usize = 2;
    /// 中文近义候选（dt=at）
    pub const ALTERNATIVES: usize = 5;
    /// 英文同义词（dt=ss）
    pub const SYNONYMS: usize = 11;
    /// 英英释义（dt=md）
    pub const DEFINITIONS: usize = 12;
    /// 例句（dt=ex）
    pub const EXAMPLES: usize = 13;
}

/// 429 时的重试退避（毫秒）。第一项为 0，表示首次不等待。
const BACKOFF_MS: [u64; 4] = [0, 1000, 2000, 4000];

pub struct GoogleClient {
    http: reqwest::Client,
}

impl GoogleClient {
    /// `proxy` 形如 `http://127.0.0.1:7890` 或 `socks5://...`。
    /// 传 `None` 或空串时不显式设代理，reqwest 仍会自动读取
    /// `HTTP_PROXY` / `HTTPS_PROXY` 环境变量。
    pub fn new(proxy: Option<&str>) -> Result<Self, TranslateError> {
        let mut builder = reqwest::Client::builder()
            .timeout(Duration::from_secs(20))
            .user_agent(
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 \
                 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
            );
        if let Some(p) = proxy {
            if !p.trim().is_empty() {
                validate_proxy(p.trim())?;
                builder = builder.proxy(reqwest::Proxy::all(p.trim())?);
            }
        }
        Ok(Self {
            http: builder.build()?,
        })
    }

    fn build_url(&self, text: &str, from: &str, to: &str) -> Result<Url, TranslateError> {
        let mut url = Url::parse(ENDPOINT)?;
        {
            let mut qp = url.query_pairs_mut();
            qp.append_pair("client", "gtx");
            qp.append_pair("sl", from);
            qp.append_pair("tl", to);
            qp.append_pair("q", text);
            for dt in DT_PARAMS {
                qp.append_pair("dt", dt);
            }
        }
        Ok(url)
    }

    /// 发起一次翻译。带 429 退避重试。
    pub async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        if text.trim().is_empty() {
            return Err(TranslateError::Empty);
        }
        if text.chars().count() > 5000 {
            return Err(TranslateError::Invalid(
                "一次最多翻译 5000 个字符，请分段翻译".into(),
            ));
        }
        if from.is_empty() || to.is_empty() || to == "auto" {
            return Err(TranslateError::Invalid(
                "请选择有效的源语言和目标语言".into(),
            ));
        }
        let url = self.build_url(text, from, to)?;
        let started = Instant::now();

        for delay in BACKOFF_MS {
            if delay > 0 {
                tokio::time::sleep(Duration::from_millis(delay)).await;
            }
            let resp = self.http.get(url.clone()).send().await?;
            let status = resp.status().as_u16();
            match status {
                200 => {
                    let raw: Value = resp.json().await?;
                    let mut result = parse(&raw, text, to)?;
                    result.elapsed_ms = started.elapsed().as_millis() as u64;
                    return Ok(result);
                }
                429 => {
                    continue;
                }
                s => return Err(TranslateError::Status { status: s }),
            }
        }
        Err(TranslateError::RateLimited {
            attempts: BACKOFF_MS.len() as u32,
        })
    }
}

pub fn validate_proxy(proxy: &str) -> Result<(), TranslateError> {
    if proxy.is_empty() {
        return Ok(());
    }
    let url = Url::parse(proxy).map_err(|_| {
        TranslateError::Invalid("代理须为完整地址，如 http://127.0.0.1:7890".into())
    })?;
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h") || url.host_str().is_none()
    {
        return Err(TranslateError::Invalid(
            "代理仅支持 HTTP、HTTPS、SOCKS5、SOCKS5H".into(),
        ));
    }
    if url.path() != "/" && !url.path().is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(TranslateError::Invalid(
            "代理地址不能包含路径、查询参数或片段".into(),
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 安全取值：数组的第 i 个元素。
fn at(v: &Value, i: usize) -> Option<&Value> {
    v.as_array().and_then(|a| a.get(i))
}

/// 安全取值：把值当字符串。
fn as_str(v: &Value) -> Option<&str> {
    v.as_str()
}

/// 安全取值：取数组第 i 个元素并当字符串。
fn str_at(v: &Value, i: usize) -> Option<String> {
    at(v, i).and_then(as_str).map(|s| s.to_string())
}

/// 安全取值：取数组第 i 个元素并当数组。
fn arr_at(v: &Value, i: usize) -> Vec<Value> {
    at(v, i)
        .and_then(|x| x.as_array())
        .cloned()
        .unwrap_or_default()
}

/// 把响应 JSON 解析成 [`TranslateResult`]。
///
/// 对每个下标都做类型与边界检查：Google 在「查询是整句」时会把
/// 词典相关的下标返回 `null` 或直接截短数组。
pub fn parse(
    raw: &Value,
    query: &str,
    target_lang: &str,
) -> Result<TranslateResult, TranslateError> {
    let root = raw
        .as_array()
        .ok_or_else(|| TranslateError::Parse("顶层不是数组".into()))?;

    // --- [0] 译文分段 + 罗马化 ---
    let mut segments = Vec::new();
    let mut romanization = None;
    let mut phonetic = None;
    for row in arr_at(raw, index::TRANSLATION) {
        let first = at(&row, 0);
        match first {
            // 正常译文行：[译文, 原文, ...]
            Some(Value::String(t)) => {
                let source = str_at(&row, 1).unwrap_or_default();
                segments.push(Segment {
                    text: t.clone(),
                    source,
                });
            }
            // 罗马化行：[null, null, 拼音, 音标]
            _ => {
                if let Some(p) = str_at(&row, 2) {
                    romanization = Some(p);
                }
                if let Some(p) = str_at(&row, 3) {
                    phonetic = Some(p);
                }
            }
        }
    }

    let translation = segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    if translation.trim().is_empty() {
        return Err(TranslateError::Parse("服务未返回译文，请稍后重试".into()));
    }

    // --- [2] 检测到的源语言 ---
    let source_lang = root
        .get(index::DETECTED_LANG)
        .and_then(as_str)
        .unwrap_or("auto")
        .to_string();

    // --- [1] 词典 ---
    let dictionary = parse_dictionary(raw);

    // --- [5] 中文近义候选 ---
    let alternatives = parse_alternatives(raw);

    // --- [11] 英文同义词 ---
    let synonyms = parse_synonyms(raw);

    // --- [12] 英英释义 ---
    let definitions = parse_definitions(raw);

    // --- [13] 例句 ---
    let examples = parse_examples(raw);

    let is_word = !dictionary.is_empty()
        || !definitions.is_empty()
        || !synonyms.is_empty()
        || !examples.is_empty();

    Ok(TranslateResult {
        query: query.to_string(),
        source_lang,
        target_lang: target_lang.to_string(),
        engine: ID.to_string(),
        translation,
        segments,
        romanization,
        phonetic,
        phonetic_uk: None,
        speak_url: None,
        target_speak_url: None,
        dictionary,
        alternatives,
        suggestions: Vec::new(),
        synonyms,
        definitions,
        examples,
        phrases: Vec::new(),
        related: Vec::new(),
        sections: Vec::new(),
        is_word,
        cached: false,
        elapsed_ms: 0,
    })
}

/// `[1]`：`[[pos, [译词...], [反查...], 原词, n], ...]`
fn parse_dictionary(raw: &Value) -> Vec<DictEntry> {
    let mut out = Vec::new();
    for entry in arr_at(raw, index::DICTIONARY) {
        let pos = str_at(&entry, 0).unwrap_or_default();
        if pos.is_empty() {
            continue;
        }
        let terms = arr_at(&entry, 1)
            .iter()
            .filter_map(as_str)
            .map(|s| s.to_string())
            .collect::<Vec<_>>();

        let mut back = Vec::new();
        for b in arr_at(&entry, 2) {
            let term = match str_at(&b, 0) {
                Some(t) => t,
                None => continue,
            };
            let source_words = arr_at(&b, 1)
                .iter()
                .filter_map(as_str)
                .map(|s| s.to_string())
                .collect::<Vec<_>>();
            let score = at(&b, 3).and_then(|v| v.as_f64()).unwrap_or(0.0);
            back.push(BackEntry {
                term,
                source_words,
                score,
            });
        }

        out.push(DictEntry {
            pos,
            terms,
            back,
            exams: Vec::new(),
        });
    }
    out
}

/// `[5]`：`[[原词, null, [[候选, ...], ...], ...], ...]`
fn parse_alternatives(raw: &Value) -> Vec<String> {
    let mut out = Vec::new();
    for group in arr_at(raw, index::ALTERNATIVES) {
        // 候选列表位于第 2 个元素
        for cand in arr_at(&group, 2) {
            if let Some(t) = str_at(&cand, 0) {
                if !out.contains(&t) {
                    out.push(t);
                }
            }
        }
    }
    out
}

/// `[11]`：`[[pos, [[[词...], id]], 原词, n], ...]`
fn parse_synonyms(raw: &Value) -> Vec<SynonymGroup> {
    let mut out = Vec::new();
    for group in arr_at(raw, index::SYNONYMS) {
        let pos = str_at(&group, 0).unwrap_or_default();
        let mut words = Vec::new();
        for block in arr_at(&group, 1) {
            // block = [[词...], id]
            for w in arr_at(&block, 0) {
                if let Some(s) = as_str(&w) {
                    words.push(s.to_string());
                }
            }
        }
        if !words.is_empty() {
            out.push(SynonymGroup {
                pos,
                words,
                gloss: None,
            });
        }
    }
    out
}

/// `[12]`：`[[pos, [[释义, id, 例句?], ...], 原词, n], ...]`
fn parse_definitions(raw: &Value) -> Vec<DefinitionGroup> {
    let mut out = Vec::new();
    for group in arr_at(raw, index::DEFINITIONS) {
        let pos = str_at(&group, 0).unwrap_or_default();
        let mut defs = Vec::new();
        for d in arr_at(&group, 1) {
            let text = match str_at(&d, 0) {
                Some(t) => t,
                None => continue,
            };
            defs.push(Definition {
                text,
                example: str_at(&d, 2),
                similar: Vec::new(),
            });
        }
        if !defs.is_empty() {
            out.push(DefinitionGroup {
                pos,
                definitions: defs,
                source: None,
            });
        }
    }
    out
}

/// `[13]`：`[[[例句, ...], ...], ...]`。Google 只给英文例句，没有译文。
fn parse_examples(raw: &Value) -> Vec<Example> {
    let mut out = Vec::new();
    for block in arr_at(raw, index::EXAMPLES) {
        for item in block.as_array().cloned().unwrap_or_default() {
            if let Some(s) = str_at(&item, 0) {
                if !out.iter().any(|e: &Example| e.text == s) {
                    out.push(Example {
                        text: s,
                        translation: None,
                        source: None,
                    });
                }
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------
// 驱动
// ---------------------------------------------------------------------------

pub struct GoogleDriver {
    client: GoogleClient,
}

impl GoogleDriver {
    pub const ID: &'static str = ID;

    pub fn new(proxy: Option<&str>) -> Result<Self, TranslateError> {
        Ok(Self {
            client: GoogleClient::new(proxy)?,
        })
    }
}

#[async_trait]
impl Driver for GoogleDriver {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            id: ID.into(),
            name: "Google 翻译".into(),
            note: "大陆需代理；词典、英英释义、同义词、例句最均衡".into(),
            needs_proxy: true,
            supports_sentence: true,
            supports_dictionary: true,
            supports_audio: false,
        }
    }

    async fn translate(
        &self,
        text: &str,
        from: &str,
        to: &str,
    ) -> Result<TranslateResult, TranslateError> {
        self.client.translate(text, from, to).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// 单词的完整响应（已脱敏，结构取自真实抓包）
    fn word_response() -> Value {
        json!([
            [
                ["强迫", "coercion", null, null, 2],
                [null, null, "Qiǎngpò", "kōˈərSH(ə)n"]
            ],
            [[
                "noun",
                ["强迫", "压迫", "控制", "制止"],
                [
                    ["强迫", ["coercion"], null, 0.09157243],
                    ["压迫", ["oppression", "repression"], null, 0.0077553294]
                ],
                "coercion",
                1
            ]],
            "en",
            null,
            null,
            [[
                "coercion",
                null,
                [
                    ["强迫", null, true, false, [2]],
                    ["胁迫", null, true, false, [3]]
                ],
                [[0, 8]],
                "coercion",
                0,
                0
            ]],
            1,
            [],
            [["en"], null, [1], ["en"]],
            null,
            null,
            [[
                "noun",
                [[
                    ["force", "compulsion", "constraint"],
                    "m_en_gbus0196880.007"
                ]],
                "coercion",
                1
            ]],
            [[
                "noun",
                [[
                    "the practice of persuading someone to do something by using force or threats.",
                    "m_en_gbus0196880.007",
                    "our problem cannot be solved by any form of coercion"
                ]],
                "coercion",
                1
            ]],
            [[[
                "our problem cannot be solved by any form of <b>coercion</b> but only by agreement",
                null,
                null,
                null,
                null,
                "m_en_gbus0196880.007"
            ]]]
        ])
    }

    /// 整句的响应：词典相关下标为 null / 数组更短
    fn sentence_response() -> Value {
        json!([
            [[
                "该公司早年具有颠覆性。",
                "The company was disruptive in its early years.",
                null,
                null,
                3
            ]],
            null,
            "en",
            null,
            null,
            [[
                "The company was disruptive in its early years.",
                null,
                [],
                [],
                "x",
                0,
                0
            ]],
            1,
            [],
            [["en"], null, [1], ["en"]]
        ])
    }

    #[test]
    fn parses_word_fields() {
        let r = parse(&word_response(), "coercion", "zh-CN").unwrap();
        assert_eq!(r.translation, "强迫");
        assert_eq!(r.engine, "google");
        assert_eq!(r.source_lang, "en");
        assert_eq!(r.romanization.as_deref(), Some("Qiǎngpò"));
        assert_eq!(r.phonetic.as_deref(), Some("kōˈərSH(ə)n"));
        assert_eq!(r.segments.len(), 1);
        assert_eq!(r.segments[0].source, "coercion");
    }

    #[test]
    fn parses_dictionary() {
        let r = parse(&word_response(), "coercion", "zh-CN").unwrap();
        assert_eq!(r.dictionary.len(), 1);
        let d = &r.dictionary[0];
        assert_eq!(d.pos, "noun");
        assert_eq!(d.terms, vec!["强迫", "压迫", "控制", "制止"]);
        assert_eq!(d.back.len(), 2);
        assert_eq!(d.back[1].term, "压迫");
        assert_eq!(d.back[1].source_words, vec!["oppression", "repression"]);
        assert!(d.back[0].score > 0.09);
    }

    #[test]
    fn parses_alternatives_synonyms_definitions_examples() {
        let r = parse(&word_response(), "coercion", "zh-CN").unwrap();
        assert_eq!(r.alternatives, vec!["强迫", "胁迫"]);
        assert_eq!(r.synonyms.len(), 1);
        assert_eq!(r.synonyms[0].pos, "noun");
        assert_eq!(
            r.synonyms[0].words,
            vec!["force", "compulsion", "constraint"]
        );
        assert_eq!(r.definitions.len(), 1);
        assert_eq!(r.definitions[0].definitions.len(), 1);
        assert!(
            r.definitions[0].definitions[0]
                .example
                .as_deref()
                .unwrap()
                .contains("coercion")
        );
        assert_eq!(r.examples.len(), 1);
        assert!(r.examples[0].text.contains("<b>coercion</b>"));
        assert!(r.examples[0].translation.is_none());
        assert!(r.is_word);
    }

    #[test]
    fn sentence_degrades_gracefully() {
        // 关键断言：整句响应里 [11]/[12]/[13] 根本不存在，不能 panic
        let r = parse(&sentence_response(), "The company was disruptive.", "zh-CN").unwrap();
        assert_eq!(r.translation, "该公司早年具有颠覆性。");
        assert!(r.dictionary.is_empty());
        assert!(r.synonyms.is_empty());
        assert!(r.definitions.is_empty());
        assert!(r.examples.is_empty());
        assert!(r.phrases.is_empty());
        assert!(r.related.is_empty());
        assert!(r.sections.is_empty());
        assert!(!r.is_word);
        assert!(r.phonetic.is_none());
    }

    #[test]
    fn multi_segment_keeps_order() {
        let raw = json!([
            [
                ["一号线\n\n", "line one\n\n", null, null, 3],
                ["二号线", "line two", null, null, 3]
            ],
            null,
            "en",
            null,
            null,
            null,
            1,
            [],
            [["en"], null, [1], ["en"]]
        ]);
        let r = parse(&raw, "line one\n\nline two", "zh-CN").unwrap();
        assert_eq!(r.segments.len(), 2);
        assert_eq!(r.segments[0].text, "一号线\n\n");
        assert_eq!(r.segments[1].text, "二号线");
        assert_eq!(r.translation, "一号线\n\n二号线");
    }

    #[test]
    fn rejects_missing_translation() {
        for raw in [
            json!([]),
            json!([null]),
            json!({"error":"failure"}),
            json!([[[null, null, "pinyin"]]]),
        ] {
            assert!(parse(&raw, "hello", "zh-CN").is_err());
        }
    }

    #[test]
    fn validates_proxy_schemes() {
        for proxy in [
            "",
            "http://127.0.0.1:7890",
            "https://localhost:7890",
            "socks5://localhost:1080",
            "socks5h://localhost:1080",
        ] {
            assert!(validate_proxy(proxy).is_ok());
        }
        for proxy in [
            "localhost:7890",
            "file:///tmp/proxy",
            "ftp://localhost",
            "http://localhost/path",
            "http://localhost?token=x",
        ] {
            assert!(validate_proxy(proxy).is_err());
        }
    }
}
