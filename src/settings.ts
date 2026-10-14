import { invoke, isTauri } from "@tauri-apps/api/core";
import type { ExportFormat } from "./types";

export interface Settings {
  proxy: string;
  autoTranslate: boolean;
  autoSpeak: boolean;
  langPair: string;
  /** 当前翻译引擎 id */
  engine: string;
  /** 历史记录策略：all（默认，无上限）或 off */
  historyMode: "all" | "off";
  /** 皮肤 id，对应 :root[data-theme] */
  theme: string;
  /** 字号档位，对应 :root[data-font] */
  font: string;
  /** 启动时的窗口宽度（逻辑像素） */
  windowWidth: number;
  /** 启动时的窗口高度（逻辑像素） */
  windowHeight: number;
  /** 是否在手动调整窗口后覆盖上面的默认尺寸 */
  rememberWindow: boolean;
  /** 划词翻译总开关：划选文字后弹出悬浮图标 */
  selectTranslate: boolean;
  /** UI Automation 读不到选区时，是否退到「模拟 Ctrl+C 读剪贴板」 */
  clipboardFallback: boolean;
  /** 是否在资源管理器右键菜单里注册「翻译（translater）」 */
  shellMenu: boolean;
}

/** 皮肤清单。新增皮肤要同时改 theme.css 与 src-tauri/src/settings.rs。 */
export const THEMES: { id: string; label: string }[] = [
  { id: "plain", label: "简约 plain" },
  { id: "meridian", label: "远点 meridian" },
];

/** 字号档位，`--fs` 的取值写在 theme.css 里，这里只做展示与校验。 */
export const FONT_SCALES: { id: string; label: string }[] = [
  { id: "sm", label: "小" },
  { id: "md", label: "标准" },
  { id: "lg", label: "大" },
  { id: "xl", label: "特大" },
];

/** 窗口尺寸预设，同时供「视图」菜单和设置弹窗使用。 */
export const WINDOW_PRESETS: { width: number; height: number; label: string }[] = [
  { width: 900, height: 620, label: "紧凑" },
  { width: 1020, height: 760, label: "标准" },
  { width: 1280, height: 820, label: "宽屏" },
  { width: 1440, height: 900, label: "大屏" },
];

/** 与 tauri.conf.json 的 minWidth / minHeight 保持一致。 */
export const WINDOW_MIN = { width: 720, height: 480 };
export const WINDOW_MAX = { width: 3840, height: 2160 };

export const DEFAULTS: Settings = {
  proxy: "",
  autoTranslate: true,
  autoSpeak: false,
  langPair: "auto|zh-CN",
  engine: "google",
  historyMode: "all",
  theme: "plain",
  font: "md",
  windowWidth: 1020,
  windowHeight: 760,
  rememberWindow: false,
  selectTranslate: true,
  clipboardFallback: true,
  shellMenu: true,
};

/** 主窗口与划词悬浮图标共用的皮肤键，写在 localStorage 里做首帧兜底。 */
export const THEME_STORAGE_KEY = "translater.theme";

let pendingSave: Promise<void> = Promise.resolve();

export async function loadSettings(): Promise<Settings> {
  if (isTauri()) return invoke<Settings>("load_settings");
  try {
    const raw = JSON.parse(localStorage.getItem("translater.settings") ?? "{}");
    return { ...DEFAULTS, ...raw };
  } catch {
    return { ...DEFAULTS };
  }
}

export function saveSettings(settings: Settings): Promise<void> {
  const snapshot = { ...settings };
  const task = pendingSave.catch(() => {}).then(async () => {
    if (isTauri()) await invoke("save_settings", { settings: snapshot });
    else localStorage.setItem("translater.settings", JSON.stringify(snapshot));
  });
  pendingSave = task;
  return task;
}

/** 把非法或缺失的值收敛到合法档位，避免界面渲染出空主题。 */
export function normalizeSettings(s: Settings): Settings {
  const theme = THEMES.some((t) => t.id === s.theme) ? s.theme : DEFAULTS.theme;
  const font = FONT_SCALES.some((f) => f.id === s.font) ? s.font : DEFAULTS.font;
  const clamp = (n: number, min: number, max: number) =>
    Number.isFinite(n) ? Math.min(max, Math.max(min, Math.round(n))) : min;
  return {
    ...s,
    theme,
    font,
    windowWidth: clamp(s.windowWidth, WINDOW_MIN.width, WINDOW_MAX.width),
    windowHeight: clamp(s.windowHeight, WINDOW_MIN.height, WINDOW_MAX.height),
    rememberWindow: Boolean(s.rememberWindow),
    selectTranslate: s.selectTranslate !== false,
    clipboardFallback: s.clipboardFallback !== false,
    shellMenu: s.shellMenu !== false,
  };
}

/** `1020 × 760` 这样的展示文本，预设命中时带上名字。 */
export function windowLabel(width: number, height: number): string {
  const preset = WINDOW_PRESETS.find((p) => p.width === width && p.height === height);
  return preset ? `${preset.label} ${width} × ${height}` : `${width} × ${height}`;
}

/** 导出格式的展示信息，供下拉框与文件对话框复用。 */
export const EXPORT_FORMATS: { id: ExportFormat; label: string; ext: string; hint: string }[] = [
  { id: "csv", label: "CSV 表格", ext: "csv", hint: "Excel 可直接打开，带 BOM 不会乱码" },
  { id: "markdown", label: "Markdown", ext: "md", hint: "按日期分组，适合当学习笔记" },
  { id: "anki", label: "Anki 导入", ext: "txt", hint: "两列制表符分隔，基本型可直接导入" },
  { id: "json", label: "JSON 备份", ext: "json", hint: "含完整结果，用于备份或二次处理" },
];

/** 生成默认文件名，形如 `translater-history-20261012-1400.csv`。 */
export function defaultExportName(format: ExportFormat): string {
  const d = new Date();
  const p = (n: number) => String(n).padStart(2, "0");
  const stamp =
    `${d.getFullYear()}${p(d.getMonth() + 1)}${p(d.getDate())}` +
    `-${p(d.getHours())}${p(d.getMinutes())}`;
  const ext = EXPORT_FORMATS.find((f) => f.id === format)?.ext ?? "txt";
  return `translater-history-${stamp}.${ext}`;
}
