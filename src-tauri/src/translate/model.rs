//! 翻译结果的统一数据模型。
//!
//! 各驱动把自家响应映射到这里，前端只认这一套结构。
//! 字段与 `#[serde(rename_all = "camelCase")]` 对齐，前端用驼峰访问。

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
    /// 产出这个结果的引擎 id，如 "google" / "youdao"
    #[serde(default)]
    pub engine: String,
    /// 合并后的译文（保留接口返回的段落换行）
    pub translation: String,
    /// 分段译文，保留原文的段落结构
    pub segments: Vec<Segment>,
    /// 拼音 / 罗马化，如 "Qiǎngpò"
    pub romanization: Option<String>,
    /// 国际音标（**美式**）。有道取 `usphone`，Google 只有一套，也按美式处理。
    pub phonetic: Option<String>,
    /// 英式音标（有道提供 `ukphone`，Google 没有）
    #[serde(default)]
    pub phonetic_uk: Option<String>,
    /// 原文发音音频地址（有道提供）
    #[serde(default)]
    pub speak_url: Option<String>,
    /// 译文发音音频地址（有道提供）
    #[serde(default)]
    pub target_speak_url: Option<String>,
    /// 词典：词性 + 译词 + 反查（Google dt=bd / 有道 ec）
    pub dictionary: Vec<DictEntry>,
    /// 中文近义候选（Google dt=at），如 ["强迫","胁迫","强制"]
    pub alternatives: Vec<String>,
    /// 同近义词，按词性分组（Google dt=ss / 有道 syno）
    pub synonyms: Vec<SynonymGroup>,
    /// 英英释义，按词性分组（Google dt=md / 有道 ee）
    pub definitions: Vec<DefinitionGroup>,
    /// 例句，可能带译文与出处
    #[serde(default)]
    pub examples: Vec<Example>,
    /// 词组短语（有道 phrs）
    #[serde(default)]
    pub phrases: Vec<Phrase>,
    /// 同根词 / 词形变化（有道 rel_word）
    #[serde(default)]
    pub related: Vec<RelatedGroup>,
    /// 其他板块：柯林斯 / 专业释义 / 网络释义 / 词源 / 辨析 / AI 释义
    #[serde(default)]
    pub sections: Vec<Section>,
    /// 是否为单词（决定前端显示哪些板块）
    pub is_word: bool,
    /// 是否命中本地缓存
    pub cached: bool,
    /// 本次请求耗时（毫秒）；命中缓存时为 0
    pub elapsed_ms: u64,
}

impl TranslateResult {
    /// 是否包含词典类数据
    pub fn has_lexical_data(&self) -> bool {
        !self.dictionary.is_empty()
            || !self.synonyms.is_empty()
            || !self.definitions.is_empty()
            || !self.examples.is_empty()
            || !self.phrases.is_empty()
            || !self.related.is_empty()
            || !self.sections.is_empty()
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
    /// 词性，如 "noun" / "n." / "verb"
    pub pos: String,
    /// 目标语译词，如 ["强迫", "压迫", "控制"]
    pub terms: Vec<String>,
    /// 反查：每个译词对应哪些源语词（Google 提供，有道为空）
    #[serde(default)]
    pub back: Vec<BackEntry>,
    /// 考试标签，如 ["CET4", "考研"]（有道提供）
    #[serde(default)]
    pub exams: Vec<String>,
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
    /// 该组同义词的整体释义（有道提供）
    #[serde(default)]
    pub gloss: Option<String>,
}

/// 英英释义分组。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DefinitionGroup {
    pub pos: String,
    pub definitions: Vec<Definition>,
    /// 释义来源，如 "WordNet"
    #[serde(default)]
    pub source: Option<String>,
}

/// 单条英英释义。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Definition {
    /// 释义正文
    pub text: String,
    /// 该释义附带的例句（可能没有）
    pub example: Option<String>,
    /// 近义表达（有道 WordNet 的 similar-words）
    #[serde(default)]
    pub similar: Vec<String>,
}

/// 一条例句。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Example {
    /// 例句原文，目标词可能用 `<b>` 包裹
    pub text: String,
    /// 译文（Google 不提供，有道提供）
    #[serde(default)]
    pub translation: Option<String>,
    /// 出处，如 "《牛津词典》" / "BBC"
    #[serde(default)]
    pub source: Option<String>,
}

/// 词组短语。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Phrase {
    /// 词组本身
    pub term: String,
    /// 释义
    pub translation: String,
    /// 来源标注
    #[serde(default)]
    pub source: Option<String>,
}

/// 同根词 / 词形变化分组。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedGroup {
    pub pos: String,
    pub words: Vec<RelatedWord>,
}

/// 同根词条目。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RelatedWord {
    pub word: String,
    #[serde(default)]
    pub gloss: Option<String>,
}

/// 通用板块。用于承载形态各异的附加数据（柯林斯 / 专业释义 / 网络释义 …）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// 板块标识，如 "collins" / "special" / "web" / "etym" / "ai"
    pub key: String,
    /// 展示名，如 "柯林斯"
    pub title: String,
    /// 副标题 / 数据来源说明
    #[serde(default)]
    pub note: Option<String>,
    /// 正文是否按 Markdown 渲染（AI 释义用）
    #[serde(default)]
    pub markdown: bool,
    pub groups: Vec<SectionGroup>,
}

/// 板块内的一组内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionGroup {
    /// 组标签：词性 / 学科 / 序号
    #[serde(default)]
    pub label: String,
    /// 组级说明
    #[serde(default)]
    pub note: Option<String>,
    pub items: Vec<SectionItem>,
}

/// 板块内的一条内容。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SectionItem {
    /// 正文（英文释义 / 术语 / 段落）
    pub text: String,
    /// 中文对应（译文 / 译词）
    #[serde(default)]
    pub translation: Option<String>,
    /// 出处 / 引用数等
    #[serde(default)]
    pub note: Option<String>,
    /// 附加例句
    #[serde(default)]
    pub extra: Vec<Example>,
}
