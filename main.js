import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { enable, isEnabled, disable } from "@tauri-apps/plugin-autostart";

const appWindow = getCurrentWindow();

// ---- 應用狀態 ----
let config = {
  services: [],          // { id, name, url, profile }
  activeId: null,
  intervalSec: 60,
  alwaysOnTop: false,
  splitMode: false,      // true = 並排顯示全部服務
  splitDir: "h",         // "h" = 左右、"v" = 上下
  paused: false,         // true = 暫停自動刷新
};
let refreshTimer = null;
let countdownTimer = null;
let secondsLeft = 0;
let editingId = null;     // 正在編輯的服務 id (null = 新增)

// ---- DOM ----
const $ = (id) => document.getElementById(id);
const tabbar = $("tabbar");
const stageEmpty = $("stageEmpty");
const refreshBar = $("refreshBar");
const countdownLabel = $("countdownLabel");
const serviceList = $("serviceList");

// ============ 啟動 ============
init();

async function init() {
  try {
    config = await invoke("load_config");
  } catch (e) {
    console.error("load_config failed", e);
  }
  // 自啟狀態以系統實際值為準
  try { $("autostart").checked = await isEnabled(); } catch {}

  bindUI();
  renderTabs();
  renderSettings();

  // 套用置頂
  await appWindow.setAlwaysOnTop(!!config.alwaysOnTop);
  $("pinBtn").classList.toggle("pinned", !!config.alwaysOnTop);

  if (config.splitDir !== "v") config.splitDir = "h";
  syncViewButtons();
  syncPauseButton();

  // 開啟初始畫面
  if (config.services.length) {
    if (
      !config.activeId ||
      !config.services.find((s) => s.id === config.activeId)
    ) {
      config.activeId = config.services[0].id;
    }
    await applyLayout();
  } else {
    stageEmpty.style.display = "flex";
    refreshBar.style.display = "none";
  }
}

// ============ UI 綁定 ============
function bindUI() {
  $("settingsBtn").onclick = openSettings;
  $("emptySettingsBtn").onclick = openSettings;
  $("settingsCloseBtn").onclick = closeSettings;
  $("reloadBtn").onclick = () => reloadActive(true);

  $("pauseBtn").onclick = () => {
    config.paused = !config.paused;
    syncPauseButton();
    startRefreshLoop();
    persist();
  };

  $("splitBtn").onclick = async () => {
    config.splitMode = !config.splitMode;
    syncViewButtons();
    await applyLayout();
    persist();
  };
  $("dirBtn").onclick = async () => {
    config.splitDir = config.splitDir === "v" ? "h" : "v";
    syncViewButtons();
    if (config.splitMode) await applyLayout();
    persist();
  };

  $("pinBtn").onclick = async () => {
    config.alwaysOnTop = !config.alwaysOnTop;
    await appWindow.setAlwaysOnTop(config.alwaysOnTop);
    $("pinBtn").classList.toggle("pinned", config.alwaysOnTop);
    $("alwaysOnTop").checked = config.alwaysOnTop;
    persist();
  };

  $("minBtn").onclick = () => appWindow.minimize();
  $("closeBtn").onclick = () => appWindow.close();

  $("addServiceBtn").onclick = () => openServiceModal(null);
  $("svcCancel").onclick = closeServiceModal;
  $("svcSave").onclick = saveServiceFromModal;

  $("saveBtn").onclick = saveSettings;

  $("alwaysOnTop").onchange = (e) => {
    config.alwaysOnTop = e.target.checked;
  };
  $("intervalSelect").onchange = (e) => {
    config.intervalSec = parseInt(e.target.value, 10);
  };
}

// ============ 分頁 ============
function renderTabs() {
  tabbar.innerHTML = "";
  for (const s of config.services) {
    const b = document.createElement("button");
    // 並排模式下全部視為作用中;單一模式下只標示 activeId
    const on = config.splitMode || s.id === config.activeId;
    b.className = "tab" + (on ? " active" : "");
    b.textContent = s.name;
    b.onclick = () => activateService(s.id);
    tabbar.appendChild(b);
  }
}

// 切到單一模式並聚焦某服務(並排模式下點分頁 = 放大檢視該服務)
async function activateService(id) {
  const svc = config.services.find((s) => s.id === id);
  if (!svc) return;
  config.activeId = id;
  config.splitMode = false;
  syncViewButtons();
  await applyLayout();
  persist();
}

// 目前應顯示的服務清單:並排=全部、單一=只有 activeId
function visibleServices() {
  if (config.splitMode) return config.services;
  const svc = config.services.find((s) => s.id === config.activeId);
  return svc ? [svc] : [];
}

// 把目前該顯示的服務交給 Rust 端排版(依 profile 隔離登入狀態)
async function applyLayout() {
  const list = visibleServices();
  if (!list.length) {
    stageEmpty.style.display = "flex";
    refreshBar.style.display = "none";
    return;
  }
  stageEmpty.style.display = "none";
  refreshBar.style.display = "block";
  renderTabs();
  try {
    await invoke("layout_services", {
      services: list.map((s) => ({ id: s.id, url: s.url, profile: s.profile })),
      dir: config.splitDir,
    });
  } catch (e) {
    console.error("layout_services failed", e);
  }
  startRefreshLoop();
}

// 同步暫停按鈕的外觀
function syncPauseButton() {
  const btn = $("pauseBtn");
  btn.textContent = config.paused ? "▶" : "⏸";
  btn.title = config.paused ? "繼續自動刷新" : "暫停自動刷新";
  btn.classList.toggle("pinned", config.paused);
}

// 同步兩個檢視按鈕的外觀
function syncViewButtons() {
  $("splitBtn").classList.toggle("pinned", config.splitMode);
  const dirBtn = $("dirBtn");
  dirBtn.textContent = config.splitDir === "v" ? "⬍" : "⬌";
  dirBtn.title = config.splitDir === "v" ? "目前:上下(點擊改左右)" : "目前:左右(點擊改上下)";
}

// ============ 自動刷新 ============
function startRefreshLoop() {
  clearInterval(refreshTimer);
  clearInterval(countdownTimer);
  if (config.paused) {
    countdownLabel.textContent = "自動刷新：已暫停";
    return;
  }
  if (!config.intervalSec || config.intervalSec <= 0) {
    countdownLabel.textContent = "自動刷新：關閉";
    return;
  }
  secondsLeft = config.intervalSec;
  updateCountdown();
  countdownTimer = setInterval(() => {
    secondsLeft -= 1;
    if (secondsLeft <= 0) secondsLeft = config.intervalSec;
    updateCountdown();
  }, 1000);
  refreshTimer = setInterval(() => reloadActive(false), config.intervalSec * 1000);
}

function updateCountdown() {
  countdownLabel.textContent = `${secondsLeft}s 後自動刷新`;
}

async function reloadActive(manual) {
  const list = visibleServices();
  for (const svc of list) {
    try {
      await invoke("reload_service_webview", { id: svc.id });
    } catch (e) {
      console.error("reload failed", e);
    }
  }
  if (manual && config.intervalSec > 0 && !config.paused) {
    secondsLeft = config.intervalSec;
    updateCountdown();
  }
}

// ============ 設定面板 ============
async function openSettings() {
  // 暫時隱藏子 webview,避免蓋住設定面板
  try { await invoke("hide_service_webview"); } catch {}
  renderSettings();
  $("settingsOverlay").classList.remove("hidden");
}

async function closeSettings() {
  $("settingsOverlay").classList.add("hidden");
  await applyLayout();
}

function renderSettings() {
  $("intervalSelect").value = String(config.intervalSec);
  $("alwaysOnTop").checked = !!config.alwaysOnTop;

  serviceList.innerHTML = "";
  if (!config.services.length) {
    const li = document.createElement("li");
    li.className = "hint";
    li.textContent = "尚無服務,點右上「+ 新增」加入 Claude 或 Gemini。";
    serviceList.appendChild(li);
  }
  for (const s of config.services) {
    const li = document.createElement("li");
    li.className = "service-item";
    li.innerHTML = `
      <span class="badge"></span>
      <div class="meta">
        <div class="nm"></div>
        <div class="url"></div>
      </div>
      <div class="acts">
        <button class="ghost small edit">編輯</button>
        <button class="ghost small del">刪除</button>
      </div>`;
    li.querySelector(".nm").textContent = s.name;
    li.querySelector(".url").textContent = s.url;
    li.querySelector(".edit").onclick = () => openServiceModal(s.id);
    li.querySelector(".del").onclick = () => deleteService(s.id);
    serviceList.appendChild(li);
  }
}

async function saveSettings() {
  config.intervalSec = parseInt($("intervalSelect").value, 10);
  config.alwaysOnTop = $("alwaysOnTop").checked;

  // 自啟
  try {
    const want = $("autostart").checked;
    const have = await isEnabled();
    if (want && !have) await enable();
    if (!want && have) await disable();
  } catch (e) {
    console.error("autostart toggle failed", e);
  }

  await appWindow.setAlwaysOnTop(config.alwaysOnTop);
  $("pinBtn").classList.toggle("pinned", config.alwaysOnTop);

  await persist();
  startRefreshLoop();
  closeSettings();
}

// ============ 服務編輯 modal ============
function openServiceModal(id) {
  editingId = id;
  const svc = id ? config.services.find((s) => s.id === id) : null;
  $("modalTitle").textContent = svc ? "編輯服務" : "新增服務";
  $("svcName").value = svc ? svc.name : "";
  $("svcUrl").value = svc ? svc.url : "";
  $("svcProfile").value = svc ? svc.profile : "";
  $("serviceModal").classList.remove("hidden");
}

function closeServiceModal() {
  $("serviceModal").classList.add("hidden");
  editingId = null;
}

function slugify(s) {
  return (s || "")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "") || "svc";
}

function saveServiceFromModal() {
  const name = $("svcName").value.trim();
  let url = $("svcUrl").value.trim();
  let profile = $("svcProfile").value.trim();

  if (!name || !url) { alert("請填寫名稱與網址"); return; }
  if (!/^https?:\/\//i.test(url)) url = "https://" + url;
  if (!profile) profile = slugify(name);
  profile = slugify(profile);

  if (editingId) {
    const svc = config.services.find((s) => s.id === editingId);
    const changed = svc.url !== url || svc.profile !== profile;
    svc.name = name; svc.url = url; svc.profile = profile;
    // 網址 / 分身改變 → 銷毀舊 webview,下次排版時以新設定重建
    if (changed) invoke("destroy_service_webview", { id: svc.id }).catch(() => {});
  } else {
    config.services.push({
      id: "s" + Date.now().toString(36),
      name, url, profile,
    });
  }
  closeServiceModal();
  renderTabs();
  renderSettings();
  persist();
}

function deleteService(id) {
  if (!confirm("確定刪除此服務?(登入分身資料不會被刪除)")) return;
  config.services = config.services.filter((s) => s.id !== id);
  if (config.activeId === id) config.activeId = config.services[0]?.id ?? null;
  invoke("destroy_service_webview", { id }).catch(() => {});
  renderTabs();
  renderSettings();
  persist();
}

// ============ 持久化 ============
async function persist() {
  try { await invoke("save_config", { config }); }
  catch (e) { console.error("save_config failed", e); }
}
