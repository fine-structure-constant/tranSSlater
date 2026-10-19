//! UI Automation 取词。
//!
//! 这是首选路径：**纯只读**，不碰剪贴板、不模拟按键，因此不会干扰用户的复制粘贴
//! 历史，也不会像 Ctrl+C 那样在资源管理器里误触发「复制文件」。
//!
//! Edge 的 Acrobat PDF 提供者可能返回非空但偏移的 GetSelection/GetText 结果。
//! 因此同时返回文档来源，PDF 由调用方通过复制取词，不信任它的字符偏移。

use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationElement, IUIAutomationTextPattern,
    UIA_DocumentControlTypeId, UIA_TextPatternId, UIA_WindowControlTypeId,
};

/// PDF 的段落、分页和批注可能有多层包装；必须在找到文字后继续识别文档祖先。
const MAX_ANCESTORS: usize = 12;

#[derive(Default)]
pub struct SelectionRead {
    text: Option<String>,
    is_pdf: bool,
}

impl SelectionRead {
    /// PDF 只能采用实际复制的结果；关闭兜底或复制失败时不展示偏移文字。
    pub fn into_text(
        self,
        can_copy: bool,
        copy: impl FnOnce() -> Option<String>,
    ) -> Option<String> {
        if self.is_pdf || self.text.is_none() {
            if can_copy {
                copy().filter(|s| !s.trim().is_empty())
            } else {
                None
            }
        } else {
            self.text
        }
    }
}

/// 在调用线程上初始化 COM（多线程套间）。重复调用是安全的。
pub fn init_com() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

/// 读取当前选中的文字，同时检查焦点与光标位置的祖先；焦点可能停留在 PDF 外层
/// 查看器，不能因为先读到了非空文字就跳过光标处的 PDF 文档标识。
pub fn read_selection(point: Option<(i32, i32)>) -> SelectionRead {
    let mut selection = SelectionRead::default();
    unsafe {
        let Ok(automation): windows::core::Result<IUIAutomation> =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
        else {
            return selection;
        };

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
            let mut current = root;
            for _ in 0..=MAX_ANCESTORS {
                let control_type = current.CurrentControlType().ok();
                if control_type == Some(UIA_DocumentControlTypeId) {
                    let framework = current.CurrentFrameworkId().unwrap_or_default().to_string();
                    let name = current.CurrentName().unwrap_or_default().to_string();
                    let role = current
                        .CurrentLocalizedControlType()
                        .unwrap_or_default()
                        .to_string();
                    if is_pdf_document(&framework, &name, &role) {
                        selection.is_pdf = true;
                        return selection;
                    }
                }
                if selection.text.is_none() {
                    selection.text = text_from(&current);
                }
                if control_type == Some(UIA_WindowControlTypeId) {
                    break;
                }
                let Some(walker) = &walker else { break };
                let Ok(parent) = walker.GetParentElement(&current) else {
                    break;
                };
                current = parent;
            }
        }
    }
    selection
}

/// 只检查 Document 节点的语义标识，不能把正文提到 PDF 或文件名后缀当成 PDF。
/// Edge Acrobat 实测暴露 Chrome / Document / "PDF Document" 包装节点；
/// 其它提供者也可能直接通过 LocalizedControlType 描述 PDF 文档。
fn is_pdf_document(framework: &str, name: &str, localized_type: &str) -> bool {
    localized_type.to_ascii_lowercase().contains("pdf")
        || (framework.eq_ignore_ascii_case("Chrome")
            && name.trim().eq_ignore_ascii_case("PDF Document"))
}

/// 从单个元素上取选区文字。元素不支持 TextPattern 或没有选中内容时返回 `None`。
unsafe fn text_from(element: &IUIAutomationElement) -> Option<String> {
    let pattern: IUIAutomationTextPattern =
        unsafe { element.GetCurrentPatternAs(UIA_TextPatternId).ok()? };
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
    if cleaned.is_empty() {
        None
    } else {
        Some(cleaned)
    }
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

    #[test]
    fn identifies_edge_acrobat_document_without_using_the_filename() {
        assert!(is_pdf_document("Chrome", "PDF Document", "文档"));
        assert!(is_pdf_document("Chrome", "", "PDF document"));
        assert!(is_pdf_document("Chrome", "", "PDF 文档"));
        assert!(!is_pdf_document("Chrome", "Oresteia.pdf", "文档"));
        assert!(!is_pdf_document(
            "Chrome",
            "How to read a PDF document",
            "document"
        ));
        assert!(!is_pdf_document("Win32", "PDF Document", "document"));
    }

    #[test]
    fn nonempty_shifted_pdf_selection_uses_the_actual_copy() {
        let read = SelectionRead {
            text: Some("te too a".into()),
            is_pdf: true,
        };
        assert_eq!(
            read.into_text(true, || Some("advocate".into())).as_deref(),
            Some("advocate")
        );
    }

    #[test]
    fn pdf_does_not_reuse_shifted_text_if_copy_fails_or_is_disabled() {
        for copied in [None, Some(String::new()), Some("  ".into())] {
            let read = SelectionRead {
                text: Some("te too a".into()),
                is_pdf: true,
            };
            assert_eq!(read.into_text(true, || copied), None);
        }
        let read = SelectionRead {
            text: Some("te too a".into()),
            is_pdf: true,
        };
        assert_eq!(read.into_text(false, || panic!("Copy is disabled")), None);
    }

    #[test]
    fn regular_web_selection_stays_read_only_even_with_copy_enabled() {
        for can_copy in [false, true] {
            let read = SelectionRead {
                text: Some("advocate".into()),
                is_pdf: false,
            };
            assert_eq!(
                read.into_text(can_copy, || panic!("Valid web selection must not copy"))
                    .as_deref(),
                Some("advocate")
            );
        }
    }

    #[test]
    fn missing_selection_still_respects_copy_fallback() {
        assert_eq!(
            SelectionRead::default()
                .into_text(true, || Some("witness".into()))
                .as_deref(),
            Some("witness")
        );
        assert_eq!(
            SelectionRead::default().into_text(false, || panic!("Copy is disabled")),
            None
        );
    }
}
