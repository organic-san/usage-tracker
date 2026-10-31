mod config;
mod layout;
mod scheduler;
mod views;

use config::AppConfig;
use layout::Geometry;
use scheduler::{Msg, Schedule, Scheduler};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State, WindowEvent};
use views::Shell;

/// 目前生效的設定,前後端都以這份為準
pub struct Settings(Mutex<AppConfig>);

impl Settings {
    pub fn get(&self) -> AppConfig {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn replace(&self, cfg: AppConfig) -> AppConfig {
        std::mem::replace(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()), cfg)
    }
}

// ---------- Commands ----------

#[tauri::command]
fn get_config(settings: State<'_, Settings>) -> AppConfig {
    settings.get()
}

/// 套用整份設定:調整置頂與排程、讓官方頁面符合新設定並存檔,回傳正規化後的結果。
#[tauri::command]
async fn set_config(app: AppHandle, config: AppConfig) -> Result<AppConfig, String> {
    let mut cfg = config;
    cfg.normalize();
    let shell = app.state::<Shell>();
    // 從替換設定到存檔都持有 ops,連續送來的設定才會依序生效
    let _ops = shell.ops();
    let prev = app.state::<Settings>().replace(cfg.clone());
    if prev.always_on_top != cfg.always_on_top {
        if let Some(win) = app.get_window("main") {
            let _ = win.set_always_on_top(cfg.always_on_top);
        }
    }
    if prev.interval_sec != cfg.interval_sec || prev.paused != cfg.paused {
        app.state::<Scheduler>().send(Msg::Reschedule);
    }
    let _ = views::sync(&app, &cfg);
    config::save(&app, &cfg)?;
    Ok(cfg)
}

/// 前端回報舞台位置(數值來自 CSS)。第一次回報之後才開始建立官方頁面。
#[tauri::command]
async fn set_geometry(app: AppHandle, geometry: Geometry) {
    let shell = app.state::<Shell>();
    let _ops = shell.ops();
    shell.set_geometry(geometry);
    let _ = views::sync(&app, &app.state::<Settings>().get());
}

/// 設定頁開啟時把官方頁面藏起來:子 webview 永遠蓋在 HTML 之上
#[tauri::command]
async fn set_stage_hidden(app: AppHandle, hidden: bool) {
    let shell = app.state::<Shell>();
    let _ops = shell.ops();
    shell.set_stage_hidden(hidden);
    let _ = views::sync(&app, &app.state::<Settings>().get());
}

/// 拖曳分隔線時的即時預覽,不存檔;放開後前端會以 set_config 存下最終比例
#[tauri::command]
async fn preview_weights(app: AppHandle, weights: Vec<f64>) {
    if app.state::<Shell>().preview_weights(weights) {
        views::relayout(&app);
    }
}

/// 立即刷新(id 為空 = 畫面上全部),並重新起算倒數
#[tauri::command]
fn refresh(scheduler: State<'_, Scheduler>, id: Option<String>) {
    scheduler.send(Msg::RefreshNow(id));
}

#[tauri::command]
fn get_schedule(scheduler: State<'_, Scheduler>) -> Schedule {
    scheduler.current()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    use tauri_plugin_window_state::StateFlags;

    tauri::Builder::default()
        // 記住主視窗的大小與位置,重開後自動還原
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(Shell::default())
        .setup(|app| {
            let cfg = config::load(app.handle());
            if let Some(win) = app.get_window("main") {
                let _ = win.set_always_on_top(cfg.always_on_top);
            }
            app.manage(Settings(Mutex::new(cfg)));
            scheduler::start(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_config,
            set_config,
            set_geometry,
            set_stage_hidden,
            preview_weights,
            refresh,
            get_schedule
        ])
        .on_window_event(|window, event| {
            if let WindowEvent::Resized(_) = event {
                if window.label() == "main" && views::on_resized(window.app_handle(), window) {
                    if let Some(scheduler) = window.try_state::<Scheduler>() {
                        scheduler.send(Msg::Restored);
                    }
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
