import { api } from "./api.js";
import { store, notify, enqueue, setUi } from "./store.js";
import { activate, closeSettings, openSettings } from "./actions.js";
import { initTopbar } from "./topbar.js";
import { initRail } from "./rail.js";
import { initStage } from "./stage.js";
import { initSettings } from "./settings.js";

init();

async function init() {
  try {
    store.config = await api.getConfig();
    store.schedule = await api.getSchedule();
  } catch (e) {
    console.error("init failed", e);
  }

  initTopbar();
  initRail();
  initStage();
  initSettings();
  bindKeys();
  api.onSchedule((schedule) => {
    store.schedule = schedule;
    notify("schedule");
  });
  notify("config");

  // 介面重新載入時(例如開發中),Rust 端可能還停在「設定頁開啟」的狀態
  enqueue(() => api.setStageHidden(false));
}

function bindKeys() {
  // 這些快捷鍵只在焦點位於本介面時有效;焦點在官方頁面裡時由該頁面自己處理
  document.addEventListener("keydown", (e) => {
    const key = e.key.toLowerCase();
    if (key === "escape") {
      if (store.ui.editing) setUi({ editing: null });
      else closeSettings();
    } else if (e.ctrlKey && /^[1-9]$/.test(key)) {
      const s = store.config.services[Number(key) - 1];
      if (s) activate(s.id);
      e.preventDefault();
    } else if (e.ctrlKey && key === ",") {
      openSettings();
      e.preventDefault();
    } else if (key === "f5" || (e.ctrlKey && key === "r")) {
      // 攔下來改成刷新官方頁面,否則會重新載入本介面
      api.refresh().catch(console.error);
      e.preventDefault();
    }
  });
  // 介面本身不需要瀏覽器預設的右鍵選單(輸入框除外)
  document.addEventListener("contextmenu", (e) => {
    if (!e.target.closest("input, textarea")) e.preventDefault();
  });
}
