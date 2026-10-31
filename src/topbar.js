import { getCurrentWindow } from "@tauri-apps/api/window";
import { api } from "./api.js";
import { store, subscribe } from "./store.js";
import { togglePaused, togglePin } from "./actions.js";
import { icon } from "./icons.js";
import { $, colorOf, findService, onScreenIds } from "./util.js";

// 上方標題列:目前檢視的名稱、刷新倒數與視窗控制。整條除了按鈕之外都能拖曳視窗。

const appWindow = getCurrentWindow();
let lastProgress = 0;

export function initTopbar() {
  const refreshBtn = $("refreshBtn");
  $("refreshIcon").innerHTML = icon("refresh");
  refreshBtn.onclick = () => {
    refreshBtn.classList.remove("spin");
    void refreshBtn.offsetWidth; // 重新觸發旋轉動畫
    refreshBtn.classList.add("spin");
    api.refresh().catch(console.error);
  };
  $("pauseBtn").onclick = togglePaused;
  $("pinBtn").innerHTML = icon("pin");
  $("pinBtn").onclick = togglePin;
  $("minBtn").innerHTML = icon("minimize");
  $("minBtn").onclick = () => appWindow.minimize();
  $("closeBtn").innerHTML = icon("close");
  $("closeBtn").onclick = () => appWindow.close();

  subscribe(render);
  setInterval(tick, 1000);
}

function render() {
  const cfg = store.config;
  const ids = onScreenIds(cfg);
  let title = "Usage Tracker";
  let color = null;
  if (store.ui.settingsOpen) {
    title = "設定";
  } else if (cfg.splitMode && ids.length > 1) {
    title = `並排檢視 · ${ids.length}`;
  } else if (ids.length) {
    const s = findService(cfg, ids[0]);
    title = s.name;
    color = colorOf(s);
  }
  $("nowTitle").textContent = title;
  const dot = $("nowDot");
  dot.hidden = !color;
  if (color) dot.style.setProperty("--c", color);

  const pause = $("pauseBtn");
  pause.innerHTML = icon(cfg.paused ? "play" : "pause");
  pause.title = cfg.paused ? "繼續自動刷新" : "暫停自動刷新";
  pause.classList.toggle("on", cfg.paused);

  const pin = $("pinBtn");
  pin.classList.toggle("on", cfg.alwaysOnTop);
  pin.title = cfg.alwaysOnTop ? "取消置頂" : "視窗置頂";

  tick();
}

function tick() {
  const { nextAt, intervalSec, paused } = store.schedule;
  let text = "手動";
  let progress = 0;
  if (paused) {
    text = "已暫停";
  } else if (intervalSec && nextAt) {
    const left = Math.max(0, nextAt - Date.now());
    text = formatSeconds(Math.ceil(left / 1000));
    progress = Math.min(1, Math.max(0, 1 - left / (intervalSec * 1000)));
  }
  $("countdown").textContent = text;
  $("refreshBtn").title = paused || !intervalSec ? "立即重新整理" : `${text} 後自動刷新,點擊立即重新整理`;

  const bar = $("progress");
  // 倒數重新起算時直接歸零,不要倒著播動畫
  bar.classList.toggle("instant", progress < lastProgress);
  bar.style.transform = `scaleX(${progress})`;
  lastProgress = progress;
}

function formatSeconds(s) {
  return s >= 60 ? `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}` : `${s}s`;
}
