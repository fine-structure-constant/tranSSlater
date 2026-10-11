//! Google 翻译接口的数据模型。
//!
//! 字段与 `translate_a/single` 响应的对应关系见 `google.rs` 的解析函数。

use serde::{Deserialize, Serialize};

/// 一次翻译的完整结果。前端拿到的就是它。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateResult {
    /// 原文
    pub query: String,
    /// 检测到（或指定）的源语言，如 "en"
    pub source_lang: String,
    /// 目标语言，如 "zh-CN"
    pub target_lang: String,
    /// 合并后的译文（保留接口返回的段落换行）
    pub translation: String,
    /// 分段译文，保留原文的段落结构
    pub segments: Vec<Segment>,
    /// 拼音 / 罗马化，如 "Qiǎngpò"
    pub romanization: Option<String>,
    /// 国际音标，如 "kōˈərSH(ə)n"
    pub phonetic: Option<String>,
    /// 词典：词性 + 译词 + 反查（对应 dt=bd）
    pub dictionary: Vec<DictEntry>,
    /// 中文近义候选（对应 dt=at），如 ["强迫","胁迫","强制"]
    pub alternatives: Vec<String>,
    /// 英文同义词，按词性分组（对应 dt=ss）
    pub synonyms: Vec<SynonymGroup>,
    /// 英英释义，按词性分组（对应 dt=md）
    pub definitions: Vec<DefinitionGroup>,
    /// 例句，原文中的目标词用 `<b>` 包裹（对应 dt=ex）
    pub examples: Vec<String>,
    /// 是否为单词（决定前端显示哪些板块）
    pub is_word: bool,
    /// 是否命中本地缓存
    pub cached: bool,
    /// 本次请求耗时（毫秒）；命中缓存时为 0
    pub elapsed_ms: u64,
}

impl TranslateResult {
    /// 是否包含词典类数据（英英释义 / 同义词 / 例句任一非空）
    pub fn has_lexical_data(&self) -> bool {
        !self.dictionary.is_empty()
            || !self.synonyms.is_empty()
            || !self.definitions.is_empty()
            || !self.examples.is_empty()
    }
}

/// 一段译文及其对应的原文片段。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Segment {
    pub text: String,
    pub source: String,
}

/// 词典条目：一个词性下的所有译法。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DictEntry {
    /// 词性，如 "noun" / "verb" / "adj"
    pub pos: String,
    /// 目标语译词，如 ["强迫", "压迫", "控制"]
    pub terms: Vec<String>,
    /// 反查：每个译词对应哪些源语词，以及 Google 给的频率分
    pub back: Vec<BackEntry>,
}

/// 反查条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BackEntry {
    /// 目标语译词
    pub term: String,
    /// 该译词在源语中对应的词
    pub source_words: Vec<String>,
    /// 频率分（Google 内部权重，越大越常用）
    pub score: f64,
}

/// 同义词分组。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynonymGroup {
    pub pos: String,
    pub words: Vec<String>,
}

/// 英英释义分组。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionGroup {
    pub pos: String,
    pub definitions: Vec<Definition>,
}

/// 单条英英释义。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    /// 释义正文
    pub text: String,
    /// 该释义附带的例句（可能没有）
    pub example: Option<String>,
}
