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

/// 皮肤清单。新增皮肤要同时改 src/theme.css 与 src/settings.ts。
pub const THEMES: &[&str] = &["plain", "meridian"];

/// 字号档位。具体倍率写在 src/theme.css 的 `[data-font]` 里。
pub const FONT_SCALES: &[&str] = &["sm", "md", "lg", "xl"];

/// 窗口尺寸边界，与 tauri.conf.json 的 minWidth / minHeight 对齐。
pub const WINDOW_MIN: (f64, f64) = (720.0, 480.0);
pub const WINDOW_MAX: (f64, f64) = (3840.0, 2160.0);

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
    /// 界面皮肤 id，对应 `<html data-theme>`
    pub theme: String,
    /// 字号档位，对应 `<html data-font>`
    pub font: String,
    /// 启动时的窗口宽度（逻辑像素）
    pub window_width: f64,
    /// 启动时的窗口高度（逻辑像素）
    pub window_height: f64,
    /// 是否在用户手动缩放窗口后覆盖上面的默认尺寸
    pub remember_window: bool,
    /// 划词翻译总开关：划选文字后是否弹出悬浮图标
    pub select_translate: bool,
    /// UI Automation 读不到选区时，是否允许退到「模拟 Ctrl+C 读剪贴板」
    pub clipboard_fallback: bool,
    /// 是否在资源管理器右键菜单里注册「翻译（tranSSlater）」
    pub shell_menu: bool,
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
            theme: "plain".into(),
            font: "md".into(),
            window_width: 1020.0,
            window_height: 760.0,
            remember_window: false,
            select_translate: true,
            clipboard_fallback: true,
            shell_menu: true,
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
        if !THEMES.contains(&self.theme.as_str()) {
            return Err(TranslateError::Invalid(format!("不支持的皮肤：{}", self.theme)));
        }
        if !FONT_SCALES.contains(&self.font.as_str()) {
            return Err(TranslateError::Invalid(format!("不支持的字号：{}", self.font)));
        }
        if !self.window_size_in_range() {
            return Err(TranslateError::Invalid(format!(
                "窗口尺寸超出允许范围（{}×{} 到 {}×{}）",
                WINDOW_MIN.0, WINDOW_MIN.1, WINDOW_MAX.0, WINDOW_MAX.1
            )));
        }
        Ok(())
    }

    fn window_size_in_range(&self) -> bool {
        let ok = |v: f64, min: f64, max: f64| v.is_finite() && (min..=max).contains(&v);
        ok(self.window_width, WINDOW_MIN.0, WINDOW_MAX.0)
            && ok(self.window_height, WINDOW_MIN.1, WINDOW_MAX.1)
    }

    /// 启动时用的窗口尺寸（逻辑像素）。越界时收敛到边界，宁可开小也不要开在屏幕外。
    pub fn window_size(&self) -> (f64, f64) {
        let clamp = |v: f64, min: f64, max: f64| if v.is_finite() { v.clamp(min, max) } else { min };
        (
            clamp(self.window_width, WINDOW_MIN.0, WINDOW_MAX.0),
            clamp(self.window_height, WINDOW_MIN.1, WINDOW_MAX.1),
        )
    }

    /// 字号档位对应的倍率。必须与 `src/theme.css` 里 `[data-font]` 的 `--fs` 一致，
    /// 划词悬浮图标会用它做阻尼缩放。
    pub fn font_scale(&self) -> f64 {
        font_scale(&self.font)
    }
}

/// 字号 id → 倍率。未登记的值退回 1.0，不 panic。
pub fn font_scale(font: &str) -> f64 {
    match font {
        "sm" => 0.875,
        "lg" => 1.125,
        "xl" => 1.25,
        _ => 1.0,
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
        assert_eq!(s.theme, "plain");
        assert_eq!(s.font, "md");
        assert_eq!(s.window_size(), (1020.0, 760.0));
        assert!(!s.remember_window);
        assert!(s.select_translate, "划词翻译默认开启");
        assert!(s.clipboard_fallback, "剪贴板兜底默认开启");
        assert!(s.shell_menu, "资源管理器右键菜单默认注册");
        assert_eq!(s.font_scale(), 1.0);
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
                ..s.clone()
            }
            .validate(&engines())
            .is_err()
        );
        assert!(
            Settings {
                theme: "neon".into(),
                ..s.clone()
            }
            .validate(&engines())
            .is_err(),
            "未登记的皮肤要拒绝，否则界面会退回没有令牌的裸样式"
        );
        assert!(
            Settings {
                font: "huge".into(),
                ..s.clone()
            }
            .validate(&engines())
            .is_err()
        );
        assert!(
            Settings {
                window_width: 200.0,
                ..s.clone()
            }
            .validate(&engines())
            .is_err(),
            "窗口比最小尺寸还窄会导致布局挤成一团"
        );
        assert!(
            Settings {
                window_height: 10_000.0,
                ..s.clone()
            }
            .validate(&engines())
            .is_err()
        );
    }

    #[test]
    fn window_size_clamps_instead_of_panicking() {
        let s = Settings {
            window_width: -5.0,
            window_height: f64::NAN,
            ..Settings::default()
        };
        assert_eq!(s.window_size(), WINDOW_MIN);
    }

    #[test]
    fn every_registered_theme_and_font_passes_validation() {
        for theme in THEMES {
            for font in FONT_SCALES {
                let s = Settings {
                    theme: (*theme).into(),
                    font: (*font).into(),
                    ..Settings::default()
                };
                assert!(s.validate(&engines()).is_ok(), "{theme}/{font} 应被接受");
            }
        }
    }

    #[test]
    fn every_font_scale_maps_to_a_usable_multiplier() {
        // 这些值必须与 src/theme.css 里 :root[data-font=...] 的 --fs 一致
        for (id, scale) in [("sm", 0.875), ("md", 1.0), ("lg", 1.125), ("xl", 1.25)] {
            let s = Settings {
                font: id.into(),
                ..Settings::default()
            };
            assert_eq!(s.font_scale(), scale, "{id} 的倍率与 theme.css 不一致");
        }
        // 未登记的值不该出现，但真出现了也不能 panic
        assert_eq!(
            Settings {
                font: "?".into(),
                ..Settings::default()
            }
            .font_scale(),
            1.0
        );
    }

    #[test]
    fn old_settings_file_without_new_fields_still_loads() {
        // 兼容旧版本写下的 settings.json：缺 engine / historyMode / theme 等字段时用默认值
        let s: Settings =
            serde_json::from_str(r#"{"proxy":"","autoTranslate":true,"autoSpeak":false,"langPair":"en|zh-CN"}"#)
                .unwrap();
        assert_eq!(s.engine, "google");
        assert_eq!(s.history_mode, "all");
        assert_eq!(s.theme, "plain");
        assert_eq!(s.font, "md");
        assert!(s.validate(&engines()).is_ok());
    }

    #[test]
    fn file_roundtrip_and_rejection_preserves_valid_settings() {
        let folder =
            std::env::temp_dir().join(format!("transslater-settings-test-{}", std::process::id()));
        let path = folder.join("settings.json");
        let mut s = load(&path, &engines()).unwrap();
        s.auto_speak = true;
        s.engine = "youdao".into();
        s.theme = "meridian".into();
        s.font = "lg".into();
        s.window_width = 1280.0;
        s.window_height = 820.0;
        s.remember_window = true;
        s.select_translate = false;
        s.clipboard_fallback = false;
        s.shell_menu = false;
        save(&path, &s, &engines()).unwrap();
        let reloaded = load(&path, &engines()).unwrap();
        assert!(reloaded.auto_speak);
        assert_eq!(reloaded.engine, "youdao");
        assert_eq!(reloaded.theme, "meridian");
        assert_eq!(reloaded.font, "lg");
        assert_eq!(reloaded.window_size(), (1280.0, 820.0));
        assert!(reloaded.remember_window);
        assert!(!reloaded.select_translate);
        assert!(!reloaded.clipboard_fallback);
        assert!(!reloaded.shell_menu);
        s.proxy = "invalid".into();
        assert!(save(&path, &s, &engines()).is_err());
        assert_eq!(load(&path, &engines()).unwrap().proxy, "");
        fs::write(&path, "broken").unwrap();
        assert!(load(&path, &engines()).is_err());
        fs::remove_file(&path).unwrap();
        fs::remove_dir(&folder).unwrap();
    }
}
