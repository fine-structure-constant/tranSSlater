//! 历史导出。四种格式，全部流式写出，不把历史读进内存。
//!
//! - **CSV**：带 UTF-8 BOM，Excel 直接双击不会乱码
//! - **Markdown**：按日期分组，适合当学习笔记
//! - **Anki TSV**：`原文 \t 译文` 两列，Anki「基本型」可直接导入
//! - **JSON**：完整结果，用于备份或二次处理

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

use chrono::{Local, TimeZone};

use super::history::{HistoryRecord, HistoryStore};
use super::translate::TranslateError;

/// 导出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Csv,
    Markdown,
    Anki,
    Json,
}

impl Format {
    pub fn from_id(id: &str) -> Result<Self, TranslateError> {
        Ok(match id {
            "csv" => Self::Csv,
            "markdown" | "md" => Self::Markdown,
            "anki" | "tsv" => Self::Anki,
            "json" => Self::Json,
            other => {
                return Err(TranslateError::Invalid(format!(
                    "不支持的导出格式：{other}"
                )));
            }
        })
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Markdown => "md",
            Self::Anki => "txt",
            Self::Json => "json",
        }
    }
}

/// 引擎 id → 展示名。未知 id 原样返回。
fn engine_name(id: &str) -> &str {
    match id {
        "google" => "Google 翻译",
        "youdao" => "有道词典",
        "youdaoTrans" => "有道翻译",
        other => other,
    }
}

fn fmt_datetime(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
        .unwrap_or_default()
}

fn fmt_date(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

fn fmt_time(ms: i64) -> String {
    Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|t| t.format("%H:%M:%S").to_string())
        .unwrap_or_default()
}

/// CSV 字段转义：含分隔符/引号/换行时用双引号包裹，内部引号翻倍。
fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// TSV 字段转义：Anki 按制表符分列，字段内的制表符与换行必须清掉。
fn tsv_field(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ").trim().to_string()
}

/// 释义摘要，形如 `n. 强迫，胁迫；vt. 强制`。
fn gloss_summary(record: &HistoryRecord) -> String {
    let Some(result) = record.result() else {
        return String::new();
    };
    result
        .dictionary
        .iter()
        .filter(|d| !d.terms.is_empty())
        .map(|d| format!("{} {}", d.pos, d.terms.join("，")))
        .collect::<Vec<_>>()
        .join("；")
}

/// 把整份历史写进 `path`，返回写出的条数。
pub fn write(store: &HistoryStore, format: Format, path: &Path) -> Result<usize, TranslateError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| TranslateError::Export(e.to_string()))?;
    }
    let file = File::create(path).map_err(|e| TranslateError::Export(e.to_string()))?;
    let mut out = BufWriter::new(file);

    let result = match format {
        Format::Csv => write_csv(store, &mut out),
        Format::Markdown => write_markdown(store, &mut out),
        Format::Anki => write_anki(store, &mut out),
        Format::Json => write_json(store, &mut out),
    };

    // 无论成功失败都尽量把缓冲刷出去，避免留下半截文件却不报错。
    let flush = out.flush().map_err(|e| TranslateError::Export(e.to_string()));
    let n = result?;
    flush?;
    Ok(n)
}

fn io(err: std::io::Error) -> TranslateError {
    TranslateError::Export(err.to_string())
}

fn write_csv<W: Write>(store: &HistoryStore, out: &mut W) -> Result<usize, TranslateError> {
    // BOM：没有它 Excel 会把中文按 GBK 解，直接乱码。
    out.write_all("\u{FEFF}".as_bytes()).map_err(io)?;
    writeln!(
        out,
        "时间,原文,译文,源语言,目标语言,引擎,类型,音标,释义,同义词"
    )
    .map_err(io)?;

    store.for_each(|record| {
        let result = record.result();
        let phonetic = result
            .as_ref()
            .and_then(|r| r.phonetic.clone())
            .unwrap_or_default();
        let synonyms = result
            .as_ref()
            .map(|r| {
                r.synonyms
                    .iter()
                    .flat_map(|g| g.words.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .unwrap_or_default();

        let line = [
            fmt_datetime(record.created_at),
            record.query.clone(),
            record.translation.clone(),
            record.from_lang.clone(),
            record.to_lang.clone(),
            engine_name(&record.engine).to_string(),
            if record.is_word { "单词" } else { "句子" }.to_string(),
            phonetic,
            gloss_summary(&record),
            synonyms,
        ]
        .iter()
        .map(|f| csv_field(f))
        .collect::<Vec<_>>()
        .join(",");

        writeln!(out, "{line}").map_err(io)
    })
}

fn write_markdown<W: Write>(store: &HistoryStore, out: &mut W) -> Result<usize, TranslateError> {
    let total = store.count(None)?;
    writeln!(out, "# 翻译历史\n").map_err(io)?;
    writeln!(
        out,
        "导出时间：{} · 共 {} 条\n",
        fmt_datetime(super::history::now_ms()),
        total
    )
    .map_err(io)?;

    let mut current_date = String::new();
    let mut count = 0usize;

    store.for_each(|record| {
        let date = fmt_date(record.created_at);
        if date != current_date {
            writeln!(out, "\n## {date}\n").map_err(io)?;
            current_date = date;
        }

        writeln!(out, "### {}", record.query.replace('\n', " ")).map_err(io)?;
        writeln!(
            out,
            "- 译文：{}",
            record.translation.replace('\n', " ")
        )
        .map_err(io)?;
        writeln!(
            out,
            "- 语言：{} → {} · 引擎：{} · {}",
            record.from_lang,
            record.to_lang,
            engine_name(&record.engine),
            fmt_time(record.created_at)
        )
        .map_err(io)?;

        if let Some(result) = record.result() {
            if let Some(phonetic) = result.phonetic.as_deref().filter(|p| !p.is_empty()) {
                writeln!(out, "- 音标：/{}/", phonetic.trim_matches('/')).map_err(io)?;
            }
            let gloss = gloss_summary(&record);
            if !gloss.is_empty() {
                writeln!(out, "- 释义：{gloss}").map_err(io)?;
            }
            for group in result.synonyms.iter().take(2) {
                if !group.words.is_empty() {
                    writeln!(out, "- 近义词：{}", group.words.join("、")).map_err(io)?;
                }
            }
            if let Some(first) = result.examples.first() {
                writeln!(out, "- 例句：{}", first.text.replace("<b>", "").replace("</b>", ""))
                    .map_err(io)?;
            }
        }
        writeln!(out).map_err(io)?;

        count += 1;
        Ok(())
    })?;

    Ok(count)
}

fn write_anki<W: Write>(store: &HistoryStore, out: &mut W) -> Result<usize, TranslateError> {
    store.for_each(|record| {
        // 两列：正面原文、背面译文。Anki「基本型」可直接导入。
        writeln!(
            out,
            "{}\t{}",
            tsv_field(&record.query),
            tsv_field(&record.translation)
        )
        .map_err(io)
    })
}

fn write_json<W: Write>(store: &HistoryStore, out: &mut W) -> Result<usize, TranslateError> {
    writeln!(out, "[").map_err(io)?;
    let mut first = true;
    let n = store.for_each(|record| {
        let entry = serde_json::json!({
            "id": record.id,
            "query": record.query,
            "fromLang": record.from_lang,
            "toLang": record.to_lang,
            "engine": record.engine,
            "engineName": engine_name(&record.engine),
            "translation": record.translation,
            "isWord": record.is_word,
            "createdAt": record.created_at,
            "exportedAt": fmt_datetime(record.created_at),
            "result": record.result(),
        });
        let text =
            serde_json::to_string(&entry).map_err(|e| TranslateError::Export(e.to_string()))?;
        if !first {
            writeln!(out, ",").map_err(io)?;
        }
        first = false;
        write!(out, "  {text}").map_err(io)
    })?;
    writeln!(out, "\n]").map_err(io)?;
    Ok(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::translate::google::parse as parse_google;
    use crate::translate::TranslateResult;
    use serde_json::json;

    fn sample(query: &str, translation: &str, lang: &str) -> TranslateResult {
        let raw = json!([[[translation, query]], null, lang]);
        parse_google(&raw, query, "zh-CN").unwrap()
    }

    fn seed() -> HistoryStore {
        let s = HistoryStore::open_in_memory().unwrap();
        s.add(&sample("coercion", "强迫", "en"), 1_760_000_000_000).unwrap();
        s.add(&sample("你好，世界", "Hello, world", "zh-CN"), 1_760_000_060_000)
            .unwrap();
        s
    }

    fn render(format: Format) -> String {
        let store = seed();
        let dir = std::env::temp_dir().join(format!("translater-export-{}", std::process::id()));
        let path = dir.join(format!("out.{}", format.extension()));
        let n = write(&store, format, &path).unwrap();
        assert_eq!(n, 2);
        let text = std::fs::read_to_string(&path).unwrap();
        std::fs::remove_dir_all(&dir).ok();
        text
    }

    #[test]
    fn csv_has_bom_quotes_and_all_rows() {
        let text = render(Format::Csv);
        assert!(text.starts_with('\u{FEFF}'), "CSV 必须带 BOM 才能让 Excel 正确识别中文");
        assert!(text.contains("时间,原文,译文"));
        assert!(text.contains("coercion"));
        // 全角逗号不是 CSV 分隔符，不必加引号；英文逗号必须被引号包裹。
        assert!(text.contains("你好，世界"), "原文里的全角逗号应原样保留");
        assert!(text.contains("\"Hello, world\""), "含英文逗号的字段必须加引号");
        assert_eq!(text.lines().count(), 3);
    }

    #[test]
    fn markdown_groups_by_date_and_lists_fields() {
        let text = render(Format::Markdown);
        assert!(text.starts_with("# 翻译历史"));
        assert!(text.contains("共 2 条"));
        assert!(text.contains("### coercion"));
        assert!(text.contains("- 译文：强迫"));
        assert!(text.contains("引擎：Google 翻译"));
    }

    #[test]
    fn anki_is_two_tab_separated_columns() {
        let text = render(Format::Anki);
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        for line in &lines {
            assert_eq!(line.matches('\t').count(), 1, "Anki 必须是两列");
        }
        assert!(lines[0].starts_with("coercion\t"));
        assert!(!text.contains('\n') || !text.contains("\t\n"));
    }

    #[test]
    fn json_is_valid_and_keeps_full_result() {
        let text = render(Format::Json);
        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap();
        let arr = parsed.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["query"], "coercion");
        assert_eq!(arr[0]["result"]["translation"], "强迫");
        assert_eq!(arr[0]["engineName"], "Google 翻译");
    }

    #[test]
    fn unknown_format_is_rejected() {
        assert!(Format::from_id("pdf").is_err());
        assert_eq!(Format::from_id("md").unwrap(), Format::Markdown);
        assert_eq!(Format::from_id("anki").unwrap(), Format::Anki);
    }

    #[test]
    fn engine_names_are_human_readable() {
        assert_eq!(engine_name("youdao"), "有道词典");
        assert_eq!(engine_name("youdaoTrans"), "有道翻译");
        assert_eq!(engine_name("unknown-x"), "unknown-x");
    }

    #[test]
    fn csv_field_escapes_quotes_and_newlines() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("line1\nline2"), "\"line1\nline2\"");
        assert_eq!(csv_field("plain"), "plain");
    }
}
