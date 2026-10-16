//! 划词悬浮图标。
//!
//! 一个无边框、置顶、不抢焦点、不进任务栏的小窗口，**只负责渲染**。点击交给鼠标
//! 钩子处理（见 `hook.rs`），因为这个窗口带 `WS_EX_NOACTIVATE`，鼠标焦点永远落不
//! 到它身上，依赖 WebView 自己的 click 事件不可靠。
//!
//! 样式上跟主界面走：皮肤名通过 `eval` 写进 `data-theme`，尺寸随显示器 DPI 和用户
//! 设置的字号缩放。

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, SetWindowLongPtrW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST,
};

pub const LABEL: &str = "bubble";

/// 图标的逻辑设计尺寸。是一枚横着的字标（「translate」+ 底边彩虹条），不是方块。
const WIDTH: f64 = 96.0;
const HEIGHT: f64 = 30.0;
/// 极端字号下的兜底下限，别缩成看不清的一条。
const MIN_WIDTH: f64 = 60.0;
const MIN_HEIGHT: f64 = 20.0;
/// 相对光标的偏移。
const GAP: f64 = 14.0;
/// 离屏幕边缘至少留这么多（物理像素）。
const EDGE_MARGIN: i32 = 4;

/// 创建（或取回）图标窗口。创建时不显示。
fn ensure(app: &AppHandle) -> Result<WebviewWindow, String> {
    if let Some(existing) = app.get_webview_window(LABEL) {
        return Ok(existing);
    }

    let window = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("bubble.html".into()))
        .title("tranSSlater 划词")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .focusable(false)
        // focusable 只约束原生窗口；WebView2 默认仍会 MoveFocus，打断中文输入法。
        .focused(false)
        .visible(false)
        .build()
        .map_err(|e| format!("悬浮图标窗口创建失败：{e}"))?;

    forbid_activation(&window);
    Ok(window)
}

/// 直接把扩展样式钉死。
///
/// Tauri 的 `focusable(false)` 已经会设 `WS_EX_NOACTIVATE`，但这是整个功能的关键
/// 前提 —— 图标一冒出来就抢走浏览器焦点的话，用户体验是灾难性的，所以这里再显式
/// 确认一次。
fn forbid_activation(window: &WebviewWindow) {
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let wanted = current as u32
            | WS_EX_NOACTIVATE.0
            | WS_EX_TOOLWINDOW.0
            | WS_EX_TOPMOST.0;
        if wanted != current as u32 {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, wanted as isize);
        }
    }
}

/// 图标尺寸。
///
/// 字号只做阻尼缩放（开方）：图标是系统级控件，跟着字号线性放大会太夸张。
///
/// 返回 `(逻辑宽, 逻辑高, 物理宽, 物理高)`。CSS 里的像素是逻辑像素，窗口尺寸要的是
/// 物理像素，两者差一个显示器 DPI 系数 —— 把物理值当 CSS 值用，圆角和彩虹条的
/// 比例会随 DPI 走样，所以这里分开算。
fn metrics(font_scale: f64, dpi_scale: f64) -> (f64, f64, i32, i32) {
    let damp = font_scale.clamp(0.5, 2.0).sqrt();
    let logical_w = (WIDTH * damp).round().max(MIN_WIDTH);
    let logical_h = (HEIGHT * damp).round().max(MIN_HEIGHT);
    let dpi = if dpi_scale.is_finite() && dpi_scale > 0.0 {
        dpi_scale
    } else {
        1.0
    };
    let physical_w = (logical_w * dpi).round().max(1.0) as i32;
    let physical_h = (logical_h * dpi).round().max(1.0) as i32;
    (logical_w, logical_h, physical_w, physical_h)
}

/// `i32::clamp` 在 `lo > hi` 时会 panic。图标比工作区还宽（极小显示器或极端缩放）
/// 就会碰到这种情况，所以自己收敛：放不下就贴左上角。
fn clamp_pos(value: i32, lo: i32, hi: i32) -> i32 {
    if lo >= hi {
        lo
    } else {
        value.clamp(lo, hi)
    }
}

/// 在光标附近显示图标，返回它在屏幕上的矩形（物理像素）。
pub fn show_at(
    app: &AppHandle,
    x: i32,
    y: i32,
    theme: &str,
    font_scale: f64,
) -> Result<(i32, i32, i32, i32), String> {
    let window = ensure(app)?;

    let (work, dpi_scale) = monitor_at(x, y);
    let (logical_w, logical_h, physical_w, physical_h) = metrics(font_scale, dpi_scale);
    let gap = (GAP * dpi_scale).round() as i32;

    let ox = clamp_pos(
        x + gap,
        work.left + EDGE_MARGIN,
        work.right - physical_w - EDGE_MARGIN,
    );
    let oy = clamp_pos(
        y + gap,
        work.top + EDGE_MARGIN,
        work.bottom - physical_h - EDGE_MARGIN,
    );

    window
        .set_size(PhysicalSize::new(physical_w as u32, physical_h as u32))
        .map_err(|e| e.to_string())?;
    window
        .set_position(PhysicalPosition::new(ox, oy))
        .map_err(|e| e.to_string())?;

    let script = format!(
        "window.__applyAppearance && window.__applyAppearance({}, {:.0}, {:.0})",
        js_string(theme),
        logical_w,
        logical_h
    );
    let _ = window.eval(script);

    window.show().map_err(|e| e.to_string())?;

    Ok((ox, oy, physical_w, physical_h))
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.hide();
    }
}

/// 彻底销毁（关闭划词功能时用，省下一个 WebView2 进程）。
pub fn destroy(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(LABEL) {
        let _ = window.destroy();
    }
}

/// 仅由划词工作线程调用；隐藏 WebView 可以加载 HTML，无需先显示一帧。
pub fn prewarm(app: &AppHandle) {
    if let Err(err) = ensure(app) {
        eprintln!("{err}");
    }
}

/// 光标所在显示器的工作区（物理像素）与该显示器的缩放系数。
fn monitor_at(x: i32, y: i32) -> (RECT, f64) {
    unsafe {
        let monitor = MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return (fallback_work_area(), 1.0);
        }

        let mut dpi_x = 96u32;
        let mut dpi_y = 96u32;
        let scale = match GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) {
            Ok(()) if dpi_x > 0 => dpi_x as f64 / 96.0,
            _ => 1.0,
        };
        (info.rcWork, scale)
    }
}

fn fallback_work_area() -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    }
}

/// 把字符串转成可以塞进 `eval` 的 JS 字面量。主题名只可能是白名单里的值，
/// 这里仍然按字符串字面量转义，不拼裸标识符。
fn js_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn js_literal_escapes_quotes_and_backslashes() {
        assert_eq!(js_string("plain"), "\"plain\"");
        assert_eq!(js_string("a\"b"), "\"a\\\"b\"");
        assert_eq!(js_string("a\\b"), "\"a\\\\b\"");
        assert_eq!(js_string("a\nb"), "\"a\\nb\"");
    }

    #[test]
    fn fallback_work_area_is_a_sane_rectangle() {
        let rect = fallback_work_area();
        assert!(rect.right > rect.left);
        assert!(rect.bottom > rect.top);
    }

    #[test]
    fn metrics_keep_css_size_and_scale_the_window_by_dpi() {
        // 100% DPI、标准字号：逻辑尺寸就是设计尺寸
        assert_eq!(metrics(1.0, 1.0), (96.0, 30.0, 96, 30));
        // 150% DPI：交给 CSS 的逻辑尺寸不变，窗口物理尺寸放大
        assert_eq!(metrics(1.0, 1.5), (96.0, 30.0, 144, 45));
        // 特大字号（1.25）只做开方阻尼，不是线性放大
        assert_eq!(metrics(1.25, 1.0), (107.0, 34.0, 107, 34));
    }

    #[test]
    fn metrics_stay_usable_at_extreme_font_scales() {
        // 越界的字号会被收敛到 0.5..2.0 再开方，所以永远落在合理区间
        let (min_w, min_h, _, _) = metrics(0.01, 1.0);
        assert!(min_w >= MIN_WIDTH && min_h >= MIN_HEIGHT);
        let (max_w, max_h, _, _) = metrics(100.0, 1.0);
        assert!(max_w <= WIDTH * 2.0 && max_h <= HEIGHT * 2.0);
    }

    #[test]
    fn metrics_tolerate_a_broken_dpi_reading() {
        assert_eq!(metrics(1.0, 0.0), (96.0, 30.0, 96, 30));
        assert_eq!(metrics(1.0, f64::NAN), (96.0, 30.0, 96, 30));
    }

    #[test]
    fn clamp_pos_does_not_panic_when_the_bubble_does_not_fit() {
        // 工作区比图标还窄：lo > hi，直接贴左上角而不是 panic
        assert_eq!(clamp_pos(500, 100, 20), 100);
        // 正常区间照常收敛
        assert_eq!(clamp_pos(150, 0, 200), 150);
        assert_eq!(clamp_pos(-5, 0, 200), 0);
        assert_eq!(clamp_pos(500, 0, 200), 200);
    }
}
