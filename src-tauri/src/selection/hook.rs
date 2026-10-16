//! 全局低级鼠标钩子（`WH_MOUSE_LL`）。
//!
//! 两件事：
//! 1. 识别「划词」手势 —— 拖选（位移 + 时长都过阈值）或双击选词，识别到就丢给
//!    工作线程去取词。
//! 2. 接管悬浮图标上的点击。图标窗口是 `WS_EX_NOACTIVATE` 的，点它不会激活窗口，
//!    所以命中判定必须在这里做：落在图标矩形里就吞掉这次点击，然后通知主窗口。
//!
//! 钩子回调运行在安装它的那个线程上，**必须尽快返回**：系统对低级钩子有超时限制，
//! 超时会被静默摘掉。所以回调里只有比较和 `channel::send`，没有任何阻塞调用。

use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use windows::Win32::Foundation::{CloseHandle, HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, GetCurrentThreadId, OpenProcess, QueryFullProcessImageNameW,
    PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Input::KeyboardAndMouse::GetDoubleClickTime;
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetAncestor, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW, TranslateMessage,
    UnhookWindowsHookEx, WindowFromPoint, LLMHF_INJECTED, MSG, MSLLHOOKSTRUCT, WH_MOUSE_LL,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEWHEEL, WM_QUIT, WM_RBUTTONDOWN, GA_ROOTOWNER,
};
use windows::core::PWSTR;

use super::{bubble_hit, bubble_visible, enabled, post_accept, post_gesture, post_hide};

/// 双击判定允许的位置漂移（物理像素）。
const DOUBLE_CLICK_SLOP: i32 = 4;

/// 拖动判定阈值，与 `selection::mod` 里的常量保持一致。
const DRAG_MIN_PX: i32 = 6;
const DRAG_MIN_MS: u32 = 60;

#[derive(Default)]
struct Gesture {
    /// 本次按下时的位置与时刻，用来在抬起时判断是不是拖选。
    down: POINT,
    down_time: u32,
    /// 上一次按下的位置与时刻，用来判断是不是双击。
    last_down: POINT,
    last_down_time: u32,
    /// 本次按下属于一次双击。
    double: bool,
    /// 已经吞掉了按下事件，配套的抬起事件也要吞掉，否则目标程序会收到一个孤立的
    /// mouse-up。
    swallow_up: bool,
    /// 从应用自身开始的拖选，即使松手时鼠标已移出窗口也不能触发划词。
    started_in_app: bool,
}

static GESTURE: Mutex<Gesture> = Mutex::new(Gesture {
    down: POINT { x: 0, y: 0 },
    down_time: 0,
    last_down: POINT { x: 0, y: 0 },
    last_down_time: 0,
    double: false,
    swallow_up: false,
    started_in_app: false,
});

/// 钩子线程 id。`stop()` 靠它投递 `WM_QUIT`；0 表示线程没在跑。
static THREAD_ID: AtomicU32 = AtomicU32::new(0);

/// 在独立线程上装钩子并跑消息循环。消息循环只是为了让系统能回调钩子，
/// 这个线程不创建任何窗口。
pub fn spawn() -> Result<(), String> {
    std::thread::Builder::new()
        .name("mouse-hook".into())
        .spawn(|| unsafe { run() })
        .map_err(|e| format!("鼠标钩子线程启动失败：{e}"))?;
    Ok(())
}

/// 停掉钩子线程。线程退出时系统会自动摘掉它装的钩子。
///
/// 退出流程会先关掉 `enabled`，所以即使这一步没赶上，回调也已经什么都不做了；
/// 这里只是让钩子干净地撤掉，而不是靠进程被销毁来兜底。
pub fn stop() {
    let id = THREAD_ID.swap(0, Ordering::SeqCst);
    if id != 0 {
        unsafe {
            let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

unsafe fn run() {
    let Ok(module) = (unsafe { GetModuleHandleW(None) }) else {
        return;
    };

    // 先碰一次消息队列把它建出来。否则 `stop()` 投的 WM_QUIT 会因为队列还不存在
    // 而直接丢掉，线程就永远卡在 GetMessageW 上。
    let mut msg = MSG::default();
    unsafe {
        let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE);
    }
    THREAD_ID.store(unsafe { GetCurrentThreadId() }, Ordering::SeqCst);

    // 低级钩子的回调在本进程内执行，hMod 传本模块句柄即可
    let Ok(hook) = (unsafe {
        SetWindowsHookExW(
            WH_MOUSE_LL,
            Some(proc_),
            Some(HINSTANCE(module.0)),
            0,
        )
    }) else {
        THREAD_ID.store(0, Ordering::SeqCst);
        return;
    };

    while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }

    THREAD_ID.store(0, Ordering::SeqCst);
    let _ = unsafe { UnhookWindowsHookEx(hook) };
}

unsafe extern "system" fn proc_(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // 小于 0 必须原样传下去
    if code < 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let info = unsafe { &*(lparam.0 as *const MSLLHOOKSTRUCT) };

    // 我们自己用 SendInput 合成的鼠标事件不要再处理一遍
    if info.flags & LLMHF_INJECTED != 0 {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    // 划词关掉时钩子仍然装着（这样设置里勾上就立刻生效），但什么都不做
    if !enabled() {
        return unsafe { CallNextHookEx(None, code, wparam, lparam) };
    }

    let mut swallow = false;

    {
        // 钩子回调里绝不允许 panic 穿过 FFI 边界，锁中毒也照常取用
        let mut g = GESTURE.lock().unwrap_or_else(|e| e.into_inner());

        match wparam.0 as u32 {
            WM_LBUTTONDOWN => {
                if bubble_hit(info.pt.x, info.pt.y) {
                    // 点的是悬浮图标：吞掉这次点击，交给主窗口
                    post_accept();
                    g.swallow_up = true;
                    swallow = true;
                } else if bubble_visible() {
                    // 点在别处：收起图标，这次点击照常传下去
                    post_hide();
                }

                g.started_in_app = window_belongs_to_us(info.pt.x, info.pt.y);
                g.double = !g.started_in_app && is_double_click(&g, info.pt, info.time);

                g.down = info.pt;
                g.down_time = info.time;
                g.last_down = info.pt;
                g.last_down_time = if g.started_in_app { 0 } else { info.time };
            }

            WM_LBUTTONUP => {
                if g.swallow_up {
                    g.swallow_up = false;
                    swallow = true;
                } else if should_capture_gesture(&g, info.pt, info.time) {
                    post_gesture(info.pt.x, info.pt.y);
                }
                g.double = false;
            }

            // 右键、中键、滚轮都表示用户不打算翻译了
            WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL => {
                if bubble_visible() {
                    post_hide();
                }
            }

            _ => {}
        }
    }

    if swallow {
        // 返回非零表示吞掉这条消息，不再传给其它程序
        return LRESULT(1);
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

fn is_double_click(g: &Gesture, pt: POINT, time: u32) -> bool {
    if g.last_down_time == 0 {
        return false;
    }
    let dt = time.wrapping_sub(g.last_down_time);
    if dt > unsafe { GetDoubleClickTime() } {
        return false;
    }
    (pt.x - g.last_down.x).abs() <= DOUBLE_CLICK_SLOP
        && (pt.y - g.last_down.y).abs() <= DOUBLE_CLICK_SLOP
}

fn is_drag(g: &Gesture, pt: POINT, time: u32) -> bool {
    let dx = (pt.x - g.down.x).abs();
    let dy = (pt.y - g.down.y).abs();
    let dt = time.wrapping_sub(g.down_time);
    (dx > DRAG_MIN_PX || dy > DRAG_MIN_PX) && dt >= DRAG_MIN_MS
}

fn should_capture_gesture(g: &Gesture, pt: POINT, time: u32) -> bool {
    !g.started_in_app && (g.double || is_drag(g, pt, time))
}

// ── 前台窗口查询 ────────────────────────────────────────────

/// 指定坐标下的窗口是不是本程序自己的。在自己窗口里划词不需要翻译。
pub fn window_belongs_to_us(x: i32, y: i32) -> bool {
    belongs_to_us(unsafe { WindowFromPoint(POINT { x, y }) })
}

/// WebView2 的子 HWND 可属于独立渲染进程；应以宿主根窗口的归属为准。
fn belongs_to_us(hwnd: HWND) -> bool {
    if hwnd.is_invalid() {
        return false;
    }
    unsafe {
        let root = GetAncestor(hwnd, GA_ROOTOWNER);
        belongs_to_process(window_pid(hwnd), window_pid(root), GetCurrentProcessId())
    }
}

fn window_pid(hwnd: HWND) -> u32 {
    if hwnd.is_invalid() {
        return 0;
    }
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    pid
}

fn belongs_to_process(window_pid: u32, host_pid: u32, app_pid: u32) -> bool {
    app_pid != 0 && (window_pid == app_pid || host_pid == app_pid)
}

pub fn foreground_belongs_to_us() -> bool {
    belongs_to_us(unsafe { GetForegroundWindow() })
}

/// 前台窗口是不是资源管理器。
///
/// 需要这个判断是因为剪贴板兜底要模拟 Ctrl+C：在资源管理器里选中若干文件时按
/// Ctrl+C 会变成「复制文件」，既吓人又真的会写剪贴板。所以资源管理器下只走
/// UI Automation，读不到就放弃。
pub fn foreground_is_explorer() -> bool {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_invalid() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        process_name(pid).eq_ignore_ascii_case("explorer.exe")
    }
}

unsafe fn process_name(pid: u32) -> String {
    let Ok(handle) = (unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }) else {
        return String::new();
    };
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .is_ok();
    let _ = unsafe { CloseHandle(HANDLE(handle.0)) };
    if !ok || len == 0 {
        return String::new();
    }
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    path.rsplit(['\\', '/']).next().unwrap_or("").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gesture(down: POINT, down_time: u32) -> Gesture {
        Gesture {
            down,
            down_time,
            ..Default::default()
        }
    }

    #[test]
    fn small_movement_is_not_a_drag() {
        let g = gesture(POINT { x: 100, y: 100 }, 1_000);
        // 位移只有 3px，虽然停得够久
        assert!(!is_drag(&g, POINT { x: 103, y: 102 }, 1_500));
    }

    #[test]
    fn fast_movement_is_not_a_drag() {
        let g = gesture(POINT { x: 100, y: 100 }, 1_000);
        // 位移够大，但只花了 20ms —— 更像甩鼠标
        assert!(!is_drag(&g, POINT { x: 200, y: 100 }, 1_020));
    }

    #[test]
    fn slow_long_movement_is_a_drag() {
        let g = gesture(POINT { x: 100, y: 100 }, 1_000);
        assert!(is_drag(&g, POINT { x: 160, y: 100 }, 1_200));
    }

    #[test]
    fn double_click_needs_close_position_and_short_gap() {
        let mut g = Gesture {
            last_down: POINT { x: 50, y: 50 },
            last_down_time: 1_000,
            ..Default::default()
        };
        assert!(is_double_click(&g, POINT { x: 52, y: 51 }, 1_120));
        // 位置差太多
        assert!(!is_double_click(&g, POINT { x: 80, y: 50 }, 1_120));
        // 间隔太久（远超过默认 500ms）
        assert!(!is_double_click(&g, POINT { x: 50, y: 50 }, 9_000));
        // 首次按下时 last_down_time 是 0，不能误判成双击
        g.last_down_time = 0;
        assert!(!is_double_click(&g, POINT { x: 50, y: 50 }, 10));
    }

    #[test]
    fn time_wraps_around_instead_of_panicking() {
        // GetTickCount 大约 49.7 天回绕一次，回绕瞬间不能溢出
        let g = gesture(POINT { x: 0, y: 0 }, u32::MAX - 10);
        assert!(is_drag(&g, POINT { x: 100, y: 0 }, 100));
    }

    #[test]
    fn hosted_webview_is_ours_even_with_a_separate_renderer_process() {
        assert!(belongs_to_process(200, 100, 100));
        assert!(belongs_to_process(100, 100, 100));
        assert!(!belongs_to_process(200, 300, 100));
        assert!(!belongs_to_process(0, 0, 100));
    }

    #[test]
    fn editing_gestures_that_start_in_the_app_are_never_captured() {
        let mut g = gesture(POINT { x: 100, y: 100 }, 1_000);
        g.started_in_app = true;
        // 拖到窗外与双击选词都仍然是应用内编辑操作。
        assert!(!should_capture_gesture(&g, POINT { x: 800, y: 100 }, 1_200));
        g.double = true;
        assert!(!should_capture_gesture(&g, POINT { x: 100, y: 100 }, 1_200));
        g.started_in_app = false;
        assert!(should_capture_gesture(&g, POINT { x: 100, y: 100 }, 1_200));
    }
}
