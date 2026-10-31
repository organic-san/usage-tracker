import { api } from "./api.js";
import { store, subscribe } from "./store.js";
import {
  activate, closeSettings, moveService, openSettings,
  toggleDir, toggleInSplit, toggleSplit,
} from "./actions.js";
import { popup } from "./menu.js";
import { sortable } from "./sortable.js";
import { icon } from "./icons.js";
import { $, el, badges, colorOf, findService, onScreenIds } from "./util.js";

// 左側服務側欄:點選切換、拖曳排序、右鍵開原生選單;下方是檢視與設定按鈕。
// 服務再多也只是往下排(超出時可捲動),不會擠壓其他控制項。

export function initRail() {
  const list = $("railList");
  sortable(list, { items: ".chip:not(.add)", onMove: moveService });
  list.addEventListener("click", (e) => {
    const chip = e.target.closest(".chip");
    if (!chip) return;
    if (chip.classList.contains("add")) openSettings("new");
    else activate(chip.dataset.id);
  });
  list.addEventListener("contextmenu", (e) => {
    const chip = e.target.closest(".chip:not(.add)");
    if (!chip) return;
    e.preventDefault();
    showMenu(chip.dataset.id).catch(console.error);
  });

  $("splitBtn").innerHTML = icon("split");
  $("splitBtn").onclick = toggleSplit;
  $("dirBtn").onclick = toggleDir;
  $("settingsBtn").innerHTML = icon("settings");
  $("settingsBtn").onclick = () => (store.ui.settingsOpen ? closeSettings() : openSettings());

  subscribe((kind) => {
    if (kind !== "schedule") render();
  });
}

function render() {
  const list = $("railList");
  if (list.classList.contains("sorting")) return; // 拖曳中重繪會弄丟正在拖的元素
  const cfg = store.config;
  const shown = new Set(store.ui.settingsOpen ? [] : onScreenIds(cfg));
  const badge = badges(cfg.services);

  const chips = cfg.services.map((s, i) =>
    el(
      "button",
      {
        class: "chip" + (shown.has(s.id) ? " on" : ""),
        style: `--c:${colorOf(s)}`,
        title: i < 9 ? `${s.name}\nCtrl+${i + 1}` : s.name,
        "aria-label": s.name,
        dataset: { id: s.id },
      },
      badge.get(s.id),
    ),
  );
  chips.push(el("button", { class: "chip add", title: "新增服務", "aria-label": "新增服務", html: icon("plus") }));
  list.replaceChildren(...chips);

  const split = $("splitBtn");
  split.classList.toggle("on", cfg.splitMode);
  split.title = cfg.splitMode ? "結束並排" : "並排顯示";
  const dir = $("dirBtn");
  dir.hidden = !cfg.splitMode;
  dir.innerHTML = icon(cfg.splitDir === "v" ? "rows" : "cols");
  dir.title = cfg.splitDir === "v" ? "目前上下排列,點擊改為左右" : "目前左右排列,點擊改為上下";
  $("settingsBtn").classList.toggle("on", store.ui.settingsOpen);
}

async function showMenu(id) {
  const cfg = store.config;
  const s = findService(cfg, id);
  if (!s) return;
  const onScreen = !store.ui.settingsOpen && onScreenIds(cfg).includes(id);
  await popup([
    { text: "重新整理", enabled: onScreen, action: () => api.refresh(id) },
    { text: "並排時顯示", checked: s.inSplit, action: () => toggleInSplit(id) },
    { item: "Separator" },
    { text: "編輯…", action: () => openSettings(id) },
  ]);
}
