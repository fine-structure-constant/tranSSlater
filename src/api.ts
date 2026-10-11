/** Tauri 后端命令的薄封装。 */

import { invoke, isTauri } from "@tauri-apps/api/core";
import type { TranslateResult } from "./types";

export interface CacheStats {
  size: number;
}

/**
 * 翻译。
 * @param text 原文
 * @param from 源语言，"auto" 表示自动检测
 * @param to   目标语言
 */
export function translate(
  text: string,
  from: string,
  to: string,
): Promise<TranslateResult> {
  if (!isTauri()) return Promise.reject("当前为浏览器预览。请运行 dev.bat 或桌面程序以使用翻译功能。");
  return invoke<TranslateResult>("translate", { text, from, to });
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
