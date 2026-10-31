//! 使用者設定:資料結構、讀寫與正規化。設定以 Rust 端這份為準,前端修改後整份送回來。

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct ServiceCfg {
    pub id: String,
    pub name: String,
    pub url: String,
    /// 登入分身;相同分身的服務共用同一份登入狀態
    pub profile: String,
    /// 側欄圖示顏色(#rrggbb);空字串代表由前端依 id 自動配色
    pub color: String,
    /// 並排模式時是否顯示
    pub in_split: bool,
}

impl Default for ServiceCfg {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            url: String::new(),
            profile: String::new(),
            color: String::new(),
            in_split: true,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct AppConfig {
    /// 陣列順序就是側欄與並排的順序
    pub services: Vec<ServiceCfg>,
    pub active_id: Option<String>,
    /// 自動刷新間隔(秒),0 = 關閉
    pub interval_sec: u64,
    pub paused: bool,
    pub always_on_top: bool,
    pub split_mode: bool,
    /// "h" = 左右、"v" = 上下
    pub split_dir: String,
    /// 並排時各服務所佔比例的權重(service id → 權重),缺少的視為 1
    pub split_weights: BTreeMap<String, f64>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let svc = |id: &str, name: &str, url: &str, color: &str| ServiceCfg {
            id: id.into(),
            name: name.into(),
            url: url.into(),
            profile: id.into(),
            color: color.into(),
            in_split: true,
        };
        Self {
            services: vec![
                // ⚠ 預填值,實際網址請依官方頁面確認後在設定中修改
                svc("claude", "Claude", "https://claude.ai/settings/usage", "#d97757"),
                svc("gemini", "Gemini", "https://gemini.google.com/app", "#5b8def"),
            ],
            active_id: Some("claude".into()),
            interval_sec: 60,
            paused: false,
            always_on_top: false,
            split_mode: false,
            split_dir: "h".into(),
            split_weights: BTreeMap::new(),
        }
    }
}

impl AppConfig {
    /// 修正前端送來或舊版設定檔中不合法的值,讓後續流程不必再逐一檢查。
    pub fn normalize(&mut self) {
        let mut seen = HashSet::new();
        self.services
            .retain(|s| !s.id.is_empty() && !s.url.is_empty() && seen.insert(s.id.clone()));
        for s in &mut self.services {
            s.name = s.name.trim().to_string();
            if s.name.is_empty() {
                s.name = s.id.clone();
            }
            s.profile = sanitize_profile(&s.profile, &s.name);
        }

        if self.split_dir != "v" {
            self.split_dir = "h".into();
        }
        let known = |id: &str| self.services.iter().any(|s| s.id == id);
        if !self.active_id.as_deref().is_some_and(known) {
            self.active_id = self.services.first().map(|s| s.id.clone());
        }
        let ids: HashSet<String> = self.services.iter().map(|s| s.id.clone()).collect();
        self.split_weights
            .retain(|id, w| ids.contains(id) && w.is_finite() && *w > 0.0);
        if self.interval_sec > 0 {
            self.interval_sec = self.interval_sec.clamp(15, 24 * 3600);
        }
    }
}

/// 分身名稱會直接成為資料夾名稱,只保留小寫英數與連字號,避免跳脫到其他路徑。
pub fn sanitize_profile(profile: &str, fallback: &str) -> String {
    let slug = |s: &str| {
        let mut out = String::new();
        for c in s.to_lowercase().chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c);
            } else if !out.ends_with('-') && !out.is_empty() {
                out.push('-');
            }
        }
        out.trim_end_matches('-').to_string()
    };
    [slug(profile), slug(fallback)]
        .into_iter()
        .find(|s| !s.is_empty())
        .unwrap_or_else(|| "svc".into())
}

fn config_path(app: &AppHandle) -> PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

pub fn profile_dir(app: &AppHandle, profile: &str) -> PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| PathBuf::from("."));
    let dir = base.join("profiles").join(profile);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn load(app: &AppHandle) -> AppConfig {
    let path = config_path(app);
    let mut cfg = match std::fs::read_to_string(&path) {
        Ok(txt) => serde_json::from_str::<AppConfig>(&txt).unwrap_or_else(|e| {
            // 讀不懂時先把原檔改名保留,否則下一次存檔會直接覆蓋掉使用者原本的設定
            eprintln!("config.json 解析失敗,已改名為 config.json.bak:{e}");
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
            AppConfig::default()
        }),
        Err(_) => AppConfig::default(),
    };
    cfg.normalize();
    cfg
}

pub fn save(app: &AppHandle, cfg: &AppConfig) -> Result<(), String> {
    let path = config_path(app);
    let txt = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    // 先寫暫存檔再改名,避免寫到一半當機留下壞掉的設定檔
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, txt).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_v1_config() {
        // 舊版設定檔沒有 color / inSplit / paused / splitWeights,splitDir 可能是空字串
        let txt = r#"{"services":[{"id":"claude","name":"Claude","url":"https://claude.ai","profile":"claude"}],
            "activeId":"gone","intervalSec":60,"alwaysOnTop":true,"splitMode":false,"splitDir":""}"#;
        let mut cfg: AppConfig = serde_json::from_str(txt).unwrap();
        cfg.normalize();
        assert_eq!(cfg.services.len(), 1);
        assert!(cfg.services[0].in_split);
        assert_eq!(cfg.split_dir, "h");
        assert_eq!(cfg.active_id.as_deref(), Some("claude"));
        assert!(cfg.always_on_top);
    }

    #[test]
    fn profile_cannot_escape() {
        assert_eq!(sanitize_profile("../../Windows", "x"), "windows");
        assert_eq!(sanitize_profile("", "My Claude"), "my-claude");
        assert_eq!(sanitize_profile("..", "..."), "svc");
    }

    #[test]
    fn drops_duplicates_and_stale_weights() {
        let mut cfg = AppConfig::default();
        cfg.services.push(cfg.services[0].clone());
        cfg.split_weights.insert("claude".into(), 2.0);
        cfg.split_weights.insert("gone".into(), 1.0);
        cfg.split_weights.insert("gemini".into(), f64::NAN);
        cfg.interval_sec = 3;
        cfg.normalize();
        assert_eq!(cfg.services.len(), 2);
        assert_eq!(cfg.split_weights.len(), 1);
        assert_eq!(cfg.interval_sec, 15);
    }
}
