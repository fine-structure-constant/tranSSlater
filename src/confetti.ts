/** 单词礼炮：本地词池 + 有限时长 Canvas 粒子，不占用翻译状态与输入焦点。 */
const EMOJI = ["🎉", "✨", "🌱", "📚", "🚀", "💛", "🍀", "⭐", "🔥", "🤯", "🪩", "💥", "🥳", "🌈"];
const DURATION = 3600;
const WORD_COUNT = 40;

type Sprite = { image: HTMLCanvasElement; width: number; height: number; pixels: number };

type Particle = {
  kind: "word" | "emoji" | "paper";
  x: number;
  y: number;
  vx: number;
  vy: number;
  angle: number;
  spin: number;
  size: number;
  color: string;
  sprite: Sprite | null;
  radius: number;
  wobble: number;
};

/** 打乱副本，避免修改历史数组或总让最近的同一个词占据首位。 */
function shuffled<T>(items: readonly T[]): T[] {
  const result = [...items];
  for (let i = result.length - 1; i > 0; i--) {
    const j = Math.floor(Math.random() * (i + 1));
    [result[i], result[j]] = [result[j], result[i]];
  }
  return result;
}

function wordPool(words: readonly string[]): string[] {
  const seen = new Set<string>();
  return words.filter((word) => {
    const key = word.trim().toLowerCase();
    if (!key || [...key].length > 32 || /[\r\n\t]/u.test(key) || seen.has(key)) return false;
    seen.add(key);
    return true;
  }).map((word) => word.trim());
}

export function attachWordConfetti(button: HTMLButtonElement, loadWords: () => Promise<string[]>): void {
  const motion = window.matchMedia("(prefers-reduced-motion: reduce)");
  const feedback = document.createElement("div");
  feedback.id = "celebration-feedback";
  feedback.className = "celebration-feedback";
  feedback.setAttribute("role", "status");
  feedback.setAttribute("aria-live", "polite");
  feedback.setAttribute("aria-atomic", "true");
  document.body.append(feedback);

  let canvas: HTMLCanvasElement | null = null;
  let frame = 0;
  let timer: number | undefined;
  let generation = 0;
  let fetching = false;
  const sprites = new Map<string, Sprite>();
  let spritePixels = 0;

  function stop(): void {
    generation++;
    fetching = false;
    window.cancelAnimationFrame(frame);
    window.clearTimeout(timer);
    canvas?.remove();
    canvas = null;
    feedback.replaceChildren();
    button.removeAttribute("data-celebrating");
    button.removeAttribute("aria-busy");
  }

  function announce(words: string[], failed: boolean, quiet: boolean): void {
    const note = document.createElement("div");
    note.className = "celebration-note";
    const message = document.createElement("div");
    message.textContent = failed
      ? "历史词条暂时读取失败，先放个 emoji 礼炮。"
      : words.length ? "每查一个词，都在向前。" : "先为好奇心庆祝，下一次带上新单词。";
    note.append(message);
    if (quiet) {
      const bouquet = document.createElement("div");
      bouquet.className = "celebration-bouquet";
      for (const text of [...words.slice(0, 6), "🎉", "🌱", "✨"]) {
        const item = document.createElement("span");
        item.className = "celebration-word";
        item.textContent = text;
        bouquet.append(item);
      }
      note.append(bouquet);
    }
    feedback.replaceChildren(note);
  }

  function play(words: string[], failed: boolean): void {
    stop();
    button.dataset.celebrating = "true";
    const selected = shuffled(wordPool(words)).slice(0, WORD_COUNT);
    const quiet = motion.matches;
    announce(selected, failed, quiet);
    if (quiet) { timer = window.setTimeout(stop, DURATION); return; }

    canvas = document.createElement("canvas");
    canvas.id = "word-confetti";
    canvas.className = "word-confetti";
    canvas.setAttribute("aria-hidden", "true");
    document.body.append(canvas);
    const ctx = canvas.getContext("2d");
    if (!ctx) { timer = window.setTimeout(stop, DURATION); return; }

    const width = window.innerWidth;
    const height = window.innerHeight;
    // 全屏透明画布不必按 2x/3x 绘制；位图仍保留最多 1.5x 的清晰度。
    const dpr = Math.max(1, Math.min(window.devicePixelRatio || 1, 1.5, Math.sqrt(2_000_000 / (width * height))));
    canvas.width = Math.round(width * dpr);
    canvas.height = Math.round(height * dpr);
    const theme = getComputedStyle(document.documentElement);
    const colors = ["--accent", "--link", "--ok", "--text-2"].map((name) => theme.getPropertyValue(name).trim());
    const surface = theme.getPropertyValue("--surface").trim();
    const ink = theme.getPropertyValue("--text").trim();
    const font = theme.getPropertyValue("--font").trim();
    const scale = Number.parseFloat(theme.getPropertyValue("--fs")) || 1;
    const onAccent = theme.getPropertyValue("--on-accent").trim();
    const gravity = height * 0.95;
    const lift = Math.sqrt(2 * gravity * height * 0.82);
    const particles: Particle[] = [];

    function spriteFor(kind: "word" | "emoji", text: string, requestedSize: number, color: string, style: number): Sprite | null {
      const key = JSON.stringify([kind, text, requestedSize, color, style, font, surface, ink, onAccent, dpr, Math.round(width * 0.45)]);
      const hit = sprites.get(key);
      if (hit) { sprites.delete(key); sprites.set(key, hit); return hit; }
      let size = requestedSize;
      const fontFor = () => kind === "emoji"
        ? `${size}px "Segoe UI Emoji", "Apple Color Emoji", sans-serif`
        : `900 ${size}px ${font}`;
      ctx!.font = fontFor();
      let metrics = ctx!.measureText(text);
      if (kind === "word" && metrics.width > width * 0.45) {
        size *= width * 0.45 / metrics.width;
        ctx!.font = fontFor();
        metrics = ctx!.measureText(text);
      }
      const padding = kind === "word" ? 9 : 5;
      const spriteWidth = Math.ceil(metrics.width + padding * 2);
      const spriteHeight = Math.ceil(Math.max(size * 1.35, metrics.actualBoundingBoxAscent + metrics.actualBoundingBoxDescent) + padding * 2);
      const image = document.createElement("canvas");
      image.width = Math.ceil(spriteWidth * dpr);
      image.height = Math.ceil(spriteHeight * dpr);
      const paint = image.getContext("2d");
      if (!paint) return null;
      paint.scale(dpr, dpr);
      paint.translate(spriteWidth / 2, spriteHeight / 2);
      paint.font = fontFor();
      paint.textAlign = "center";
      paint.textBaseline = "middle";
      if (kind === "word") {
        if (style === 1) {
          paint.fillStyle = color;
          paint.fillRect(-spriteWidth / 2, -spriteHeight / 2, spriteWidth, spriteHeight);
          paint.fillStyle = onAccent;
        } else {
          paint.strokeStyle = style === 2 ? color : surface;
          paint.lineWidth = style === 2 ? 3 : 5;
          paint.lineJoin = "round";
          paint.strokeText(text, 0, 0);
          paint.fillStyle = style === 2 ? ink : color;
        }
      }
      paint.fillText(text, 0, 0);
      const sprite = { image, width: spriteWidth, height: spriteHeight, pixels: image.width * image.height };
      // 缓存最多 96 张 / 约 8 MB 像素，主题与字号也属于缓存键。
      while (sprites.size >= 96 || spritePixels + sprite.pixels > 2_000_000) {
        const oldest = sprites.keys().next().value;
        if (oldest === undefined) break;
        spritePixels -= sprites.get(oldest)!.pixels;
        sprites.delete(oldest);
      }
      sprites.set(key, sprite);
      spritePixels += sprite.pixels;
      return sprite;
    }

    function add(kind: Particle["kind"], text: string, index: number): void {
      const x = width * [0.1, 0.36, 0.64, 0.9][index % 4];
      const vy = -lift * (0.84 + Math.random() * 0.22);
      const peakTime = -vy / gravity;
      const vx = ((width * 0.5 - x) * 0.35 + (Math.random() - 0.5) * width * 0.6) / peakTime;
      // 离散尺寸让同一个词/emoji 能复用位图，避免每次产生新字号。
      const sizes = kind === "word" ? [22, 27, 33, 40] : kind === "emoji" ? [30, 38, 46, 54] : [6, 8, 10, 12];
      const size = sizes[Math.floor(index / 4) % sizes.length] * scale;
      const color = colors[index % colors.length];
      const sprite = kind === "paper" ? null : spriteFor(kind, text, size, color, Math.floor(index / 4) % 3);
      if (kind !== "paper" && !sprite) return;
      particles.push({
        kind, x, y: height + 12, vx, vy, size, color, sprite,
        angle: (Math.random() - 0.5) * (kind === "word" ? 1.2 : Math.PI * 2),
        spin: (Math.random() - 0.5) * (kind === "paper" ? 18 : kind === "emoji" ? 5 : 3.2),
        radius: sprite ? Math.hypot(sprite.width, sprite.height) / 2 : size,
        wobble: Math.random() * Math.PI * 2,
      });
    }
    // 四点同时喷出一波，仍保留词汇之间的重叠与旋转。
    const n = selected.length ? 64 : 0;
    for (let i = 0; i < n; i++) add("word", selected[i % selected.length], i);
    for (let i = 0; i < 40; i++) add("emoji", EMOJI[i % EMOJI.length], i);
    for (let i = 0; i < 128; i++) add("paper", "", i);

    const start = performance.now();
    function draw(now: number): void {
      if (!canvas || !ctx) return;
      const age = (now - start) / 1000;
      if (now - start >= DURATION) { stop(); return; }
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      ctx.globalAlpha = Math.max(0, Math.min(1, (DURATION / 1000 - age) / 0.65));
      let visible = 0;
      for (const p of particles) {
        const t = age;
        const x = p.x + p.vx * (1 - Math.exp(-0.22 * t)) / 0.22 + (Math.sin(t * 9 + p.wobble) - Math.sin(p.wobble)) * 8;
        const y = p.y + p.vy * t + gravity * t * t * 0.5;
        if (x + p.radius < 0 || x - p.radius > width || y + p.radius < 0 || y - p.radius > height) continue;
        visible++;
        const angle = p.angle + p.spin * t;
        const cos = Math.cos(angle), sin = Math.sin(angle);
        if (p.kind === "paper") {
          const flip = 0.2 + Math.abs(Math.cos(t * 15 + p.wobble)) * 0.8;
          ctx.setTransform(cos * dpr, sin * dpr, -sin * dpr * flip, cos * dpr * flip, x * dpr, y * dpr);
          ctx.fillStyle = p.color;
          ctx.fillRect(-p.size / 2, -p.size / 3, p.size, p.size * 0.67);
        } else if (p.sprite) {
          ctx.setTransform(cos * dpr, sin * dpr, -sin * dpr, cos * dpr, x * dpr, y * dpr);
          ctx.drawImage(p.sprite.image, -p.sprite.width / 2, -p.sprite.height / 2, p.sprite.width, p.sprite.height);
        }
      }
      if (!visible && age > 0.4) { stop(); return; }
      frame = window.requestAnimationFrame(draw);
    }
    frame = window.requestAnimationFrame(draw);
    // 即使 rAF 被节流，仍有明确的资源回收上限。
    timer = window.setTimeout(stop, DURATION + 150);
  }

  button.addEventListener("click", async () => {
    if (fetching) return;
    stop();
    const ticket = generation;
    fetching = true;
    button.setAttribute("aria-busy", "true");
    try {
      const words = await loadWords();
      if (ticket === generation && !document.hidden) play(words, false);
    } catch {
      if (ticket === generation && !document.hidden) play([], true);
    } finally {
      if (ticket === generation) { fetching = false; button.removeAttribute("aria-busy"); }
    }
  });
  document.addEventListener("visibilitychange", () => { if (document.hidden) stop(); });
  document.addEventListener("keydown", (event) => { if (event.key === "Escape") stop(); });
  window.addEventListener("resize", stop);
  window.addEventListener("pagehide", () => { stop(); sprites.clear(); spritePixels = 0; });
  motion.addEventListener("change", stop);
}
