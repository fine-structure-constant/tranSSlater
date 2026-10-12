/** 应用主逻辑：输入 → 翻译 → 渲染；引擎切换；历史库与导出。 */

import {
  cacheStats,
  clearCache,
  engines as fetchEngines,
  exportHistory,
  historyClear,
  historyDelete,
  historyGet,
  historyList,
  historyStats,
  translate,
} from "./api";
import { isTauri } from "@tauri-apps/api/core";
import { save as saveDialog } from "@tauri-apps/plugin-dialog";
import {
  DEFAULTS,
  EXPORT_FORMATS,
  defaultExportName,
  loadSettings,
  saveSettings,
  type Settings,
} from "./settings";
import { renderResult } from "./render";
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
const engineNote = $("engine-note");
const cacheSize = $("cache-size");
const historyCount = $("history-count");
const mask = $("mask");
const proxyInput = $<HTMLInputElement>("proxy");
const autoTranslateInput = $<HTMLInputElement>("auto-translate");
const autoSpeakInput = $<HTMLInputElement>("auto-speak");
const recordHistoryInput = $<HTMLInputElement>("record-history");
const settingsEngine = $<HTMLSelectElement>("settings-engine");
const settingsEngineNote = $("settings-engine-note");
const settingsMsg = $("settings-msg");

const drawer = $("drawer");
const drawerMask = $("drawer-mask");
const historyListEl = $("history-list");
const historyStat = $("history-stat");
const historySearch = $<HTMLInputElement>("history-search");
const historyMsg = $("history-msg");
const exportFormat = $<HTMLSelectElement>("export-format");
const exportHint = $("export-hint");
const historyMore = $<HTMLButtonElement>("btn-history-more");

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

function updateEngineNote(): void {
  const info = engineList.find((e) => e.id === enginePick.value);
  engineNote.textContent = info?.note ?? "";
  engineNote.title = info?.note ?? "";
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
input.addEventListener("input", onInput);
input.addEventListener("compositionstart", () => {
  composing = true;
  window.clearTimeout(debounceTimer);
});
input.addEventListener("compositionend", () => {
  composing = false;
  onInput();
});
window.addEventListener("resize", autoGrow);

input.addEventListener("keydown", (e) => {
  if (e.isComposing || !mask.hidden) return;
  if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
    e.preventDefault();
    window.clearTimeout(debounceTimer);
    void run(input.value);
  } else if (e.key === "Escape") {
    e.preventDefault();
    input.value = "";
    autoGrow();
    reset();
  }
});

// ── 词条区操作按钮 ──────────────────────────────────────────

document.querySelector(".query-actions")?.addEventListener("click", (e) => {
  const act = (e.target as HTMLElement).closest<HTMLElement>("[data-act]")?.dataset.act;
  if (!act) return;

  if (act === "clear") {
    input.value = "";
    autoGrow();
    reset();
    input.focus();
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
  updateEngineNote();
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

$("btn-clear-history").addEventListener("click", async () => {
  if (!historyTotal) return;
  if (!window.confirm(`确定清空全部 ${historyTotal} 条历史？此操作不可撤销。`)) return;
  try {
    const n = await historyClear();
    historyMsg.textContent = `已清空 ${n} 条`;
    await refreshHistoryStat();
    await loadHistory(true);
  } catch (err) {
    historyMsg.textContent = `清空失败：${String(err)}`;
  }
});

// ── 导出 ────────────────────────────────────────────────────

function updateExportHint(): void {
  const f = EXPORT_FORMATS.find((x) => x.id === exportFormat.value);
  exportHint.textContent = f ? f.hint : "";
}

exportFormat.addEventListener("change", updateExportHint);

$("btn-export").addEventListener("click", async () => {
  const format = exportFormat.value as ExportFormat;
  const meta = EXPORT_FORMATS.find((f) => f.id === format);
  if (!meta) return;

  if (!isTauri()) {
    historyMsg.textContent = "浏览器预览下无法导出，请运行桌面程序。";
    return;
  }
  if (!historyTotal) {
    historyMsg.textContent = "还没有可导出的记录。";
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
  } catch (err) {
    historyMsg.textContent = `导出失败：${String(err)}`;
  } finally {
    button.disabled = false;
  }
});

// ── 设置弹窗 ────────────────────────────────────────────────

function syncEngineNote(): void {
  const info = engineList.find((e) => e.id === settingsEngine.value);
  settingsEngineNote.textContent = info
    ? `${info.note}${info.needsProxy ? "（需要代理）" : "（无需代理）"}`
    : "";
}

function openSettings(): void {
  proxyInput.value = settings.proxy;
  autoTranslateInput.checked = settings.autoTranslate;
  autoSpeakInput.checked = settings.autoSpeak;
  recordHistoryInput.checked = settings.historyMode === "all";
  settingsEngine.value = settings.engine;
  syncEngineNote();
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
settingsEngine.addEventListener("change", syncEngineNote);

mask.addEventListener("click", (e) => {
  if (e.target === mask) closeSettings();
});

$("btn-save-settings").addEventListener("click", async () => {
  if (saving || !ready) return;
  const next: Settings = {
    ...settings,
    proxy: proxyInput.value.trim(),
    autoTranslate: autoTranslateInput.checked,
    autoSpeak: autoSpeakInput.checked,
    engine: settingsEngine.value,
    historyMode: recordHistoryInput.checked ? "all" : "off",
  };
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
    updateEngineNote();
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

document.addEventListener("keydown", (e) => {
  // 历史抽屉优先响应
  if (!drawer.hidden) {
    if (e.key === "Escape") {
      e.preventDefault();
      closeHistory();
    }
    return;
  }
  if (mask.hidden) {
    if (e.key.toLowerCase() === "h" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      openHistory();
    }
    return;
  }
  if (e.key === "Escape") {
    e.preventDefault();
    closeSettings();
  }
  if (e.key === "Tab") {
    const controls = Array.from(
      mask.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled), select:not(:disabled)"),
    );
    const first = controls[0];
    const last = controls[controls.length - 1];
    if (e.shiftKey && document.activeElement === first) {
      e.preventDefault();
      last.focus();
    } else if (!e.shiftKey && document.activeElement === last) {
      e.preventDefault();
      first.focus();
    }
  }
});

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
    settings = await loadSettings();
  } catch (err) {
    showAlert(`读取设置失败：${String(err)}。可打开设置重新保存。`);
  }

  if (!Array.from(langPair.options).some((o) => o.value === settings.langPair)) {
    settings.langPair = DEFAULTS.langPair;
  }
  langPair.value = settings.langPair;
  if (!engineList.some((e) => e.id === settings.engine)) {
    settings.engine = engineList[0]?.id ?? DEFAULTS.engine;
  }
  enginePick.value = settings.engine;
  updateEngineNote();
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
}

void boot();
