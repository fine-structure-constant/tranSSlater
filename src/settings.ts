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
}

export const DEFAULTS: Settings = {
  proxy: "",
  autoTranslate: true,
  autoSpeak: false,
  langPair: "auto|zh-CN",
  engine: "google",
  historyMode: "all",
};

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
