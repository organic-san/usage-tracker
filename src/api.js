import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

// Rust command 的薄包裝,參數名稱需與 lib.rs 的 command 一致
export const api = {
  getConfig: () => invoke("get_config"),
  setConfig: (config) => invoke("set_config", { config }),
  setGeometry: (geometry) => invoke("set_geometry", { geometry }),
  setStageHidden: (hidden) => invoke("set_stage_hidden", { hidden }),
  previewWeights: (weights) => invoke("preview_weights", { weights }),
  refresh: (id = null) => invoke("refresh", { id }),
  getSchedule: () => invoke("get_schedule"),
  onSchedule: (fn) => listen("schedule", (e) => fn(e.payload)),
};
