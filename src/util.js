export const $ = (id) => document.getElementById(id);

/**
 * 建立 DOM 元素。文字子節點一律以純文字插入;`html` 只用於內建的 SVG 圖示。
 * 屬性值為 null / false 時略過,true 時設為空字串(例如 hidden)。
 */
export function el(tag, attrs = {}, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs)) {
    if (value == null || value === false) continue;
    if (key === "class") node.className = value;
    else if (key === "style") node.style.cssText = value;
    else if (key === "html") node.innerHTML = value;
    else if (key === "dataset") Object.assign(node.dataset, value);
    else if (key.startsWith("on")) node.addEventListener(key.slice(2), value);
    else node.setAttribute(key, value === true ? "" : value);
  }
  node.append(...children.flat().filter((c) => c != null && c !== false));
  return node;
}

export const PALETTE = [
  "#d97757", "#5b8def", "#46a758", "#e2b340",
  "#a66fd8", "#33b3a6", "#e5677c", "#8d94a3",
];

/** 服務的代表色:設定值合法就用設定值,否則依 id 固定挑一個 */
export function colorOf(svc) {
  if (/^#[0-9a-f]{6}$/i.test(svc.color || "")) return svc.color;
  let h = 0;
  for (const ch of svc.id) h = (h * 31 + ch.charCodeAt(0)) >>> 0;
  return PALETTE[h % PALETTE.length];
}

/** 圖示上的字:多個單字取前兩個字首,否則取第一個字 */
function initials(name) {
  const words = name.trim().split(/\s+/).filter(Boolean);
  const first = (w) => [...w][0] ?? "";
  const text = words.length > 1 ? first(words[0]) + first(words[1]) : first(words[0] ?? "?");
  return text.toUpperCase();
}

/** 每個服務圖示上的字(id → 文字)。只有一個字又和別人重複時改取前兩個字,例如 Ch / Cu / Co */
export function badges(services) {
  const base = services.map((s) => initials(s.name));
  const count = {};
  for (const b of base) count[b] = (count[b] || 0) + 1;
  return new Map(
    services.map((s, i) => {
      let text = base[i];
      if (count[text] > 1 && [...text].length === 1) {
        const chars = [...s.name.trim()];
        text = chars[0].toUpperCase() + (chars[1] ?? "").toLowerCase();
      }
      return [s.id, text];
    }),
  );
}

/** 與 Rust config::sanitize_profile 相同的規則 */
export function slugify(s) {
  return (s || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

export function shortUrl(url) {
  return url.replace(/^https?:\/\//i, "").replace(/\/$/, "");
}

export function findService(cfg, id) {
  return cfg.services.find((s) => s.id === id);
}

/** 舞台上顯示的服務,規則需與 Rust views::plan_of 一致 */
export function onScreenIds(cfg) {
  if (!cfg.services.length) return [];
  if (cfg.splitMode) {
    const chosen = cfg.services.filter((s) => s.inSplit);
    return (chosen.length ? chosen : cfg.services).map((s) => s.id);
  }
  const active = findService(cfg, cfg.activeId) ?? cfg.services[0];
  return [active.id];
}

export function weightOf(cfg, id) {
  return cfg.splitWeights?.[id] ?? 1;
}
