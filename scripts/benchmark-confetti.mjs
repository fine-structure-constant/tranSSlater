// Local renderer benchmark with public fixture words and mocked desktop IPC.
import { createRequire } from "node:module";
import { mkdir, writeFile } from "node:fs/promises";
const { chromium } = createRequire(import.meta.url)("playwright");
const browser = await chromium.launch({ channel: "msedge", headless: true });
const label = process.argv[2] || "current";
const textControl = process.argv.includes("--text-control");
const percentile = (values, q) => [...values].sort((a, b) => a - b)[Math.min(values.length - 1, Math.floor(values.length * q))] ?? 0;
try {
  const page = await browser.newPage({ viewport: { width: 1020, height: 760 }, deviceScaleFactor: 1.5 });
  await page.addInitScript(({ textControl }) => {
    // 相同词汇、粒子轨迹和随机种子，仅替换逐帧文字 / 缓存贴图的绘制路径。
    let seed = 246813579;
    Math.random = () => { seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0; return seed / 4294967296; };
    window.isTauri = true;
    window.__TAURI_INTERNALS__ = { invoke: async cmd => {
      if (cmd === "engines") return [{ id: "google", name: "Google", note: "", needsProxy: true }];
      if (cmd === "load_settings") return { proxy: "", autoTranslate: false, autoSpeak: false, langPair: "en|zh-CN", engine: "google", historyMode: "all", theme: "plain", font: "md", windowWidth: 1020, windowHeight: 760, rememberWindow: false };
      if (cmd === "history_stats") return { total: 12, sizeBytes: 10000 };
      if (cmd === "cache_stats") return { size: 0 };
      if (cmd === "history_celebration_words") return ["curiosity", "hello", "好奇心", "discover", "serendipity", "acoustics", "concerto", "salute", "personal", "irrational", "tauri", "mutex"];
      throw new Error(`Unexpected command: ${cmd}`);
    }};
    const originalRaf = window.requestAnimationFrame.bind(window);
    window.requestAnimationFrame = callback => originalRaf(time => {
      const start = performance.now();
      const painted = window.benchmark?.paint ?? 0;
      callback(time);
      // 排除 Playwright waitForFunction 自己的 rAF 轮询。
      if (window.benchmarkActive && window.benchmark.paint > painted) {
        window.benchmark.cpu.push(performance.now() - start);
        if (window.benchmark.last) window.benchmark.intervals.push(time - window.benchmark.last);
        window.benchmark.last = time;
      }
    });
    const descriptions = new WeakMap();
    for (const method of ["fillText", "strokeText", "measureText", "drawImage", "clearRect", "fillRect"]) {
      const original = CanvasRenderingContext2D.prototype[method];
      CanvasRenderingContext2D.prototype[method] = function(...args) {
        if (window.benchmarkActive) {
          if (method in window.benchmark) window.benchmark[method]++;
          if (this.canvas.id === "word-confetti" && method !== "measureText") window.benchmark.paint++;
          if ((method === "fillText" || method === "strokeText") && this.canvas.id === "word-confetti") window.benchmark.liveText++;
        }
        if (this.canvas.id !== "word-confetti") {
          const info = descriptions.get(this.canvas) ?? {};
          if (method === "fillText") Object.assign(info, { text: args[0], font: this.font, fill: this.fillStyle });
          if (method === "strokeText") Object.assign(info, { stroke: this.strokeStyle, lineWidth: this.lineWidth });
          if (method === "fillRect") Object.assign(info, { background: this.fillStyle, rect: args });
          descriptions.set(this.canvas, info);
        }
        if (textControl && method === "drawImage" && this.canvas.id === "word-confetti") {
          const info = descriptions.get(args[0]);
          if (info?.text) {
            this.save();
            this.font = info.font;
            this.textAlign = "center";
            this.textBaseline = "middle";
            if (info.background) { this.fillStyle = info.background; this.fillRect(...info.rect); }
            if (info.stroke) { this.strokeStyle = info.stroke; this.lineWidth = info.lineWidth; this.lineJoin = "round"; this.strokeText(info.text, 0, 0); }
            this.fillStyle = info.fill;
            this.fillText(info.text, 0, 0);
            this.restore();
            return;
          }
        }
        return original.apply(this, args);
      };
    }
  }, { textControl });
  await page.goto("http://127.0.0.1:1420");
  await page.waitForFunction(() => !document.querySelector("#input").disabled);
  const runs = [];
  for (let i = 0; i < 2; i++) {
    await page.evaluate(() => {
      window.benchmark = { cpu: [], intervals: [], last: 0, paint: 0, fillText: 0, strokeText: 0, measureText: 0, drawImage: 0, liveText: 0 };
      window.benchmarkActive = true;
    });
    await page.locator("#btn-celebrate").click();
    await page.waitForFunction(() => document.querySelector("#word-confetti"));
    await page.waitForFunction(() => !document.querySelector("#word-confetti"), null, { timeout: 8000 });
    const raw = await page.evaluate(() => { window.benchmarkActive = false; return window.benchmark; });
    runs.push({ run: i + 1, frames: raw.cpu.length, frameMedianMs: percentile(raw.intervals, 0.5), frameP95Ms: percentile(raw.intervals, 0.95), drawMedianMs: percentile(raw.cpu, 0.5), drawP95Ms: percentile(raw.cpu, 0.95), drawTotalMs: raw.cpu.reduce((sum, n) => sum + n, 0), fillText: raw.fillText, strokeText: raw.strokeText, measureText: raw.measureText, drawImage: raw.drawImage, liveText: raw.liveText });
  }
  const result = { label, textControl, renderer: "Microsoft Edge headless / Windows", viewport: "1020x760", deviceScaleFactor: 1.5, note: "Mocked IPC. A/B uses the same seeded particles, counting only actual canvas frames. Text control redraws the same sprite text each frame. Browser timings do not guarantee native WebView FPS.", runs };
  await mkdir("test-results", { recursive: true });
  await writeFile(`test-results/confetti-${label}.json`, JSON.stringify(result, null, 2));
  console.log(JSON.stringify(result, null, 2));
} finally { await browser.close(); }
