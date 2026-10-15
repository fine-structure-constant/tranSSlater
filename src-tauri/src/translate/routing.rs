//! Local, allocation-free routing before cache lookup and network requests.
//! The existing auto -> Chinese setting now means Chinese/English auto-switching.

use super::{youdao, youdao_trans};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LanguageHint {
    Chinese,
    English,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Route<'a> {
    pub from: &'a str,
    pub to: &'a str,
    pub engine: &'a str,
}

fn is_han(c: char) -> bool {
    matches!(c as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff | 0x20000..=0x323af)
}

/// Kana, Hangul and other alphabets take precedence over Han in mixed input.
/// Han-only Japanese cannot be distinguished from Chinese without a language model.
pub fn language_hint(text: &str) -> LanguageHint {
    let mut han = false;
    let mut latin = false;
    for c in text.chars() {
        if is_han(c) {
            han = true;
        } else if c.is_ascii_alphabetic() {
            latin = true;
        } else if c.is_alphabetic() {
            return LanguageHint::Other;
        }
    }
    if han {
        LanguageHint::Chinese
    } else if latin {
        LanguageHint::English
    } else {
        LanguageHint::Other
    }
}

/// Multi-token English, punctuation and long Chinese strings bypass the dictionary.
/// Ambiguous short Chinese entries can use the dictionary driver's existing fallback.
fn dictionary_candidate(text: &str, language: LanguageHint) -> bool {
    let text = text.trim();
    match language {
        LanguageHint::Chinese => {
            let mut count = 0;
            for c in text.chars() {
                if !is_han(c) {
                    return false;
                }
                count += 1;
                if count > 4 {
                    return false;
                }
            }
            count > 0
        }
        LanguageHint::English => {
            let mut count = 0;
            let mut previous_letter = false;
            for c in text.chars() {
                count += 1;
                if count > 64 {
                    return false;
                }
                if c.is_ascii_alphabetic() {
                    previous_letter = true;
                } else if matches!(c, '\'' | '’' | '-') && previous_letter {
                    previous_letter = false;
                } else {
                    return false;
                }
            }
            previous_letter
        }
        LanguageHint::Other => false,
    }
}

pub fn resolve<'a>(text: &str, from: &'a str, to: &'a str, engine: &'a str) -> Route<'a> {
    let hint = language_hint(text);
    let to = if from == "auto" && to == "zh-CN" && hint == LanguageHint::Chinese {
        "en"
    } else {
        to
    };
    let is_youdao = matches!(engine, youdao::ID | youdao_trans::ID);
    let dictionary_pair = match hint {
        LanguageHint::Chinese => matches!(from, "auto" | "zh" | "zh-CN") && to == "en",
        LanguageHint::English => matches!(from, "auto" | "en") && matches!(to, "zh" | "zh-CN"),
        LanguageHint::Other => false,
    };
    let engine = if is_youdao {
        if dictionary_pair && dictionary_candidate(text, hint) {
            youdao::ID
        } else {
            youdao_trans::ID
        }
    } else {
        engine
    };
    Route { from, to, engine }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_target_switches_chinese_but_preserves_upstream_source_detection() {
        for text in ["你好", "这是一段中文。", "用 Rust 开发桌面应用", "𠮷"] {
            let r = resolve(text, "auto", "zh-CN", "google");
            assert_eq!((r.from, r.to, r.engine), ("auto", "en", "google"), "{text}");
        }
        for text in [
            "hello",
            "This is English.",
            "こんにちは",
            "日本語です",
            "한국어",
            "Привет",
            "café",
            "123",
            "🙂",
        ] {
            assert_eq!(
                resolve(text, "auto", "zh-CN", "google").to,
                "zh-CN",
                "{text}"
            );
        }
    }

    #[test]
    fn either_youdao_selection_uses_dictionary_for_words() {
        for selected in [youdao::ID, youdao_trans::ID] {
            for text in [
                "hello",
                "don't",
                "mother-in-law",
                "can’t",
                " 强迫 ",
                "你好",
                "苹果公司",
            ] {
                assert_eq!(
                    resolve(text, "auto", "zh-CN", selected).engine,
                    youdao::ID,
                    "{selected}: {text}"
                );
            }
        }
    }

    #[test]
    fn sentences_and_non_bilingual_queries_skip_dictionary_probe() {
        for selected in [youdao::ID, youdao_trans::ID] {
            for text in [
                "hello world",
                "Hello!",
                "hello\nworld",
                "这是很长的一段话",
                "你好。",
                "こんにちは",
                "한국어",
                "123",
                "https://example.com",
                "hello--world",
                "-hello",
            ] {
                assert_eq!(
                    resolve(text, "auto", "zh-CN", selected).engine,
                    youdao_trans::ID,
                    "{selected}: {text}"
                );
            }
            assert_eq!(
                resolve("强迫", "zh-CN", "ja", selected).engine,
                youdao_trans::ID
            );
            assert_eq!(
                resolve("hello", "en", "en", selected).engine,
                youdao_trans::ID
            );
            assert_eq!(
                resolve(&"a".repeat(65), "auto", "zh-CN", selected).engine,
                youdao_trans::ID
            );
        }
    }

    #[test]
    fn explicit_language_pairs_and_non_youdao_engines_are_preserved() {
        assert_eq!(
            resolve("中文", "zh-CN", "fr", "google"),
            Route {
                from: "zh-CN",
                to: "fr",
                engine: "google"
            }
        );
        assert_eq!(resolve("中文", "auto", "ja", "google").to, "ja");
        assert_eq!(
            resolve("word", "auto", "zh-CN", "unknown").engine,
            "unknown"
        );
        assert_eq!(resolve("中文", "zh-CN", "zh-CN", "youdao").to, "zh-CN");
    }

    #[test]
    fn youdao_family_routes_share_cache_keys_for_the_same_effective_request() {
        use super::super::CacheKey;
        for text in ["hello", "中文", "This is a paragraph."] {
            let a = resolve(text, "auto", "zh-CN", youdao::ID);
            let b = resolve(text, "auto", "zh-CN", youdao_trans::ID);
            assert_eq!(
                CacheKey::new(text, a.from, a.to, a.engine),
                CacheKey::new(text, b.from, b.to, b.engine)
            );
        }
        let automatic = resolve("中文", "auto", "zh-CN", youdao::ID);
        let manual = resolve("中文", "zh-CN", "zh-CN", youdao::ID);
        assert_ne!(
            CacheKey::new("中文", automatic.from, automatic.to, automatic.engine),
            CacheKey::new("中文", manual.from, manual.to, manual.engine)
        );
    }
}
