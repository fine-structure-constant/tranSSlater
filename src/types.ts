/**
 * 与 Rust 侧 `translate::model` 一一对应的类型。
 * Rust 结构体带 `#[serde(rename_all = "camelCase")]`，所以这里用驼峰。
 */

export interface TranslateResult {
  query: string;
  sourceLang: string;
  targetLang: string;
  /** 合并后的译文，多段用换行拼接 */
  translation: string;
  /** 分段译文，保留原文段落结构 */
  segments: Segment[];
  /** 拼音 / 罗马化 */
  romanization: string | null;
  /** 国际音标 */
  phonetic: string | null;
  dictionary: DictEntry[];
  /** 中文近义候选 */
  alternatives: string[];
  synonyms: SynonymGroup[];
  definitions: DefinitionGroup[];
  /** 例句，目标词用 <b> 包裹 */
  examples: string[];
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
}

export interface BackEntry {
  term: string;
  sourceWords: string[];
  score: number;
}

export interface SynonymGroup {
  pos: string;
  words: string[];
}

export interface DefinitionGroup {
  pos: string;
  definitions: Definition[];
}

export interface Definition {
  text: string;
  example: string | null;
}

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
