/** 应用主逻辑：输入 → 翻译 → 渲染。 */

import { cacheStats, clearCache, translate } from "./api";
import { isTauri } from "@tauri-apps/api/core";
import { DEFAULTS, loadSettings, saveSettings, type Settings } from "./settings";
import { renderResult } from "./render";
import type { TranslateResult } from "./types";
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
const cacheSize = $("cache-size");
const mask = $("mask");
const proxyInput = $<HTMLInputElement>("proxy");
const autoTranslateInput = $<HTMLInputElement>("auto-translate");
const autoSpeakInput = $<HTMLInputElement>("auto-speak");
const settingsMsg = $("settings-msg");

// ── 设置 ────────────────────────────────────────────────────

let settings: Settings = { ...DEFAULTS };
let ready = false;
let composing = false;
let saving = false;

// ── 运行时状态 ──────────────────────────────────────────────

/** 请求序号，用于丢弃过期响应 */
let seq = 0;
let current: TranslateResult | null = null;
let debounceTimer: number | undefined;

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

  setStatus("翻译中", true);
  setLoading(true);
  hideAlert();

  try {
    const r = await translate(q, from, to);
    if (my !== seq) return; // 已被更新的请求取代

    current = r;
    empty.hidden = true;
    result.hidden = false;
    renderResult(result, r, queryAgain);
    renderMeta(r);
    setStatus(r.cached ? `缓存 · 0 ms` : `${r.elapsedMs} ms`);
    void refreshCache();

    if (settings.autoSpeak && r.isWord) speak(q, r.sourceLang);
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
input.addEventListener("compositionstart", () => { composing = true; window.clearTimeout(debounceTimer); });
input.addEventListener("compositionend", () => { composing = false; onInput(); });
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
  const act = (e.target as HTMLElement).closest<HTMLElement>("[data-act]")
    ?.dataset.act;
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
    speak(input.value.trim(), current?.sourceLang ?? "en");
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

function speak(text: string, lang: string): void {
  if (!text) return;
  if (!("speechSynthesis" in window)) { setStatus("当前系统不支持朗读"); return; }
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
    return "Google 接口限流（HTTP 429），多次重试仍未成功。稍等片刻再试，或配置一个稳定的代理。";
  }
  if (s.includes("dns") || s.includes("connect") || s.includes("timed out") || s.includes("网络请求失败")) {
    return "无法连接 Google。请检查网络，在设置中填写可用代理后重试。";
  }
  return s;
}

/** 渲染查询词条下方的音标 / 拼音 / 语言标签。 */
function renderMeta(r: TranslateResult): void {
  queryMeta.innerHTML = "";

  if (r.phonetic) {
    const s = document.createElement("span");
    s.className = "ipa";
    s.textContent = `/${r.phonetic.replace(/^\/|\/$/g, "")}/`;
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
}

async function refreshCache(): Promise<void> {
  try {
    const s = await cacheStats();
    cacheSize.textContent = String(s.size);
  } catch {
    /* 忽略 */
  }
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

$("btn-swap").addEventListener("click", () => {
  const [from, to] = langPair.value.split("|");
  const source = from === "auto" ? current?.sourceLang : from;
  if (!source || source === "auto") { setStatus("请先翻译一次以检测源语言"); return; }
  const reversed = `${to}|${source}`;
  const option = Array.from(langPair.options).find((o) => o.value === reversed);
  if (option) {
    if (current) { input.value = current.translation; autoGrow(); }
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
  } catch (err) { showAlert(`清空缓存失败：${String(err)}`); }
});

// ── 设置弹窗 ────────────────────────────────────────────────

function openSettings(): void {
  proxyInput.value = settings.proxy;
  autoTranslateInput.checked = settings.autoTranslate;
  autoSpeakInput.checked = settings.autoSpeak;
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
  const next: Settings = { ...settings, proxy: proxyInput.value.trim(), autoTranslate: autoTranslateInput.checked, autoSpeak: autoSpeakInput.checked };
  const button = $<HTMLButtonElement>("btn-save-settings");
  saving = true;
  button.disabled = true;
  settingsMsg.style.color = "";
  settingsMsg.textContent = "正在保存…";
  window.clearTimeout(debounceTimer);
  seq++;
  setLoading(false);
  setStatus("");
  if (!current) { result.replaceChildren(); result.hidden = true; }
  try {
    await saveSettings(next);
    settings = next;
    settingsMsg.textContent = "已保存";
    saving = false;
    closeSettings();
    if (input.value.trim()) void run(input.value);
  } catch (err) {
    setLoading(false);
    settingsMsg.style.color = "#c0392b";
    settingsMsg.textContent = `保存失败：${String(err)}`;
  } finally { saving = false; button.disabled = false; }
});

document.addEventListener("keydown", (e) => {
  if (mask.hidden) return;
  if (e.key === "Escape") { e.preventDefault(); closeSettings(); }
  if (e.key === "Tab") {
    const controls = Array.from(mask.querySelectorAll<HTMLElement>("button:not(:disabled), input:not(:disabled)"));
    const first = controls[0], last = controls[controls.length - 1];
    if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
    else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
  }
});

// ── 启动 ────────────────────────────────────────────────────

async function boot(): Promise<void> {
  input.disabled = true;
  langPair.disabled = true;
  try { settings = await loadSettings(); }
  catch (err) { showAlert(`读取设置失败：${String(err)}。可打开设置重新保存。`); }
  if (!Array.from(langPair.options).some((o) => o.value === settings.langPair)) settings.langPair = DEFAULTS.langPair;
  langPair.value = settings.langPair;
  ready = true;
  input.disabled = false;
  langPair.disabled = false;
  autoGrow();
  input.focus();

  if (!isTauri()) showAlert("浏览器预览模式。运行 dev.bat 或桌面程序后即可翻译。");
  void refreshCache();
}

void boot();
