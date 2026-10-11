/** 把 TranslateResult 渲染成 DOM。 */

import type {
  DefinitionGroup,
  DictEntry,
  SynonymGroup,
  TranslateResult,
} from "./types";
import { langName } from "./types";

/** 转义 HTML。 */
function esc(s: string): string {
  return s
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;");
}

/**
 * Google 的例句里用 `<b>` 标出查询词。先整体转义，
 * 再把 `<b>` / `</b>` 放回来——只允许这一种标签，避免注入。
 */
function allowBold(s: string): string {
  return esc(s)
    .replace(/&lt;b&gt;/g, "<b>")
    .replace(/&lt;\/b&gt;/g, "</b>");
}

function el<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  cls?: string,
  html?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (cls) node.className = cls;
  if (html !== undefined) node.innerHTML = html;
  return node;
}

/** 每个板块的渲染函数。返回 null 表示该板块无内容。 */
type Panel = {
  key: string;
  label: string;
  count?: number;
  render: () => Node;
};

/**
 * 渲染整块结果。
 * @param onQuery 点击可查词元素（近义词 / 译词）时的回调
 */
export function renderResult(
  root: HTMLElement,
  r: TranslateResult,
  onQuery: (text: string) => void,
): void {
  root.innerHTML = "";

  // ── 主译文 ──────────────────────────────────────────────
  const head = el("div", "result-head");
  const srcLabel =
    r.sourceLang === r.targetLang
      ? langName(r.sourceLang)
      : `${langName(r.sourceLang)} → ${langName(r.targetLang)}`;
  head.appendChild(el("span", "label", esc(srcLabel)));
  if (r.cached) head.appendChild(el("span", "label", "缓存命中"));
  root.appendChild(head);

  if (r.segments.length > 1) {
    // 多段：保留原文/译文对照
    const box = el("div", "segments");
    for (const seg of r.segments) {
      const item = el("div", "segment");
      if (seg.source.trim()) {
        item.appendChild(el("div", "src", esc(seg.source.trim())));
      }
      item.appendChild(el("div", "dst", esc(seg.text.trim())));
      box.appendChild(item);
    }
    root.appendChild(box);
  } else {
    root.appendChild(
      el(
        "div",
        r.isWord ? "main-trans" : "main-trans sentence",
        esc(r.translation),
      ),
    );
  }

  // ── 组装可用板块 ────────────────────────────────────────
  const panels: Panel[] = [];

  if (r.dictionary.length > 0 || r.alternatives.length > 0) {
    panels.push({
      key: "simple",
      label: "简明",
      render: () => simplePanel(r.dictionary, r.alternatives, onQuery),
    });
  }
  if (r.definitions.length > 0) {
    panels.push({
      key: "def",
      label: "英英释义",
      count: countDefinitions(r.definitions),
      render: () => definitionsPanel(r.definitions),
    });
  }
  if (r.synonyms.length > 0) {
    panels.push({
      key: "syn",
      label: "同近义词",
      render: () => synonymsPanel(r.synonyms, onQuery),
    });
  }
  if (r.examples.length > 0) {
    panels.push({
      key: "ex",
      label: "例句",
      count: r.examples.length,
      render: () => examplesPanel(r.examples),
    });
  }

  // 整句查询时没有词典板块，到此为止
  if (panels.length === 0) return;

  // ── 标签栏 ──────────────────────────────────────────────
  const tabBar = el("div", "tabs");
  const panelHost = el("div", "panel");

  const buttons: HTMLButtonElement[] = [];

  panels.forEach((p, i) => {
    const btn = el("button", "tab");
    btn.type = "button";
    btn.innerHTML =
      esc(p.label) +
      (p.count !== undefined ? `<span class="count">${p.count}</span>` : "");
    btn.addEventListener("click", () => select(i));
    tabBar.appendChild(btn);
    buttons.push(btn);
  });

  function select(index: number): void {
    buttons.forEach((b, i) => b.classList.toggle("active", i === index));
    // DocumentFragment 插入 DOM 后会被清空，切回标签时必须重新渲染。
    panelHost.replaceChildren(panels[index].render());
  }

  root.appendChild(tabBar);
  root.appendChild(panelHost);
  select(0);
}

function countDefinitions(groups: DefinitionGroup[]): number {
  return groups.reduce((n, g) => n + g.definitions.length, 0);
}

// ── 简明 ────────────────────────────────────────────────────

function simplePanel(
  entries: DictEntry[],
  alternatives: string[],
  onQuery: (text: string) => void,
): Node {
  const box = document.createDocumentFragment();

  for (const entry of entries) {
    const row = el("div", "dict-entry");

    row.appendChild(el("div", "pos", esc(entry.pos)));

    const terms = el("div", "terms");
    entry.terms.forEach((t, i) => {
      if (i > 0) terms.appendChild(el("span", "sep", "，"));
      const span = el("button", "term", esc(t));
      span.type = "button";
      span.title = `查询「${t}」`;
      span.addEventListener("click", () => onQuery(t));
      terms.appendChild(span);
    });
    row.appendChild(terms);

    // 反查：译词 ← 源语词。取前 4 组，避免刷屏。
    const back = entry.back.filter((b) => b.sourceWords.length > 0).slice(0, 4);
    if (back.length > 0) {
      const backRow = el("div", "back-row");
      back.forEach((b) => {
        const pair = el("span", "pair");
        pair.appendChild(el("span", "", esc(b.term)));
        pair.appendChild(el("span", "arrow", "←"));
        pair.appendChild(el("span", "", esc(b.sourceWords.join(", "))));
        backRow.appendChild(pair);
      });
      row.appendChild(backRow);
    }

    box.appendChild(row);
  }

  if (alternatives.length > 0) {
    const block = el("div", "chip-block");
    block.style.marginTop = entries.length > 0 ? "20px" : "0";
    block.appendChild(el("div", "pos", "其他译法"));
    const chips = el("div", "chips");
    for (const a of alternatives) {
      const chip = el("button", "chip zh", esc(a));
      chip.type = "button";
      chip.title = `查询「${a}」`;
      chip.addEventListener("click", () => onQuery(a));
      chips.appendChild(chip);
    }
    block.appendChild(chips);
    box.appendChild(block);
  }

  return box;
}

// ── 英英释义 ────────────────────────────────────────────────

function definitionsPanel(groups: DefinitionGroup[]): Node {
  const box = document.createDocumentFragment();
  for (const group of groups) {
    const g = el("div", "def-group");
    g.appendChild(el("div", "pos", esc(group.pos)));
    group.definitions.forEach((d, i) => {
      const item = el("div", "def-item");
      item.appendChild(el("div", "num", `${i + 1}.`));
      item.appendChild(el("div", "text", esc(d.text)));
      if (d.example) item.appendChild(el("div", "ex", esc(d.example)));
      g.appendChild(item);
    });
    box.appendChild(g);
  }
  return box;
}

// ── 同近义词 ────────────────────────────────────────────────

function synonymsPanel(
  groups: SynonymGroup[],
  onQuery: (text: string) => void,
): Node {
  const box = document.createDocumentFragment();
  for (const group of groups) {
    const block = el("div", "chip-block");
    block.appendChild(el("div", "pos", esc(group.pos)));
    const chips = el("div", "chips");
    for (const w of group.words) {
      const chip = el("button", "chip", esc(w));
      chip.type = "button";
      chip.title = `查询「${w}」`;
      chip.addEventListener("click", () => onQuery(w));
      chips.appendChild(chip);
    }
    block.appendChild(chips);
    box.appendChild(block);
  }
  return box;
}

// ── 例句 ────────────────────────────────────────────────────

function examplesPanel(examples: string[]): Node {
  const list = el("div", "example-list");
  examples.forEach((e, i) => {
    const item = el("div", "example-item");
    item.appendChild(el("div", "num", `${i + 1}.`));
    item.appendChild(el("div", "", allowBold(e)));
    list.appendChild(item);
  });
  return list;
}
