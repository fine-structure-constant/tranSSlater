//! 划词翻译。
//!
//! 数据流：全局低级鼠标钩子识别「划词」手势 → 工作线程读取选区文字（优先 UI
//! Automation，失败才回退到模拟 Ctrl+C 读剪贴板）→ 光标旁浮出小图标 → 点击
//! 图标把文字交给主窗口翻译。
//!
//! 两条硬约束：
//! 1. **不改动原文**。取词全程只读，翻译结果只出现在本程序窗口里，绝不回写。
//! 2. **钩子回调必须立刻返回**。低级钩子超时会被 Windows 摘掉，所以回调里只做
//!    位运算和一次 channel send，取词这种几十毫秒的活全丢给工作线程。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use tauri::{AppHandle, Emitter, Manager};

#[cfg(windows)]
mod bubble;
#[cfg(windows)]
mod clipboard;
#[cfg(windows)]
mod hook;
#[cfg(windows)]
mod uia;

/// 送进主窗口的选区文字上限。翻译接口本身限制 5000 个 Unicode 字符。
pub const MAX_SELECTION_CHARS: usize = 5000;

/// 工作线程收到的任务。
enum Job {
    /// 一次划词手势结束，参数是光标的物理坐标。
    Gesture(i32, i32),
    /// 收起悬浮图标。
    Hide,
    /// 用户点了悬浮图标。
    Accept,
}

struct Shared {
    app: AppHandle,
    tx: Sender<Job>,
    enabled: AtomicBool,
    clipboard_fallback: AtomicBool,
    /// 悬浮图标的屏幕矩形（物理像素），钩子线程拿它做命中判定。
    bubble_rect: Mutex<Option<(i32, i32, i32, i32)>>,
    /// 最近一次取到的选区文字，点图标时取走。
    pending: Mutex<String>,
    /// 当前皮肤名，用来让悬浮图标跟主界面一致。
    theme: Mutex<String>,
    /// 当前字号系数，用来缩放图标。
    scale: Mutex<f64>,
}

static SHARED: OnceLock<Shared> = OnceLock::new();

fn shared() -> Option<&'static Shared> {
    SHARED.get()
}

// ── 对外 API ────────────────────────────────────────────────

/// 安装钩子与工作线程。重复调用是空操作。
///
/// `enabled` 为 false 时仍然安装钩子，只是回调里直接返回 —— 这样设置里勾上
/// 「划词翻译」就能立刻生效，不用重启。
pub fn install(app: &AppHandle, enabled: bool, clipboard_fallback: bool, theme: &str, scale: f64) -> Result<(), String> {
    if SHARED.get().is_some() {
        return Ok(());
    }

    let (tx, rx) = std::sync::mpsc::channel();
    SHARED
        .set(Shared {
            app: app.clone(),
            tx,
            enabled: AtomicBool::new(enabled),
            clipboard_fallback: AtomicBool::new(clipboard_fallback),
            bubble_rect: Mutex::new(None),
            pending: Mutex::new(String::new()),
            theme: Mutex::new(theme.to_string()),
            scale: Mutex::new(scale),
        })
        .map_err(|_| "划词模块重复初始化".to_string())?;

    std::thread::Builder::new()
        .name("selection-worker".into())
        .spawn(move || worker(rx))
        .map_err(|e| format!("划词工作线程启动失败：{e}"))?;

    #[cfg(windows)]
    hook::spawn()?;

    Ok(())
}

pub fn set_enabled(on: bool) {
    if let Some(s) = shared() {
        s.enabled.store(on, Ordering::SeqCst);
        if !on {
            let _ = s.tx.send(Job::Hide);
        }
    }
}

pub fn set_clipboard_fallback(on: bool) {
    if let Some(s) = shared() {
        s.clipboard_fallback.store(on, Ordering::SeqCst);
    }
}

/// 皮肤或字号变了，悬浮图标跟着变。
pub fn set_appearance(theme: &str, scale: f64) {
    if let Some(s) = shared() {
        *s.theme.lock().unwrap_or_else(|e| e.into_inner()) = theme.to_string();
        *s.scale.lock().unwrap_or_else(|e| e.into_inner()) = scale;
    }
}

pub fn enabled() -> bool {
    shared().is_some_and(|s| s.enabled.load(Ordering::SeqCst))
}

/// 启动一小会儿之后预热悬浮图标窗口，免得第一次划词时还要等 WebView2 起来。
pub fn prewarm(app: AppHandle) {
    #[cfg(windows)]
    bubble::prewarm(app);
    #[cfg(not(windows))]
    let _ = app;
}

/// 关掉划词功能时销毁图标窗口，省下一个 WebView2 进程。
pub fn destroy_bubble(app: &AppHandle) {
    #[cfg(windows)]
    bubble::destroy(app);
    #[cfg(not(windows))]
    let _ = app;
}

/// 退出前收摊。
///
/// 顺序有讲究，每一步都在缩小「程序已经关了、钩子还在工作」的窗口：
/// 1. 先关 `enabled` —— 钩子回调第一件事就是查它，之后立刻变成空操作；
/// 2. 再把命中矩形清掉并销毁图标窗口 —— 不会有可见的 WebView 残留；
/// 3. 最后停钩子线程，线程退出时系统自动摘掉钩子。
pub fn shutdown(app: &AppHandle) {
    if let Some(s) = shared() {
        s.enabled.store(false, Ordering::SeqCst);
        *s.bubble_rect.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    #[cfg(windows)]
    {
        bubble::destroy(app);
        hook::stop();
    }

    #[cfg(not(windows))]
    let _ = app;
}

// ── 供钩子线程调用（必须是常数时间）────────────────────────

/// 光标是否落在悬浮图标上。
pub fn bubble_hit(x: i32, y: i32) -> bool {
    let Some(s) = shared() else { return false };
    let rect = *s.bubble_rect.lock().unwrap_or_else(|e| e.into_inner());
    match rect {
        Some((bx, by, bw, bh)) => x >= bx && x < bx + bw && y >= by && y < by + bh,
        None => false,
    }
}

pub fn bubble_visible() -> bool {
    shared().is_some_and(|s| {
        s.bubble_rect
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    })
}

pub fn post_gesture(x: i32, y: i32) {
    if let Some(s) = shared() {
        let _ = s.tx.send(Job::Gesture(x, y));
    }
}

pub fn post_accept() {
    if let Some(s) = shared() {
        let _ = s.tx.send(Job::Accept);
    }
}

pub fn post_hide() {
    if let Some(s) = shared() {
        let _ = s.tx.send(Job::Hide);
    }
}

// ── 工作线程 ────────────────────────────────────────────────

fn worker(rx: Receiver<Job>) {
    #[cfg(windows)]
    uia::init_com();

    for job in rx {
        let Some(s) = shared() else { continue };
        match job {
            Job::Gesture(x, y) => on_gesture(s, x, y),
            Job::Hide => hide_now(s),
            Job::Accept => on_accept(s),
        }
    }
}

fn on_gesture(s: &Shared, x: i32, y: i32) {
    if !s.enabled.load(Ordering::SeqCst) {
        return;
    }

    #[cfg(not(windows))]
    let _ = (s, x, y);

    #[cfg(windows)]
    {
        // 在本程序自己的窗口里划词没有意义
        if hook::window_belongs_to_us(x, y) {
            return;
        }

        // 松手那一刻目标程序可能还没把选区提交给 UI Automation，等一下再问
        std::thread::sleep(std::time::Duration::from_millis(45));

        let mut text = uia::read_selection(Some((x, y))).unwrap_or_default();

        if text.trim().is_empty()
            && s.clipboard_fallback.load(Ordering::SeqCst)
            && !hook::foreground_is_explorer()
        {
            text = clipboard::read_selection_via_copy().unwrap_or_default();
        }

        let text = text.trim();
        if text.is_empty() {
            hide_now(s);
            return;
        }

        *s.pending.lock().unwrap_or_else(|e| e.into_inner()) = truncate(text, MAX_SELECTION_CHARS);

        let theme = s.theme.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let scale = *s.scale.lock().unwrap_or_else(|e| e.into_inner());

        match bubble::show_at(&s.app, x, y, &theme, scale) {
            Ok(rect) => {
                *s.bubble_rect.lock().unwrap_or_else(|e| e.into_inner()) = Some(rect);
            }
            Err(_) => hide_now(s),
        }
    }
}

fn on_accept(s: &Shared) {
    let text = std::mem::take(&mut *s.pending.lock().unwrap_or_else(|e| e.into_inner()));
    hide_now(s);
    if text.is_empty() {
        return;
    }

    // 主窗口置顶并聚焦，再把文字交给前端 —— 前端已经加载完，listen 一定注册好了
    if let Some(win) = s.app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
    let _ = s.app.emit_to("main", "translate-selection", text);
}

fn hide_now(s: &Shared) {
    *s.bubble_rect.lock().unwrap_or_else(|e| e.into_inner()) = None;
    #[cfg(windows)]
    bubble::hide(&s.app);
}

/// 按字符（而不是字节）截断，避免把多字节字符切一半。
fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    text.chars().take(max_chars).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_counts_characters_not_bytes() {
        // "中文abc" 是 5 个字符，重复 2000 次共 10000 个字符
        let text = "中文abc".repeat(2000);
        let cut = truncate(&text, 10);
        assert_eq!(cut.chars().count(), 10);
        assert_eq!(cut, "中文abc中文abc");
    }

    #[test]
    fn truncate_leaves_short_text_alone() {
        assert_eq!(truncate("hello", 10), "hello");
        assert_eq!(truncate("", 10), "");
    }

    #[test]
    fn bubble_hit_is_false_before_anything_is_shown() {
        assert!(!bubble_hit(10, 10));
        assert!(!bubble_visible());
        assert!(!enabled());
    }
}
