import { chromium } from 'playwright';
import { readFile, mkdir } from 'node:fs/promises';
import assert from 'node:assert/strict';

// Reuse the desktop engine/result fixture, exercising the actual Android UI.
const source = await readFile(new URL('./test-ui.mjs', import.meta.url), 'utf8');
const first = source.indexOf('  await page.addInitScript(() => {') + '  await page.addInitScript(() => {'.length;
const last = source.lastIndexOf('  });', source.indexOf('  await page.goto('));
const fixture = `(() => {${source.slice(first, last)} })();`;
await mkdir('test-results/android', { recursive: true });
const browser = await chromium.launch({ channel: 'msedge', headless: true });
let checks = 0;
const check = (value, message) => { assert.ok(value, message); console.log(`PASS ${++checks}: ${message}`); };
try {
  for (const [width, height] of [[320, 740], [390, 844], [844, 390], [800, 1024]]) {
    const context = await browser.newContext({ viewport: { width, height }, hasTouch: true });
    await context.addInitScript({ content: fixture });
    await context.addInitScript(() => {
      window.fakeSettings.autoTranslate = false;
      window.fakeHistory = Array.from({ length: 120 }, (_, i) => ({
        id: i + 1, query: `word${i + 1}`, fromLang: 'en', toLang: 'zh-CN',
        engine: 'youdao', translation: `词条${i + 1}`, isWord: true, createdAt: 1760000000000 + i,
      }));
      const invoke = window.__TAURI_INTERNALS__.invoke;
      window.__TAURI_INTERNALS__.invoke = async (command, args) => {
        if (command === 'app_info') return { version: '0.1.0', dataDir: '/data/user/0/com.transslater.app/files', configDir: '/data/user/0/com.transslater.app/files', settingsPath: '/data/user/0/com.transslater.app/files/settings.json', historyPath: '/data/user/0/com.transslater.app/files/history.db' };
        if (command === 'plugin:dialog|save') return 'content://com.android.providers.downloads.documents/document/1234';
        if (command === 'export_history') { window.exportedTo = args; return window.fakeHistory.length; }
        return invoke(command, args);
      };
    });
    const page = await context.newPage();
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.goto('http://127.0.0.1:1426');
    await page.waitForFunction(() => !document.querySelector('#input').disabled);
    check(await page.title() === 'tranSSlater', `${width}: application name`);
    check(await page.locator('[data-cmd="window"], #settings-window, #select-translate').count() === 0, `${width}: desktop-only controls removed`);
    await page.locator('#input').fill('hello');
    await page.locator('#input').press('Control+Enter');
    await page.waitForFunction(() => document.querySelector('.main-trans')?.textContent === '译文：hello');
    check(await page.locator('#result').isVisible(), `${width}: translation result`);
    for (const theme of ['plain', 'meridian']) {
      await page.evaluate(theme => { document.documentElement.dataset.theme = theme; }, theme);
      const escaped = await page.evaluate(() => [...document.querySelectorAll('.chrome button, .toolbar select, #input')].filter(el => {
        const r = el.getBoundingClientRect();
        return r.width && (r.left < -1 || r.right > innerWidth + 1);
      }).map(el => el.id || el.className));
      check(!escaped.length, `${width}/${theme}: controls fit viewport (${escaped})`);
      await page.screenshot({ path: `test-results/android/${width}-${theme}.png` });
    }
    await page.locator('#btn-settings').click();
    await page.locator('#settings-font').selectOption('xl');
    await page.locator('#btn-save-settings').click();
    await page.waitForFunction(() => document.querySelector('#mask').hidden);
    check(await page.evaluate(() => document.documentElement.dataset.font) === 'xl', `${width}: touch settings save`);
    await page.locator('#btn-history').click();
    await page.waitForFunction(() => document.querySelectorAll('.history-item').length > 0);
    check(await page.locator('.history-item').count() === 50, `${width}: first history page`);
    await page.locator('#btn-history-more').click();
    check(await page.locator('.history-item').count() === 100, `${width}: history pagination`);
    await page.locator('#history-search').fill('word120');
    await page.waitForFunction(() => document.querySelectorAll('.history-item').length === 1);
    check((await page.locator('.history-item').innerText()).includes('word120'), `${width}: older history search`);
    await page.locator('#export-format').selectOption('json');
    await page.locator('#btn-export').click();
    await page.waitForFunction(() => document.querySelector('#history-msg').textContent.includes('已导出'));
    check(await page.evaluate(() => window.exportedTo.path.startsWith('content://')) && (await page.locator('#history-msg').innerText()).includes('120'), `${width}: system document export includes full history despite filter`);
    await page.screenshot({ path: `test-results/android/${width}-history.png` });
    check(errors.length === 0, `${width}: no uncaught script errors ${errors}`);
    check(await page.evaluate(() => !window.calls.some(c => ['resize_window', 'toggle_maximize'].includes(c.cmd))), `${width}: no desktop window IPC`);
    await context.close();
  }
  console.log(`Completed ${checks} Android UI checks (Edge Chromium, touch emulation, mocked IPC).`);
} finally { await browser.close(); }
