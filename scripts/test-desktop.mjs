// Run against the actual production EXE, with Vite stopped and real Tauri IPC.
import assert from "node:assert/strict";
import { spawn } from "node:child_process";
import { once } from "node:events";
import { mkdir } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { chromium } from "playwright";

const executable = path.resolve(process.argv[2] ?? "artifacts/tranSSlater.exe");
const delay = ms => new Promise(resolve => setTimeout(resolve, ms));
const viteRunning = await new Promise(resolve => {
  const socket = net.connect(1420, "127.0.0.1");
  socket.setTimeout(1000);
  socket.once("connect", () => { socket.destroy(); resolve(true); });
  socket.once("error", () => resolve(false));
  socket.once("timeout", () => { socket.destroy(); resolve(false); });
});
assert.equal(viteRunning, false, "Stop Vite on port 1420 before the standalone EXE test");

const portServer = net.createServer();
portServer.listen(0, "127.0.0.1");
await once(portServer, "listening");
const port = portServer.address().port;
await new Promise(resolve => portServer.close(resolve));
await mkdir("test-results", { recursive: true });

const child = spawn(executable, [], {
  windowsHide: true,
  env: {
    ...process.env,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
  },
  stdio: "ignore",
});
let launchError;
child.on("error", error => { launchError = error; });
let browser;
let main;
let checks = 0;
const check = (condition, message) => {
  assert.ok(condition, message);
  console.log(`PASS ${++checks}: ${message}`);
};
try {
  const deadline = Date.now() + 30_000;
  while (true) {
    if (launchError) throw launchError;
    assert.equal(child.exitCode, null, "EXE exited before opening its WebView; close any existing instance");
    try {
      const response = await fetch(`http://127.0.0.1:${port}/json/list`, {
        signal: AbortSignal.timeout(1000),
      });
      if ((await response.json()).some(target => target.type === "page")) break;
    } catch { /* WebView2 is still starting. */ }
    assert.ok(Date.now() < deadline, "WebView2 did not start within 30 seconds");
    await delay(100);
  }

  browser = await chromium.connectOverCDP(`http://127.0.0.1:${port}`);
  const context = browser.contexts()[0];
  // Also retain failed navigations: chrome-error:// must fail this test.
  main = context.pages().find(page => !page.url().includes("bubble.html"));
  assert.ok(main, "Main WebView is missing");
  await main.waitForURL(url => url.href !== "about:blank", {
    timeout: 15_000, waitUntil: "domcontentloaded",
  });
  check(/^https?:\/\/tauri\.localhost\//.test(main.url()),
    `main loads embedded production assets (${main.url()})`);
  const errors = [];
  main.on("pageerror", error => errors.push(error.message));
  // Observe a fresh document load as well as startup.
  await main.reload();
  await main.waitForFunction(() => document.querySelector("#input")?.disabled === false);
  check(await main.title() === "tranSSlater", "main document and JavaScript load without Vite");
  check(await main.locator('#lang-pair option[value="auto|zh-CN"]').innerText() === "自动中英互译",
    "production UI exposes the automatic bilingual mode");
  const engines = await main.evaluate(() => window.__TAURI__.core.invoke("engines"));
  check(engines.map(engine => engine.id).join(",") === "google,youdao,youdaoTrans",
    "real backend IPC returns all three engines");
  check(engines.filter(engine => engine.id.startsWith("youdao")).every(engine => engine.note.includes("自动分流")),
    "production backend advertises routing on both Youdao entry points");
  const eventPayload = await main.evaluate(async () => {
    let receive;
    const delivered = new Promise(resolve => { receive = resolve; });
    const unlisten = await window.__TAURI__.event.listen("desktop-smoke", event => receive(event.payload));
    let timer;
    try {
      await window.__TAURI__.event.emitTo("main", "desktop-smoke", "connected");
      return await Promise.race([
        delivered,
        new Promise((_, reject) => {
          timer = setTimeout(() => reject(new Error("Tauri event delivery timed out")), 5000);
        }),
      ]);
    } finally {
      clearTimeout(timer);
      await unlisten();
    }
  });
  check(eventPayload === "connected", "real Tauri event channel can deliver to the main window");
  await main.waitForFunction(() => document.querySelector("#engine")?.options.length === 3);
  check(await main.locator("#alert").isHidden(), "startup settings and selection listener connect without errors");
  await main.screenshot({ path: "test-results/desktop-main.png" });

  // Prewarming is asynchronous. Enabled installations must load the second HTML entry too.
  const settings = await main.evaluate(() => window.__TAURI__.core.invoke("load_settings"));
  if (settings.selectTranslate) {
    let bubble;
    const bubbleDeadline = Date.now() + 15_000;
    while (!(bubble = context.pages().find(page => page.url().endsWith("/bubble.html")))) {
      assert.ok(Date.now() < bubbleDeadline, "Selection bubble did not load its embedded HTML entry");
      await delay(100);
    }
    check(/^https?:\/\/tauri\.localhost\//.test(bubble.url()),
      `bubble loads embedded production assets (${bubble.url()})`);
    bubble.on("pageerror", error => errors.push(error.message));
    await bubble.reload();
    await bubble.waitForFunction(() => typeof window.__applyAppearance === "function");
    check(await bubble.locator(".bubble__word").textContent() === "translate",
      "bubble HTML and JavaScript load without Vite");
    // A hidden WebView has no reliable compositor screenshot. Its styles are
    // covered by test-ui.mjs; verify the production document and script here.
  } else {
    console.log("SKIP: bubble disabled in the user's settings");
  }
  check(errors.length === 0, `no uncaught WebView JavaScript errors (${errors.join(", ")})`);
  await main.evaluate(() => { void window.__TAURI__.core.invoke("quit_app"); });
  const exitDeadline = Date.now() + 10_000;
  while (child.exitCode === null && Date.now() < exitDeadline) await delay(100);
  check(child.exitCode === 0, "quit shuts down the EXE including the hidden selection window");
  console.log(`Completed ${checks} production desktop checks.`);
} finally {
  // Only terminate the process started by this test.
  if (child.exitCode === null && !launchError) child.kill();
  await browser?.close().catch(() => {});
}
