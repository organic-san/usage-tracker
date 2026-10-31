import { api } from "./api.js";
import { store, subscribe, commit } from "./store.js";
import { activate, openSettings } from "./actions.js";
import { icon } from "./icons.js";
import { $, el, badges, colorOf, findService, onScreenIds, weightOf } from "./util.js";

// 舞台:官方頁面(Rust 端的子 webview)會依這裡回報的位置蓋上來。
// 前端用 flex 畫出同樣切分的格子,負責官方頁面以外的部分:並排時的名稱列、可拖曳的分隔線、載入中提示。

const MIN_TILE = 120; // 拖曳分隔線時每格至少保留的長度

let resizing = false;
let lastGeometry = "";

export function initStage() {
  const stage = $("stage");
  new ResizeObserver(reportGeometry).observe(stage);
  $("emptyAdd").onclick = () => openSettings("new");

  const tiles = $("tiles");
  tiles.addEventListener("pointerdown", onDividerDown);
  // 雙擊分隔線:平均分配
  tiles.addEventListener("dblclick", (e) => {
    if (!e.target.closest(".divider")) return;
    const ids = onScreenIds(store.config);
    commit((c) => ids.forEach((id) => delete c.splitWeights[id]));
  });

  subscribe((kind) => {
    if (kind !== "schedule") render();
  });
}

/** 把舞台位置(來自 CSS)告訴 Rust;只有數值改變時才送 */
function reportGeometry() {
  const r = $("stage").getBoundingClientRect();
  const css = getComputedStyle(document.documentElement);
  const px = (name) => parseFloat(css.getPropertyValue(name)) || 0;
  const geometry = {
    top: r.top,
    left: r.left,
    right: window.innerWidth - r.right,
    bottom: window.innerHeight - r.bottom,
    gap: px("--gap"),
    header: px("--tile-head"),
  };
  const key = JSON.stringify(geometry);
  if (key === lastGeometry) return;
  lastGeometry = key;
  api.setGeometry(geometry).catch(console.error);
}

function render() {
  const cfg = store.config;
  const open = store.ui.settingsOpen;
  $("settings").hidden = !open;
  $("empty").hidden = open || cfg.services.length > 0;
  if (!resizing) renderTiles(open ? [] : onScreenIds(cfg));
}

function renderTiles(ids) {
  const cfg = store.config;
  const tiles = $("tiles");
  tiles.className = `tiles ${cfg.splitDir === "v" ? "v" : "h"}${ids.length === 1 ? " single" : ""}`;
  const badge = badges(cfg.services);
  const nodes = [];
  ids.forEach((id, i) => {
    const s = findService(cfg, id);
    if (i > 0) {
      nodes.push(el("div", { class: "divider", title: "拖曳調整比例,雙擊平均分配", dataset: { index: i - 1 } }));
    }
    nodes.push(
      el(
        "div",
        { class: "tile", style: `--c:${colorOf(s)};flex-grow:${weightOf(cfg, id)}`, dataset: { id } },
        el(
          "div",
          { class: "tile-head" },
          el("span", { class: "dot" }),
          el("span", { class: "tile-name" }, s.name),
          el("button", { class: "icon-btn xs", title: "重新整理", html: icon("refresh"), onclick: () => api.refresh(id) }),
          el("button", { class: "icon-btn xs", title: "單獨檢視", html: icon("expand"), onclick: () => activate(id) }),
        ),
        // 官方頁面會蓋住這一塊;只有頁面還沒出現時才看得到
        el(
          "div",
          { class: "tile-body" },
          el("span", { class: "avatar lg" }, badge.get(id)),
          el("span", { class: "loading" }, `正在載入 ${s.name}…`),
        ),
      ),
    );
  });
  tiles.replaceChildren(...nodes);
}

// ---- 拖曳分隔線調整比例 ----

let previewing = false;
let pendingWeights = null;

/** 拖曳中把權重即時送給 Rust 重新排版;前一次還沒完成就只保留最新的一筆 */
async function preview(weights) {
  pendingWeights = weights;
  if (previewing) return;
  previewing = true;
  while (pendingWeights) {
    const w = pendingWeights;
    pendingWeights = null;
    await api.previewWeights(w).catch(console.error);
  }
  previewing = false;
}

function onDividerDown(e) {
  const divider = e.target.closest(".divider");
  if (!divider || e.button !== 0) return;
  e.preventDefault();

  const i = Number(divider.dataset.index);
  const tileEls = [...$("tiles").querySelectorAll(".tile")];
  const vertical = store.config.splitDir === "v";
  const sizeOf = (node) => {
    const r = node.getBoundingClientRect();
    return vertical ? r.height : r.width;
  };
  const a = tileEls[i];
  const b = tileEls[i + 1];
  const sa = sizeOf(a);
  const sb = sizeOf(b);
  const ids = tileEls.map((node) => node.dataset.id);
  const weights = ids.map((id) => weightOf(store.config, id));
  const pair = weights[i] + weights[i + 1];
  const min = Math.min(MIN_TILE, (sa + sb) / 2);
  const start = vertical ? e.clientY : e.clientX;

  // 捕捉指標,游標移到官方頁面上方時也能繼續拖
  divider.setPointerCapture(e.pointerId);
  divider.classList.add("active");
  resizing = true;

  const move = (ev) => {
    const delta = (vertical ? ev.clientY : ev.clientX) - start;
    const na = Math.max(min, Math.min(sa + sb - min, sa + delta));
    weights[i] = (pair * na) / (sa + sb);
    weights[i + 1] = pair - weights[i];
    a.style.flexGrow = weights[i];
    b.style.flexGrow = weights[i + 1];
    preview(weights.slice());
  };
  const up = () => {
    divider.removeEventListener("pointermove", move);
    divider.removeEventListener("pointerup", up);
    divider.removeEventListener("pointercancel", up);
    divider.classList.remove("active");
    resizing = false;
    commit((c) => ids.forEach((id, k) => (c.splitWeights[id] = Math.round(weights[k] * 1000) / 1000)));
  };
  divider.addEventListener("pointermove", move);
  divider.addEventListener("pointerup", up);
  divider.addEventListener("pointercancel", up);
}
