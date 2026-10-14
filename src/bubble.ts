/**
 * 划词悬浮图标的渲染。
 *
 * 这里**不处理点击**。图标窗口带 `WS_EX_NOACTIVATE`，鼠标焦点永远落不到它身上，
 * 依赖 WebView 的 click 事件不可靠；点击由 Rust 侧的全局鼠标钩子接管
 * （见 `src-tauri/src/selection/hook.rs`）。
 *
 * Rust 在每次显示图标前会调 `window.__applyAppearance(theme, width, height)`，
 * 这样图标永远和主界面用同一套皮肤令牌、同一档字号。传进来的是**逻辑像素**，
 * 和 CSS 像素一一对应，所以比例不会随显示器 DPI 走样。
 */

/** 与 `src/main.ts` 共用。Rust 的 eval 要等窗口建好才到，先用它兜第一帧，避免白闪。 */
const THEME_KEY = "translater.theme";

type BubbleWindow = Window & {
  __applyAppearance?: (theme: string, width: number, height: number) => void;
};

function applyTheme(theme: string): void {
  document.documentElement.dataset.theme = theme;
}

const bubbleWindow = window as BubbleWindow;

bubbleWindow.__applyAppearance = (theme: string, width: number, height: number): void => {
  applyTheme(theme);
  const style = document.documentElement.style;
  style.setProperty("--bubble-width", `${width}px`);
  style.setProperty("--bubble-height", `${height}px`);
};

applyTheme(localStorage.getItem(THEME_KEY) ?? "plain");

// 主窗口换皮肤时同步过来（同源窗口之间会派发 storage 事件）
window.addEventListener("storage", (event) => {
  if (event.key === THEME_KEY && event.newValue) {
    applyTheme(event.newValue);
  }
});

export {};
