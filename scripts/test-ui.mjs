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
  await page.addInitScript(() => {
    window.isTauri = true;
    window.calls = [];
    window.fakeSettings = { proxy: "", autoTranslate: true, autoSpeak: false, langPair: "auto|zh-CN" };
    const cache = new Set();
    window.__TAURI_INTERNALS__ = { invoke: async (cmd, args) => {
      window.calls.push({ cmd, args });
      if (cmd === "load_settings") return window.fakeSettings;
      if (cmd === "save_settings") {
        if (args.settings.proxy === "invalid") throw "输入或设置无效：代理地址无效";
        window.fakeSettings = args.settings;
        return;
      }
      if (cmd === "cache_stats") return { size: cache.size };
      if (cmd === "clear_cache") { cache.clear(); return; }
      if (cmd === "translate") {
        await new Promise(resolve => setTimeout(resolve, args.text === "slow" ? 350 : 20));
        if (args.text === "network-failure") throw "connect error";
        const cached = cache.has(args.text);
        cache.add(args.text);
        return {
          query: args.text, sourceLang: args.from === "auto" ? "en" : args.from,
          targetLang: args.to, translation: `译文：${args.text}`,
          segments: [{ source: args.text, text: `译文：${args.text}` }],
          dictionary: [{ pos: "noun", terms: ["译词"], back: [] }], alternatives: ["候选"],
          definitions: [{ pos: "noun", definitions: [{ text: "fixture definition", example: "example" }] }],
          synonyms: [{ pos: "noun", words: ["synonym"] }],
          examples: ['a <b>word</b> <img src=x onerror="window.injected=true">'],
          phonetic: "test", romanization: null, isWord: true, cached, elapsedMs: 20,
        };
      }
      throw `Unexpected command: ${cmd}`;
    }};
  });
  await page.goto("http://127.0.0.1:1420");
  await page.waitForFunction(() => !document.querySelector("#input").disabled);
  check(await page.locator("#mask").isHidden(), "settings overlay hidden at startup");
  check(await page.locator("#empty").isVisible(), "empty state visible at startup");
  await page.locator("#input").fill("hello");
  await page.locator("#input").press("Control+Enter");
  await page.waitForFunction(() => document.querySelector(".main-trans")?.textContent === "译文：hello");
  check(await page.locator("#empty").isHidden(), "empty state hidden after result");
  await page.getByRole("button", { name: "英英释义" }).click();
  check((await page.locator(".panel").innerText()).includes("fixture definition"), "definition tab renders");
  await page.getByRole("button", { name: "简明", exact: true }).click();
  check((await page.locator(".panel").innerText()).includes("译词"), "returning to dictionary retains fragment content");
  await page.getByRole("button", { name: "英英释义" }).click();
  check((await page.locator(".panel").innerText()).includes("fixture definition"), "revisiting definition retains content");
  await page.getByRole("button", { name: "例句" }).click();
  check(await page.locator(".panel img").count() === 0 && await page.locator(".panel b").count() === 1, "examples escape markup while preserving bold");
  await page.screenshot({ path: "test-results/result.png" });
  await page.locator("#btn-settings").click();
  check(await page.locator("#mask").isVisible(), "settings open on demand");
  await page.locator("#proxy").fill("invalid");
  await page.locator("#btn-save-settings").click();
  await page.waitForFunction(() => document.querySelector("#settings-msg").textContent.includes("保存失败"));
  check(await page.evaluate(() => window.fakeSettings.proxy === ""), "invalid settings do not replace saved configuration");
  await page.locator("#proxy").fill("");
  await page.locator("#auto-translate").uncheck();
  await page.locator("#btn-save-settings").click();
  await page.waitForFunction(() => document.querySelector("#mask").hidden);
  check(await page.locator("#mask").isHidden(), "successful save closes settings");
  await page.locator("#input").fill("network-failure");
  await page.locator("#input").press("Control+Enter");
  await page.waitForFunction(() => !document.querySelector("#alert").hidden);
  check(await page.locator(".skeleton").count() === 0 && await page.locator("#result").isHidden(), "network error removes skeleton and stale result");
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
  check(await page.locator("#empty").isVisible() && await page.locator("#result").isHidden(), "Escape restores clean empty state");
  await page.locator("#btn-settings").click();
  await page.locator("#auto-translate").check();
  await page.locator("#btn-save-settings").click();
  await page.waitForFunction(() => document.querySelector("#mask").hidden);
  const count = await page.evaluate(() => window.calls.filter(c => c.cmd === "translate").length);
  await page.locator("#input").fill("debounced input");
  await page.locator('[data-act="clear"]').click();
  await page.waitForTimeout(800);
  check(await page.evaluate(() => window.calls.filter(c => c.cmd === "translate").length) === count, "clear cancels debounced translation");
  await page.locator("#btn-settings").click();
  await page.locator("#btn-save-settings").focus();
  await page.keyboard.press("Tab");
  check(await page.locator("#btn-close-settings").evaluate(el => el === document.activeElement), "dialog traps keyboard focus");
  await page.keyboard.press("Escape");
  check(await page.locator("#btn-settings").evaluate(el => el === document.activeElement), "closing dialog restores opener focus");
  await page.setViewportSize({ width: 720, height: 480 });
  await page.screenshot({ path: "test-results/compact.png" });
  check(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth), "minimum desktop viewport does not overflow");
  check(errors.length === 0, `no uncaught UI errors (${errors.join(", ")})`);
  const preview = await browser.newPage();
  await preview.goto("http://127.0.0.1:1420");
  await preview.waitForFunction(() => !document.querySelector("#input").disabled);
  check((await preview.locator("#alert").innerText()).includes("浏览器预览"), "browser preview explains desktop requirement");
  console.log(`${checks} UI regression checks passed. Screenshots use mocked IPC, not live translation.`);
} finally { await browser.close(); }
