// Regression tests against the real UI with a deterministic Tauri IPC fixture.
// Optional first argument: absolute path to an available Playwright package.
import { createRequire } from "node:module";
import assert from "node:assert/strict";
import { mkdir } from "node:fs/promises";
const require = createRequire(import.meta.url);
const { chromium } = require(process.argv[2] || "playwright");
const browser = await chromium.launch({ channel: "msedge", headless: true });
await mkdir("test-results", { recursive: true });
let checks = 0;
const check = (value, message) => { assert.ok(value, message); console.log(`PASS ${++checks}: ${message}`); };
try {
  const page = await browser.newPage({ viewport: { width: 1020, height: 730 } });
  const errors = [];
  page.on("pageerror", e => errors.push(e.message));
  // 清空历史用的是 window.confirm，Playwright 默认会自动取消。
  page.on("dialog", d => d.accept());
  await page.addInitScript(() => {
    window.isTauri = true;
    window.calls = [];
    window.fakeSettings = {
      proxy: "", autoTranslate: true, autoSpeak: false, langPair: "auto|zh-CN",
      engine: "google", historyMode: "all",
    };
    window.fakeHistory = [
      { id: 2, query: "coercion", fromLang: "en", toLang: "zh-CN", engine: "youdao",
        translation: "强迫", isWord: true, createdAt: 1760000060000 },
      { id: 1, query: "hello", fromLang: "en", toLang: "zh-CN", engine: "google",
        translation: "你好", isWord: true, createdAt: 1760000000000 },
    ];
    window.exportedTo = null;
    const cache = new Set();
    const engines = [
      { id: "google", name: "Google 翻译", note: "大陆需代理", needsProxy: true, supportsSentence: true, supportsDictionary: true, supportsAudio: false },
      { id: "youdao", name: "有道词典", note: "国内直连，词典最全", needsProxy: false, supportsSentence: true, supportsDictionary: true, supportsAudio: true },
      { id: "youdaoTrans", name: "有道翻译", note: "国内直连，词句皆可", needsProxy: false, supportsSentence: true, supportsDictionary: false, supportsAudio: true },
    ];
    window.__TAURI_INTERNALS__ = { invoke: async (cmd, args) => {
      window.calls.push({ cmd, args });
      if (cmd === "load_settings") return window.fakeSettings;
      if (cmd === "save_settings") {
        if (args.settings.proxy === "invalid") throw "输入或设置无效：代理地址无效";
        window.fakeSettings = args.settings;
        return;
      }
      if (cmd === "engines") return engines;
      if (cmd === "cache_stats") return { size: cache.size };
      if (cmd === "clear_cache") { cache.clear(); return; }
      if (cmd === "history_stats") return { total: window.fakeHistory.length, sizeBytes: 20480 };
      if (cmd === "history_list") {
        const filter = args.filter ?? "";
        const items = window.fakeHistory.filter(
          h => !filter || h.query.includes(filter) || h.translation.includes(filter));
        return { items: items.slice(args.offset, args.offset + args.limit), total: items.length };
      }
      if (cmd === "history_get") {
        const hit = window.fakeHistory.find(h => h.id === args.id);
        if (!hit) return null;
        return {
          query: hit.query, sourceLang: hit.fromLang, targetLang: hit.toLang, engine: hit.engine,
          translation: hit.translation, segments: [{ source: hit.query, text: hit.translation }],
          romanization: null, phonetic: null, phoneticUk: null, speakUrl: null, targetSpeakUrl: null,
          dictionary: [{ pos: "noun", terms: ["强迫"], back: [], exams: [] }], alternatives: [],
          synonyms: [], definitions: [], examples: [], phrases: [], related: [], sections: [],
          isWord: hit.isWord, cached: true, elapsedMs: 0,
        };
      }
      if (cmd === "history_delete") {
        window.fakeHistory = window.fakeHistory.filter(h => h.id !== args.id);
        return true;
      }
      if (cmd === "history_clear") {
        const n = window.fakeHistory.length;
        window.fakeHistory = [];
        return n;
      }
      if (cmd === "export_history") {
        window.exportedTo = { format: args.format, path: args.path };
        return 2;
      }
      if (cmd === "plugin:dialog|save") return "C:\\tmp\\history.csv";
      if (cmd === "translate") {
        await new Promise(resolve => setTimeout(resolve, args.text === "slow" ? 350 : 20));
        if (args.text === "network-failure") throw "connect error";
        const cached = cache.has(args.text);
        cache.add(args.text);
        return {
          query: args.text, sourceLang: args.from === "auto" ? "en" : args.from,
          targetLang: args.to, engine: args.engine ?? "google",
          translation: `译文：${args.text}`,
          segments: [{ source: args.text, text: `译文：${args.text}` }],
          romanization: "Qiǎngpò", phonetic: "test", phoneticUk: "test-uk",
          speakUrl: null, targetSpeakUrl: null,
          dictionary: [{ pos: "noun", terms: ["译词"], back: [], exams: ["CET4"] }],
          alternatives: ["候选"],
          definitions: [{ pos: "noun", definitions: [{ text: "fixture definition", example: "example", similar: ["near"] }], source: "WordNet" }],
          synonyms: [{ pos: "noun", words: ["synonym"], gloss: "同义说明" }],
          examples: [{ text: 'a <b>word</b> <img src=x onerror="window.injected=true">', translation: "一个词", source: "《牛津词典》" }],
          phrases: [{ term: "take off", translation: "起飞", source: "21世纪" }],
          related: [{ pos: "adj.", words: [{ word: "coercive", gloss: "强制的" }] }],
          sections: [
            { key: "collins", title: "柯林斯", note: "柯林斯英汉双解大词典", markdown: false, groups: [
              { label: "teɪk", note: null, items: [
                { text: "to hold something", translation: "拿", note: "1",
                  extra: [{ text: "Let me take your coat.", translation: "我来帮你拿大衣。", source: null }] }] }] },
            { key: "ai", title: "AI 释义", note: "由有道 AI 生成，仅供参考", markdown: true, groups: [
              { label: "", note: null, items: [
                { text: "### 核心\n- 表示拿走\n**重点**", translation: null, note: null, extra: [] }] }] },
          ],
          isWord: true, cached, elapsedMs: 20,
        };
      }
      throw `Unexpected command: ${cmd}`;
    }};
  });
  await page.goto("http://127.0.0.1:1420");
  await page.waitForFunction(() => !document.querySelector("#input").disabled);

  // ── 基础状态 ──────────────────────────────────────────────
  check(await page.locator("#mask").isHidden(), "settings overlay hidden at startup");
  check(await page.locator("#drawer").isHidden(), "history drawer hidden at startup");
  check(await page.locator("#empty").isVisible(), "empty state visible at startup");

  // ── 引擎选择器 ────────────────────────────────────────────
  check(await page.locator("#engine option").count() === 3, "engine selector is populated from the backend");
  check(await page.locator("#engine").inputValue() === "google", "engine selector restores the saved engine");
  check((await page.locator("#engine-note").innerText()).includes("代理"), "engine note explains proxy requirement");
  check((await page.locator("#history-count").innerText()) === "2", "toolbar shows the history total");

  // ── 翻译与板块 ────────────────────────────────────────────
  await page.locator("#input").fill("hello");
  await page.locator("#input").press("Control+Enter");
  await page.waitForFunction(() => document.querySelector(".main-trans")?.textContent === "译文：hello");
  check(await page.locator("#empty").isHidden(), "empty state hidden after result");
  check((await page.locator("#query-meta").innerText()).includes("美"), "phonetic shows both US and UK readings");
  check((await page.locator("#query-meta").innerText()).includes("Google 翻译"), "result meta names the engine");
  check((await page.locator(".dict-head .exam").innerText()) === "CET4", "dictionary shows exam tags");

  const tabNames = await page.locator(".tab").allInnerTexts();
  for (const name of ["简明", "柯林斯", "英英释义", "词组", "同根词", "同近义词", "例句", "AI 释义"]) {
    check(tabNames.some(t => t.startsWith(name)), `tab "${name}" is present`);
  }

  await page.getByRole("button", { name: "英英释义" }).click();
  check((await page.locator(".panel").innerText()).includes("fixture definition"), "definition tab renders");
  check((await page.locator(".panel").innerText()).includes("WordNet"), "definition source is shown");

  await page.getByRole("button", { name: "简明", exact: true }).click();
  check((await page.locator(".panel").innerText()).includes("译词"), "returning to dictionary retains fragment content");

  await page.getByRole("button", { name: "例句" }).click();
  check(await page.locator(".panel img").count() === 0 && await page.locator(".panel b").count() === 1,
    "examples escape markup while preserving bold");
  check((await page.locator(".panel").innerText()).includes("一个词"), "example translation is rendered");
  check((await page.locator(".panel").innerText()).includes("《牛津词典》"), "example source is rendered");

  await page.getByRole("button", { name: "词组" }).click();
  check((await page.locator(".panel").innerText()).includes("take off"), "phrase tab renders");

  await page.getByRole("button", { name: "柯林斯" }).click();
  check((await page.locator(".panel").innerText()).includes("to hold something"), "collins section renders");
  check((await page.locator(".panel").innerText()).includes("我来帮你拿大衣。"), "collins example translation renders");

  await page.getByRole("button", { name: "AI 释义" }).click();
  check(await page.locator(".panel .md h4").count() === 1, "AI markdown heading renders");
  check((await page.locator(".panel .md b").innerText()).includes("重点"), "AI markdown bold renders");
  check((await page.locator(".panel .md li").count()) === 1, "AI markdown list renders");
  await page.screenshot({ path: "test-results/result.png" });

  // ── 切换引擎 ──────────────────────────────────────────────
  await page.locator("#engine").selectOption("youdao");
  await page.waitForFunction(() => document.querySelector("#query-meta")?.innerText.includes("有道词典"));
  const lastTranslate = await page.evaluate(() => window.calls.filter(c => c.cmd === "translate").pop());
  check(lastTranslate.args.engine === "youdao", "switching engine passes the new engine to the backend");
  check(await page.evaluate(() => window.fakeSettings.engine === "youdao"), "engine choice is persisted");

  // ── 历史抽屉 ──────────────────────────────────────────────
  await page.locator("#btn-history").click();
  await page.waitForFunction(() => !document.querySelector("#drawer").hidden);
  check(await page.locator(".history-item").count() === 2, "history drawer lists stored entries");
  check((await page.locator("#history-stat").innerText()).includes("2 条"), "history drawer shows the total");
  check((await page.locator("#history-stat").innerText()).includes("KB"), "history drawer shows the database size");
  check((await page.locator(".history-item").first().innerText()).includes("有道词典"), "history entry resolves the engine name");

  await page.locator("#history-search").fill("coercion");
  await page.waitForFunction(() => document.querySelectorAll(".history-item").length === 1);
  check((await page.locator(".history-item").innerText()).includes("强迫"), "history search filters by query");

  await page.locator("#history-search").fill("zzz");
  await page.waitForFunction(() => document.querySelector(".history-empty") !== null);
  check((await page.locator(".history-empty").innerText()).includes("没有匹配"), "empty search shows a clear message");

  await page.locator("#history-search").fill("");
  await page.waitForFunction(() => document.querySelectorAll(".history-item").length === 2);

  await page.locator(".history-item").first().locator(".hi-del").click();
  await page.waitForFunction(() => document.querySelectorAll(".history-item").length === 1);
  check(await page.evaluate(() => window.fakeHistory.length === 1), "deleting an entry removes it from the store");

  await page.locator("#export-format").selectOption("anki");
  check((await page.locator("#export-hint").innerText()).includes("制表符"), "export hint follows the selected format");
  await page.locator("#btn-export").click();
  await page.waitForFunction(() => window.exportedTo !== null);
  check(await page.evaluate(() => window.exportedTo.format === "anki"), "export passes the chosen format");
  check((await page.locator("#history-msg").innerText()).includes("已导出 2 条"), "export reports the row count");

  // 上一步已经删掉 coercion，这里剩下的那条是 hello，断言要跟着走。
  const entry = page.locator(".history-item").first();
  const expectedQuery = (await entry.locator(".hi-query").innerText()).trim();
  const expectedTranslation = (await entry.locator(".hi-trans").innerText()).trim();
  await entry.click();
  await page.waitForFunction(() => document.querySelector("#drawer").hidden);
  check((await page.locator(".main-trans").innerText()).includes(expectedTranslation),
    "clicking a history entry restores the result");
  check((await page.locator("#input").inputValue()) === expectedQuery,
    "restoring history fills the input box");

  await page.locator("#btn-history").click();
  await page.waitForFunction(() => !document.querySelector("#drawer").hidden);
  await page.locator("#btn-clear-history").click();
  await page.waitForFunction(() => document.querySelector(".history-empty") !== null);
  check(await page.evaluate(() => window.fakeHistory.length === 0), "clearing history empties the store");
  await page.locator("#btn-close-history").click();
  check(await page.locator("#drawer").isHidden(), "history drawer closes");

  // ── 设置 ──────────────────────────────────────────────────
  await page.locator("#btn-settings").click();
  check(await page.locator("#mask").isVisible(), "settings open on demand");
  check(await page.locator("#settings-engine option").count() === 3, "settings dialog lists engines");
  // 说明行展示的是所选引擎的 note（与工具栏 #engine-note 一致），不是引擎名。
  const engineNote = await page.locator("#settings-engine-note").innerText();
  check(engineNote.includes("国内直连") && engineNote.includes("无需代理"),
    "settings dialog describes the selected engine");

  await page.locator("#proxy").fill("invalid");
  await page.locator("#btn-save-settings").click();
  await page.waitForFunction(() => document.querySelector("#settings-msg").textContent.includes("保存失败"));
  check(await page.evaluate(() => window.fakeSettings.proxy === ""), "invalid settings do not replace saved configuration");

  await page.locator("#proxy").fill("");
  await page.locator("#auto-translate").uncheck();
  await page.locator("#settings-engine").selectOption("google");
  await page.locator("#record-history").uncheck();
  await page.locator("#btn-save-settings").click();
  await page.waitForFunction(() => document.querySelector("#mask").hidden);
  check(await page.locator("#mask").isHidden(), "successful save closes settings");
  check(await page.evaluate(() => window.fakeSettings.historyMode === "off"), "history recording toggle is persisted");
  check(await page.locator("#engine").inputValue() === "google", "settings engine choice syncs to the toolbar");

  // ── 错误与竞态 ────────────────────────────────────────────
  await page.locator("#input").fill("network-failure");
  await page.locator("#input").press("Control+Enter");
  await page.waitForFunction(() => !document.querySelector("#alert").hidden);
  check(await page.locator(".skeleton").count() === 0 && await page.locator("#result").isHidden(),
    "network error removes skeleton and stale result");

  await page.locator("#input").fill("slow");
  await page.locator("#input").press("Control+Enter");
  await page.waitForFunction(() => window.calls.some(c => c.cmd === "translate" && c.args.text === "slow"));
  await page.locator("#input").fill("new input");
  await page.waitForTimeout(450);
  check(await page.locator("#result").isHidden(), "editing invalidates pending response immediately");

  await page.locator("#input").press("Control+Enter");
  await page.waitForFunction(() => document.querySelector(".main-trans")?.textContent === "译文：new input");
  check(await page.locator("#alert").isHidden(), "new request recovers after error");

  await page.locator("#input").press("Control+Enter");
  await page.waitForFunction(() => document.querySelector("#status").textContent.includes("缓存"));
  check((await page.locator("#status").innerText()).includes("缓存"), "cached result is identified");

  await page.locator("#btn-swap").click();
  check(await page.locator("#lang-pair").inputValue() === "zh-CN|en", "auto-detected language can be swapped");
  check(await page.locator("#input").inputValue() === "译文：new input", "swap uses prior translation as source");

  await page.locator("#input").fill("x".repeat(5001));
  await page.locator("#input").press("Control+Enter");
  check((await page.locator("#alert").innerText()).includes("5000"), "oversized text shows actionable validation");
  await page.locator("#input").press("Escape");
  check(await page.locator("#empty").isVisible() && await page.locator("#result").isHidden(),
    "Escape restores clean empty state");

  // ── 快捷键与焦点 ──────────────────────────────────────────
  await page.keyboard.press("Control+h");
  await page.waitForFunction(() => !document.querySelector("#drawer").hidden);
  check(await page.locator("#drawer").isVisible(), "Ctrl+H opens the history drawer");
  await page.keyboard.press("Escape");
  check(await page.locator("#drawer").isHidden(), "Escape closes the history drawer");

  await page.locator("#btn-settings").click();
  await page.locator("#btn-save-settings").focus();
  await page.keyboard.press("Tab");
  check(await page.locator("#btn-close-settings").evaluate(el => el === document.activeElement),
    "dialog traps keyboard focus");
  await page.keyboard.press("Escape");
  check(await page.locator("#btn-settings").evaluate(el => el === document.activeElement),
    "closing dialog restores opener focus");

  // ── 布局 ──────────────────────────────────────────────────
  await page.setViewportSize({ width: 720, height: 480 });
  await page.screenshot({ path: "test-results/compact.png" });
  check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    "minimum desktop viewport does not overflow");
  await page.locator("#btn-history").click();
  await page.screenshot({ path: "test-results/history.png" });
  check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth),
    "history drawer does not overflow the narrow viewport");
  await page.locator("#btn-close-history").click();

  check(errors.length === 0, `no uncaught UI errors (${errors.join(", ")})`);

  const preview = await browser.newPage();
  await preview.goto("http://127.0.0.1:1420");
  await preview.waitForFunction(() => !document.querySelector("#input").disabled);
  check((await preview.locator("#alert").innerText()).includes("浏览器预览"),
    "browser preview explains desktop requirement");
  console.log(`${checks} UI regression checks passed. Screenshots use mocked IPC, not live translation.`);
} finally { await browser.close(); }
