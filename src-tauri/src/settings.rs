//! 用户设置存放在应用配置目录，先验证并写入成功，再替换运行时客户端。
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub proxy: String,
    pub auto_translate: bool,
    pub auto_speak: bool,
    pub lang_pair: String,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            proxy: String::new(),
            auto_translate: true,
            auto_speak: false,
            lang_pair: "auto|zh-CN".into(),
        }
    }
}

impl Settings {
    pub fn validate(&self) -> Result<(), TranslateError> {
        validate_proxy(&self.proxy)?;
        if !LANG_PAIRS.contains(&self.lang_pair.as_str()) {
            return Err(TranslateError::Invalid("不支持的语言对".into()));
        }
        Ok(())
    }
}

pub fn load(path: &Path) -> Result<Settings, TranslateError> {
    match fs::read_to_string(path) {
        Ok(raw) => {
            let settings: Settings = serde_json::from_str(&raw).map_err(|_| {
                TranslateError::Settings("settings.json 格式有误，请修复或移走后重新保存".into())
            })?;
            settings.validate()?;
            Ok(settings)
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(err) => Err(TranslateError::Settings(err.to_string())),
    }
}

pub fn save(path: &Path, settings: &Settings) -> Result<(), TranslateError> {
    settings.validate()?;
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

    #[test]
    fn defaults_and_validation() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.lang_pair, "auto|zh-CN");
        assert!(s.auto_translate);
        assert!(s.validate().is_ok());
        assert!(
            Settings {
                lang_pair: "auto|auto".into(),
                ..s.clone()
            }
            .validate()
            .is_err()
        );
        assert!(
            Settings {
                proxy: "bad".into(),
                ..s
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn file_roundtrip_and_rejection_preserves_valid_settings() {
        let folder =
            std::env::temp_dir().join(format!("translater-settings-test-{}", std::process::id()));
        let path = folder.join("settings.json");
        let mut s = load(&path).unwrap();
        s.auto_speak = true;
        save(&path, &s).unwrap();
        assert!(load(&path).unwrap().auto_speak);
        s.proxy = "invalid".into();
        assert!(save(&path, &s).is_err());
        assert_eq!(load(&path).unwrap().proxy, "");
        fs::write(&path, "broken").unwrap();
        assert!(load(&path).is_err());
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&folder).unwrap();
    }
}
