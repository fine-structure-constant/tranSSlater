/** 应用主逻辑：输入 → 翻译 → 渲染；菜单栏；引擎切换；历史库与导出。 */

import {
  appInfo,
  cacheStats,
  clearCache,
  engines as fetchEngines,
  exportHistory,
  historyClear,
  historyCelebrationWords,
  historyDelete,
  historyGet,
  historyList,
  historyStats,
  quitApp,
  resizeWindow,
  shellMenuRegistered,
  takeStartupText,
  toggleMaximize,
  translate,
  type AppInfo,
} from "./api";
import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  DEFAULTS,
  EXPORT_FORMATS,
  FONT_SCALES,
  THEMES,
  THEME_STORAGE_KEY,
  WINDOW_MIN,
  WINDOW_PRESETS,
  defaultExportName,
  loadSettings,
  normalizeSettings,
  saveSettings,
  windowLabel,
  type Settings,
} from "./settings";
import { renderResult } from "./render";
import { attachWordConfetti } from "./confetti";
import type { EngineInfo, ExportFormat, HistorySummary, TranslateResult } from "./types";
import { langName } from "./types";

const $ = <T extends HTMLElement = HTMLElement>(id: string): T =>
  document.getElementById(id) as T;

// ── DOM ─────────────────────────────────────────────────────

const input = $<HTMLTextAreaElement>("input");
const queryMeta = $("query-meta");
const result = $("result");
const empty = $("empty");
const alertBox = $("alert");
const status = $("status");
const langPair = $<HTMLSelectElement>("lang-pair");
const enginePick = $<HTMLSelectElement>("engine");
const cacheSize = $("cache-size");
const historyCount = $("history-count");
const mask = $("mask");
const proxyInput = $<HTMLInputElement>("proxy");
const autoTranslateInput = $<HTMLInputElement>("auto-translate");
const autoSpeakInput = $<HTMLInputElement>("auto-speak");
const recordHistoryInput = $<HTMLInputElement>("record-history");
const settingsEngine = $<HTMLSelectElement>("settings-engine");
const settingsTheme = $<HTMLSelectElement>("settings-theme");
const settingsFont = $<HTMLSelectElement>("settings-font");
const settingsWindow = $<HTMLSelectElement>("settings-window");
const rememberWindowInput = $<HTMLInputElement>("remember-window");
const selectTranslateInput = $<HTMLInputElement>("select-translate");
const clipboardFallbackInput = $<HTMLInputElement>("clipboard-fallback");
const shellMenuInput = $<HTMLInputElement>("shell-menu");
const settingsMsg = $("settings-msg");

const helpMask = $("help-mask");
const helpBody = $("help-body");

const drawer = $("drawer");
const drawerMask = $("drawer-mask");
const historyListEl = $("history-list");
const historyStat = $("history-stat");
const historySearch = $<HTMLInputElement>("history-search");
const historyMsg = $("history-msg");
const exportFormat = $<HTMLSelectElement>("export-format");
const exportHint = $("export-hint");
const historyMore = $<HTMLButtonElement>("btn-history-more");

const menubar = $("menubar");
const menus = Array.from(menubar.querySelectorAll<HTMLElement>(".menu"));

attachWordConfetti($<HTMLButtonElement>("btn-celebrate"), historyCelebrationWords);

// ── 设置与运行时状态 ────────────────────────────────────────

let settings: Settings = { ...DEFAULTS };
let ready = false;
let composing = false;
let saving = false;
let engineList: EngineInfo[] = [];

/** 请求序号，用于丢弃过期响应 */
let seq = 0;
let current: TranslateResult | null = null;
let debounceTimer: number | undefined;
let audio: HTMLAudioElement | null = null;

/// 窗口缩放落盘的防抖句柄
let resizeTimer: number | undefined;
/// 「说明」里的引擎表格只需填一次
let helpFilled = false;

const PAGE_SIZE = 50;
let historyOffset = 0;
let historyFilter = "";
let historyTotal = 0;
/// 数据库体积，`-1` 表示还没读到。
let historySize = -1;

// ── 引擎 ────────────────────────────────────────────────────

function engineName(id: string): string {
  return engineList.find((e) => e.id === id)?.name ?? id;
}

function currentEngine(): string {
  return enginePick.value || settings.engine;
}

function fillEngineOptions(): void {
  for (const select of [enginePick, settingsEngine]) {
    select.replaceChildren();
    for (const info of engineList) {
      const option = document.createElement("option");
      option.value = info.id;
      option.textContent = info.name;
      select.appendChild(option);
    }
  }
}

// ── 皮肤与字号 ──────────────────────────────────────────────

/**
 * 皮肤和字号都只是 `<html>` 上的一个属性：CSS 里所有颜色/圆角走 `[data-theme]`，
 * 所有字号乘 `[data-font]` 给出的 `--fs`。这里不做任何布局改动。
 */
function applyAppearance(): void {
  document.documentElement.dataset.theme = settings.theme;
  document.documentElement.dataset.font = settings.font;
  // 划词悬浮图标是另一个窗口，靠 localStorage 兜第一帧的皮肤，避免闪一下默认样式
  try {
    localStorage.setItem(THEME_STORAGE_KEY, settings.theme);
  } catch {
    // 隐私模式下写不进去，不影响主流程
  }
}

async function setTheme(theme: string): Promise<void> {
  if (!THEMES.some((t) => t.id === theme) || theme === settings.theme) return;
  settings.theme = theme;
  applyAppearance();
  settingsTheme.value = theme;
  syncMenuChecks();
  try {
    await saveSettings(settings);
    setStatus(`已切换到 ${THEMES.find((t) => t.id === theme)?.label ?? theme}`);
    window.setTimeout(() => setStatus(""), 1600);
  } catch (err) {
    showAlert(`皮肤设置未保存：${String(err)}`);
  }
}

async function setFont(font: string): Promise<void> {
  if (!FONT_SCALES.some((f) => f.id === font) || font === settings.font) return;
  settings.font = font;
  applyAppearance();
  settingsFont.value = font;
  syncMenuChecks();
  autoGrow();
  try {
    await saveSettings(settings);
  } catch (err) {
    showAlert(`字号设置未保存：${String(err)}`);
  }
}

// ── 翻译主流程 ──────────────────────────────────────────────

async function run(text: string): Promise<void> {
  window.clearTimeout(debounceTimer);
  if (!ready) return;
  const q = text.trim();
  if (!q) {
    reset();
    return;
  }

  const my = ++seq;
  current = null;
  queryMeta.replaceChildren();
  empty.hidden = true;
  if ([...q].length > 5000) {
    result.hidden = true;
    setStatus("");
    showAlert("一次最多翻译 5000 个字符，请分段翻译。");
    return;
  }
  const [from, to] = langPair.value.split("|");
  const engine = currentEngine();

  setStatus("翻译中", true);
  setLoading(true);
  hideAlert();

  try {
    const r = await translate(q, from, to, engine);
    if (my !== seq) return; // 已被更新的请求取代

    current = r;
    empty.hidden = true;
    result.hidden = false;
    renderResult(result, r, queryAgain);
    renderMeta(r);
    setStatus(r.cached ? "缓存 · 0 ms" : `${r.elapsedMs} ms`);
    void refreshCache();
    void refreshHistoryStat();

    if (settings.autoSpeak && r.isWord) speak(q, r.sourceLang, r.speakUrl);
  } catch (err) {
    if (my !== seq) return;
    current = null;
    result.replaceChildren();
    result.hidden = true;
    setStatus("");
    showAlert(explainError(err));
  } finally {
    if (my === seq) setLoading(false);
  }
}

/** 点击近义词 / 译词时：填回输入框并立即翻译。 */
function queryAgain(text: string): void {
  input.value = text;
  autoGrow();
  input.focus();
  void run(text);
}

/** 用一条历史记录回填界面。 */
function showResult(r: TranslateResult): void {
  window.clearTimeout(debounceTimer);
  seq++;
  input.value = r.query;
  autoGrow();
  current = r;
  empty.hidden = true;
  result.hidden = false;
  renderResult(result, r, queryAgain);
  renderMeta(r);
  hideAlert();
  setLoading(false);
  setStatus(r.cached ? "缓存 · 0 ms" : `${r.elapsedMs} ms`);
}

function reset(): void {
  window.clearTimeout(debounceTimer);
  seq++;
  current = null;
  result.hidden = true;
  result.innerHTML = "";
  empty.hidden = false;
  queryMeta.innerHTML = "";
  hideAlert();
  setStatus("");
  setLoading(false);
}

// ── 输入框 ──────────────────────────────────────────────────

function autoGrow(): void {
  input.style.height = "auto";
  const max = window.innerHeight * 0.34;
  input.style.height = `${Math.min(input.scrollHeight, max)}px`;
}

function onInput(): void {
  autoGrow();
  window.clearTimeout(debounceTimer);
  seq++; // 输入一变就作废旧响应，不等 debounce 触发。
  current = null;
  result.replaceChildren();
  result.hidden = true;
  queryMeta.replaceChildren();
  setLoading(false);
  hideAlert();
  setStatus("");

  const text = input.value.trim();
  if (!text) {
    reset();
    return;
  }
  empty.hidden = true;
  if (!settings.autoTranslate || composing || !ready) return;

  // 长文本给更长的停顿，避免打字途中反复请求
  const delay = text.length <= 120 ? 650 : 1100;
  debounceTimer = window.setTimeout(() => void run(text), delay);
}
/** 清空输入框并回到初始状态。菜单、Esc 与输入框的 × 都走这里。 */
function clearInput(): void {
  input.value = "";
  autoGrow();
  reset();
  input.focus();
}

input.addEventListener("input", onInput);
input.addEventListener("compositionstart", () => {
  composing = true;
  window.clearTimeout(debounceTimer);
});
input.addEventListener("compositionend", () => {
  composing = false;
  onInput();
});

/** 窗口尺寸一变：输入框跟着长，开了「记住窗口大小」就把新尺寸落盘。 */
window.addEventListener("resize", () => {
  autoGrow();
  if (!settings.rememberWindow || !isTauri()) return;
  window.clearTimeout(resizeTimer);
  resizeTimer = window.setTimeout(() => void persistWindowSize(), 600);
});

input.addEventListener("keydown", (e) => {
  if (e.isComposing || !mask.hidden || !helpMask.hidden) return;
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    window.clearTimeout(debounceTimer);
    void run(input.value);
  } else if (e.key === "Escape") {
    // 菜单开着时先关菜单，交给全局处理器，别顺手把输入清掉
    if (menuIsOpen()) return;
    e.preventDefault();
    clearInput();
  }
});

// ── 词条区操作按钮 ──────────────────────────────────────────

document.querySelector(".query-actions")?.addEventListener("click", (e) => {
  const act = (e.target as HTMLElement).closest<HTMLElement>("[data-act]")?.dataset.act;
  if (!act) return;

  if (act === "clear") {
    clearInput();
  } else if (act === "go") {
    window.clearTimeout(debounceTimer);
    void run(input.value);
  } else if (act === "copy") {
    void copyTranslation();
  } else if (act === "speak") {
    speak(input.value.trim(), current?.sourceLang ?? "en", current?.speakUrl ?? null);
  }
});

async function copyTranslation(): Promise<void> {
  const text = current?.translation ?? "";
  if (!text) return;
  try {
    await navigator.clipboard.writeText(text);
    setStatus("已复制译文");
    window.setTimeout(() => setStatus(""), 1400);
  } catch {
    setStatus("复制失败");
  }
}

// ── 朗读 ────────────────────────────────────────────────────

/**
 * 优先播放引擎返回的音频（有道会给出 mp3，音质比系统 TTS 稳），
 * 没有音频时退回系统 SpeechSynthesis。
 */
function speak(text: string, lang: string, url: string | null): void {
  if (!text) return;

  if (url) {
    audio?.pause();
    const player = new Audio(url);
    audio = player;
    player.play().catch(() => setStatus("发音音频播放失败"));
    return;
  }

  if (!("speechSynthesis" in window)) {
    setStatus("当前系统不支持朗读");
    return;
  }
  window.speechSynthesis.cancel();
  const u = new SpeechSynthesisUtterance(text);
  u.lang = lang === "auto" ? "en" : lang;
  u.rate = 0.95;
  u.onerror = () => setStatus("朗读失败，请检查系统是否安装对应语言的语音包");
  window.speechSynthesis.speak(u);
}

// ── 界面状态 ────────────────────────────────────────────────

function setStatus(text: string, busy = false): void {
  status.textContent = text;
  status.classList.toggle("busy", busy);
}

function setLoading(on: boolean): void {
  result.setAttribute("aria-busy", String(on));
  if (on && !current) {
    result.hidden = false;
    result.innerHTML =
      '<div class="skeleton line w60"></div>' +
      '<div class="skeleton line w80"></div>' +
      '<div class="skeleton line w40"></div>';
  }
  result.classList.toggle("loading", on);
}

function showAlert(msg: string): void {
  alertBox.textContent = msg;
  alertBox.hidden = false;
}

function hideAlert(): void {
  alertBox.hidden = true;
}

/** 把后端的错误串翻译成可读的提示。 */
function explainError(err: unknown): string {
  const s = typeof err === "string" ? err : String(err);

  if (s.includes("429") || s.includes("限流")) {
    return "Google 接口限流（HTTP 429），多次重试仍未成功。稍等片刻再试，或换用有道的两个引擎（国内直连）。";
  }
  if (
    s.includes("dns") ||
    s.includes("connect") ||
    s.includes("timed out") ||
    s.includes("网络请求失败")
  ) {
    return "无法连接翻译服务。Google 需要代理，请到设置里填写；有道的两个引擎不需要代理，可直接切换。";
  }
  return s;
}

/** 渲染查询词条下方的音标 / 拼音 / 语言标签。 */
function renderMeta(r: TranslateResult): void {
  queryMeta.innerHTML = "";

  if (r.phonetic || r.phoneticUk) {
    const s = document.createElement("span");
    s.className = "ipa";
    const us = r.phonetic ? `美 /${r.phonetic.replace(/^\/|\/$/g, "")}/` : "";
    const uk = r.phoneticUk ? `英 /${r.phoneticUk.replace(/^\/|\/$/g, "")}/` : "";
    s.textContent = [us, uk].filter(Boolean).join("  ");
    queryMeta.appendChild(s);
  }
  if (r.romanization) {
    const s = document.createElement("span");
    s.className = "pin";
    s.textContent = r.romanization;
    queryMeta.appendChild(s);
  }
  if (langPair.value.startsWith("auto|")) {
    const s = document.createElement("span");
    s.className = "tag";
    s.textContent = `检测到 ${langName(r.sourceLang)}`;
    queryMeta.appendChild(s);
  }
  const engineTag = document.createElement("span");
  engineTag.className = "tag";
  engineTag.textContent = engineName(r.engine);
  queryMeta.appendChild(engineTag);
}

async function refreshCache(): Promise<void> {
  try {
    const s = await cacheStats();
    cacheSize.textContent = String(s.size);
  } catch {
    /* 忽略 */
  }
}

async function refreshHistoryStat(): Promise<void> {
  try {
    const s = await historyStats();
    historySize = s.sizeBytes;
    historyCount.textContent = String(s.total);
    renderHistoryStat();
  } catch {
    /* 忽略 */
  }
}

/** 抽屉里的统计行。条数来自列表（受搜索过滤影响），体积来自数据库。 */
function renderHistoryStat(): void {
  const size = historySize >= 0 ? ` · ${formatBytes(historySize)}` : "";
  historyStat.textContent = `${historyTotal} 条${size}`;
}

function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}

// ── 工具栏 ──────────────────────────────────────────────────

langPair.addEventListener("change", () => {
  settings.langPair = langPair.value;
  void saveSettings(settings).catch((err) => showAlert(`语言选择未保存：${String(err)}`));
  if (input.value.trim()) {
    window.clearTimeout(debounceTimer);
    void run(input.value);
  }
});

enginePick.addEventListener("change", () => {
  settings.engine = enginePick.value;
  void saveSettings(settings).catch((err) => showAlert(`引擎选择未保存：${String(err)}`));
  if (input.value.trim()) {
    window.clearTimeout(debounceTimer);
    void run(input.value);
  }
});

$("btn-swap").addEventListener("click", () => {
  const [from, to] = langPair.value.split("|");
  const source = from === "auto" ? current?.sourceLang : from;
  if (!source || source === "auto") {
    setStatus("请先翻译一次以检测源语言");
    return;
  }
  const reversed = `${to}|${source}`;
  const option = Array.from(langPair.options).find((o) => o.value === reversed);
  if (option) {
    if (current) {
      input.value = current.translation;
      autoGrow();
    }
    langPair.value = reversed;
    langPair.dispatchEvent(new Event("change"));
  } else {
    setStatus("该语言对不支持互换");
    window.setTimeout(() => setStatus(""), 1600);
  }
});

$("btn-clear-cache").addEventListener("click", async () => {
  try {
    await clearCache();
    await refreshCache();
    setStatus("缓存已清空");
  } catch (err) {
    showAlert(`清空缓存失败：${String(err)}`);
  }
});

// ── 历史抽屉 ────────────────────────────────────────────────

async function openHistory(): Promise<void> {
  drawer.hidden = false;
  drawerMask.hidden = false;
  historySearch.value = historyFilter;
  // 先取体积再取列表：反过来列表的条数会被统计行覆盖掉。
  await refreshHistoryStat();
  await loadHistory(true);
  historySearch.focus();
}

function closeHistory(): void {
  drawer.hidden = true;
  drawerMask.hidden = true;
  $("btn-history").focus();
}

$("btn-history").addEventListener("click", openHistory);
$("btn-close-history").addEventListener("click", closeHistory);
drawerMask.addEventListener("click", closeHistory);

/** 加载历史。`reset=true` 时从第一页重新加载。 */
async function loadHistory(reset: boolean): Promise<void> {
  if (reset) {
    historyOffset = 0;
    historyListEl.replaceChildren();
  }
  try {
    const page = await historyList(historyOffset, PAGE_SIZE, historyFilter);
    historyTotal = page.total;
    renderHistoryStat();
    historyOffset += page.items.length;
    historyMore.hidden = historyOffset >= page.total;

    if (reset && page.items.length === 0) {
      historyListEl.replaceChildren(
        emptyHistoryNode(
          historyFilter ? `没有匹配「${historyFilter}」的记录` : "还没有翻译记录",
        ),
      );
      return;
    }
    for (const item of page.items) historyListEl.appendChild(historyNode(item));
  } catch (err) {
    historyMsg.textContent = `读取历史失败：${String(err)}`;
  }
}

function emptyHistoryNode(text: string): HTMLElement {
  const node = document.createElement("div");
  node.className = "history-empty";
  node.textContent = text;
  return node;
}

function historyNode(item: HistorySummary): HTMLElement {
  const node = document.createElement("div");
  node.className = "history-item";
  node.tabIndex = 0;
  node.title = "点击回填到主界面";

  const query = document.createElement("div");
  query.className = "hi-query";
  query.textContent = item.query;

  const trans = document.createElement("div");
  trans.className = "hi-trans";
  trans.textContent = item.translation;

  const meta = document.createElement("div");
  meta.className = "hi-meta";
  for (const text of [
    new Date(item.createdAt).toLocaleString("zh-CN", { hour12: false }),
    engineName(item.engine),
    `${langName(item.fromLang)} → ${langName(item.toLang)}`,
  ]) {
    const span = document.createElement("span");
    span.textContent = text;
    meta.appendChild(span);
  }

  const actions = document.createElement("div");
  actions.className = "hi-actions";
  const del = document.createElement("button");
  del.className = "hi-del";
  del.type = "button";
  del.textContent = "×";
  del.title = "删除这条记录";
  del.addEventListener("click", (e) => {
    e.stopPropagation();
    void removeHistory(item.id, node);
  });
  actions.appendChild(del);

  node.append(query, trans, meta, actions);

  const restore = async (): Promise<void> => {
    try {
      const r = await historyGet(item.id);
      if (!r) {
        showAlert("这条记录的完整内容已无法读取。");
        return;
      }
      showResult(r);
      closeHistory();
    } catch (err) {
      showAlert(`读取记录失败：${String(err)}`);
    }
  };
  node.addEventListener("click", () => void restore());
  node.addEventListener("keydown", (e) => {
    if (e.key === "Enter" || e.key === " ") {
      e.preventDefault();
      void restore();
    }
  });

  return node;
}

async function removeHistory(id: number, node: HTMLElement): Promise<void> {
  try {
    await historyDelete(id);
    node.remove();
    historyOffset = Math.max(0, historyOffset - 1);
    historyTotal = Math.max(0, historyTotal - 1);
    renderHistoryStat();
    await refreshHistoryStat();
    if (!historyListEl.querySelector(".history-item")) {
      historyListEl.replaceChildren(emptyHistoryNode("还没有翻译记录"));
    }
  } catch (err) {
    historyMsg.textContent = `删除失败：${String(err)}`;
  }
}

historyMore.addEventListener("click", () => void loadHistory(false));

let searchTimer: number | undefined;
historySearch.addEventListener("input", () => {
  window.clearTimeout(searchTimer);
  searchTimer = window.setTimeout(() => {
    historyFilter = historySearch.value.trim();
    void loadHistory(true);
  }, 220);
});

$("btn-clear-history").addEventListener("click", () => void clearAllHistory());

/** 清空整个历史库。抽屉按钮与「文件」菜单共用。 */
async function clearAllHistory(): Promise<void> {
  if (!historyTotal) {
    try {
      historyTotal = (await historyStats()).total;
    } catch {
      /* 读不到就当没有 */
    }
  }
  if (!historyTotal) {
    setStatus("还没有翻译记录");
    return;
  }
  if (!window.confirm(`确定清空全部 ${historyTotal} 条历史？此操作不可撤销。`)) return;
  try {
    const n = await historyClear();
    historyMsg.textContent = `已清空 ${n} 条`;
    historyTotal = 0;
    historyOffset = 0;
    await refreshHistoryStat();
    if (!drawer.hidden) await loadHistory(true);
    setStatus(`已清空 ${n} 条历史`);
  } catch (err) {
    historyMsg.textContent = `清空失败：${String(err)}`;
    showAlert(`清空历史失败：${String(err)}`);
  }
}

// ── 导出 ────────────────────────────────────────────────────

function updateExportHint(): void {
  const f = EXPORT_FORMATS.find((x) => x.id === exportFormat.value);
  exportHint.textContent = f ? f.hint : "";
}

exportFormat.addEventListener("change", updateExportHint);

$("btn-export").addEventListener("click", () =>
  void runExport(exportFormat.value as ExportFormat),
);

/**
 * 导出历史。抽屉的「导出」按钮与「文件」菜单共用这一条路径，
 * 所以从菜单里选格式也能得到同样的进度与错误反馈。
 */
async function runExport(format: ExportFormat): Promise<void> {
  const meta = EXPORT_FORMATS.find((f) => f.id === format);
  if (!meta) return;

  if (!isTauri()) {
    historyMsg.textContent = "浏览器预览下无法导出，请运行桌面程序。";
    showAlert("浏览器预览下无法导出，请运行 dev.bat 或桌面程序。");
    return;
  }
  if (historyTotal <= 0) {
    try {
      historyTotal = (await historyStats()).total;
      renderHistoryStat();
    } catch {
      /* 读不到就当空 */
    }
  }
  if (historyTotal <= 0) {
    historyMsg.textContent = "还没有可导出的记录。";
    showAlert("还没有可导出的记录。");
    return;
  }

  const button = $<HTMLButtonElement>("btn-export");
  button.disabled = true;
  historyMsg.textContent = "正在导出…";
  try {
    const path = await saveDialog({
      defaultPath: defaultExportName(format),
      filters: [{ name: meta.label, extensions: [meta.ext] }],
    });
    if (!path) {
      historyMsg.textContent = "";
      return;
    }
    const n = await exportHistory(format, path);
    historyMsg.textContent = `已导出 ${n} 条到 ${path}`;
    setStatus(`已导出 ${n} 条`);
  } catch (err) {
    historyMsg.textContent = `导出失败：${String(err)}`;
    showAlert(`导出失败：${String(err)}`);
  } finally {
    button.disabled = false;
  }
}

// ── 设置弹窗 ────────────────────────────────────────────────

/** 用一个常量数组重建下拉框的选项。 */
function fillSelect(
  select: HTMLSelectElement,
  items: { value: string; label: string }[],
): void {
  select.replaceChildren();
  for (const item of items) {
    const option = document.createElement("option");
    option.value = item.value;
    option.textContent = item.label;
    select.appendChild(option);
  }
}

function fillWindowOptions(): void {
  fillSelect(
    settingsWindow,
    WINDOW_PRESETS.map((p) => ({
      value: windowKey(p.width, p.height),
      label: `${p.label} ${p.width} × ${p.height}`,
    })),
  );
}

/** 皮肤 / 字号 / 窗口尺寸三个下拉框都从常量生成，避免和菜单里的选项对不上。 */
function fillSettingsOptions(): void {
  fillSelect(settingsTheme, THEMES.map((t) => ({ value: t.id, label: t.label })));
  fillSelect(settingsFont, FONT_SCALES.map((f) => ({ value: f.id, label: f.label })));
  fillWindowOptions();
}

/**
 * 当前尺寸不在预设里时补一个选项，否则下拉框会悄悄跳到别的预设上。
 * 每次调用先重建预设，避免上一轮的「当前 xxx」残留在列表里。
 */
function syncWindowOptions(width: number, height: number): void {
  fillWindowOptions();
  const key = windowKey(width, height);
  if (!WINDOW_PRESETS.some((p) => windowKey(p.width, p.height) === key)) {
    const option = document.createElement("option");
    option.value = key;
    option.textContent = `当前 ${width} × ${height}`;
    settingsWindow.appendChild(option);
  }
  settingsWindow.value = key;
}

function openSettings(): void {
  proxyInput.value = settings.proxy;
  autoTranslateInput.checked = settings.autoTranslate;
  autoSpeakInput.checked = settings.autoSpeak;
  recordHistoryInput.checked = settings.historyMode === "all";
  settingsEngine.value = settings.engine;
  settingsTheme.value = settings.theme;
  settingsFont.value = settings.font;
  rememberWindowInput.checked = settings.rememberWindow;
  selectTranslateInput.checked = settings.selectTranslate;
  clipboardFallbackInput.checked = settings.clipboardFallback;
  shellMenuInput.checked = settings.shellMenu;
  syncWindowOptions(settings.windowWidth, settings.windowHeight);
  settingsMsg.textContent = "";
  settingsMsg.style.color = "";
  mask.hidden = false;
  proxyInput.focus();
}

function closeSettings(): void {
  if (saving) return;
  mask.hidden = true;
  $("btn-settings").focus();
}

$("btn-settings").addEventListener("click", openSettings);
$("btn-close-settings").addEventListener("click", closeSettings);

mask.addEventListener("click", (e) => {
  if (e.target === mask) closeSettings();
});

$("btn-save-settings").addEventListener("click", async () => {
  if (saving || !ready) return;
  const [width, height] = settingsWindow.value.split("x").map(Number);
  const next: Settings = {
    ...settings,
    proxy: proxyInput.value.trim(),
    autoTranslate: autoTranslateInput.checked,
    autoSpeak: autoSpeakInput.checked,
    engine: settingsEngine.value,
    historyMode: recordHistoryInput.checked ? "all" : "off",
    theme: settingsTheme.value,
    font: settingsFont.value,
    windowWidth: Number.isFinite(width) ? width : settings.windowWidth,
    windowHeight: Number.isFinite(height) ? height : settings.windowHeight,
    rememberWindow: rememberWindowInput.checked,
    selectTranslate: selectTranslateInput.checked,
    clipboardFallback: clipboardFallbackInput.checked,
    shellMenu: shellMenuInput.checked,
  };
  const resized =
    next.windowWidth !== settings.windowWidth || next.windowHeight !== settings.windowHeight;

  const button = $<HTMLButtonElement>("btn-save-settings");
  saving = true;
  button.disabled = true;
  settingsMsg.style.color = "";
  settingsMsg.textContent = "正在保存…";
  window.clearTimeout(debounceTimer);
  seq++;
  setLoading(false);
  setStatus("");
  if (!current) {
    result.replaceChildren();
    result.hidden = true;
  }
  try {
    await saveSettings(next);
    settings = next;
    enginePick.value = settings.engine;
    applyAppearance();
    syncMenuChecks();
    if (resized) {
      await resizeWindow(settings.windowWidth, settings.windowHeight).catch((err) =>
        showAlert(`调整窗口失败：${String(err)}`),
      );
    }
    settingsMsg.textContent = "已保存";
    saving = false;
    closeSettings();
    if (input.value.trim()) void run(input.value);
  } catch (err) {
    setLoading(false);
    settingsMsg.style.color = "#c0392b";
    settingsMsg.textContent = `保存失败：${String(err)}`;
  } finally {
    saving = false;
    button.disabled = false;
  }
});

// ── 说明弹窗 ────────────────────────────────────────────────

/**
 * 界面上不再挂小字说明，引擎能力、快捷键、数据位置统一放在这里。
 * 引擎表格由后端返回的 `EngineInfo` 驱动，加引擎不用改这个文件。
 */
function fillHelpEngines(): void {
  if (helpFilled) return;
  helpFilled = true;

  const tbody = $("help-engine-table").querySelector("tbody");
  const notes = $("help-engine-notes");
  tbody?.replaceChildren();
  notes.replaceChildren();

  for (const info of engineList) {
    const row = document.createElement("tr");
    for (const text of [
      info.name,
      info.needsProxy ? "需要" : "直连",
      info.supportsSentence ? "✓" : "—",
      info.supportsDictionary ? "✓" : "—",
      info.supportsAudio ? "✓" : "—",
    ]) {
      const cell = document.createElement("td");
      cell.textContent = text;
      row.appendChild(cell);
    }
    tbody?.appendChild(row);

    const li = document.createElement("li");
    const name = document.createElement("b");
    name.textContent = info.name;
    li.append(name, document.createTextNode(`：${info.note}`));
    notes.appendChild(li);
  }

  if (engineList.length === 0) {
    const li = document.createElement("li");
    li.textContent = "浏览器预览下读不到引擎列表，运行桌面程序后这里会列出三个引擎。";
    notes.appendChild(li);
  }
}

async function fillHelpPaths(): Promise<void> {
  let info: AppInfo | null = null;
  try {
    info = await appInfo();
  } catch {
    info = null;
  }

  const rows: [string, string][] = info
    ? [
        ["版本", info.version],
        ["设置文件", info.settingsPath],
        ["历史数据库", info.historyPath],
        ["配置目录", info.configDir],
        ["数据目录", info.dataDir],
      ]
    : [
        ["版本", "浏览器预览"],
        ["设置文件", "—"],
        ["历史数据库", "—"],
        ["配置目录", "—"],
        ["数据目录", "—"],
      ];

  const tbody = $("help-paths-table").querySelector("tbody");
  tbody?.replaceChildren();
  for (const [label, value] of rows) {
    const row = document.createElement("tr");
    const th = document.createElement("th");
    th.textContent = label;
    const td = document.createElement("td");
    td.textContent = value;
    row.append(th, td);
    tbody?.appendChild(row);
  }

  $("help-version").textContent = info
    ? `当前版本 ${info.version}，设置文件在 ${info.settingsPath}。`
    : "浏览器预览模式，不读取本机路径。";
}

function openHelp(section: string): void {
  fillHelpEngines();
  void fillHelpPaths();
  helpMask.hidden = false;
  helpBody.scrollTop = 0;
  $("btn-close-help").focus();

  if (section && section !== "all") {
    const target = document.getElementById(`help-${section}`);
    // 等弹窗完成布局再滚动，否则算出来的位置是旧的
    if (target) requestAnimationFrame(() => target.scrollIntoView({ block: "start" }));
  }
}

function closeHelp(): void {
  helpMask.hidden = true;
  const last = menus[menus.length - 1];
  const button = last?.querySelector<HTMLButtonElement>(".menu-btn");
  (button ?? $("btn-settings")).focus();
}

$("btn-close-help").addEventListener("click", closeHelp);
$("btn-close-help-2").addEventListener("click", closeHelp);
helpMask.addEventListener("click", (e) => {
  if (e.target === helpMask) closeHelp();
});

// ── 菜单栏 ──────────────────────────────────────────────────

interface MenuParts {
  button: HTMLButtonElement;
  popup: HTMLElement;
}

function menuParts(menu: HTMLElement): MenuParts {
  return {
    button: menu.querySelector<HTMLButtonElement>(".menu-btn")!,
    popup: menu.querySelector<HTMLElement>(".menu-pop")!,
  };
}

function menuItems(popup: HTMLElement): HTMLButtonElement[] {
  return Array.from(popup.querySelectorAll<HTMLButtonElement>(".menu-item"));
}

function menuIsOpen(): boolean {
  return menus.some((m) => menuParts(m).button.getAttribute("aria-expanded") === "true");
}

function openMenu(menu: HTMLElement, focusFirstItem = false): void {
  for (const other of menus) {
    const { button, popup } = menuParts(other);
    const active = other === menu;
    button.setAttribute("aria-expanded", String(active));
    popup.hidden = !active;
  }
  if (focusFirstItem) menuItems(menuParts(menu).popup)[0]?.focus();
}

function menuCloseAll(restoreFocus = false): void {
  const opened = menus.find(
    (m) => menuParts(m).button.getAttribute("aria-expanded") === "true",
  );
  for (const menu of menus) {
    const { button, popup } = menuParts(menu);
    button.setAttribute("aria-expanded", "false");
    popup.hidden = true;
  }
  if (restoreFocus && opened) menuParts(opened).button.focus();
}

/** 把「视图」菜单里的单选 / 复选标记对齐到当前设置。 */
function syncMenuChecks(): void {
  const state: Record<string, string | boolean> = {
    theme: settings.theme,
    font: settings.font,
    window: windowKey(settings.windowWidth, settings.windowHeight),
    "remember-window": settings.rememberWindow,
    "select-translate": settings.selectTranslate,
    "clipboard-fallback": settings.clipboardFallback,
    "shell-menu": settings.shellMenu,
  };
  for (const item of menubar.querySelectorAll<HTMLButtonElement>(".menu-item")) {
    const cmd = item.dataset.cmd ?? "";
    if (!(cmd in state)) continue;
    const expected = state[cmd];
    const on = typeof expected === "boolean" ? expected : item.dataset.arg === expected;
    item.classList.toggle("on", on);
    item.setAttribute("aria-checked", String(on));
  }
}

menubar.addEventListener("click", (e) => {
  const target = e.target as HTMLElement;
  const button = target.closest<HTMLButtonElement>(".menu-btn");
  if (button) {
    const menu = button.closest<HTMLElement>(".menu")!;
    if (button.getAttribute("aria-expanded") === "true") menuCloseAll();
    else openMenu(menu);
    return;
  }
  const item = target.closest<HTMLButtonElement>(".menu-item");
  if (item) {
    menuCloseAll();
    void runCommand(item.dataset.cmd ?? "", item.dataset.arg ?? "");
  }
});

// 菜单已经打开时，滑到另一个标题就跟着切，和原生菜单栏一致
for (const menu of menus) {
  menuParts(menu).button.addEventListener("pointerenter", () => {
    if (!menuIsOpen()) return;
    if (!menuParts(menu).popup.hidden) return; // 已经是展开的那个
    openMenu(menu);
  });
}

menubar.addEventListener("keydown", (e) => {
  const target = e.target as HTMLElement;
  const button = target.closest<HTMLButtonElement>(".menu-btn");
  if (button) {
    const menu = button.closest<HTMLElement>(".menu")!;
    const index = menus.indexOf(menu);
    if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      e.preventDefault();
      const step = e.key === "ArrowRight" ? 1 : menus.length - 1;
      const next = menus[(index + step) % menus.length];
      openMenu(next);
      menuParts(next).button.focus();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      openMenu(menu, true);
    }
    return;
  }

  const item = target.closest<HTMLButtonElement>(".menu-item");
  if (!item) return;
  const popup = item.closest<HTMLElement>(".menu-pop")!;
  const items = menuItems(popup);
  const index = items.indexOf(item);
  if (e.key === "ArrowDown" || e.key === "ArrowUp") {
    e.preventDefault();
    const step = e.key === "ArrowDown" ? 1 : items.length - 1;
    items[(index + step) % items.length].focus();
  } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
    e.preventDefault();
    const menu = popup.closest<HTMLElement>(".menu")!;
    const at = menus.indexOf(menu);
    const step = e.key === "ArrowRight" ? 1 : menus.length - 1;
    const next = menus[(at + step) % menus.length];
    openMenu(next);
    menuParts(next).button.focus();
  }
});

document.addEventListener("pointerdown", (e) => {
  if (!menuIsOpen()) return;
  if (!menubar.contains(e.target as Node)) menuCloseAll();
});

/** 双击窗口框架放大 / 还原（菜单按钮除外，双击它们只该开关菜单）。 */
document.querySelector<HTMLElement>(".chrome")?.addEventListener("dblclick", (e) => {
  if ((e.target as HTMLElement).closest("button")) return;
  void toggleMaximize().catch(() => {});
});

// ── 菜单命令 ────────────────────────────────────────────────

async function runCommand(cmd: string, arg: string): Promise<void> {
  switch (cmd) {
    case "export": {
      const format = EXPORT_FORMATS.find((f) => f.id === arg)?.id;
      if (!format) return;
      exportFormat.value = format;
      updateExportHint();
      await runExport(format);
      return;
    }
    case "open-history":
      void openHistory();
      return;
    case "clear-history":
      await clearAllHistory();
      return;
    case "settings":
      openSettings();
      return;
    case "quit":
      await quitApp().catch((err) => showAlert(`退出失败：${String(err)}`));
      return;
    case "translate":
      window.clearTimeout(debounceTimer);
      void run(input.value);
      return;
    case "copy":
      await copyTranslation();
      return;
    case "speak":
      speak(input.value.trim(), current?.sourceLang ?? "en", current?.speakUrl ?? null);
      return;
    case "swap":
      $("btn-swap").click();
      return;
    case "clear-input":
      clearInput();
      return;
    case "clear-cache":
      $("btn-clear-cache").click();
      return;
    case "theme":
      await setTheme(arg);
      return;
    case "font":
      await setFont(arg);
      return;
    case "window":
      await applyWindowPreset(arg);
      return;
    case "remember-window":
      await toggleRememberWindow();
      return;
    case "select-translate":
      await toggleSelectTranslate();
      return;
    case "clipboard-fallback":
      await toggleClipboardFallback();
      return;
    case "shell-menu":
      await toggleShellMenu();
      return;
    case "help":
      openHelp(arg || "all");
      return;
    default:
  }
}

// ── 划词翻译与资源管理器右键菜单 ────────────────────────────

/**
 * 这三个开关都落在设置文件里，后端在 `apply_settings` 里做幂等同步：
 * 划词开关决定鼠标钩子是否处理手势、悬浮图标是否保留；右键菜单开关决定注册表项。
 */
async function applyToggle(patch: Partial<Settings>, message: string): Promise<void> {
  const next: Settings = { ...settings, ...patch };
  try {
    await saveSettings(next);
    settings = next;
    syncMenuChecks();
    setStatus(message);
    window.setTimeout(() => setStatus(""), 1800);
  } catch (err) {
    // 保存失败就把菜单勾选退回真实状态，别让界面撒谎
    syncMenuChecks();
    showAlert(`设置未保存：${String(err)}`);
  }
}

async function toggleSelectTranslate(): Promise<void> {
  const on = !settings.selectTranslate;
  await applyToggle(
    { selectTranslate: on },
    on ? "划词翻译已开启" : "划词翻译已关闭",
  );
}

async function toggleClipboardFallback(): Promise<void> {
  const on = !settings.clipboardFallback;
  await applyToggle(
    { clipboardFallback: on },
    on ? "剪贴板兜底已开启" : "剪贴板兜底已关闭",
  );
}

async function toggleShellMenu(): Promise<void> {
  const on = !settings.shellMenu;
  await applyToggle(
    { shellMenu: on },
    on ? "资源管理器右键菜单已加入" : "资源管理器右键菜单已移除",
  );
}

/** `1280x820` → 立刻改窗口尺寸并记住。 */
async function applyWindowPreset(preset: string): Promise<void> {
  const [width, height] = preset.split("x").map(Number);
  if (!Number.isFinite(width) || !Number.isFinite(height)) return;
  await setWindowSize(width, height);
}

async function setWindowSize(width: number, height: number): Promise<void> {
  settings.windowWidth = width;
  settings.windowHeight = height;
  syncWindowOptions(width, height);
  syncMenuChecks();
  try {
    await resizeWindow(width, height);
  } catch (err) {
    showAlert(`调整窗口失败：${String(err)}`);
  }
  try {
    await saveSettings(settings);
    setStatus(windowLabel(width, height));
    window.setTimeout(() => setStatus(""), 1600);
  } catch (err) {
    showAlert(`窗口尺寸未保存：${String(err)}`);
  }
}

/** 用户自己拖出来的尺寸，只在「记住窗口大小」打开时才覆盖默认值。 */
async function persistWindowSize(): Promise<void> {
  const width = Math.round(window.innerWidth);
  const height = Math.round(window.innerHeight);
  if (width < WINDOW_MIN.width || height < WINDOW_MIN.height) return;
  if (width === settings.windowWidth && height === settings.windowHeight) return;
  settings.windowWidth = width;
  settings.windowHeight = height;
  syncWindowOptions(width, height);
  syncMenuChecks();
  try {
    await saveSettings(settings);
  } catch {
    /* 静默失败：缩放窗口不该弹错误框打断操作 */
  }
}

async function toggleRememberWindow(): Promise<void> {
  settings.rememberWindow = !settings.rememberWindow;
  rememberWindowInput.checked = settings.rememberWindow;
  syncMenuChecks();
  try {
    await saveSettings(settings);
    setStatus(settings.rememberWindow ? "已记住窗口大小" : "不再记住窗口大小");
    window.setTimeout(() => setStatus(""), 1600);
  } catch (err) {
    showAlert(`设置未保存：${String(err)}`);
  }
}

/** `1020x760`，菜单项与下拉框共用的键。 */
function windowKey(width: number, height: number): string {
  return `${width}x${height}`;
}

/** 弹窗内的 Tab 循环，防止焦点跑到弹窗背后的界面上。 */
function trapFocus(e: KeyboardEvent, container: HTMLElement): void {
  if (e.key !== "Tab") return;
  const controls = Array.from(
    container.querySelectorAll<HTMLElement>(
      "button:not(:disabled), input:not(:disabled), select:not(:disabled)",
    ),
  );
  const first = controls[0];
  const last = controls[controls.length - 1];
  if (!first || !last) return;
  if (e.shiftKey && document.activeElement === first) {
    e.preventDefault();
    last.focus();
  } else if (!e.shiftKey && document.activeElement === last) {
    e.preventDefault();
    first.focus();
  }
}

document.addEventListener("keydown", (e) => {
  // 覆盖层优先级：历史抽屉 > 说明 > 设置 > 菜单
  if (!drawer.hidden) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeHistory();
    }
    return;
  }
  if (!helpMask.hidden) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeHelp();
    }
    trapFocus(e, helpMask);
    return;
  }
  if (!mask.hidden) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeSettings();
    }
    trapFocus(e, mask);
    return;
  }

  if (e.key === "F1") {
    e.preventDefault();
    menuCloseAll();
    openHelp("all");
    return;
  }
  // Alt 聚焦菜单栏，之后用 ← → 选菜单、↑ ↓ 选项、Enter 执行
  if (e.key === "Alt") {
    e.preventDefault();
    menus[0]?.querySelector<HTMLButtonElement>(".menu-btn")?.focus();
    return;
  }
  if (e.key === "Escape") {
    if (menuIsOpen()) {
      e.preventDefault();
      menuCloseAll(true);
    }
    return;
  }
  if (e.key.toLowerCase() === "h" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    menuCloseAll();
    void openHistory();
  }
});

// ── 启动 ────────────────────────────────────────────────────

// ── 划词 / 右键菜单把文字送进来 ─────────────────────────────

/**
 * 外部来源（划词悬浮图标、资源管理器右键菜单）送来的文字统一走这里：
 * 填进输入框、立即翻译、把窗口提到最前。**不改动任何外部内容**，
 * 原文只在这里被读取，翻译结果也只出现在本窗口。
 */
function translateExternal(text: string): void {
  const value = text.trim();
  if (!value) return;

  window.clearTimeout(debounceTimer);
  seq++;
  input.value = value;
  autoGrow();
  input.setSelectionRange(value.length, value.length);
  input.focus();
  hideAlert();
  void run(value);
}

// ── 启动 ────────────────────────────────────────────────────

async function boot(): Promise<void> {
  input.disabled = true;
  langPair.disabled = true;
  enginePick.disabled = true;

  try {
    engineList = await fetchEngines();
  } catch {
    engineList = [];
  }
  fillEngineOptions();

  try {
    settings = normalizeSettings(await loadSettings());
  } catch (err) {
    showAlert(`读取设置失败：${String(err)}。可打开设置重新保存。`);
  }

  // 皮肤与字号尽早落地，免得先闪一下默认外观
  applyAppearance();
  fillSettingsOptions();

  if (!Array.from(langPair.options).some((o) => o.value === settings.langPair)) {
    settings.langPair = DEFAULTS.langPair;
  }
  langPair.value = settings.langPair;
  if (!engineList.some((e) => e.id === settings.engine)) {
    settings.engine = engineList[0]?.id ?? DEFAULTS.engine;
  }
  enginePick.value = settings.engine;
  rememberWindowInput.checked = settings.rememberWindow;
  syncWindowOptions(settings.windowWidth, settings.windowHeight);
  syncMenuChecks();
  updateExportHint();

  ready = true;
  input.disabled = false;
  langPair.disabled = false;
  enginePick.disabled = false;
  autoGrow();
  input.focus();

  if (!isTauri()) showAlert("浏览器预览模式。运行 dev.bat 或桌面程序后即可翻译。");
  void refreshCache();
  void refreshHistoryStat();

  void initExternalInput();
}

/**
 * 接上两条外部输入通道，并把「资源管理器右键菜单」的勾选对齐到注册表真实状态
 * （用户可能在别处删掉了注册表项，界面不能凭设置文件想当然）。
 */
async function initExternalInput(): Promise<void> {
  if (isTauri()) {
    try {
      await listen<string>("translate-selection", (event) => {
        translateExternal(event.payload ?? "");
      });
    } catch (err) {
      showAlert(`划词通道未接上：${String(err)}`);
    }
  }

  // 冷启动时 `--translate-file` 带进来的内容，后端只给一次
  try {
    const startup = await takeStartupText();
    if (startup) translateExternal(startup);
  } catch {
    // 浏览器预览下没有这个命令
  }

  if (!isTauri()) return;
  try {
    const registered = await shellMenuRegistered();
    if (registered !== settings.shellMenu) {
      settings.shellMenu = registered;
      syncMenuChecks();
      shellMenuInput.checked = registered;
    }
  } catch {
    // 非 Windows 或注册表不可读时忽略
  }
}

void boot();
