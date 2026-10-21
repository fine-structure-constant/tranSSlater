//! 有道词典驱动。
//!
//! 端点：`https://dict.youdao.com/jsonapi?q=...&dicts=...`
//!
//! 特点（2026-10 实测）：
//! - **无需 key**、**国内直连**，是 Google 不可用时的主力兜底
//! - 数据比 Google 免费端点更全：柯林斯词典、专业释义（论文库）、词源、
//!   词语辨析、词组短语、同根词、真题例句、AI 释义
//! - 用 `dicts` 参数只取需要的板块，响应从 625KB 压到 ~70KB
//! - `oxford*` / `webster` / `wordElaboration` 返回 `encryptedData`，需要
//!   逆向 JS 才能解，**本驱动不取**
//!
//! 拼写错误读取 typos 候选；既无词典数据也无候选时才回落到「有道翻译」。

use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;
use url::Url;

use super::driver::{Driver, EngineInfo};
use super::error::TranslateError;
use super::model::*;
use super::youdao_trans::{self, YoudaoTransClient};

pub const ID: &str = "youdao";
const ENDPOINT: &str = "https://dict.youdao.com/jsonapi";
const REFERER: &str = "https://dict.youdao.com/";

/// 只请求这些板块。`count` 必须不小于列表长度，否则后面的会被丢掉。
const DICTS: &str = r#"{"count":19,"dicts":[["ec"],["ce"],["jec"],["fec"],["kec"],["phrs"],["syno"],["ee"],["special"],["web_trans"],["blng_sents_part"],["auth_sents_part"],["collins_primary"],["rel_word"],["etym"],["discriminate"],["aiDefinition"],["expand_ec"],["typos"]]}"#;

/// 一次查询最多取多少条例句。
const MAX_EXAMPLES: usize = 15;
/// 词组短语上限。
const MAX_PHRASES: usize = 80;
/// 单个板块内每组最多几条。
const MAX_PER_GROUP: usize = 8;

// ---------------------------------------------------------------------------
// 取值助手
// ---------------------------------------------------------------------------

/// JSON Pointer 取值。
///
/// 路径可以省略开头的 `/`：`serde_json::Value::pointer` 对不带 `/` 的路径
/// 一律返回 `None`（不会报错），少写一个斜杠就会静默丢字段，所以这里统一补齐。
fn ptr<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() || path.starts_with('/') {
        root.pointer(path)
    } else {
        root.pointer(&format!("/{path}"))
    }
}

fn p<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    ptr(root, path)
}

fn ps<'a>(root: &'a Value, path: &str) -> Option<&'a str> {
    p(root, path).and_then(|v| v.as_str())
}

fn pa<'a>(root: &'a Value, path: &str) -> &'a [Value] {
    arr(p(root, path))
}

fn arr(v: Option<&Value>) -> &[Value] {
    v.and_then(|x| x.as_array()).map(|x| x.as_slice()).unwrap_or(&[])
}

fn s_of(v: Option<&Value>) -> Option<String> {
    v.and_then(|x| x.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

/// 去掉 HTML 标签，只保留 `<b>`，并还原常见实体。
///
/// 有道的例句/出处里会混 `<i>`、`<span class="...">` 等标签。
fn clean(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut tag = String::new();
    let mut in_tag = false;
    for ch in input.chars() {
        match ch {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag.trim().to_ascii_lowercase();
                if t == "b" {
                    out.push_str("<b>");
                } else if t == "/b" {
                    out.push_str("</b>");
                }
            }
            _ if in_tag => tag.push(ch),
            _ => out.push(ch),
        }
    }
    out.replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

/// `l.i` 可能是字符串、字符串数组，元素还可能是带 `#text` 的对象。
fn collect_i(v: Option<&Value>) -> Vec<String> {
    fn one(v: &Value) -> Option<String> {
        match v {
            Value::String(s) => {
                let t = s.trim();
                (!t.is_empty()).then(|| t.to_string())
            }
            Value::Object(o) => o
                .get("#text")
                .and_then(|t| t.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string),
            _ => None,
        }
    }
    match v {
        Some(Value::Array(items)) => items.iter().filter_map(one).collect(),
        Some(other) => one(other).into_iter().collect(),
        None => Vec::new(),
    }
}

/// 把 `"n. 强迫，胁迫；高压政治"` 拆成 `("n.", "强迫，胁迫；高压政治")`。
///
/// 没有词性前缀时返回空词性，避免把 `"the act of ..."` 的首词误当词性。
fn split_pos(line: &str) -> (String, String) {
    let t = line.trim();
    let mut end = 0usize;
    for (i, ch) in t.char_indices() {
        if ch.is_ascii_alphabetic() || ch == '.' {
            end = i + ch.len_utf8();
        } else {
            break;
        }
    }
    let head = &t[..end];
    if head.ends_with('.') && head.chars().any(|c| c.is_ascii_alphabetic()) {
        (head.to_string(), t[end..].trim().to_string())
    } else {
        (String::new(), t.to_string())
    }
}

/// 按中英常见分隔符拆译词。
fn split_terms(s: &str) -> Vec<String> {
    s.split(['；', ';', '，', ',', '、'])
        .map(str::trim)
        .filter(|x| !x.is_empty())
        .map(str::to_string)
        .collect()
}

/// 有道发音音频地址。`uk=true` 取英音，否则取美音。
fn voice_url(word: &str, uk: bool) -> Option<String> {
    if word.trim().is_empty() {
        return None;
    }
    let mut url = Url::parse("https://dict.youdao.com/dictvoice").ok()?;
    url.query_pairs_mut()
        .append_pair("audio", word)
        .append_pair("type", if uk { "1" } else { "2" });
    Some(url.to_string())
}

// ---------------------------------------------------------------------------
// 解析
// ---------------------------------------------------------------------------

/// 把 jsonapi 响应映射成 [`TranslateResult`]。
///
/// 返回 `None` 表示这次查询既无词典数据，也无拼写候选（通常是整句），
/// 调用方应回落到翻译接口。
pub fn parse(raw: &Value, query: &str, target_lang: &str) -> Option<TranslateResult> {
    let (dictionary, exams) = parse_bilingual(raw);
    let mut dictionary = dictionary;
    for entry in dictionary.iter_mut() {
        entry.exams = exams.clone();
    }

    let synonyms = parse_synonyms(raw);
    let definitions = parse_definitions(raw);
    let examples = parse_examples(raw);
    let phrases = parse_phrases(raw);
    let related = parse_related(raw);
    let sections = parse_sections(raw);
    let suggestions = parse_suggestions(raw, query);

    let has_any = !dictionary.is_empty()
        || !synonyms.is_empty()
        || !definitions.is_empty()
        || !examples.is_empty()
        || !phrases.is_empty()
        || !related.is_empty()
        || !sections.is_empty();
    if !has_any && suggestions.is_empty() {
        return None;
    }

    // 译文取第一个词性的第一个译词，与有道网页端的做法一致。
    let translation = dictionary
        .first()
        .and_then(|d| d.terms.first())
        .cloned()
        .unwrap_or_default();

    let phonetic = ps(raw, "/simple/word/0/usphone")
        .or_else(|| ps(raw, "/ec/word/0/usphone"))
        .map(str::to_string);
    let phonetic_uk = ps(raw, "/simple/word/0/ukphone")
        .or_else(|| ps(raw, "/ec/word/0/ukphone"))
        .map(str::to_string);

    // meta.guessLanguage 是识别出的源语言；le/lang 是词典方向，不能当源语言。
    let source_lang = ps(raw, "/meta/guessLanguage")
        .or_else(|| ps(raw, "/le"))
        .map(normalize_lang)
        .unwrap_or_else(|| "auto".into());

    let segments = if translation.is_empty() {
        Vec::new()
    } else {
        vec![Segment { text: translation.clone(), source: query.to_string() }]
    };

    Some(TranslateResult {
        query: query.to_string(),
        source_lang,
        target_lang: target_lang.to_string(),
        engine: ID.to_string(),
        translation,
        segments,
        romanization: None,
        phonetic,
        phonetic_uk,
        speak_url: if has_any { voice_url(query, false) } else { None },
        target_speak_url: None,
        dictionary,
        alternatives: Vec::new(),
        suggestions,
        synonyms,
        definitions,
        examples,
        phrases,
        related,
        sections,
        is_word: has_any,
        cached: false,
        elapsed_ms: 0,
    })
}

fn parse_suggestions(raw: &Value, query: &str) -> Vec<QuerySuggestion> {
    let mut out: Vec<QuerySuggestion> = Vec::new();
    for typo in pa(raw, "/typos/typo") {
        let Some(word) = s_of(typo.get("word")) else { continue };
        if word.eq_ignore_ascii_case(query.trim())
            || out.iter().any(|s| s.word.eq_ignore_ascii_case(&word))
        {
            continue;
        }
        out.push(QuerySuggestion {
            word,
            translation: s_of(typo.get("trans")),
        });
        if out.len() >= MAX_PER_GROUP { break; }
    }
    out
}

fn normalize_lang(code: &str) -> String {
    match code {
        "zh" | "chn" => "zh-CN".into(),
        "eng" => "en".into(),
        other => other.to_ascii_lowercase(),
    }
}

/// `ec` / `ce` / `jec` … ：词性 + 译词。
fn parse_bilingual(root: &Value) -> (Vec<DictEntry>, Vec<String>) {
    const KEYS: [&str; 6] = ["ec", "ce", "jec", "fec", "kec", "dec"];
    let mut out: Vec<DictEntry> = Vec::new();
    let mut exams: Vec<String> = Vec::new();

    for key in KEYS {
        let node = match root.get(key) {
            Some(n) => n,
            None => continue,
        };
        if exams.is_empty() {
            exams = arr(node.get("exam_type"))
                .iter()
                .filter_map(|v| v.as_str())
                .map(str::to_string)
                .collect();
        }
        for word in arr(node.get("word")) {
            for trs in arr(word.get("trs")) {
                for tr in arr(trs.get("tr")) {
                    let l = match tr.get("l") {
                        Some(l) => l,
                        None => continue,
                    };
                    let pos_field = l
                        .get("pos")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .trim()
                        .to_string();
                    let items = collect_i(l.get("i"));

                    if pos_field.is_empty() {
                        // ec 风格：每行自带 "n. " 前缀
                        for line in items {
                            let (pos, rest) = split_pos(&line);
                            let terms = split_terms(&rest);
                            if !terms.is_empty() {
                                push_entry(&mut out, if pos.is_empty() { "-" } else { &pos }, terms);
                            }
                        }
                    } else {
                        // ce 风格：词性是独立字段，i 里已经是纯译词
                        let terms: Vec<String> =
                            items.iter().flat_map(|s| split_terms(s)).collect();
                        if !terms.is_empty() {
                            push_entry(&mut out, &pos_field, terms);
                        }
                    }
                }
            }
        }
        if !out.is_empty() {
            break;
        }
    }
    (out, exams)
}

fn push_entry(out: &mut Vec<DictEntry>, pos: &str, terms: Vec<String>) {
    if let Some(existing) = out.iter_mut().find(|e| e.pos == pos) {
        for t in terms {
            if !existing.terms.contains(&t) {
                existing.terms.push(t);
            }
        }
    } else {
        out.push(DictEntry {
            pos: pos.to_string(),
            terms,
            back: Vec::new(),
            exams: Vec::new(),
        });
    }
}

/// `syno`：同义词。
fn parse_synonyms(root: &Value) -> Vec<SynonymGroup> {
    let mut out = Vec::new();
    for group in pa(root, "/syno/synos") {
        let node = match group.get("syno") {
            Some(n) => n,
            None => continue,
        };
        let words: Vec<String> = arr(node.get("ws"))
            .iter()
            .filter_map(|w| s_of(w.get("w")))
            .collect();
        if words.is_empty() {
            continue;
        }
        out.push(SynonymGroup {
            pos: s_of(node.get("pos")).unwrap_or_default(),
            words,
            gloss: s_of(node.get("tran")),
        });
    }
    out
}

/// `ee`：WordNet 英英释义。
fn parse_definitions(root: &Value) -> Vec<DefinitionGroup> {
    let source = s_of(p(root, "/ee/source/name"));
    let mut out = Vec::new();
    for trs in pa(root, "/ee/word/trs") {
        let mut definitions = Vec::new();
        for tr in arr(trs.get("tr")) {
            let text = match collect_i(tr.pointer("/l/i")).into_iter().next() {
                Some(t) => clean(&t),
                None => continue,
            };
            let example = tr
                .pointer("/exam/i/f/l/0/i")
                .and_then(|v| v.as_str())
                .map(|s| clean(s));
            let similar = arr(tr.get("similar-words"))
                .iter()
                .filter_map(|s| s_of(s.get("similar")))
                .collect();
            definitions.push(Definition {
                text,
                example,
                similar,
            });
        }
        if !definitions.is_empty() {
            out.push(DefinitionGroup {
                pos: s_of(trs.get("pos")).unwrap_or_default(),
                definitions,
                source: source.clone(),
            });
        }
    }
    out
}

fn push_example(out: &mut Vec<Example>, mut item: Example) {
    // 有道同一句会以「带尾随空格」和「不带」两种形态各返回一次，先归一化再查重。
    item.text = item.text.trim().to_string();
    if out.len() >= MAX_EXAMPLES || item.text.is_empty() {
        return;
    }
    if out.iter().any(|e| e.text == item.text) {
        return;
    }
    out.push(item);
}

/// 例句：双语例句 + 真题例句 + 权威例句。
fn parse_examples(root: &Value) -> Vec<Example> {
    let mut out = Vec::new();

    for pair in pa(root, "/blng_sents_part/sentence-pair") {
        let text = ps(pair, "sentence-eng").or_else(|| ps(pair, "sentence"));
        if let Some(t) = text {
            push_example(
                &mut out,
                Example {
                    text: clean(t),
                    translation: ps(pair, "sentence-translation").map(clean),
                    source: None,
                },
            );
        }
    }

    for sent in pa(root, "/expand_ec/word/0/transList/0/content/sents") {
        if let Some(t) = ps(sent, "sentOrig") {
            push_example(
                &mut out,
                Example {
                    text: clean(t),
                    translation: ps(sent, "sentTrans").map(clean),
                    source: ps(sent, "source").map(clean),
                },
            );
        }
    }

    for sent in pa(root, "/auth_sents_part/sent") {
        if let Some(t) = ps(sent, "foreign") {
            push_example(
                &mut out,
                Example {
                    text: clean(t),
                    translation: None,
                    source: ps(sent, "source").map(clean),
                },
            );
        }
    }

    out
}

/// `phrs`：词组短语。
fn parse_phrases(root: &Value) -> Vec<Phrase> {
    let mut out = Vec::new();
    for item in pa(root, "/phrs/phrs") {
        let phr = match item.get("phr") {
            Some(x) => x,
            None => continue,
        };
        let term = match ps(phr, "/headword/l/i") {
            Some(t) => clean(t),
            None => continue,
        };
        let translation = arr(phr.get("trs"))
            .iter()
            .filter_map(|t| ps(t, "/tr/l/i"))
            .map(clean)
            .collect::<Vec<_>>()
            .join("；");
        if translation.is_empty() {
            continue;
        }
        out.push(Phrase {
            term,
            translation,
            source: ps(phr, "source").map(clean).filter(|s| !s.is_empty()),
        });
        if out.len() >= MAX_PHRASES {
            break;
        }
    }
    out
}

/// `rel_word`：同根词 / 词形变化。
fn parse_related(root: &Value) -> Vec<RelatedGroup> {
    let mut out = Vec::new();
    for group in pa(root, "/rel_word/rels") {
        let rel = match group.get("rel") {
            Some(r) => r,
            None => continue,
        };
        let words: Vec<RelatedWord> = arr(rel.get("words"))
            .iter()
            .filter_map(|w| {
                let word = s_of(w.get("word"))?;
                Some(RelatedWord {
                    word,
                    gloss: s_of(w.get("tran")).map(|g| g.trim_start().to_string()),
                })
            })
            .collect();
        if !words.is_empty() {
            out.push(RelatedGroup {
                pos: s_of(rel.get("pos")).unwrap_or_default(),
                words,
            });
        }
    }
    out
}

/// 其余板块：柯林斯 / 专业释义 / 网络释义 / 词源 / 词语辨析 / AI 释义。
fn parse_sections(root: &Value) -> Vec<Section> {
    let mut out = Vec::new();
    out.extend(parse_collins(root));
    out.extend(parse_special(root));
    out.extend(parse_web_trans(root));
    out.extend(parse_discriminate(root));
    out.extend(parse_etym(root));
    out.extend(parse_ai(root));
    out
}

/// `collins_primary`：柯林斯星级词典（未加密，结构最干净）。
fn parse_collins(root: &Value) -> Option<Section> {
    let mut groups = Vec::new();
    for cat in pa(root, "/collins_primary/gramcat") {
        let mut items = Vec::new();
        for sense in arr(cat.get("senses")) {
            let text = match s_of(sense.get("definition")) {
                Some(t) => t,
                None => continue,
            };
            let extra: Vec<Example> = arr(sense.get("examples"))
                .iter()
                .filter_map(|e| {
                    let t = s_of(e.get("example"))?;
                    Some(Example {
                        text: clean(&t),
                        translation: s_of(e.pointer("/sense/word")).map(|s| clean(&s)),
                        source: None,
                    })
                })
                .take(2)
                .collect();
            items.push(SectionItem {
                text,
                translation: s_of(sense.get("word")).map(|s| clean(&s)),
                note: s_of(sense.get("sensenumber")),
                extra,
            });
            if items.len() >= MAX_PER_GROUP * 2 {
                break;
            }
        }
        if !items.is_empty() {
            groups.push(SectionGroup {
                label: s_of(cat.get("pronunciation")).unwrap_or_default(),
                note: None,
                items,
            });
        }
    }
    (!groups.is_empty()).then(|| Section {
        key: "collins".into(),
        title: "柯林斯".into(),
        note: Some("柯林斯英汉双解大词典".into()),
        markdown: false,
        groups,
    })
}

/// `special`：专业释义，来自论文库，带学科分类与引用数。
fn parse_special(root: &Value) -> Option<Section> {
    let mut groups = Vec::new();
    for item in pa(root, "/special/entries").iter().take(6) {
        let entry = match item.get("entry") {
            Some(e) => e,
            None => continue,
        };
        let mut items = Vec::new();
        for trs in arr(entry.get("trs")) {
            let tr = match trs.get("tr") {
                Some(t) => t,
                None => continue,
            };
            let text = match s_of(tr.get("nat")) {
                Some(t) => t,
                None => continue,
            };
            let extra = match (s_of(tr.get("engSent")), s_of(tr.get("chnSent"))) {
                (Some(en), zh) => vec![Example {
                    text: clean(&en),
                    translation: zh.map(|s| clean(&s)),
                    source: None,
                }],
                (None, Some(zh)) => vec![Example {
                    text: clean(&zh),
                    translation: None,
                    source: None,
                }],
                _ => Vec::new(),
            };
            items.push(SectionItem {
                text,
                translation: None,
                note: s_of(tr.get("docTitle")).map(|s| {
                    let cite = s_of(tr.get("cite")).unwrap_or_default();
                    if cite.is_empty() {
                        clean(&s)
                    } else {
                        format!("{}（引用 {}）", clean(&s), cite)
                    }
                }),
                extra,
            });
            if items.len() >= MAX_PER_GROUP {
                break;
            }
        }
        if !items.is_empty() {
            groups.push(SectionGroup {
                label: s_of(entry.get("major")).unwrap_or_default(),
                note: None,
                items,
            });
        }
    }
    (!groups.is_empty()).then(|| Section {
        key: "special".into(),
        title: "专业释义".into(),
        note: ps(root, "/special/summary/text").map(clean),
        markdown: false,
        groups,
    })
}

/// `web_trans`：网络释义，来源是网页语料。
fn parse_web_trans(root: &Value) -> Option<Section> {
    let mut groups = Vec::new();
    for item in pa(root, "/web_trans/web-translation").iter().take(8) {
        let key = match s_of(item.get("key")) {
            Some(k) => k,
            None => continue,
        };
        let mut items = Vec::new();
        for t in arr(item.get("trans")).iter().take(6) {
            let value = match s_of(t.get("value")) {
                Some(v) => v,
                None => continue,
            };
            let extra: Vec<Example> = arr(t.pointer("/summary/line"))
                .iter()
                .filter_map(|l| l.as_str())
                .take(1)
                .map(|l| Example {
                    text: clean(l),
                    translation: None,
                    source: None,
                })
                .collect();
            items.push(SectionItem {
                text: value,
                translation: None,
                note: t.get("support").and_then(|v| v.as_i64()).map(|n| format!("{n} 次引用")),
                extra,
            });
        }
        if !items.is_empty() {
            groups.push(SectionGroup {
                label: key,
                note: None,
                items,
            });
        }
    }
    (!groups.is_empty()).then(|| Section {
        key: "web".into(),
        title: "网络释义".into(),
        note: Some("来自网页语料，仅供参考".into()),
        markdown: false,
        groups,
    })
}

/// `discriminate`：近义词辨析。
fn parse_discriminate(root: &Value) -> Option<Section> {
    let mut groups = Vec::new();
    for item in pa(root, "/discriminate/data").iter().take(4) {
        let label = s_of(item.get("tran")).unwrap_or_default();
        let items: Vec<SectionItem> = arr(item.get("usages"))
            .iter()
            .filter_map(|u| {
                let head = s_of(u.get("headword"))?;
                Some(SectionItem {
                    text: head,
                    translation: s_of(u.get("usage")).map(|s| clean(&s)),
                    note: None,
                    extra: Vec::new(),
                })
            })
            .collect();
        if !items.is_empty() {
            groups.push(SectionGroup {
                label,
                note: ps(item, "source").map(clean),
                items,
            });
        }
    }
    (!groups.is_empty()).then(|| Section {
        key: "discriminate".into(),
        title: "词语辨析".into(),
        note: None,
        markdown: false,
        groups,
    })
}

/// `etym`：词源。
fn parse_etym(root: &Value) -> Option<Section> {
    let items: Vec<SectionItem> = pa(root, "/etym/etyms/zh")
        .iter()
        .filter_map(|e| {
            let text = s_of(e.get("value"))?;
            Some(SectionItem {
                text: clean(&text),
                translation: None,
                note: s_of(e.get("source")),
                extra: Vec::new(),
            })
        })
        .collect();
    (!items.is_empty()).then(|| Section {
        key: "etym".into(),
        title: "词源".into(),
        note: None,
        markdown: false,
        groups: vec![SectionGroup {
            label: String::new(),
            note: None,
            items,
        }],
    })
}

/// `aiDefinition`：AI 释义，正文是 Markdown。
fn parse_ai(root: &Value) -> Option<Section> {
    let items: Vec<SectionItem> = pa(root, "/aiDefinition/words")
        .iter()
        .filter_map(|w| {
            let content = s_of(w.get("content"))?;
            Some(SectionItem {
                text: clean(&content),
                translation: None,
                note: None,
                extra: Vec::new(),
            })
        })
        .collect();
    (!items.is_empty()).then(|| Section {
        key: "ai".into(),
        title: "AI 释义".into(),
        note: Some("由有道 AI 生成，仅供参考".into()),
        markdown: true,
        groups: vec![SectionGroup {
            label: String::new(),
            note: None,
            items,
        }],
    })
}

// ---------------------------------------------------------------------------
// 驱动
// ---------------------------------------------------------------------------

pub struct YoudaoDictClient {
    http: reqwest::Client,
    /// 整句兜底用的翻译客户端。
    fallback: YoudaoTransClient,
}

impl YoudaoDictClient {
    pub fn new() -> Result<Self, TranslateError> {
        Ok(Self {
            http: reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(20))
                .user_agent(youdao_trans::USER_AGENT)
                .build()?,
            fallback: YoudaoTransClient::new()?,
        })
    }

    async fn raw(&self, query: &str) -> Result<Value, TranslateError> {
        let resp = self
            .http
            .get(ENDPOINT)
            .query(&[("q", query), ("dicts", DICTS)])
            .header("Referer", REFERER)
            .send()
            .await?;
        let status = resp.status().as_u16();
        if status != 200 {
            return Err(TranslateError::Status { status });
        }
        Ok(resp.json().await?)
    }
}

pub struct YoudaoDictDriver {
    client: YoudaoDictClient,
}

impl YoudaoDictDriver {
    pub const ID: &'static str = ID;

    pub fn new() -> Result<Self, TranslateError> {
        Ok(Self {
            client: YoudaoDictClient::new()?,
        })
    }
}

#[async_trait]
impl Driver for YoudaoDictDriver {
    fn info(&self) -> EngineInfo {
        EngineInfo {
            id: ID.into(),
            name: "有道词典".into(),
            note: "国内直连；有道系自动分流：单词查词典，句段直接用翻译；词典含柯林斯、专业释义等".into(),
            needs_proxy: false,
            supports_sentence: true,
            supports_dictionary: true,
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
        let raw = self.client.raw(text).await?;

        if let Some(mut result) = parse(&raw, text, to) {
            result.elapsed_ms = started.elapsed().as_millis() as u64;
            return Ok(result);
        }

        // 整句等既无词典数据、也无拼写候选的查询才使用翻译兜底。
        let body = self.client.fallback.raw(text, from, to).await?;
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
    fn requests_spelling_suggestions_with_a_complete_section_count() {
        let requested: Value = serde_json::from_str(DICTS).unwrap();
        let sections = requested["dicts"].as_array().unwrap();
        assert_eq!(requested["count"].as_u64().unwrap() as usize, sections.len());
        assert!(sections.iter().any(|s| s == &json!(["typos"])));
    }

    #[test]
    fn real_misspelling_response_returns_choices_instead_of_translation_fallback() {
        let raw: Value = serde_json::from_str(include_str!("../../tests/fixtures/youdao-translateor.json")).unwrap();
        let r = parse(&raw, "translateor", "zh-CN").unwrap();
        assert_eq!(r.query, "translateor");
        assert_eq!(r.source_lang, "en");
        assert_eq!(r.engine, ID);
        assert_eq!(r.suggestions.iter().map(|s| s.word.as_str()).collect::<Vec<_>>(), ["translator", "translation"]);
        assert_eq!(r.suggestions[0].translation.as_deref(), Some("n. 译者；翻译器"));
        assert!(r.is_suggestion_only());
        assert!(r.translation.is_empty());
        assert!(r.segments.is_empty());
        assert!(!r.is_word);
        assert!(r.speak_url.is_none());
    }

    #[test]
    fn suggestion_parsing_skips_invalid_duplicate_and_self_candidates() {
        let raw = json!({"typos":{"typo":[{}, {"word":null}, {"word":" "}, {"word":"translateor"},
            {"word":" translator ","trans":" n. 译者 "}, {"word":"TRANSLATOR"}, {"word":"translation","trans":" "}]}});
        let r = parse(&raw, "translateor", "zh-CN").unwrap();
        assert_eq!(r.suggestions.len(), 2);
        assert_eq!(r.suggestions[0].word, "translator");
        assert_eq!(r.suggestions[0].translation.as_deref(), Some("n. 译者"));
        assert!(r.suggestions[1].translation.is_none());
        assert!(parse(&json!({"typos":{"typo":"not an array"}}), "unknown", "zh-CN").is_none());
    }

    #[test]
    fn suggestions_never_replace_a_valid_dictionary_result() {
        let mut raw = word_response();
        raw["typos"] = json!({"typo":[{"word":"coercive"}]});
        let r = parse(&raw, "coercion", "zh-CN").unwrap();
        assert!(!r.translation.is_empty());
        assert!(!r.dictionary.is_empty());
        assert!(!r.is_suggestion_only());
        assert!(r.is_word);
        assert_eq!(r.suggestions[0].word, "coercive");
    }

    #[test]
    fn historical_results_without_suggestions_still_deserialize() {
        let r = parse(&word_response(), "coercion", "zh-CN").unwrap();
        let mut value = serde_json::to_value(&r).unwrap();
        value.as_object_mut().unwrap().remove("suggestions");
        let legacy: TranslateResult = serde_json::from_value(value).unwrap();
        assert!(legacy.suggestions.is_empty());
        assert_eq!(legacy.translation, r.translation);
    }

    fn word_response() -> Value {
        json!({
            "ec": {
                "exam_type": ["CET4", "考研"],
                "word": [{
                    "usphone": "koʊˈɜːrʒn",
                    "ukphone": "kəʊˈɜːʃ(ə)n",
                    "trs": [
                        {"tr": [{"l": {"i": ["n. 强迫，胁迫；高压政治，威压"]}}]},
                        {"tr": [{"l": {"i": ["vt. 强制，迫使"]}}]}
                    ]
                }]
            },
            "simple": {"word": [{"usphone": "koʊˈɜːrʒn", "ukphone": "kəʊˈɜːʃ(ə)n"}]},
            "meta": {"guessLanguage": "en"},
            "syno": {"synos": [{"syno": {"pos": "n.", "ws": [{"w": "enforcement"}, {"w": "constraint"}], "tran": "强制；强迫"}}]},
            "ee": {
                "source": {"name": "WordNet"},
                "word": {"trs": [{"pos": "n.", "tr": [
                    {"l": {"i": "the act of compelling by force of authority"}},
                    {"l": {"i": "using force to cause something"},
                     "exam": {"i": {"f": {"l": [{"i": "they didn`t have to use coercion"}]}}},
                     "similar-words": [{"similar": "compulsion"}]}
                ]}]}
            },
            "blng_sents_part": {"sentence-pair": [
                {"sentence": "He claimed he had only acted under coercion.",
                 "sentence-eng": "He claimed he had only acted under <b>coercion</b>.",
                 "sentence-translation": "他声称他只是受了胁迫才采取了行动。"}
            ]},
            "phrs": {"phrs": [{"phr": {"headword": {"l": {"i": "coercion policy"}},
                                       "trs": [{"tr": {"l": {"i": "强制政策"}}}], "source": "21世纪"}}]},
            "rel_word": {"rels": [{"rel": {"pos": "adj.", "words": [{"word": "coercive", "tran": " 强制的；胁迫的"}]}}]},
            "collins_primary": {"gramcat": [{"pronunciation": "koʊˈɜːrʒn", "senses": [
                {"sensenumber": "1", "definition": "Coercion is the act of persuading someone.",
                 "word": "强迫；胁迫",
                 "examples": [{"example": "He was under coercion.", "sense": {"word": "他受到了胁迫。"}}]}
            ]}]},
            "special": {"summary": {"text": "以上统计来源于 2,447,534 篇论文数据"},
                        "entries": [{"entry": {"major": "法学", "trs": [
                            {"tr": {"nat": "强制", "chnSent": "国家<b>强制</b>与私法自治的界限。",
                                    "engSent": "state <b>coercion</b> and private autonomy",
                                    "cite": "126", "docTitle": "论公司法中的国家强制"}}]}}]},
            "web_trans": {"web-translation": [{"key": "coercion", "trans": [
                {"value": "强迫", "support": 350, "summary": {"line": ["所谓MICE…<b>强迫</b>（Coercion）…"]}}
            ]}]},
            "etym": {"etyms": {"zh": [{"source": "有道", "value": "源自拉丁语 coercere。"}]}},
            "discriminate": {"data": [{"tran": "抓住，握紧", "usages": [
                {"headword": "grasp", "usage": "指紧紧抓住、抓牢。"}
            ]}]},
            "aiDefinition": {"words": [{"word": "coercion", "content": "### 核心\ncoercion 表示<span class=\"highlight\">强迫</span>。"}]}
        })
    }

    #[test]
    fn parses_bilingual_entries() {
        let r = parse(&word_response(), "coercion", "zh-CN").unwrap();
        assert_eq!(r.translation, "强迫");
        assert_eq!(r.engine, "youdao");
        assert_eq!(r.source_lang, "en");
        assert_eq!(r.dictionary.len(), 2);
        assert_eq!(r.dictionary[0].pos, "n.");
        assert_eq!(r.dictionary[0].terms, vec!["强迫", "胁迫", "高压政治", "威压"]);
        assert_eq!(r.dictionary[0].exams, vec!["CET4", "考研"]);
        assert_eq!(r.dictionary[1].pos, "vt.");
        assert_eq!(r.phonetic.as_deref(), Some("koʊˈɜːrʒn"));
        assert_eq!(r.phonetic_uk.as_deref(), Some("kəʊˈɜːʃ(ə)n"));
        assert!(r.speak_url.as_deref().unwrap().contains("dictvoice"));
        assert!(r.is_word);
    }

    #[test]
    fn parses_all_sections() {
        let r = parse(&word_response(), "coercion", "zh-CN").unwrap();
        assert_eq!(r.synonyms.len(), 1);
        assert_eq!(r.synonyms[0].words, vec!["enforcement", "constraint"]);
        assert_eq!(r.synonyms[0].gloss.as_deref(), Some("强制；强迫"));

        assert_eq!(r.definitions.len(), 1);
        assert_eq!(r.definitions[0].source.as_deref(), Some("WordNet"));
        assert_eq!(r.definitions[0].definitions.len(), 2);
        assert_eq!(
            r.definitions[0].definitions[1].example.as_deref(),
            Some("they didn`t have to use coercion")
        );
        assert_eq!(r.definitions[0].definitions[1].similar, vec!["compulsion"]);

        assert_eq!(r.examples.len(), 1);
        assert!(r.examples[0].text.contains("<b>coercion</b>"));
        assert_eq!(
            r.examples[0].translation.as_deref(),
            Some("他声称他只是受了胁迫才采取了行动。")
        );

        assert_eq!(r.phrases.len(), 1);
        assert_eq!(r.phrases[0].term, "coercion policy");
        assert_eq!(r.phrases[0].translation, "强制政策");
        assert_eq!(r.phrases[0].source.as_deref(), Some("21世纪"));

        assert_eq!(r.related.len(), 1);
        assert_eq!(r.related[0].words[0].word, "coercive");
        assert_eq!(r.related[0].words[0].gloss.as_deref(), Some("强制的；胁迫的"));

        let keys: Vec<&str> = r.sections.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, vec!["collins", "special", "web", "discriminate", "etym", "ai"]);

        let collins = &r.sections[0];
        assert_eq!(collins.groups[0].items[0].text, "Coercion is the act of persuading someone.");
        assert_eq!(collins.groups[0].items[0].translation.as_deref(), Some("强迫；胁迫"));
        assert_eq!(
            collins.groups[0].items[0].extra[0].translation.as_deref(),
            Some("他受到了胁迫。")
        );

        let special = &r.sections[1];
        assert_eq!(special.groups[0].label, "法学");
        assert!(special.groups[0].items[0].extra[0].text.contains("<b>coercion</b>"));

        let ai = r.sections.iter().find(|s| s.key == "ai").unwrap();
        assert!(ai.markdown);
        // span 被剥掉，内容保留
        assert!(!ai.groups[0].items[0].text.contains("<span"));
        assert!(ai.groups[0].items[0].text.contains("强迫"));
    }

    #[test]
    fn chinese_query_uses_ce_shape() {
        let raw = json!({
            "ce": {"word": [{"trs": [
                {"tr": [{"l": {"pos": "vt.", "i": ["", {"#text": "compel"}, {"#text": "force"}]}}]}
            ]}]},
            "meta": {"guessLanguage": "zh"}
        });
        let r = parse(&raw, "强迫", "en").unwrap();
        assert_eq!(r.source_lang, "zh-CN");
        assert_eq!(r.dictionary[0].pos, "vt.");
        assert_eq!(r.dictionary[0].terms, vec!["compel", "force"]);
        assert_eq!(r.translation, "compel");
    }

    #[test]
    fn sentence_returns_none_so_caller_falls_back() {
        let raw = json!({"input": "hello world", "meta": {"guessLanguage": "en"}, "le": "en", "lang": "eng"});
        assert!(parse(&raw, "hello world", "zh-CN").is_none());
    }

    #[test]
    fn splits_pos_without_swallowing_words() {
        assert_eq!(split_pos("n. 强迫，胁迫"), ("n.".into(), "强迫，胁迫".into()));
        assert_eq!(split_pos("adj. 强制的"), ("adj.".into(), "强制的".into()));
        assert_eq!(
            split_pos("the act of compelling"),
            (String::new(), "the act of compelling".into())
        );
        assert_eq!(split_pos("强迫"), (String::new(), "强迫".into()));
    }

    #[test]
    fn strips_html_but_keeps_bold() {
        assert_eq!(clean("<i>BBC</i>: a <b>word</b>"), "BBC: a <b>word</b>");
        assert_eq!(clean("&amp;&nbsp;x"), "& x");
        assert_eq!(clean("<span class=\"highlight\">x</span>"), "x");
    }
}
