//! 剪贴板兜底。
//!
//! UI Automation 读不到选区、或 PDF 提供者的字符偏移不可靠时走这里。它要模拟 `Ctrl+C`，副作用比只读的
//! UI Automation 大得多。流程是「快照 → 清空 → Ctrl+C → 轮询读回 → 还原」。
//!
//! 两个关键设计：
//! 1. **发 Ctrl+C 之前先把剪贴板清空**。这样如果目标程序根本不响应 Ctrl+C（没有
//!    选区、或者不支持复制），我们读到的就是空，能准确判断「没取到词」，而不是把
//!    用户上一次复制的内容误当成选区拿去翻译。
//! 2. **还原尽量完整**。所有以内存句柄承载的格式（文本、HTML、RTF、文件列表、
//!    DIB 位图……）都按原样存回来；只有真正无法用内存还原的 GDI 句柄类格式
//!    （CF_BITMAP / CF_METAFILEPICT / CF_PALETTE / CF_ENHMETAFILE 等）会丢。

use std::time::{Duration, Instant};

use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, OpenClipboard,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock,
};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, SendInput, VIRTUAL_KEY, VK_C, VK_CONTROL,
};
use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;

/// 等待目标程序把选区写进剪贴板的最长时间。
const COPY_TIMEOUT: Duration = Duration::from_millis(500);
const POLL_INTERVAL: Duration = Duration::from_millis(20);

// 无法用「内存块」还原的剪贴板格式：它们承载的是 GDI 句柄或由拥有者窗口绘制。
const CF_BITMAP: u32 = 2;
const CF_METAFILEPICT: u32 = 3;
const CF_PALETTE: u32 = 9;
const CF_ENHMETAFILE: u32 = 14;
const CF_OWNERDISPLAY: u32 = 0x0080;
const CF_DSPTEXT: u32 = 0x0081;
const CF_DSPBITMAP: u32 = 0x0082;
const CF_DSPMETAFILEPICT: u32 = 0x0083;
const CF_DSPENHMETAFILE: u32 = 0x008E;

fn is_restorable(format: u32) -> bool {
    !matches!(
        format,
        CF_BITMAP
            | CF_METAFILEPICT
            | CF_PALETTE
            | CF_ENHMETAFILE
            | CF_OWNERDISPLAY
            | CF_DSPTEXT
            | CF_DSPBITMAP
            | CF_DSPMETAFILEPICT
            | CF_DSPENHMETAFILE
    )
}

/// 剪贴板快照：格式 id → 原始字节。
struct Snapshot(Vec<(u32, Vec<u8>)>);

/// 用 Ctrl+C 取当前选中的文字，并把剪贴板还原回去。
///
/// 取不到（没有选区、目标程序不响应复制、前台是提权窗口被 UIPI 拦下）时返回 `None`。
pub fn read_selection_via_copy(source_window: HWND) -> Option<String> {
    if source_window.is_invalid() || unsafe { GetForegroundWindow() } != source_window {
        return None;
    }
    let saved = unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }
        let snapshot = snapshot();
        // 清空是为了区分「复制成功但内容是空」和「压根没复制」
        let _ = EmptyClipboard();
        let _ = CloseClipboard();
        snapshot
    };

    let mut text = None;
    if unsafe { GetForegroundWindow() } == source_window && send_ctrl_c() {
        text = poll_text();
    }

    unsafe {
        if OpenClipboard(None).is_ok() {
            restore(&saved);
            let _ = CloseClipboard();
        }
    }

    if unsafe { GetForegroundWindow() } == source_window { text } else { None }
}

/// 调用方必须已经打开剪贴板。
unsafe fn snapshot() -> Snapshot {
    let mut items: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut format = 0u32;
    loop {
        format = unsafe { EnumClipboardFormats(format) };
        if format == 0 {
            break;
        }
        if !is_restorable(format) {
            continue;
        }
        let Ok(handle) = (unsafe { GetClipboardData(format) }) else {
            continue;
        };
        if handle.is_invalid() {
            continue;
        }
        let hglobal = HGLOBAL(handle.0);
        let size = unsafe { GlobalSize(hglobal) };
        if size == 0 {
            continue;
        }
        let ptr = unsafe { GlobalLock(hglobal) };
        if ptr.is_null() {
            continue;
        }
        let bytes = unsafe { std::slice::from_raw_parts(ptr as *const u8, size) }.to_vec();
        let _ = unsafe { GlobalUnlock(hglobal) };
        items.push((format, bytes));
    }
    Snapshot(items)
}

/// 调用方必须已经打开剪贴板。
unsafe fn restore(saved: &Snapshot) {
    let _ = unsafe { EmptyClipboard() };
    for (format, bytes) in &saved.0 {
        let Ok(handle) = (unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }) else {
            continue;
        };
        let ptr = unsafe { GlobalLock(handle) };
        if ptr.is_null() {
            let _ = unsafe { GlobalFree(Some(handle)) };
            continue;
        }
        unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr as *mut u8, bytes.len()) };
        let _ = unsafe { GlobalUnlock(handle) };

        // 成功之后内存所有权归系统，只有失败才需要我们释放
        if unsafe { SetClipboardData(*format, Some(HANDLE(handle.0))) }.is_err() {
            let _ = unsafe { GlobalFree(Some(handle)) };
        }
    }
}

fn send_ctrl_c() -> bool {
    unsafe {
        // 用户自己可能正按着 Ctrl，那就别去动这个键，免得松开时打断他的组合键
        let ctrl_held = (GetAsyncKeyState(VK_CONTROL.0 as i32) as u16) & 0x8000 != 0;

        let mut inputs: Vec<INPUT> = Vec::with_capacity(4);
        if !ctrl_held {
            inputs.push(key_event(VK_CONTROL, false));
        }
        inputs.push(key_event(VK_C, false));
        inputs.push(key_event(VK_C, true));
        if !ctrl_held {
            inputs.push(key_event(VK_CONTROL, true));
        }

        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) as usize == inputs.len()
    }
}

fn key_event(vk: VIRTUAL_KEY, release: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if release {
                    KEYEVENTF_KEYUP
                } else {
                    KEYBD_EVENT_FLAGS(0)
                },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

fn poll_text() -> Option<String> {
    let deadline = Instant::now() + COPY_TIMEOUT;
    loop {
        if let Some(text) = read_text() {
            let trimmed = text.trim();
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
        if Instant::now() >= deadline {
            return None;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
}

fn read_text() -> Option<String> {
    unsafe {
        if OpenClipboard(None).is_err() {
            return None;
        }
        let text = (|| {
            let handle = GetClipboardData(CF_UNICODETEXT.0 as u32).ok()?;
            if handle.is_invalid() {
                return None;
            }
            let hglobal = HGLOBAL(handle.0);
            let ptr = GlobalLock(hglobal);
            if ptr.is_null() {
                return None;
            }
            let value = read_wide(ptr as *const u16);
            let _ = GlobalUnlock(hglobal);
            Some(value)
        })();
        let _ = CloseClipboard();
        text
    }
}

/// 读一个以 0 结尾的 UTF-16 字符串。
unsafe fn read_wide(ptr: *const u16) -> String {
    let mut len = 0usize;
    while unsafe { *ptr.add(len) } != 0 {
        len += 1;
        // 剪贴板文本的合理上限，防止指针有问题时一路读下去
        if len >= 1 << 22 {
            break;
        }
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(ptr, len) })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_and_rich_formats_are_restorable() {
        assert!(is_restorable(1)); // CF_TEXT
        assert!(is_restorable(13)); // CF_UNICODETEXT
        assert!(is_restorable(15)); // CF_HDROP
        assert!(is_restorable(0xC000)); // 注册格式（Chrome 的 CF_HTML 就是这一类）
    }

    #[test]
    fn gdi_handle_formats_are_not_restorable() {
        for format in [
            CF_BITMAP,
            CF_METAFILEPICT,
            CF_PALETTE,
            CF_ENHMETAFILE,
            CF_OWNERDISPLAY,
            CF_DSPTEXT,
            CF_DSPBITMAP,
            CF_DSPMETAFILEPICT,
            CF_DSPENHMETAFILE,
        ] {
            assert!(!is_restorable(format), "格式 {format} 不该被当成可还原");
        }
    }

    #[test]
    fn reads_utf16_until_nul() {
        let buf: Vec<u16> = "划词 hello".encode_utf16().chain(std::iter::once(0)).collect();
        let text = unsafe { read_wide(buf.as_ptr()) };
        assert_eq!(text, "划词 hello");
    }

    #[test]
    fn empty_string_reads_as_empty() {
        let buf: Vec<u16> = vec![0];
        assert_eq!(unsafe { read_wide(buf.as_ptr()) }, "");
    }
}
