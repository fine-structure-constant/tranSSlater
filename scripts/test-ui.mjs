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
      theme: "plain", font: "md", windowWidth: 1020, windowHeight: 760, rememberWindow: false,
      selectTranslate: true, clipboardFallback: true, shellMenu: true,
    };
    // 划词 / 右键菜单通道的替身
    window.eventListeners = [];
    window.startupText = null;
    window.registryHasShellMenu = true;
    let callbackSeq = 0;
    const transformCallback = (callback, once = false) => {
      const id = ++callbackSeq;
      window[`_${id}`] = (payload) => {
        if (once) delete window[`_${id}`];
        callback(payload);
      };
      return id;
    };
    // 测试里用它模拟后端 emit("translate-selection", ...) 到主窗口
    window.emitEvent = (event, payload) => {
      for (const listener of window.eventListeners) {
        if (listener.event === event) window[`_${listener.handler}`]({ event, id: 0, payload });
      }
    };
    window.fakeHistory = [
      { id: 2, query: "coercion", fromLang: "en", toLang: "zh-CN", engine: "youdao",
        translation: "强迫", isWord: true, createdAt: 1760000060000 },
      { id: 1, query: "hello", fromLang: "en", toLang: "zh-CN", engine: "google",
        translation: "你好", isWord: true, createdAt: 1760000000000 },
    ];
    window.exportedTo = null;
    window.resizedTo = null;
    window.maximizeCalls = 0;
    window.quitCalled = false;
    window.fakeCelebrationWords = ["coercion", "hello", "好奇心"];
    window.confettiFailure = false;
    window.confettiDelay = 0;
    window.drawnConfetti = new Set();
    window.confettiSamples = [];
    window.confettiFrameTextCount = 0;
    window.peakConfettiTextCount = 0;
    window.liveConfettiTextCalls = 0;
    window.confettiFirstFrame = null;
    window.confettiFrameCounts = [];
    const spriteText = new WeakMap();
    const originalRaf = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = callback => {
      const scheduledAt = performance.now();
      return originalRaf(time => {
        window.confettiFrameTextCount = 0;
        // 原点检查固定在第一帧，避免机器忙时首帧已飞出几十像素。
        const firstLaunch = !window.confettiFirstFrame && document.querySelector("#word-confetti");
        callback(firstLaunch ? scheduledAt : time);
        window.peakConfettiTextCount = Math.max(window.peakConfettiTextCount, window.confettiFrameTextCount);
        if (window.confettiFrameTextCount) {
          window.confettiFrameCounts.push(window.confettiFrameTextCount);
          if (!window.confettiFirstFrame) window.confettiFirstFrame = window.confettiSamples.slice(0, window.confettiFrameTextCount);
        }
      });
    };
    const originalFillText = CanvasRenderingContext2D.prototype.fillText;
    CanvasRenderingContext2D.prototype.fillText = function(text, ...args) {
      if (this.canvas.id === "word-confetti") window.liveConfettiTextCalls++;
      else spriteText.set(this.canvas, { text, size: Number(this.font.match(/([\d.]+)px/)?.[1] ?? 0) });
      return originalFillText.call(this, text, ...args);
    };
    const originalStrokeText = CanvasRenderingContext2D.prototype.strokeText;
    CanvasRenderingContext2D.prototype.strokeText = function(...args) {
      if (this.canvas.id === "word-confetti") window.liveConfettiTextCalls++;
      return originalStrokeText.apply(this, args);
    };
    const originalDrawImage = CanvasRenderingContext2D.prototype.drawImage;
    CanvasRenderingContext2D.prototype.drawImage = function(source, ...args) {
      const info = spriteText.get(source);
      if (this.canvas.id === "word-confetti" && info) {
        window.drawnConfetti.add(info.text);
        window.confettiFrameTextCount++;
        const transform = this.getTransform();
        const dpr = this.canvas.width / innerWidth;
        if (window.confettiSamples.length < 6000) window.confettiSamples.push({ ...info, x: transform.e / dpr, y: transform.f / dpr });
      }
      return originalDrawImage.call(this, source, ...args);
    };
    const cache = new Set();
    const engines = [
      { id: "google", name: "Google 翻译", note: "大陆需代理", needsProxy: true, supportsSentence: true, supportsDictionary: true, supportsAudio: false },
      { id: "youdao", name: "有道词典", note: "国内直连，无需代理；柯林斯、专业释义、词源、词组短语、同根词最全", needsProxy: false, supportsSentence: true, supportsDictionary: true, supportsAudio: true },
      { id: "youdaoTrans", name: "有道翻译", note: "国内直连，无需代理；单词与整句都能翻", needsProxy: false, supportsSentence: true, supportsDictionary: false, supportsAudio: true },
    ];
    window.__TAURI_INTERNALS__ = { transformCallback, invoke: async (cmd, args) => {
      window.calls.push({ cmd, args });
      if (cmd === "plugin:event|listen") {
        window.eventListeners.push({ event: args.event, handler: args.handler });
        return window.eventListeners.length;
      }
      if (cmd === "plugin:event|unlisten") {
        window.eventListeners = window.eventListeners.filter(l => l.handler !== args.eventId);
        return null;
      }
      if (cmd === "load_settings") return window.fakeSettings;
      if (cmd === "save_settings") {
        if (args.settings.proxy === "invalid") throw "输入或设置无效：代理地址无效";
        window.fakeSettings = args.settings;
        return;
      }
      if (cmd === "take_startup_text") {
        const text = window.startupText;
        window.startupText = null;
        return text;
      }
      if (cmd === "shell_menu_registered") return window.registryHasShellMenu;
      if (cmd === "engines") return engines;
      if (cmd === "cache_stats") return { size: cache.size };
      if (cmd === "clear_cache") { cache.clear(); return; }
      if (cmd === "resize_window") { window.resizedTo = { width: args.width, height: args.height }; return; }
      if (cmd === "toggle_maximize") { window.maximizeCalls++; return true; }
      if (cmd === "quit_app") { window.quitCalled = true; return; }
      if (cmd === "app_info") return {
        version: "0.1.0",
        settingsPath: "C:\\Users\\test\\AppData\\Roaming\\com.tauri-app.translater\\settings.json",
        historyPath: "C:\\Users\\test\\AppData\\Roaming\\com.tauri-app.translater\\history.db",
        configDir: "C:\\Users\\test\\AppData\\Roaming\\com.tauri-app.translater",
        dataDir: "C:\\Users\\test\\AppData\\Roaming\\com.tauri-app.translater",
      };
      if (cmd === "history_stats") return { total: window.fakeHistory.length, sizeBytes: 20480 };
      if (cmd === "history_celebration_words") {
        await new Promise(resolve => setTimeout(resolve, window.confettiDelay));
        if (window.confettiFailure) throw "history read failed";
        return window.fakeCelebrationWords;
      }
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
  check(await page.locator("#engine-note").count() === 0,
    "engine small print is gone from the toolbar");
  check((await page.locator("#history-count").innerText()) === "2", "toolbar shows the history total");

  // ── 皮肤与字号 ────────────────────────────────────────────
  check(await page.evaluate(() => document.documentElement.dataset.theme === "plain"),
    "plain is the default skin");
  check(await page.evaluate(() => document.documentElement.dataset.font === "md"),
    "standard is the default font scale");
  check(await page.locator(".spectrum").isHidden(),
    "plain hides the rainbow stripe decoration");

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
  check(await page.locator("#settings-theme option").count() === 2, "settings dialog lists every skin");
  check(await page.locator("#settings-font option").count() === 4, "settings dialog lists every font scale");
  check(await page.locator("#settings-window option").count() === 4, "settings dialog lists the window presets");
  check(await page.locator("#settings-theme").inputValue() === "plain", "settings reflect the active skin");
  check(await page.locator("#settings-font").inputValue() === "md", "settings reflect the active font scale");
  check(await page.locator("#settings-window").inputValue() === "1020x760",
    "settings reflect the active window size");

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

  // ── 菜单：皮肤 / 字号 / 窗口 ──────────────────────────────
  const menuTitle = (name) => page.locator(".menu-btn", { hasText: name });
  // 把令牌换算成 rgb()，方便和元素的实际颜色比对
  const tokenColor = (token) => page.evaluate((name) => {
    const probe = document.createElement("span");
    probe.style.color = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    document.body.appendChild(probe);
    const rgb = getComputedStyle(probe).color;
    probe.remove();
    return rgb;
  }, token);
  const plainTextColor = await tokenColor("--text");

  await menuTitle("视图").click();
  check(await menuTitle("视图").getAttribute("aria-expanded") === "true",
    "clicking a menu title expands it");
  check(await page.locator('.menu-pop:not([hidden]) [data-cmd="theme"][data-arg="plain"]')
    .getAttribute("aria-checked") === "true", "the active skin is marked in the menu");

  await page.locator('[data-cmd="theme"][data-arg="meridian"]').click();
  await page.waitForFunction(() => document.documentElement.dataset.theme === "meridian");
  check(await page.locator(".menu-pop:not([hidden])").count() === 0, "picking a menu item closes the menu");
  check(await menuTitle("视图").getAttribute("aria-expanded") === "false",
    "the menu title drops its expanded state after a pick");
  await page.waitForFunction(() => window.fakeSettings.theme === "meridian");
  check(await page.evaluate(() => window.fakeSettings.theme === "meridian"), "skin choice is persisted");
  check(await page.locator(".spectrum").isVisible(), "meridian shows the rainbow stripe");
  check(await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--radius").trim() === "3px"),
    "meridian swaps the radius token");
  const meridianTextColor = await tokenColor("--text");
  check(meridianTextColor !== plainTextColor, "meridian overrides the text token");

  // 换皮肤之后结果区（词典 / 例句 / 面板）也要跟着换色，且仍然可读
  await page.locator("#input").fill("hello");
  await page.locator("#input").press("Control+Enter");
  await page.waitForFunction(() => document.querySelector(".main-trans")?.textContent === "译文：hello");
  check(await page.evaluate(() => getComputedStyle(document.querySelector(".main-trans")).color)
    === meridianTextColor, "the result text follows the --text token");
  await page.waitForTimeout(250);
  await page.screenshot({ path: "test-results/meridian.png" });

  await menuTitle("视图").click();
  await page.locator('[data-cmd="font"][data-arg="xl"]').click();
  await page.waitForFunction(() => document.documentElement.dataset.font === "xl");
  check(await page.evaluate(() =>
    getComputedStyle(document.documentElement).getPropertyValue("--fs").trim() === "1.25"),
    "font scale switches the --fs multiplier");
  await page.waitForFunction(() => window.fakeSettings.font === "xl");
  check(await page.evaluate(() => window.fakeSettings.font === "xl"), "font choice is persisted");

  await menuTitle("视图").click();
  await page.locator('[data-cmd="window"][data-arg="1280x820"]').click();
  await page.waitForFunction(() => window.resizedTo !== null);
  check(await page.evaluate(() => window.resizedTo.width === 1280 && window.resizedTo.height === 820),
    "window preset resizes the window");
  await page.waitForFunction(() => window.fakeSettings.windowWidth === 1280);
  check(await page.evaluate(() => window.fakeSettings.windowWidth === 1280), "window preset is persisted");

  await menuTitle("视图").click();
  await page.locator('[data-cmd="remember-window"]').click();
  await page.waitForFunction(() => window.fakeSettings.rememberWindow === true);
  check(await page.evaluate(() => window.fakeSettings.rememberWindow === true),
    "remember-window toggle is persisted");
  await menuTitle("视图").click();
  check(await page.locator('[data-cmd="remember-window"]').getAttribute("aria-checked") === "true",
    "remember-window shows as checked");
  await page.locator('[data-cmd="remember-window"]').click();
  await page.waitForFunction(() => window.fakeSettings.rememberWindow === false);
  check(await page.evaluate(() => window.fakeSettings.rememberWindow === false),
    "remember-window can be turned back off");

  // 双击窗口框架放大 / 还原（x 落在菜单栏右侧的空白处）
  await page.locator(".chrome").dblclick({ position: { x: 500, y: 12 } });
  check(await page.evaluate(() => window.maximizeCalls === 1), "double-clicking the chrome toggles maximize");

  // ── 划词翻译与资源管理器右键菜单 ──────────────────────────
  await menuTitle("视图").click();
  for (const cmd of ["select-translate", "clipboard-fallback", "shell-menu"]) {
    check(await page.locator(`.menu-pop:not([hidden]) [data-cmd="${cmd}"]`).count() === 1,
      `the view menu exposes ${cmd}`);
  }
  check(await page.locator('[data-cmd="select-translate"]').getAttribute("aria-checked") === "true",
    "selection translation starts enabled");
  check(await page.locator('[data-cmd="clipboard-fallback"]').getAttribute("aria-checked") === "true",
    "the clipboard fallback starts enabled");
  check(await page.locator('[data-cmd="shell-menu"]').getAttribute("aria-checked") === "true",
    "the explorer context menu starts enabled");

  await page.locator('[data-cmd="select-translate"]').click();
  await page.waitForFunction(() => window.fakeSettings.selectTranslate === false);
  check(await page.evaluate(() => window.fakeSettings.selectTranslate === false),
    "turning selection translation off is persisted");
  await menuTitle("视图").click();
  check(await page.locator('[data-cmd="select-translate"]').getAttribute("aria-checked") === "false",
    "the check mark follows the saved value");
  await page.locator('[data-cmd="select-translate"]').click();
  await page.waitForFunction(() => window.fakeSettings.selectTranslate === true);

  await menuTitle("视图").click();
  await page.locator('[data-cmd="shell-menu"]').click();
  await page.waitForFunction(() => window.fakeSettings.shellMenu === false);
  check(await page.evaluate(() => window.fakeSettings.shellMenu === false),
    "turning the explorer context menu off is persisted");

  // 后端把划词结果 emit 过来：填进输入框并翻译
  await page.evaluate(() => window.emitEvent("translate-selection", "  coercion  "));
  await page.waitForFunction(() => document.querySelector("#input").value === "coercion");
  check(await page.locator("#input").inputValue() === "coercion",
    "a selected-text event fills the input, trimmed");
  await page.waitForFunction(() => document.querySelector(".main-trans")?.textContent === "译文：coercion");
  check((await page.locator(".main-trans").innerText()).includes("译文：coercion"),
    "a selected-text event triggers translation");
  check(await page.evaluate(() => window.calls.some(c => c.cmd === "translate" && c.args.text === "coercion")),
    "the selected text is what gets sent to the engine");

  // 设置弹窗里的三个开关
  await page.locator("#btn-settings").click();
  check(await page.locator("#select-translate").isChecked(), "settings expose the selection toggle");
  check(await page.locator("#clipboard-fallback").isChecked(), "settings expose the clipboard fallback toggle");
  check(await page.locator("#shell-menu").isChecked() === false,
    "settings reflect the saved explorer-menu value");
  await page.locator("#btn-close-settings").click();

  // ── 说明 ──────────────────────────────────────────────────
  await page.keyboard.press("F1");
  await page.waitForFunction(() => !document.querySelector("#help-mask").hidden);
  check(await page.locator("#help-mask").isVisible(), "F1 opens the help dialog");
  check(await page.locator("#help-engine-table tbody tr").count() === 3, "help lists every engine");
  const helpNotes = await page.locator("#help-engine-notes").innerText();
  check(helpNotes.includes("国内直连") && helpNotes.includes("柯林斯") && helpNotes.includes("词源"),
    "help carries the engine small print that left the toolbar");
  check((await page.locator("#help-paths-table").innerText()).includes("settings.json"),
    "help lists the settings file location");
  check((await page.locator("#help-version").innerText()).includes("0.1.0"), "help shows the app version");
  check(await page.locator("#help-select").count() === 1, "help documents selection translation");
  const helpSelect = await page.locator("#help-select").innerText();
  check(helpSelect.includes("不会被替换"), "help states the original text is never replaced");
  check(helpSelect.includes("UI Automation"), "help explains how the selection is read");
  check(helpSelect.includes("浏览器"), "help explains why browsers cannot host a menu item");
  check((await page.locator("#help-shell-menu").innerText()).includes("HKCU"),
    "help explains where the explorer menu entry is written");
  await page.keyboard.press("Escape");
  check(await page.locator("#help-mask").isHidden(), "Escape closes the help dialog");

  await menuTitle("说明").click();
  await page.locator('[data-cmd="help"][data-arg="about"]').click();
  await page.waitForFunction(() => !document.querySelector("#help-mask").hidden);
  check(await page.locator("#help-mask").isVisible(), "the help menu opens the same dialog");
  await page.locator("#btn-close-help-2").click();
  check(await page.locator("#help-mask").isHidden(), "the help dialog closes from its footer button");

  // 菜单栏键盘导航
  await page.locator("#input").focus();
  await page.keyboard.press("Alt");
  check(await page.locator(".menu-btn").first().evaluate(el => el === document.activeElement),
    "Alt focuses the menu bar");
  await page.keyboard.press("ArrowDown");
  check(await page.locator('.menu-pop:not([hidden]) .menu-item').first()
    .evaluate(el => el === document.activeElement), "ArrowDown opens the menu and focuses the first item");
  await page.keyboard.press("Escape");
  check(await page.locator(".menu-pop:not([hidden])").count() === 0, "Escape closes the menu bar");

  // 还原成默认外观，后面的布局检查沿用原来的基准
  await menuTitle("视图").click();
  await page.locator('[data-cmd="theme"][data-arg="plain"]').click();
  await page.waitForFunction(() => document.documentElement.dataset.theme === "plain");
  await menuTitle("视图").click();
  await page.locator('[data-cmd="font"][data-arg="md"]').click();
  await page.waitForFunction(() => document.documentElement.dataset.font === "md");
  check(await page.locator(".spectrum").isHidden(), "switching back to plain hides the decoration again");

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

  // meridian 在最小窗口下也得装得下
  await menuTitle("视图").click();
  await page.locator('[data-cmd="theme"][data-arg="meridian"]').click();
  await page.waitForFunction(() => document.documentElement.dataset.theme === "meridian");
  // 等 0.12s 的按钮背景过渡走完，否则截图里菜单标题还留着高亮
  await page.waitForTimeout(250);
  await page.screenshot({ path: "test-results/meridian-compact.png" });
  check(await page.evaluate(() => {
    const bar = document.querySelector(".chrome");
    const right = document.querySelector(".chrome-right");
    return bar.getBoundingClientRect().right <= innerWidth + 0.5 &&
      right.getBoundingClientRect().right <= innerWidth + 0.5;
  }), "the menu bar stays inside the window at the minimum size");

  // ── 单词礼炮：真正绘制本地词与 emoji，不触碰翻译与历史写入 ─────
  await page.setViewportSize({ width: 1020, height: 760 });
  await page.emulateMedia({ reducedMotion: "no-preference" });
  const celebrate = page.getByRole("button", { name: "单词礼炮", exact: true });
  check(await celebrate.isVisible(), "word confetti button is visible beside history");
  const queryBefore = await page.locator("#input").inputValue();
  const commandsBefore = await page.evaluate(() => window.calls.length);
  await celebrate.click();
  await page.waitForFunction(() => window.drawnConfetti.has("coercion") && window.drawnConfetti.has("好奇心") && window.drawnConfetti.has("🎉"));
  await page.waitForFunction(() => window.peakConfettiTextCount >= 80);
  await page.waitForTimeout(600);
  // 一次读取同一帧附近的状态，避免跨多次 IPC 时动画已自然结束。
  const storm = await page.evaluate(({ queryBefore, commandsBefore }) => {
    const canvas = document.querySelector("#word-confetti");
    // 固定这一帧的位图用于截图；检查的仍是实际正在运行的 Canvas。
    // 避免截图工具等待字体时，3.6 秒的礼炮已经自动结束。
    if (canvas) {
      const capture = document.createElement("img");
      capture.id = "confetti-capture-frame";
      capture.className = "word-confetti";
      capture.setAttribute("aria-hidden", "true");
      capture.src = canvas.toDataURL();
      capture.style.zIndex = "82";
      document.body.append(capture);
    }
    const cells = new Set(window.confettiSamples.filter(p => p.x >= 0 && p.x < innerWidth && p.y >= 0 && p.y < innerHeight)
      .map(p => `${Math.floor(p.x / innerWidth * 4)},${Math.floor(p.y / innerHeight * 3)}`));
    return {
      singleCanvas: document.querySelectorAll("#word-confetti").length === 1,
      dense: window.peakConfettiTextCount >= 80 && window.peakConfettiTextCount <= 104,
      sized: window.confettiSamples.filter(p => window.fakeCelebrationWords.includes(p.text)).every(p => p.size <= 40),
      spread: cells.size >= 6,
      cached: window.liveConfettiTextCalls === 0,
      fourBottomOrigins: window.confettiFirstFrame?.every(p => p.y > innerHeight * 0.7 && [0.1, 0.36, 0.64, 0.9].some(anchor => Math.abs(p.x / innerWidth - anchor) < 0.045)),
      singleWave: window.confettiFrameCounts.every(n => n <= window.confettiFrameCounts[0]),
      pointerThrough: !!canvas && getComputedStyle(canvas).pointerEvents === "none",
      focus: document.activeElement === document.querySelector("#btn-celebrate"),
      input: document.querySelector("#input").value === queryBefore,
      readOnly: window.calls.slice(commandsBefore).every(c => c.cmd === "history_celebration_words"),
    };
  }, { queryBefore, commandsBefore });
  check(storm.singleCanvas, "confetti draws history words and emoji on one canvas");
  check(storm.dense, "single burst has a bounded, overlapping group of word and emoji sprites");
  check(storm.sized, "word sprites use the smaller size range");
  check(storm.spread, "bottom burst spreads upward through the window");
  check(storm.cached, "animation never lays out or strokes live text in its frames");
  check(storm.fourBottomOrigins, "all sprites launch from the four bottom positions");
  check(storm.singleWave, "no second wave appears after the initial burst");
  check(storm.pointerThrough, "confetti cannot intercept pointer input");
  check(storm.focus, "celebrating does not steal keyboard focus");
  check(storm.input, "celebration preserves translation input");
  check(storm.readOnly, "celebration only reads its local word pool");
  await page.locator("#confetti-capture-frame").evaluate(img => img.decode());
  await page.screenshot({ path: "test-results/confetti-meridian.png" });
  await page.locator("#confetti-capture-frame").evaluate(img => img.remove());
  await celebrate.click();
  await page.waitForFunction(() => !document.querySelector("#btn-celebrate").hasAttribute("aria-busy"));
  check(await page.locator("#word-confetti").count() === 1, "repeated celebrations replace the previous canvas");
  await page.waitForFunction(() => !document.querySelector("#word-confetti"), null, { timeout: 5000 });
  check(await page.locator("#celebration-feedback").innerText() === "", "finished celebration removes canvas and feedback");

  await page.emulateMedia({ reducedMotion: "reduce" });
  await celebrate.focus();
  await page.keyboard.press("Enter");
  await page.waitForFunction(() => document.querySelector(".celebration-bouquet"));
  check(await page.locator("#word-confetti").count() === 0, "reduced motion uses a static bouquet instead of flying particles");
  check((await page.locator("#celebration-feedback").innerText()).includes("好奇心"), "keyboard-triggered static celebration includes actual history words");
  await page.setViewportSize({ width: 720, height: 480 });
  await celebrate.click();
  await page.waitForFunction(() => document.querySelector(".celebration-bouquet"));
  await page.screenshot({ path: "test-results/confetti-reduced.png" });
  check(await celebrate.evaluate(el => el.getBoundingClientRect().right <= innerWidth), "confetti button fits the compact window");
  await page.keyboard.press("Escape");
  check(await page.locator("#celebration-feedback").innerText() === "", "Escape dismisses the celebration");

  await page.evaluate(() => { window.fakeCelebrationWords = []; });
  await celebrate.click();
  await page.waitForFunction(() => document.querySelector("#celebration-feedback").textContent.includes("好奇心"));
  check(await page.locator(".celebration-word").allTextContents().then(words => words.length === 3 && words.includes("🎉")),
    "empty history celebrates with emoji without inventing learned words");
  await page.keyboard.press("Escape");
  await page.evaluate(() => { window.confettiFailure = true; });
  await celebrate.click();
  await page.waitForFunction(() => document.querySelector("#celebration-feedback").textContent.includes("读取失败"));
  check(await page.locator(".celebration-bouquet").isVisible(), "history failure still offers emoji with a readable explanation");
  await page.evaluate(() => { window.confettiFailure = false; window.fakeCelebrationWords = ["fresh-word"]; });
  await celebrate.click();
  await page.waitForFunction(() => document.querySelector("#celebration-feedback").textContent.includes("fresh-word"));
  check(!(await page.locator("#celebration-feedback").innerText()).includes("coercion"), "retry uses fresh history rather than retaining deleted words");
  await page.keyboard.press("Escape");

  // 双击只发一次请求；页面隐藏会让未完成的读取失效。
  await page.evaluate(() => { window.confettiDelay = 300; });
  const readsBefore = await page.evaluate(() => window.calls.filter(c => c.cmd === "history_celebration_words").length);
  await celebrate.dblclick();
  await page.waitForFunction(() => document.querySelector(".celebration-bouquet"));
  check(await page.evaluate(start => window.calls.filter(c => c.cmd === "history_celebration_words").length === start + 1, readsBefore),
    "double-click coalesces the pending history read");
  await page.keyboard.press("Escape");
  await celebrate.click();
  await page.evaluate(() => {
    Object.defineProperty(document, "hidden", { configurable: true, get: () => true });
    document.dispatchEvent(new Event("visibilitychange"));
  });
  await page.waitForTimeout(400);
  check(await page.locator("#celebration-feedback").innerText() === "", "hidden page cancels a pending celebration");
  await page.evaluate(() => { delete document.hidden; window.confettiDelay = 0; });
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await menuTitle("视图").click();
  await page.locator('[data-cmd="theme"][data-arg="plain"]').click();
  await page.setViewportSize({ width: 1020, height: 760 });
  await page.evaluate(() => { window.fakeCelebrationWords = ["coercion", "hello", "好奇心", "curiosity", "discover", "serendipity"]; });
  await celebrate.click();
  await page.waitForFunction(() => document.querySelector("#word-confetti"));
  await page.waitForTimeout(650);
  await page.screenshot({ path: "test-results/confetti-plain.png" });
  await page.setViewportSize({ width: 720, height: 480 });
  await page.waitForFunction(() => !document.querySelector("#word-confetti"), null, { polling: 10, timeout: 1000 });
  check(await page.locator("#word-confetti").count() === 0, "window resize cleans up active particles");

  // ── 冷启动：--translate-file 与注册表状态同步 ─────────────
  await page.addInitScript(() => {
    window.startupText = "from-command-line";
    // 用户可能在别处删掉了注册表项：界面要以注册表为准，不能凭设置文件想当然
    window.fakeSettings = { ...window.fakeSettings, shellMenu: true };
    window.registryHasShellMenu = false;
  });
  await page.reload();
  await page.waitForFunction(() => document.querySelector("#input").value === "from-command-line");
  check(await page.locator("#input").inputValue() === "from-command-line",
    "a cold start with --translate-file loads the file text");
  await page.waitForFunction(() => document.querySelector(".main-trans")?.textContent === "译文：from-command-line");
  check((await page.locator(".main-trans").innerText()).includes("译文：from-command-line"),
    "the startup file is translated without any click");
  check(await page.evaluate(() => window.startupText === null), "startup text is consumed exactly once");
  await page.waitForFunction(() =>
    document.querySelector('[data-cmd="shell-menu"]').getAttribute("aria-checked") === "false");
  check(await page.locator('[data-cmd="shell-menu"]').getAttribute("aria-checked") === "false",
    "the shell menu check follows the registry, not the settings file");

  // ── 划词悬浮图标页 ────────────────────────────────────────
  // 图标窗口是个独立的 WebView，页面只负责渲染，点击由 Rust 钩子接管。
  const bubble = await browser.newPage({ viewport: { width: 120, height: 40 } });
  const bubbleErrors = [];
  bubble.on("pageerror", e => bubbleErrors.push(e.message));
  await bubble.addInitScript(() => localStorage.setItem("transslater.theme", "meridian"));
  await bubble.goto("http://127.0.0.1:1420/bubble.html");
  await bubble.waitForFunction(() => typeof window.__applyAppearance === "function");
  check(await bubble.locator(".bubble__word").innerText() === "translate",
    "the bubble is a translate wordmark, not a glyph");
  check(await bubble.locator(".bubble__spectrum i").count() === 4,
    "the bubble carries the four-segment rainbow bar");
  check(await bubble.evaluate(() => document.documentElement.dataset.theme) === "meridian",
    "the bubble restores the last theme before Rust says anything");

  // Rust 传进来的是逻辑像素：CSS 尺寸跟它走，窗口物理尺寸另算，比例不会随 DPI 走样
  await bubble.evaluate(() => window.__applyAppearance("plain", 120, 40));
  const bubbleStyle = await bubble.evaluate(() => {
    const root = document.documentElement;
    const pill = getComputedStyle(document.querySelector(".bubble"));
    const word = getComputedStyle(document.querySelector(".bubble__word"));
    const bar = getComputedStyle(document.querySelector(".bubble__spectrum"));
    const segments = [...document.querySelectorAll(".bubble__spectrum i")]
      .map(el => getComputedStyle(el).width);
    return {
      width: root.style.getPropertyValue("--bubble-width"),
      height: root.style.getPropertyValue("--bubble-height"),
      theme: root.dataset.theme,
      radius: pill.borderTopLeftRadius,
      background: pill.backgroundColor,
      fontSize: word.fontSize,
      letterSpacing: word.letterSpacing,
      barHeight: bar.height,
      segments,
    };
  });
  const near = (value, expected) => Math.abs(parseFloat(value) - expected) < 0.05;
  check(bubbleStyle.width === "120px" && bubbleStyle.height === "40px" && bubbleStyle.theme === "plain",
    "Rust can restyle the bubble through __applyAppearance");
  check(near(bubbleStyle.radius, 20) && near(bubbleStyle.fontSize, 13.6),
    "plain draws a pill: radius and glyph both scale with the height");
  check(near(bubbleStyle.barHeight, 5.2), "the rainbow bar is a fixed fraction of the height");
  // 段宽是相对内边距算的（要减掉左右各 1px 边框），所以断言占比而不是绝对像素
  const segmentTotal = bubbleStyle.segments.reduce((sum, w) => sum + parseFloat(w), 0);
  const shares = bubbleStyle.segments.map(w => parseFloat(w) / segmentTotal);
  check(shares.every((s, i) => Math.abs(s - [0.4, 0.25, 0.2, 0.15][i]) < 0.005),
    `rainbow segments keep the 40/25/20/15 split (${shares.map(s => s.toFixed(3)).join(", ")})`);
  check(parseFloat(bubbleStyle.letterSpacing) > 0.5, "the wordmark is letter-spaced, not cramped");
  await bubble.screenshot({ path: "test-results/bubble-plain.png" });

  await bubble.evaluate(() => { localStorage.setItem("transslater.theme", "meridian"); });
  await bubble.reload();
  await bubble.waitForFunction(() => document.documentElement.dataset.theme === "meridian");
  // 重新注入一次尺寸：reload 之后 Rust 那边还没调过 __applyAppearance
  await bubble.evaluate(() => window.__applyAppearance("meridian", 120, 40));
  const meridianBubble = await bubble.evaluate(() => {
    const pill = getComputedStyle(document.querySelector(".bubble"));
    return { background: pill.backgroundColor, radius: pill.borderTopLeftRadius };
  });
  check(meridianBubble.background !== bubbleStyle.background,
    "switching the skin repaints the bubble from theme tokens");
  check(near(meridianBubble.radius, 7.2),
    "meridian squares the corners off instead of drawing a pill");
  await bubble.screenshot({ path: "test-results/bubble-meridian.png" });
  check(bubbleErrors.length === 0, `no uncaught bubble errors (${bubbleErrors.join(", ")})`);

  check(errors.length === 0, `no uncaught UI errors (${errors.join(", ")})`);

  const preview = await browser.newPage();
  await preview.goto("http://127.0.0.1:1420");
  await preview.waitForFunction(() => !document.querySelector("#input").disabled);
  check((await preview.locator("#alert").innerText()).includes("浏览器预览"),
    "browser preview explains desktop requirement");
  console.log(`${checks} UI regression checks passed. Screenshots use mocked IPC, not live translation.`);
} finally { await browser.close(); }
