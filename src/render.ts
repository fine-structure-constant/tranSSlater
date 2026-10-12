/** 把 TranslateResult 渲染成 DOM。 */

import type {
  DefinitionGroup,
  DictEntry,
  Example,
  RelatedGroup,
  Section,
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
 * 后端返回的例句里用 `<b>` 标出查询词。先整体转义，
 * 再把 `<b>` / `</b>` 放回来——只允许这一种标签，避免注入。
 */
function allowBold(s: string): string {
  return esc(s)
    .replace(/&lt;b&gt;/g, "<b>")
    .replace(/&lt;\/b&gt;/g, "</b>");
}

/**
 * 极简 Markdown 渲染，只支持有道的 AI 释义用到的语法：
 * 标题、无序列表、`**粗体**`、行内代码。
 * 输入先整体转义，因此不会引入注入。
 */
function miniMarkdown(src: string): string {
  const inline = (s: string): string =>
    s.replace(/\*\*(.+?)\*\*/g, "<b>$1</b>").replace(/`([^`]+)`/g, "<code>$1</code>");

  const out: string[] = [];
  let inList = false;
  for (const raw of esc(src).split(/\r?\n/)) {
    const line = raw.trimEnd();
    const heading = /^#{1,6}\s+(.*)$/.exec(line);
    const bullet = /^\s*[-*]\s+(.*)$/.exec(line);

    if (heading) {
      if (inList) {
        out.push("</ul>");
        inList = false;
      }
      out.push(`<h4>${inline(heading[1])}</h4>`);
      continue;
    }
    if (bullet) {
      if (!inList) {
        out.push("<ul>");
        inList = true;
      }
      out.push(`<li>${inline(bullet[1])}</li>`);
      continue;
    }
    if (inList) {
      out.push("</ul>");
      inList = false;
    }
    if (line.trim()) out.push(`<p>${inline(line)}</p>`);
  }
  if (inList) out.push("</ul>");
  return out.join("");
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

/** 一条例句：原文（保留 <b>）+ 译文 + 出处。 */
function exampleNode(ex: Example): HTMLElement {
  const item = el("div", "example-item plain");
  item.appendChild(el("div", "ex-text", allowBold(ex.text)));
  if (ex.translation) item.appendChild(el("div", "ex-tran", esc(ex.translation)));
  if (ex.source) item.appendChild(el("div", "ex-source", esc(ex.source)));
  return item;
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
 * @param onQuery 点击可查词元素（近义词 / 译词 / 词组）时的回调
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
      if (seg.source.trim()) item.appendChild(el("div", "src", esc(seg.source.trim())));
      item.appendChild(el("div", "dst", esc(seg.text.trim())));
      box.appendChild(item);
    }
    root.appendChild(box);
  } else {
    root.appendChild(
      el("div", r.isWord ? "main-trans" : "main-trans sentence", esc(r.translation)),
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
  // 柯林斯排在前面：它是唯一有完整英文释义 + 中文对译 + 例句的权威词典。
  const collins = r.sections.find((s) => s.key === "collins");
  if (collins) {
    panels.push({
      key: "collins",
      label: collins.title,
      count: countSectionItems(collins),
      render: () => sectionPanel(collins),
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
  if (r.phrases.length > 0) {
    panels.push({
      key: "phrs",
      label: "词组",
      count: r.phrases.length,
      render: () => phrasesPanel(r.phrases, onQuery),
    });
  }
  if (r.related.length > 0) {
    panels.push({
      key: "rel",
      label: "同根词",
      render: () => relatedPanel(r.related, onQuery),
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
  // 其余板块按后端给的顺序追加（专业释义 / 网络释义 / 辨析 / 词源 / AI 释义）
  for (const section of r.sections) {
    if (section.key === "collins") continue;
    panels.push({
      key: `sec-${section.key}`,
      label: section.title,
      count: section.key === "ai" || section.key === "etym" ? undefined : countSectionItems(section),
      render: () => sectionPanel(section),
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
      esc(p.label) + (p.count !== undefined ? `<span class="count">${p.count}</span>` : "");
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

function countSectionItems(section: Section): number {
  return section.groups.reduce((n, g) => n + g.items.length, 0);
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

    const head = el("div", "dict-head");
    head.appendChild(el("div", "pos", esc(entry.pos)));
    for (const exam of entry.exams.slice(0, 6)) {
      head.appendChild(el("span", "exam", esc(exam)));
    }
    row.appendChild(head);

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
    const head = el("div", "dict-head");
    head.appendChild(el("div", "pos", esc(group.pos)));
    if (group.source) head.appendChild(el("span", "exam", esc(group.source)));
    g.appendChild(head);

    group.definitions.forEach((d, i) => {
      const item = el("div", "def-item");
      item.appendChild(el("div", "num", `${i + 1}.`));
      const body = el("div", "def-body");
      body.appendChild(el("div", "text", esc(d.text)));
      if (d.example) body.appendChild(el("div", "ex", esc(d.example)));
      if (d.similar.length > 0) {
        body.appendChild(el("div", "ex", `近义：${esc(d.similar.join("、"))}`));
      }
      item.appendChild(body);
      g.appendChild(item);
    });
    box.appendChild(g);
  }
  return box;
}

// ── 词组短语 ────────────────────────────────────────────────

function phrasesPanel(
  phrases: { term: string; translation: string; source: string | null }[],
  onQuery: (text: string) => void,
): Node {
  const list = el("div", "phrase-list");
  for (const phrase of phrases) {
    const row = el("div", "phrase-item");
    const term = el("button", "phrase-term", esc(phrase.term));
    term.type = "button";
    term.title = `查询「${phrase.term}」`;
    term.addEventListener("click", () => onQuery(phrase.term));
    row.appendChild(term);
    row.appendChild(el("div", "phrase-tran", esc(phrase.translation)));
    if (phrase.source) row.appendChild(el("div", "ex-source", esc(phrase.source)));
    list.appendChild(row);
  }
  return list;
}

// ── 同根词 ──────────────────────────────────────────────────

function relatedPanel(groups: RelatedGroup[], onQuery: (text: string) => void): Node {
  const box = document.createDocumentFragment();
  for (const group of groups) {
    const block = el("div", "chip-block");
    block.appendChild(el("div", "pos", esc(group.pos)));
    const chips = el("div", "chips");
    for (const item of group.words) {
      const chip = el("button", "chip rel");
      chip.type = "button";
      chip.title = `查询「${item.word}」`;
      chip.innerHTML = `<span class="rel-word">${esc(item.word)}</span>` +
        (item.gloss ? `<span class="rel-gloss">${esc(item.gloss)}</span>` : "");
      chip.addEventListener("click", () => onQuery(item.word));
      chips.appendChild(chip);
    }
    block.appendChild(chips);
    box.appendChild(block);
  }
  return box;
}

// ── 同近义词 ────────────────────────────────────────────────

function synonymsPanel(groups: SynonymGroup[], onQuery: (text: string) => void): Node {
  const box = document.createDocumentFragment();
  for (const group of groups) {
    const block = el("div", "chip-block");
    const head = el("div", "dict-head");
    head.appendChild(el("div", "pos", esc(group.pos)));
    if (group.gloss) head.appendChild(el("span", "gloss", esc(group.gloss)));
    block.appendChild(head);

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

function examplesPanel(examples: Example[]): Node {
  const list = el("div", "example-list");
  examples.forEach((e, i) => {
    const item = el("div", "example-item");
    item.appendChild(el("div", "num", `${i + 1}.`));
    const body = el("div", "example-body");
    body.appendChild(el("div", "ex-text", allowBold(e.text)));
    if (e.translation) body.appendChild(el("div", "ex-tran", esc(e.translation)));
    if (e.source) body.appendChild(el("div", "ex-source", esc(e.source)));
    item.appendChild(body);
    list.appendChild(item);
  });
  return list;
}

// ── 通用板块（柯林斯 / 专业释义 / 网络释义 / 辨析 / 词源 / AI） ──

function sectionPanel(section: Section): Node {
  const box = document.createDocumentFragment();
  if (section.note) box.appendChild(el("p", "section-note", esc(section.note)));

  for (const group of section.groups) {
    const g = el("div", "sec-group");
    if (group.label) g.appendChild(el("div", "pos", esc(group.label)));
    if (group.note) g.appendChild(el("div", "sec-note", esc(group.note)));

    for (const item of group.items) {
      const node = el("div", "sec-item");
      if (section.markdown) {
        node.appendChild(el("div", "md", miniMarkdown(item.text)));
      } else if (item.text.includes("\n")) {
        // 词源这类整段文本要保留换行
        node.appendChild(el("div", "pre", esc(item.text)));
      } else {
        node.appendChild(el("div", "sec-text", esc(item.text)));
      }
      if (item.translation) node.appendChild(el("div", "sec-tran", esc(item.translation)));
      if (item.note) node.appendChild(el("div", "sec-note", esc(item.note)));
      for (const ex of item.extra) node.appendChild(exampleNode(ex));
      g.appendChild(node);
    }
    box.appendChild(g);
  }
  return box;
}
