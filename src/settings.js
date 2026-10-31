import { enable, disable, isEnabled } from "@tauri-apps/plugin-autostart";
import { getVersion } from "@tauri-apps/api/app";
import { store, subscribe, commit, setUi } from "./store.js";
import { closeSettings, moveService, toggleInSplit } from "./actions.js";
import { sortable } from "./sortable.js";
import { icon } from "./icons.js";
import { $, el, badges, colorOf, findService, PALETTE, shortUrl, slugify } from "./util.js";

// 設定頁:顯示在舞台區域(開啟期間官方頁面會先藏起來),所有變更立即生效。

const INTERVALS = [
  [0, "關閉"],
  [30, "30 秒"],
  [60, "1 分鐘"],
  [120, "2 分鐘"],
  [300, "5 分鐘"],
  [600, "10 分鐘"],
  [1800, "30 分鐘"],
];

let draft = null; // 編輯表單的內容 { key, name, url, profile, color, error, fresh }
let confirmId = null; // 等待確認刪除的服務
let lastListKey = "";

export function initSettings() {
  $("settingsClose").innerHTML = icon("close");
  $("settingsClose").onclick = closeSettings;
  $("addService").innerHTML = `${icon("plus")}<span>新增</span>`;
  $("addService").onclick = () => setUi({ editing: "new" });
  sortable($("svcList"), { items: ".svc", handle: ".grip", onMove: moveService });

  const interval = $("interval");
  interval.onchange = () => commit((c) => (c.intervalSec = Number(interval.value)));
  $("alwaysOnTop").onchange = (e) => commit((c) => (c.alwaysOnTop = e.target.checked));

  // 開機自啟以系統實際狀態為準,不存在設定檔裡
  const autostart = $("autostart");
  isEnabled()
    .then((on) => (autostart.checked = on))
    .catch(() => (autostart.disabled = true));
  autostart.onchange = async () => {
    const want = autostart.checked;
    try {
      await (want ? enable() : disable());
    } catch (e) {
      console.error("autostart toggle failed", e);
      autostart.checked = !want;
    }
  };

  getVersion()
    .then((v) => ($("version").textContent = `Usage Tracker v${v}`))
    .catch(() => {});

  subscribe((kind) => {
    if (kind !== "schedule") render();
  });
}

function render() {
  if (!store.ui.settingsOpen) {
    draft = null;
    confirmId = null;
    lastListKey = "";
    return;
  }
  const cfg = store.config;

  const options = INTERVALS.some(([v]) => v === cfg.intervalSec)
    ? INTERVALS
    : [...INTERVALS, [cfg.intervalSec, `${cfg.intervalSec} 秒`]];
  const interval = $("interval");
  if (interval.options.length !== options.length) {
    interval.replaceChildren(...options.map(([v, text]) => el("option", { value: v }, text)));
  }
  interval.value = String(cfg.intervalSec);
  $("alwaysOnTop").checked = cfg.alwaysOnTop;

  syncDraft();
  // 清單只在內容真的改變時重建,避免打字到一半輸入框被換掉
  const key = JSON.stringify([cfg.services, draft?.key, confirmId]);
  if (key !== lastListKey) renderList();
}

function syncDraft() {
  const editing = store.ui.editing;
  const svc = editing && editing !== "new" ? findService(store.config, editing) : null;
  if (!editing || (editing !== "new" && !svc)) {
    draft = null;
    return;
  }
  if (draft?.key === editing) return;
  draft = {
    key: editing,
    name: svc?.name ?? "",
    url: svc?.url ?? "",
    profile: svc?.profile ?? "",
    color: svc ? colorOf(svc) : unusedColor(),
    error: "",
    fresh: true,
  };
  confirmId = null;
}

function unusedColor() {
  const used = new Set(store.config.services.map(colorOf));
  return PALETTE.find((c) => !used.has(c)) ?? PALETTE[0];
}

function renderList() {
  const cfg = store.config;
  lastListKey = JSON.stringify([cfg.services, draft?.key, confirmId]);
  const badge = badges(cfg.services);
  const rows = cfg.services.map((s) => (draft?.key === s.id ? editor() : row(s, badge.get(s.id))));
  if (draft?.key === "new") rows.push(editor());
  if (!rows.length) rows.push(el("li", { class: "svc-empty" }, "還沒有服務,點「新增」加入。"));
  $("svcList").replaceChildren(...rows);
  $("addService").disabled = !!draft;

  if (draft?.fresh) {
    draft.fresh = false;
    const input = $("svcList").querySelector(".editor .input");
    input?.focus();
    input?.scrollIntoView({ block: "nearest" });
  }
}

function row(s, badge) {
  const confirming = confirmId === s.id;
  const sub = confirming
    ? "登入資料會保留,之後用同一個分身加回來即可"
    : `${shortUrl(s.url)} · ${s.profile}`;
  const actions = confirming
    ? [
        el("button", { class: "btn small", onclick: () => setConfirm(null) }, "取消"),
        el("button", { class: "btn small danger", onclick: () => remove(s.id) }, "刪除"),
      ]
    : [
        el("button", {
          class: "icon-btn sm" + (s.inSplit ? " on" : ""),
          title: s.inSplit ? "並排時顯示(點擊改為不顯示)" : "並排時不顯示(點擊改為顯示)",
          "aria-pressed": String(s.inSplit),
          html: icon("split"),
          onclick: () => toggleInSplit(s.id),
        }),
        el("button", { class: "icon-btn sm", title: "編輯", html: icon("edit"), onclick: () => setUi({ editing: s.id }) }),
        el("button", { class: "icon-btn sm", title: "刪除", html: icon("trash"), onclick: () => setConfirm(s.id) }),
      ];
  return el(
    "li",
    { class: "svc" + (confirming ? " confirming" : ""), style: `--c:${colorOf(s)}`, dataset: { id: s.id } },
    el("span", { class: "grip" + (draft ? " off" : ""), title: "拖曳調整順序", html: icon("grip") }),
    el("span", { class: "avatar" }, badge),
    el("div", { class: "svc-meta" }, el("div", { class: "svc-name" }, s.name), el("div", { class: "svc-sub" }, sub)),
    el("div", { class: "svc-acts" }, actions),
  );
}

function editor() {
  const d = draft;
  const input = (field, attrs) =>
    el("input", {
      class: "input",
      value: d[field],
      spellcheck: "false",
      oninput: (e) => (d[field] = e.target.value),
      ...attrs,
    });
  const field = (label, control, hint) =>
    el("label", { class: "field" }, el("span", {}, label), control, hint ? el("small", {}, hint) : null);
  const profiles = [...new Set(store.config.services.map((s) => s.profile))];

  const swatches = PALETTE.map((c) =>
    el("button", {
      class: "swatch",
      style: `--c:${c}`,
      title: c,
      "aria-pressed": String(c === d.color),
      onclick: (e) => {
        d.color = c;
        const card = e.currentTarget.closest(".editor");
        card.style.setProperty("--c", c);
        card.querySelectorAll(".swatch").forEach((b) => b.setAttribute("aria-pressed", String(b === e.currentTarget)));
      },
    }),
  );

  return el(
    "li",
    {
      class: "editor",
      style: `--c:${d.color}`,
      onkeydown: (e) => {
        if (e.key === "Enter" && e.target.matches(".input")) save();
      },
    },
    el("div", { class: "editor-title" }, d.key === "new" ? "新增服務" : "編輯服務"),
    field("名稱", input("name", { placeholder: "例如 Claude", maxlength: 40 })),
    field("網址", input("url", { placeholder: "https://claude.ai/settings/usage" })),
    field(
      "分身(profile)",
      input("profile", { placeholder: "留空則依名稱產生", list: "profileOptions" }),
      "相同分身共用一份登入狀態;要同時登入多個帳號就用不同分身。",
    ),
    el("datalist", { id: "profileOptions" }, profiles.map((p) => el("option", { value: p }))),
    // 用 div 而非 label:label 會把點擊轉給第一個按鈕
    el("div", { class: "field" }, el("span", {}, "顏色"), el("div", { class: "swatches" }, swatches)),
    d.error ? el("p", { class: "error" }, d.error) : null,
    el(
      "div",
      { class: "editor-acts" },
      el("button", { class: "btn", onclick: () => setUi({ editing: null }) }, "取消"),
      el("button", { class: "btn primary", onclick: save }, d.key === "new" ? "新增" : "儲存"),
    ),
  );
}

function setConfirm(id) {
  confirmId = id;
  renderList();
}

function remove(id) {
  confirmId = null;
  commit((c) => {
    c.services = c.services.filter((s) => s.id !== id);
    delete c.splitWeights[id];
  });
}

function save() {
  const d = draft;
  const fail = (msg) => {
    d.error = msg;
    renderList();
  };
  const name = d.name.trim();
  let url = d.url.trim();
  if (!name) return fail("請輸入名稱");
  if (!url) return fail("請輸入網址");
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(url)) url = "https://" + url;
  let parsed;
  try {
    parsed = new URL(url);
  } catch {
    return fail("網址格式不正確");
  }
  if (!/^https?:$/.test(parsed.protocol)) return fail("只支援 http / https 網址");

  const id = d.key === "new" ? "s" + Date.now().toString(36) : d.key;
  // 名稱是中文等非英數字時 slug 會是空的,改用 id,避免不同服務意外共用同一個分身
  const profile = slugify(d.profile) || slugify(name) || id;
  commit((c) => {
    if (d.key === "new") {
      c.services.push({ id, name, url, profile, color: d.color, inSplit: true });
      if (!c.splitMode) c.activeId = id;
    } else {
      const s = c.services.find((x) => x.id === id);
      if (s) Object.assign(s, { name, url, profile, color: d.color });
    }
  });
  setUi({ editing: null });
}
