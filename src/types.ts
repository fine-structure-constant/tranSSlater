/**
 * 与 Rust 侧 `translate::model` / `driver` / `history` 一一对应的类型。
 * Rust 结构体带 `#[serde(rename_all = "camelCase")]`，所以这里用驼峰。
 */

export interface TranslateResult {
  query: string;
  sourceLang: string;
  targetLang: string;
  /** 产出这个结果的引擎 id */
  engine: string;
  /** 合并后的译文，多段用换行拼接 */
  translation: string;
  /** 分段译文，保留原文段落结构 */
  segments: Segment[];
  /** 拼音 / 罗马化 */
  romanization: string | null;
  /** 国际音标（美） */
  phonetic: string | null;
  /** 国际音标（英） */
  phoneticUk: string | null;
  /** 原文发音音频地址 */
  speakUrl: string | null;
  /** 译文发音音频地址 */
  targetSpeakUrl: string | null;
  dictionary: DictEntry[];
  /** 中文近义候选 */
  alternatives: string[];
  synonyms: SynonymGroup[];
  definitions: DefinitionGroup[];
  examples: Example[];
  /** 词组短语 */
  phrases: Phrase[];
  /** 同根词 / 词形变化 */
  related: RelatedGroup[];
  /** 其他板块：柯林斯 / 专业释义 / 网络释义 / 词源 / 辨析 / AI 释义 */
  sections: Section[];
  isWord: boolean;
  cached: boolean;
  elapsedMs: number;
}

export interface Segment {
  text: string;
  source: string;
}

export interface DictEntry {
  pos: string;
  terms: string[];
  back: BackEntry[];
  /** 考试标签，如 ["CET4", "考研"] */
  exams: string[];
}

export interface BackEntry {
  term: string;
  sourceWords: string[];
  score: number;
}

export interface SynonymGroup {
  pos: string;
  words: string[];
  /** 该组同义词的整体释义 */
  gloss: string | null;
}

export interface DefinitionGroup {
  pos: string;
  definitions: Definition[];
  /** 释义来源，如 "WordNet" */
  source: string | null;
}

export interface Definition {
  text: string;
  example: string | null;
  similar: string[];
}

/** 例句。`text` 里目标词可能被 `<b>` 包裹。 */
export interface Example {
  text: string;
  translation: string | null;
  source: string | null;
}

export interface Phrase {
  term: string;
  translation: string;
  source: string | null;
}

export interface RelatedGroup {
  pos: string;
  words: RelatedWord[];
}

export interface RelatedWord {
  word: string;
  gloss: string | null;
}

export interface Section {
  key: string;
  title: string;
  note: string | null;
  /** 正文是否按 Markdown 渲染 */
  markdown: boolean;
  groups: SectionGroup[];
}

export interface SectionGroup {
  label: string;
  note: string | null;
  items: SectionItem[];
}

export interface SectionItem {
  text: string;
  translation: string | null;
  note: string | null;
  extra: Example[];
}

/** 引擎元信息。 */
export interface EngineInfo {
  id: string;
  name: string;
  note: string;
  needsProxy: boolean;
  supportsSentence: boolean;
  supportsDictionary: boolean;
  supportsAudio: boolean;
}

/** 历史列表项（不含完整结果）。 */
export interface HistorySummary {
  id: number;
  query: string;
  fromLang: string;
  toLang: string;
  engine: string;
  translation: string;
  isWord: boolean;
  /** Unix 毫秒 */
  createdAt: number;
}

export interface HistoryPage {
  items: HistorySummary[];
  total: number;
}

export interface HistoryStats {
  total: number;
  sizeBytes: number;
}

export type ExportFormat = "csv" | "markdown" | "anki" | "json";

/** 语言代码 → 展示名，用于「检测到 xx」提示 */
export const LANG_NAME: Record<string, string> = {
  auto: "自动检测",
  en: "英语",
  "zh-CN": "中文",
  "zh-TW": "繁体中文",
  ja: "日语",
  ko: "韩语",
  fr: "法语",
  de: "德语",
  ru: "俄语",
  es: "西班牙语",
  pt: "葡萄牙语",
  it: "意大利语",
  ar: "阿拉伯语",
  th: "泰语",
  vi: "越南语",
  nl: "荷兰语",
  pl: "波兰语",
  tr: "土耳其语",
};

export function langName(code: string): string {
  return LANG_NAME[code] ?? code;
}
