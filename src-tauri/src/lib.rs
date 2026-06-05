use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{
    webview::WebviewBuilder, LogicalPosition, LogicalSize, Manager, WebviewUrl,
};

// 標題列高度,需與 styles.css 的 --title-h 一致
const TITLE_H: f64 = 46.0;

// 偽裝成一般桌面 Chrome,避免 Google 因偵測到嵌入式 webview 而封鎖 OAuth 登入
const DESKTOP_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

// ---------- 設定資料結構 ----------
#[derive(Serialize, Deserialize, Clone, Default)]
struct ServiceCfg {
    id: String,
    name: String,
    url: String,
    profile: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct AppConfig {
    services: Vec<ServiceCfg>,
    #[serde(rename = "activeId")]
    active_id: Option<String>,
    #[serde(rename = "intervalSec")]
    interval_sec: u64,
    #[serde(rename = "alwaysOnTop")]
    always_on_top: bool,
    #[serde(rename = "splitMode", default)]
    split_mode: bool,
    #[serde(rename = "splitDir", default)]
    split_dir: String,
}

fn default_config() -> AppConfig {
    AppConfig {
        services: vec![
            ServiceCfg {
                id: "claude".into(),
                name: "Claude".into(),
                // ⚠ 預填值,實際網址請依官方頁面確認後在設定中修改
                url: "https://claude.ai/settings/usage".into(),
                profile: "claude".into(),
            },
            ServiceCfg {
                id: "gemini".into(),
                name: "Gemini".into(),
                url: "https://gemini.google.com/app".into(),
                profile: "gemini".into(),
            },
        ],
        active_id: Some("claude".into()),
        interval_sec: 60,
        always_on_top: false,
        split_mode: false,
        split_dir: "h".into(),
    }
}

// 並排分割時,各子 webview 之間的細縫(露出底色作為分隔線)
const GAP: f64 = 1.0;

// 追蹤目前已建立 / 可見的子 webview,用於分割佈局與縮放
#[derive(Default)]
struct State {
    created: Mutex<Vec<String>>,  // 已建立的 service id
    visible: Mutex<Vec<String>>,  // 目前並排顯示的 service id(依序)
    dir: Mutex<String>,           // 分割方向:"h"=左右、"v"=上下
}

// 前端傳入的服務參數
#[derive(Deserialize)]
struct ServiceArg {
    id: String,
    url: String,
    profile: String,
}

fn config_path(app: &tauri::AppHandle) -> std::path::PathBuf {
    let dir = app
        .path()
        .app_config_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let _ = std::fs::create_dir_all(&dir);
    dir.join("config.json")
}

fn profile_dir(app: &tauri::AppHandle, profile: &str) -> std::path::PathBuf {
    let base = app
        .path()
        .app_data_dir()
        .unwrap_or_else(|_| std::path::PathBuf::from("."));
    let dir = base.join("profiles").join(profile);
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn webview_label(id: &str) -> String {
    format!("svc_{id}")
}

// ---------- Commands ----------

#[tauri::command]
fn load_config(app: tauri::AppHandle) -> AppConfig {
    let p = config_path(&app);
    if let Ok(txt) = std::fs::read_to_string(&p) {
        if let Ok(cfg) = serde_json::from_str::<AppConfig>(&txt) {
            return cfg;
        }
    }
    default_config()
}

#[tauri::command]
fn save_config(app: tauri::AppHandle, config: AppConfig) -> Result<(), String> {
    let p = config_path(&app);
    let txt = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(p, txt).map_err(|e| e.to_string())
}

// 主視窗中可放置子 webview 的舞台尺寸(扣掉標題列)
fn stage_size(win: &tauri::Window) -> Result<(f64, f64), String> {
    let size = win.inner_size().map_err(|e| e.to_string())?;
    let scale = win.scale_factor().unwrap_or(1.0);
    let logical = size.to_logical::<f64>(scale);
    Ok((logical.width, (logical.height - TITLE_H).max(0.0)))
}

// 計算第 i 格(共 n 格)在舞台中的位置與大小。dir = "v" 上下、其餘左右。
fn tile_rect(i: usize, n: usize, w: f64, h: f64, dir: &str) -> (f64, f64, f64, f64) {
    let nf = n.max(1) as f64;
    let i = i as f64;
    if dir == "v" {
        let th = (h - GAP * (nf - 1.0)) / nf;
        (0.0, TITLE_H + i * (th + GAP), w, th)
    } else {
        let tw = (w - GAP * (nf - 1.0)) / nf;
        (i * (tw + GAP), TITLE_H, tw, h)
    }
}

// 確保某服務的子 webview 已建立(尚未建立則以隔離 profile 建立並先藏到畫面外)
fn ensure_webview(
    app: &tauri::AppHandle,
    win: &tauri::Window,
    svc: &ServiceArg,
    state: &tauri::State<'_, State>,
    w: f64,
    h: f64,
) -> Result<(), String> {
    if state.created.lock().unwrap().contains(&svc.id) {
        return Ok(());
    }
    let parsed = svc
        .url
        .parse()
        .map_err(|_| format!("invalid url: {}", svc.url))?;
    let builder = WebviewBuilder::new(&webview_label(&svc.id), WebviewUrl::External(parsed))
        .data_directory(profile_dir(app, &svc.profile))
        .user_agent(DESKTOP_UA);
    win.add_child(
        builder,
        LogicalPosition::new(-100000.0, 0.0),
        LogicalSize::new(w, h),
    )
    .map_err(|e| e.to_string())?;
    state.created.lock().unwrap().push(svc.id.clone());
    Ok(())
}

/// 依序並排顯示傳入的服務(必要時建立)。傳入單一服務即為「單一分頁」模式。
/// 不在清單中的已建立 webview 會被移到畫面外隱藏。
#[tauri::command]
async fn layout_services(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
    services: Vec<ServiceArg>,
    dir: String,
) -> Result<(), String> {
    let win = app
        .get_window("main")
        .ok_or_else(|| "main window not found".to_string())?;
    let (w, h) = stage_size(&win)?;
    let dir = if dir == "v" { "v" } else { "h" }.to_string();

    for svc in &services {
        ensure_webview(&app, &win, svc, &state, w, h)?;
    }

    let new_ids: Vec<String> = services.iter().map(|s| s.id.clone()).collect();

    // 不在新清單的已建立 webview → 移到畫面外
    let created = { state.created.lock().unwrap().clone() };
    for id in &created {
        if !new_ids.contains(id) {
            if let Some(wv) = app.get_webview(&webview_label(id)) {
                let _ = wv.set_position(LogicalPosition::new(-100000.0, 0.0));
            }
        }
    }

    // 排版可見者
    let n = new_ids.len();
    for (i, id) in new_ids.iter().enumerate() {
        let (x, y, tw, th) = tile_rect(i, n, w, h, &dir);
        if let Some(wv) = app.get_webview(&webview_label(id)) {
            let _ = wv.set_size(LogicalSize::new(tw, th));
            let _ = wv.set_position(LogicalPosition::new(x, y));
        }
    }

    *state.visible.lock().unwrap() = new_ids;
    *state.dir.lock().unwrap() = dir;
    Ok(())
}

/// 隱藏目前所有可見的子 webview(進入設定時呼叫)
#[tauri::command]
async fn hide_service_webview(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<(), String> {
    let visible = { state.visible.lock().unwrap().clone() };
    for id in visible {
        if let Some(wv) = app.get_webview(&webview_label(&id)) {
            let _ = wv.set_position(LogicalPosition::new(-100000.0, 0.0));
        }
    }
    Ok(())
}

#[tauri::command]
async fn reload_service_webview(
    app: tauri::AppHandle,
    id: String,
) -> Result<(), String> {
    if let Some(wv) = app.get_webview(&webview_label(&id)) {
        wv.eval("window.location.reload()").map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn destroy_service_webview(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
    id: String,
) -> Result<(), String> {
    if let Some(wv) = app.get_webview(&webview_label(&id)) {
        let _ = wv.close();
    }
    state.created.lock().unwrap().retain(|x| x != &id);
    state.visible.lock().unwrap().retain(|x| x != &id);
    Ok(())
}

// 視窗大小改變時,讓所有可見的子 webview 依目前分割方向重新排版
fn on_resize(app: &tauri::AppHandle) {
    let state = app.state::<State>();
    let visible = { state.visible.lock().unwrap().clone() };
    if visible.is_empty() {
        return;
    }
    let dir = { state.dir.lock().unwrap().clone() };
    if let Some(win) = app.get_window("main") {
        if let Ok((w, h)) = stage_size(&win) {
            let n = visible.len();
            for (i, id) in visible.iter().enumerate() {
                let (x, y, tw, th) = tile_rect(i, n, w, h, &dir);
                if let Some(wv) = app.get_webview(&webview_label(id)) {
                    let _ = wv.set_size(LogicalSize::new(tw, th));
                    let _ = wv.set_position(LogicalPosition::new(x, y));
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri_plugin_window_state::StateFlags;

    tauri::Builder::default()
        // 記住主視窗的大小與位置(左右寬度、垂直高度),重開後自動還原
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(State::default())
        .invoke_handler(tauri::generate_handler![
            load_config,
            save_config,
            layout_services,
            hide_service_webview,
            reload_service_webview,
            destroy_service_webview
        ])
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Resized(_) = event {
                on_resize(&window.app_handle());
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
