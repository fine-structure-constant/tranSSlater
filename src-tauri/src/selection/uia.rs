//! UI Automation 取词。
//!
//! 这是首选路径：**纯只读**，不碰剪贴板、不模拟按键，因此不会干扰用户的复制粘贴
//! 历史，也不会像 Ctrl+C 那样在资源管理器里误触发「复制文件」。
//!
//! 读不到时返回 `None`，由调用方决定要不要退到剪贴板方案。

use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern, UIA_TextPatternId,
};

/// 往祖先链上找 TextPattern 的层数上限。Chromium 有时把 TextPattern 挂在文档节点
/// 而不是最内层元素上。
const MAX_ANCESTORS: usize = 3;

/// 在调用线程上初始化 COM（多线程套间）。重复调用是安全的。
pub fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// 读取当前选中的文字。`point` 是光标位置，用来在「焦点元素」读不到时改问
/// 「这个坐标下是什么元素」。
pub fn read_selection(point: Option<(i32, i32)>) -> Option<String> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;

        let mut roots: Vec<IUIAutomationElement> = Vec::new();
        if let Ok(el) = automation.GetFocusedElement() {
            roots.push(el);
        }
        if let Some((x, y)) = point {
            if let Ok(el) = automation.ElementFromPoint(POINT { x, y }) {
                roots.push(el);
            }
        }

        let walker = automation.ControlViewWalker().ok();

        for root in roots {
            if let Some(text) = text_from(&root) {
                return Some(text);
            }
            // 往祖先链上再找几层
            if let Some(walker) = &walker {
                let mut current = root;
                for _ in 0..MAX_ANCESTORS {
                    match walker.GetParentElement(&current) {
                        Ok(parent) => {
                            if let Some(text) = text_from(&parent) {
                                return Some(text);
                            }
                            current = parent;
                        }
                        Err(_) => break,
                    }
                }
            }
        }

        None
    }
}

/// 从单个元素上取选区文字。元素不支持 TextPattern 或没有选中内容时返回 `None`。
unsafe fn text_from(element: &IUIAutomationElement) -> Option<String> {
    let pattern: IUIAutomationTextPattern = unsafe {
        element
            .GetCurrentPatternAs(UIA_TextPatternId)
            .ok()?
    };
    let ranges = unsafe { pattern.GetSelection().ok()? };
    let count = unsafe { ranges.Length().ok()? };
    if count <= 0 {
        return None;
    }

    let mut out = String::new();
    for i in 0..count {
        let Ok(range) = (unsafe { ranges.GetElement(i) }) else {
            continue;
        };
        let Ok(text) = (unsafe { range.GetText(-1) }) else {
            continue;
        };
        if !out.is_empty() {
            out.push('\n');
        }
        out.push_str(&text.to_string());
    }

    let cleaned = normalize(&out);
    if cleaned.is_empty() { None } else { Some(cleaned) }
}

/// UI Automation 用 `\r` 分行，输入框里更希望是 `\n`。
fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_carriage_returns() {
        assert_eq!(normalize("hello\r\nworld"), "hello\nworld");
        assert_eq!(normalize("hello\rworld"), "hello\nworld");
        assert_eq!(normalize("  hello  "), "hello");
    }

    #[test]
    fn blank_text_is_dropped() {
        assert_eq!(normalize("   \r\n  "), "");
    }
}
