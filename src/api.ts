/** Tauri 后端命令的薄封装。 */

import { invoke, isTauri } from "@tauri-apps/api/core";
import type {
  EngineInfo,
  ExportFormat,
  HistoryPage,
  HistoryStats,
  TranslateResult,
} from "./types";

export interface CacheStats {
  size: number;
}

/** 桌面端才有的命令。浏览器预览时给出可读的拒绝理由。 */
const desktopOnly = "当前为浏览器预览。请运行 dev.bat 或桌面程序以使用该功能。";

/**
 * 翻译。
 * @param text   原文
 * @param from   源语言，"auto" 表示自动检测
 * @param to     目标语言
 * @param engine 引擎 id，省略则用设置里选定的引擎
 */
export function translate(
  text: string,
  from: string,
  to: string,
  engine?: string,
): Promise<TranslateResult> {
  if (!isTauri()) return Promise.reject(desktopOnly);
  return invoke<TranslateResult>("translate", { text, from, to, engine: engine ?? null });
}

/** 可用引擎列表。 */
export function engines(): Promise<EngineInfo[]> {
  if (!isTauri()) return Promise.resolve([]);
  return invoke<EngineInfo[]>("engines");
}

/** 热替换 HTTP 客户端。`proxy` 传 null 或空串表示回落到环境变量。 */
export function setProxy(proxy: string | null): Promise<void> {
  return invoke<void>("set_proxy", { proxy });
}

export function cacheStats(): Promise<CacheStats> {
  if (!isTauri()) return Promise.resolve({ size: 0 });
  return invoke<CacheStats>("cache_stats");
}

export function clearCache(): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("clear_cache");
}

// ── 窗口与应用信息 ──────────────────────────────────────────

export interface AppInfo {
  version: string;
  settingsPath: string;
  historyPath: string;
  configDir: string;
  dataDir: string;
}

/** 把主窗口调整到给定的逻辑尺寸。浏览器预览下无事发生。 */
export function resizeWindow(width: number, height: number): Promise<void> {
  if (!isTauri()) return Promise.resolve();
  return invoke<void>("resize_window", { width, height });
}

/** 放大 / 还原窗口，返回调整后是否处于最大化。 */
export function toggleMaximize(): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(false);
  return invoke<boolean>("toggle_maximize");
}

/** 版本号与数据文件位置。浏览器预览下给出占位值。 */
export function appInfo(): Promise<AppInfo | null> {
  if (!isTauri()) return Promise.resolve(null);
  return invoke<AppInfo>("app_info");
}

export function quitApp(): Promise<void> {
  if (!isTauri()) return Promise.reject(desktopOnly);
  return invoke<void>("quit_app");
}

// ── 历史 ────────────────────────────────────────────────────

export function historyList(
  offset: number,
  limit: number,
  filter?: string,
): Promise<HistoryPage> {
  if (!isTauri()) return Promise.resolve({ items: [], total: 0 });
  return invoke<HistoryPage>("history_list", { offset, limit, filter: filter ?? null });
}

export function historyGet(id: number): Promise<TranslateResult | null> {
  if (!isTauri()) return Promise.reject(desktopOnly);
  return invoke<TranslateResult | null>("history_get", { id });
}

/** 单词礼炮的只读词池，独立于历史抽屉的搜索与分页。 */
export function historyCelebrationWords(): Promise<string[]> {
  if (!isTauri()) return Promise.resolve([]);
  return invoke<string[]>("history_celebration_words");
}

export function historyDelete(id: number): Promise<boolean> {
  if (!isTauri()) return Promise.reject(desktopOnly);
  return invoke<boolean>("history_delete", { id });
}

export function historyClear(): Promise<number> {
  if (!isTauri()) return Promise.reject(desktopOnly);
  return invoke<number>("history_clear");
}

export function historyStats(): Promise<HistoryStats> {
  if (!isTauri()) return Promise.resolve({ total: 0, sizeBytes: 0 });
  return invoke<HistoryStats>("history_stats");
}

/** 导出历史到指定路径，返回写出的条数。 */
export function exportHistory(format: ExportFormat, path: string): Promise<number> {
  if (!isTauri()) return Promise.reject(desktopOnly);
  return invoke<number>("export_history", { format, path });
}
