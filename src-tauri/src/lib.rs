//! Tauri 命令层：把 `translate` 模块暴露给前端。

mod settings;
pub mod translate;

use std::sync::{Arc, Mutex, RwLock};

use serde::Serialize;
use tauri::Manager;
use translate::{CacheKey, GoogleClient, TranslateCache, TranslateError, TranslateResult};

/// 全局状态。由 Tauri 托管，命令里通过 `tauri::State` 取用。
struct AppState {
    /// 用 `RwLock<Arc<..>>` 是为了能在 `set_proxy` 里热替换客户端，
    /// 同时避免把锁持有到 `.await` 之后。
    client: RwLock<Arc<GoogleClient>>,
    cache: Mutex<TranslateCache>,
}

impl AppState {
    fn new(proxy: Option<&str>) -> Result<Self, TranslateError> {
        Ok(Self {
            client: RwLock::new(Arc::new(GoogleClient::new(proxy)?)),
            cache: Mutex::new(TranslateCache::default()),
        })
    }

    fn client(&self) -> Arc<GoogleClient> {
        Arc::clone(&self.client.read().expect("client 锁中毒"))
    }
}

/// 缓存统计，用于界面上显示。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheStats {
    size: usize,
}

/// 翻译入口。`from` 省略时自动检测；`to` 省略时译为简体中文。
#[tauri::command]
async fn translate(
    state: tauri::State<'_, AppState>,
    text: String,
    from: Option<String>,
    to: Option<String>,
) -> Result<TranslateResult, TranslateError> {
    let from = from.unwrap_or_else(|| "auto".to_string());
    let to = to.unwrap_or_else(|| "zh-CN".to_string());
    let text = text.trim();
    let key = CacheKey::new(text, &from, &to);

    // 命中缓存直接返回。注意锁的作用域要在 .await 之前结束。
    if let Some(hit) = state.cache.lock().expect("cache 锁中毒").get(&key) {
        return Ok(hit);
    }

    let result = state.client().translate(text, &from, &to).await?;

    state
        .cache
        .lock()
        .expect("cache 锁中毒")
        .put(key, result.clone());

    Ok(result)
}

/// 设置 HTTP/SOCKS5 代理。传 `None` 或空串表示改用环境变量。
#[tauri::command]
fn set_proxy(
    state: tauri::State<'_, AppState>,
    proxy: Option<String>,
) -> Result<(), TranslateError> {
    let client = GoogleClient::new(proxy.as_deref())?;
    *state.client.write().expect("client 锁中毒") = Arc::new(client);
    Ok(())
}

#[tauri::command]
fn cache_stats(state: tauri::State<'_, AppState>) -> CacheStats {
    CacheStats {
        size: state.cache.lock().expect("cache 锁中毒").len(),
    }
}

#[tauri::command]
fn clear_cache(state: tauri::State<'_, AppState>) {
    state.cache.lock().expect("cache 锁中毒").clear();
}

fn settings_path(app: &tauri::AppHandle) -> Result<std::path::PathBuf, TranslateError> {
    app.path()
        .app_config_dir()
        .map(|p| p.join("settings.json"))
        .map_err(|e| TranslateError::Settings(e.to_string()))
}

#[tauri::command]
fn load_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
) -> Result<settings::Settings, TranslateError> {
    let settings = settings::load(&settings_path(&app)?)?;
    set_proxy(state, Some(settings.proxy.clone()))?;
    Ok(settings)
}

#[tauri::command]
fn save_settings(
    app: tauri::AppHandle,
    state: tauri::State<'_, AppState>,
    settings: settings::Settings,
) -> Result<(), TranslateError> {
    settings.validate()?;
    let client = GoogleClient::new(Some(&settings.proxy))?;
    settings::save(&settings_path(&app)?, &settings)?;
    *state.client.write().expect("client 锁中毒") = Arc::new(client);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let state = AppState::new(None).expect("初始化 Google 客户端失败");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            translate,
            set_proxy,
            cache_stats,
            clear_cache,
            load_settings,
            save_settings
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
