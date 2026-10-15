//! 读取文本文件内容。
//!
//! 用途是「资源管理器右键 → 翻译（tranSSlater）」：把选中的文本文件内容读出来交给
//! 主窗口翻译。
//!
//! 编码处理顺序：BOM（UTF-8 / UTF-16LE / UTF-16BE）→ 严格 UTF-8 → GBK。
//! 最后一步是必要的：Windows 上大量老 `.txt` 是 GBK 存的，直接按 UTF-8 硬解会整篇
//! 乱码，而乱码翻译出来毫无意义。

use std::path::Path;

use encoding_rs::{GBK, UTF_16BE, UTF_16LE, UTF_8};

/// 命令行参数：`--translate-file <路径>`。
pub const ARG: &str = "--translate-file";

/// 超过这个大小就不读了。翻译接口本身只吃 5000 字符，读整本小说没有意义。
pub const MAX_FILE_BYTES: u64 = 512 * 1024;

/// 取参数值。同时支持 `--flag value` 和 `--flag=value` 两种写法。
pub fn arg_value(args: &[String], flag: &str) -> Option<String> {
    let with_equals = format!("{flag}=");
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == flag {
            return iter.next().cloned();
        }
        if let Some(rest) = arg.strip_prefix(&with_equals) {
            return Some(rest.to_string());
        }
    }
    None
}

/// 读文件并按上面的规则解码。
pub fn read(path: &Path) -> Result<String, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("读不到这个文件：{e}"))?;
    if !meta.is_file() {
        return Err("只能翻译文件，不能翻译文件夹".into());
    }
    if meta.len() == 0 {
        return Err("文件是空的".into());
    }
    if meta.len() > MAX_FILE_BYTES {
        return Err(format!(
            "文件超过 {} KB，请先截取一段再翻译",
            MAX_FILE_BYTES / 1024
        ));
    }
    let bytes = std::fs::read(path).map_err(|e| format!("读取文件失败：{e}"))?;
    decode(&bytes)
}

/// 字节 → 文本。纯函数，方便单测。
pub fn decode(bytes: &[u8]) -> Result<String, String> {
    let text = if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        UTF_8.decode(rest).0.into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        UTF_16LE.decode(rest).0.into_owned()
    } else if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        UTF_16BE.decode(rest).0.into_owned()
    } else if bytes.contains(&0) {
        // 没有 BOM 又含 NUL，基本可以断定是二进制
        return Err("这看起来是二进制文件，不是文本".into());
    } else if let Ok(utf8) = std::str::from_utf8(bytes) {
        utf8.to_string()
    } else {
        GBK.decode(bytes).0.into_owned()
    };

    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err("文件里没有可翻译的文字".into());
    }
    Ok(trimmed.chars().take(super::selection::MAX_SELECTION_CHARS).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_both_flag_forms() {
        let args: Vec<String> = ["app.exe", "--translate-file", "a.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(arg_value(&args, ARG).as_deref(), Some("a.txt"));

        let args: Vec<String> = ["app.exe", "--translate-file=b.txt"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(arg_value(&args, ARG).as_deref(), Some("b.txt"));

        let args: Vec<String> = ["app.exe"].iter().map(|s| s.to_string()).collect();
        assert_eq!(arg_value(&args, ARG), None);
    }

    #[test]
    fn missing_value_after_flag_is_none() {
        let args: Vec<String> = ["app.exe", "--translate-file"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        assert_eq!(arg_value(&args, ARG), None);
    }

    #[test]
    fn decodes_plain_utf8() {
        assert_eq!(decode("你好 hello".as_bytes()).unwrap(), "你好 hello");
    }

    #[test]
    fn strips_utf8_bom() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice("hello".as_bytes());
        assert_eq!(decode(&bytes).unwrap(), "hello");
    }

    #[test]
    fn decodes_utf16le_with_bom() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in "hi".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(decode(&bytes).unwrap(), "hi");
    }

    #[test]
    fn decodes_gbk_when_utf8_fails() {
        // 「中文」的 GBK 编码，不是合法 UTF-8
        let gbk: Vec<u8> = vec![0xD6, 0xD0, 0xCE, 0xC4];
        assert!(
            std::str::from_utf8(&gbk).is_err(),
            "前提：这串不是合法 UTF-8"
        );
        assert_eq!(decode(&gbk).unwrap(), "中文");
    }

    #[test]
    fn rejects_binary_content() {
        assert!(decode(&[0x00, 0x01, 0x02, 0x00]).is_err());
    }

    #[test]
    fn rejects_blank_content() {
        assert!(decode(b"   \r\n  ").is_err());
    }

    #[test]
    fn truncates_to_the_translation_limit() {
        let long = "字".repeat(6000);
        assert_eq!(decode(long.as_bytes()).unwrap().chars().count(), 5000);
    }

    #[test]
    fn read_reports_a_missing_file() {
        let err = read(Path::new("definitely-not-here-9d3f.txt")).unwrap_err();
        assert!(err.contains("读不到"), "实际错误：{err}");
    }
}
