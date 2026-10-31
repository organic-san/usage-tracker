import { api } from "./api.js";
import { store, commit, enqueue, setUi } from "./store.js";

// 多個介面元件共用的操作

/** 單獨檢視某個服務(並排中點選也會切回單一檢視) */
export function activate(id) {
  commit((c) => {
    c.activeId = id;
    c.splitMode = false;
  });
  closeSettings();
}

export function toggleSplit() {
  commit((c) => {
    c.splitMode = !c.splitMode;
  });
  closeSettings();
}

export function toggleDir() {
  commit((c) => {
    c.splitDir = c.splitDir === "v" ? "h" : "v";
  });
}

export function toggleInSplit(id) {
  commit((c) => {
    const s = c.services.find((x) => x.id === id);
    if (s) s.inSplit = !s.inSplit;
  });
}

export function togglePaused() {
  commit((c) => {
    c.paused = !c.paused;
  });
}

export function togglePin() {
  commit((c) => {
    c.alwaysOnTop = !c.alwaysOnTop;
  });
}

export function moveService(from, to) {
  commit((c) => {
    const [s] = c.services.splice(from, 1);
    c.services.splice(to, 0, s);
  });
}

/** 開啟設定頁;editing 可指定要直接編輯的服務 id 或 "new" */
export function openSettings(editing = null) {
  const wasOpen = store.ui.settingsOpen;
  setUi({ settingsOpen: true, editing });
  if (!wasOpen) enqueue(() => api.setStageHidden(true));
}

export function closeSettings() {
  if (!store.ui.settingsOpen) return;
  setUi({ settingsOpen: false, editing: null });
  // 排在設定變更之後:先套用新設定再顯示,才不會閃過舊的頁面
  enqueue(() => api.setStageHidden(false));
}
