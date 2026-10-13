//! Tauri 命令层：把翻译驱动、缓存与历史库暴露给前端。

mod export;
mod history;
mod settings;
pub mod translate;

use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use serde::Serialize;
use tauri::Manager;
use translate::{CacheKey, EngineHub, EngineInfo, TranslateCache, TranslateError, TranslateResult};

use export::Format;
use history::{HistoryStore, HistorySummary};

/// 运行期可变配置。改设置时整体替换，读的时候克隆出来，避免跨 `.await` 持锁。
#[derive(Clone)]
struct Runtime {
    engine: String,
    record_history: bool,
}

/// 全局状态。由 Tauri 托管，命令里通过 `tauri::State` 取用。
struct AppState {
    engines: EngineHub,
    runtime: RwLock<Runtime>,
    cache: Mutex<TranslateCache>,
    history: Mutex<HistoryStore>,
    settings_path: PathBuf,
    history_path: PathBuf,
    config_dir: PathBuf,
    data_dir: PathBuf,
}

impl AppState {
    /// 建状态，并把读到的设置一并返回：调用方还要拿它去定窗口尺寸。
    fn new(app: &tauri::AppHandle) -> Result<(Self, settings::Settings), Box<dyn std::error::Error>> {
        let config_dir = app.path().app_config_dir()?;
        let data_dir = app.path().app_data_dir()?;
        let settings_path = config_dir.join("settings.json");
        let history_path = data_dir.join("history.db");

        let engines = EngineHub::new(None)?;
        let valid: Vec<String> = engines.list().into_iter().map(|e| e.id).collect();

        // 设置文件损坏不该阻止应用启动，退回默认值即可。
        let loaded = settings::load(&settings_path, &valid).unwrap_or_default();
        engines.set_proxy(Some(&loaded.proxy))?;

        let history = HistoryStore::open(&history_path)?;

        Ok((
            Self {
                engines,
                runtime: RwLock::new(Runtime {
                    engine: loaded.engine.clone(),
                    record_history: loaded.history_mode == "all",
                }),
                cache: Mutex::new(TranslateCache::default()),
                history: Mutex::new(history),
                settings_path,
                history_path,
                config_dir,
                data_dir,
            },
            loaded,
        ))
    }

    fn runtime(&self) -> Runtime {
        self.runtime.read().expect("runtime 锁中毒").clone()
    }

    fn engine_ids(&self) -> Vec<String> {
        self.engines.list().into_iter().map(|e| e.id).collect()
    }
}

/// 启动时把窗口调整到设置里保存的尺寸。窗口找不到或调用失败都不该阻止启动。
fn apply_startup_window_size(app: &tauri::AppHandle, settings: &settings::Settings) {
    let (width, height) = settings.window_size();
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if let Err(err) = window.set_size(tauri::LogicalSize::new(width, height)) {
        eprintln!("调整窗口尺寸失败：{err}");
    }
}

/// 缓存统计，用于界面上显示。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CacheStats {
    size: usize,
}

/// 历史分页结果。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryPage {
    items: Vec<HistorySummary>,
    total: i64,
}

/// 历史库概况。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct HistoryStats {
    total: i64,
    size_bytes: u64,
}

/// 可用引擎列表。
#[tauri::command]
fn engines(state: tauri::State<'_, AppState>) -> Vec<EngineInfo> {
    state.engines.list()
}

/// 翻译入口。`from` 省略时自动检测；`to` 省略时译为简体中文；
/// `engine` 省略时用设置里选定的引擎。
#[tauri::command]
async fn translate(
    state: tauri::State<'_, AppState>,
    text: String,
    from: Option<String>,
    to: Option<String>,
    engine: Option<String>,
) -> Result<TranslateResult, TranslateError> {
    let runtime = state.runtime();
    let from = from.unwrap_or_else(|| "auto".to_string());
    let to = to.unwrap_or_else(|| "zh-CN".to_string());
    let engine_id = engine.unwrap_or(runtime.engine);
    let text = text.trim().to_string();

    let driver = state.engines.get(&engine_id)?;
    let key = CacheKey::new(&text, &from, &to, &engine_id);

    // 命中缓存直接返回。锁的作用域必须在 `.await` 之前结束。
    if let Some(hit) = state.cache.lock().expect("cache 锁中毒").get(&key) {
        return Ok(hit);
    }

    let result = driver.translate(&text, &from, &to).await?;

    // 只有真正发过请求（未命中缓存）才记历史，避免同一会话里刷出重复条目。
    if runtime.record_history {
        if let Ok(store) = state.history.lock() {
            if let Err(err) = store.add(&result, history::now_ms()) {
                eprintln!("写入历史失败：{err}");
            }
        }
    }

    state
        .cache
        .lock()
        .expect("cache 锁中毒")
        .put(key, result.clone());

    Ok(result)
}

/// 设置 HTTP/SOCKS5 代理。只影响需要翻墙的引擎。
#[tauri::command]
fn set_proxy(state: tauri::State<'_, AppState>, proxy: Option<String>) -> Result<(), TranslateError> {
    state.engines.set_proxy(proxy.as_deref())
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

// ── 窗口与路径 ──────────────────────────────────────────────

/// 应用元信息：版本号与各类数据文件的落盘位置，供「说明」对话框展示。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: String,
    settings_path: String,
    history_path: String,
    config_dir: String,
    data_dir: String,
}

/// 把窗口调整到指定逻辑尺寸。越界的值收敛到边界，避免窗口跑到屏幕外。
#[tauri::command]
fn resize_window(
    window: tauri::Window,
    width: f64,
    height: f64,
) -> Result<(), TranslateError> {
    let clamp = |v: f64, min: f64, max: f64| {
        if v.is_finite() {
            v.clamp(min, max)
        } else {
            min
        }
    };
    let width = clamp(width, settings::WINDOW_MIN.0, settings::WINDOW_MAX.0);
    let height = clamp(height, settings::WINDOW_MIN.1, settings::WINDOW_MAX.1);
    window
        .set_size(tauri::LogicalSize::new(width, height))
        .map_err(|e| TranslateError::Settings(e.to_string()))
}

/// 放大 / 还原窗口。菜单栏双击走这里。
#[tauri::command]
fn toggle_maximize(window: tauri::Window) -> Result<bool, TranslateError> {
    let maximized = window
        .is_maximized()
        .map_err(|e| TranslateError::Settings(e.to_string()))?;
    let result = if maximized {
        window.unmaximize()
    } else {
        window.maximize()
    };
    result
        .map_err(|e| TranslateError::Settings(e.to_string()))
        .map(|_| !maximized)
}

#[tauri::command]
fn app_info(app: tauri::AppHandle, state: tauri::State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        settings_path: state.settings_path.display().to_string(),
        history_path: state.history_path.display().to_string(),
        config_dir: state.config_dir.display().to_string(),
        data_dir: state.data_dir.display().to_string(),
    }
}

#[tauri::command]
fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn load_settings(state: tauri::State<'_, AppState>) -> Result<settings::Settings, TranslateError> {
    let valid = state.engine_ids();
    let loaded = settings::load(&state.settings_path, &valid)?;
    apply_settings(&state, &loaded)?;
    Ok(loaded)
}

#[tauri::command]
fn save_settings(
    state: tauri::State<'_, AppState>,
    settings: settings::Settings,
) -> Result<(), TranslateError> {
    let valid = state.engine_ids();
    // 先验证、再落盘、最后才切换运行时，任一步失败都不会让内存状态与文件不一致。
    settings.validate(&valid)?;
    settings::save(&state.settings_path, &settings, &valid)?;
    apply_settings(&state, &settings)
}

/// 把设置应用到运行时：代理热替换 + 引擎/历史策略更新。
fn apply_settings(state: &AppState, s: &settings::Settings) -> Result<(), TranslateError> {
    state.engines.set_proxy(Some(&s.proxy))?;
    *state.runtime.write().expect("runtime 锁中毒") = Runtime {
        engine: s.engine.clone(),
        record_history: s.history_mode == "all",
    };
    Ok(())
}

// ── 历史 ────────────────────────────────────────────────────

#[tauri::command]
fn history_list(
    state: tauri::State<'_, AppState>,
    offset: i64,
    limit: i64,
    filter: Option<String>,
) -> Result<HistoryPage, TranslateError> {
    let store = state.history.lock().expect("history 锁中毒");
    let filter = filter.as_deref();
    Ok(HistoryPage {
        items: store.list(offset, limit.clamp(1, 500), filter)?,
        total: store.count(filter)?,
    })
}

#[tauri::command]
fn history_celebration_words(
    state: tauri::State<'_, AppState>,
) -> Result<Vec<String>, TranslateError> {
    state
        .history
        .lock()
        .expect("history 锁中毒")
        .celebration_words()
}

#[tauri::command]
fn history_get(
    state: tauri::State<'_, AppState>,
    id: i64,
) -> Result<Option<TranslateResult>, TranslateError> {
    state.history.lock().expect("history 锁中毒").get(id)
}

#[tauri::command]
fn history_delete(state: tauri::State<'_, AppState>, id: i64) -> Result<bool, TranslateError> {
    state.history.lock().expect("history 锁中毒").delete(id)
}

#[tauri::command]
fn history_clear(state: tauri::State<'_, AppState>) -> Result<usize, TranslateError> {
    state.history.lock().expect("history 锁中毒").clear()
}

#[tauri::command]
fn history_stats(state: tauri::State<'_, AppState>) -> Result<HistoryStats, TranslateError> {
    let store = state.history.lock().expect("history 锁中毒");
    Ok(HistoryStats {
        total: store.count(None)?,
        size_bytes: store.size_bytes(),
    })
}

/// 导出历史到 `path`，返回写出的条数。
///
/// 用户在保存对话框里只填了文件名时补上扩展名，避免产出一个没有后缀的文件。
#[tauri::command]
fn export_history(
    state: tauri::State<'_, AppState>,
    format: String,
    path: String,
) -> Result<usize, TranslateError> {
    let format = Format::from_id(&format)?;
    let mut target = std::path::PathBuf::from(&path);
    if target.extension().is_none() {
        target.set_extension(format.extension());
    }
    let store = state.history.lock().expect("history 锁中毒");
    export::write(&store, format, &target)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let (state, loaded) = AppState::new(app.handle())?;
            apply_startup_window_size(app.handle(), &loaded);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            engines,
            translate,
            set_proxy,
            cache_stats,
            clear_cache,
            resize_window,
            toggle_maximize,
            app_info,
            quit_app,
            load_settings,
            save_settings,
            history_list,
            history_celebration_words,
            history_get,
            history_delete,
            history_clear,
            history_stats,
            export_history
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
