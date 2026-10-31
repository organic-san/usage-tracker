import { api } from "./api.js";

/** 前端狀態。config 是 Rust 端設定的副本;ui 只存在前端;schedule 由 Rust 排程推送。 */
export const store = {
  config: { services: [], splitWeights: {} },
  ui: {
    settingsOpen: false,
    editing: null, // 編輯中的服務 id、"new" 或 null
  },
  schedule: { nextAt: null, intervalSec: 0, paused: false },
};

const listeners = new Set();

/** 訂閱狀態變化,kind 為 "config" | "ui" | "schedule" */
export function subscribe(fn) {
  listeners.add(fn);
}

export function notify(kind) {
  for (const fn of listeners) fn(kind);
}

let queue = Promise.resolve();
let latest = 0;

/** 依序執行會改動 Rust 端狀態的呼叫,避免先送的設定比後送的晚生效 */
export function enqueue(task) {
  queue = queue.then(task).catch((e) => console.error(e));
  return queue;
}

/**
 * 修改設定:先在本地套用並重繪,操作不必等 IPC;再送到 Rust 套用與存檔。
 * Rust 會正規化後回傳,以最後一次送出的結果為準。
 */
export function commit(mutate) {
  const next = structuredClone(store.config);
  mutate(next);
  store.config = next;
  notify("config");
  const seq = ++latest;
  return enqueue(async () => {
    const saved = await api.setConfig(next);
    if (seq === latest) {
      store.config = saved;
      notify("config");
    }
  });
}

export function setUi(patch) {
  Object.assign(store.ui, patch);
  notify("ui");
}
