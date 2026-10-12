//! 用户设置存放在应用配置目录，先验证并写入成功，再替换运行时状态。
use crate::translate::{TranslateError, google::validate_proxy};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

pub const LANG_PAIRS: &[&str] = &[
    "auto|zh-CN",
    "en|zh-CN",
    "zh-CN|en",
    "en|en",
    "zh-CN|ja",
    "ja|zh-CN",
    "zh-CN|ko",
    "ko|zh-CN",
    "zh-CN|fr",
    "fr|zh-CN",
    "zh-CN|de",
    "de|zh-CN",
    "ru|zh-CN",
    "zh-CN|ru",
    "es|zh-CN",
    "zh-CN|es",
];

/// 历史库保留策略。目前只有「全部保留」和「不记录」两档。
pub const HISTORY_MODES: &[&str] = &["all", "off"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub proxy: String,
    pub auto_translate: bool,
    pub auto_speak: bool,
    pub lang_pair: String,
    /// 当前使用的翻译引擎 id
    pub engine: String,
    /// 是否记录历史：`all`（默认，无上限）或 `off`
    pub history_mode: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            proxy: String::new(),
            auto_translate: true,
            auto_speak: false,
            lang_pair: "auto|zh-CN".into(),
            engine: "google".into(),
            history_mode: "all".into(),
        }
    }
}

impl Settings {
    /// 校验格式。`valid_engines` 由引擎注册表提供，避免设置里写进不存在的引擎。
    pub fn validate(&self, valid_engines: &[String]) -> Result<(), TranslateError> {
        validate_proxy(&self.proxy)?;
        if !LANG_PAIRS.contains(&self.lang_pair.as_str()) {
            return Err(TranslateError::Invalid("不支持的语言对".into()));
        }
        if !valid_engines.iter().any(|e| e == &self.engine) {
            return Err(TranslateError::Invalid(format!(
                "不支持的翻译引擎：{}",
                self.engine
            )));
        }
        if !HISTORY_MODES.contains(&self.history_mode.as_str()) {
            return Err(TranslateError::Invalid("不支持的历史记录策略".into()));
        }
        Ok(())
    }
}

pub fn load(path: &Path, valid_engines: &[String]) -> Result<Settings, TranslateError> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let settings: Settings = serde_json::from_str(&raw).map_err(|_| {
                TranslateError::Settings("settings.json 格式有误，请修复或移走后重新保存".into())
            })?;
            settings.validate(valid_engines)?;
            Ok(settings)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(err) => Err(TranslateError::Settings(err.to_string())),
    }
}

pub fn save(
    path: &Path,
    settings: &Settings,
    valid_engines: &[String],
) -> Result<(), TranslateError> {
    settings.validate(valid_engines)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| TranslateError::Settings(e.to_string()))?;
    }
    let raw =
        serde_json::to_vec_pretty(settings).map_err(|e| TranslateError::Settings(e.to_string()))?;
    fs::write(path, raw).map_err(|e| TranslateError::Settings(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn engines() -> Vec<String> {
        vec!["google".into(), "youdao".into(), "youdaoTrans".into()]
    }

    #[test]
    fn defaults_and_validation() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.lang_pair, "auto|zh-CN");
        assert_eq!(s.engine, "google");
        assert_eq!(s.history_mode, "all");
        assert!(s.auto_translate);
        assert!(s.validate(&engines()).is_ok());
        assert!(
            Settings {
                lang_pair: "auto|auto".into(),
                ..s.clone()
            }
            .validate(&engines())
            .is_err()
        );
        assert!(
            Settings {
                proxy: "bad".into(),
                ..s.clone()
            }
            .validate(&engines())
            .is_err()
        );
        assert!(
            Settings {
                engine: "baidu".into(),
                ..s.clone()
            }
            .validate(&engines())
            .is_err(),
            "不存在的引擎必须被拒绝，否则启动后无法翻译"
        );
        assert!(
            Settings {
                history_mode: "some".into(),
                ..s
            }
            .validate(&engines())
            .is_err()
        );
    }

    #[test]
    fn old_settings_file_without_new_fields_still_loads() {
        // 兼容旧版本写下的 settings.json：缺 engine / historyMode 时用默认值
        let s: Settings =
            serde_json::from_str(r#"{"proxy":"","autoTranslate":true,"autoSpeak":false,"langPair":"en|zh-CN"}"#)
                .unwrap();
        assert_eq!(s.engine, "google");
        assert_eq!(s.history_mode, "all");
        assert!(s.validate(&engines()).is_ok());
    }

    #[test]
    fn file_roundtrip_and_rejection_preserves_valid_settings() {
        let folder =
            std::env::temp_dir().join(format!("translater-settings-test-{}", std::process::id()));
        let path = folder.join("settings.json");
        let mut s = load(&path, &engines()).unwrap();
        s.auto_speak = true;
        s.engine = "youdao".into();
        save(&path, &s, &engines()).unwrap();
        let reloaded = load(&path, &engines()).unwrap();
        assert!(reloaded.auto_speak);
        assert_eq!(reloaded.engine, "youdao");
        s.proxy = "invalid".into();
        assert!(save(&path, &s, &engines()).is_err());
        assert_eq!(load(&path, &engines()).unwrap().proxy, "");
        fs::write(&path, "broken").unwrap();
        assert!(load(&path, &engines()).is_err());
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&folder).unwrap();
    }
}
