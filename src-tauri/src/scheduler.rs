//! 自動刷新與記憶體維護的排程執行緒。前端只負責顯示倒數(收 "schedule" 事件)。
//! 排程放在 Rust 端,視窗最小化或前端被節流時照樣準時,暫停刷新時也照樣做記憶體維護。

use crate::views::{self, Shell};
use crate::Settings;
use serde::Serialize;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager};

const MAINTAIN_EVERY: Duration = Duration::from_secs(60);

pub enum Msg {
    /// 間隔或暫停狀態改變,重新起算倒數
    Reschedule,
    /// 立即刷新(None = 畫面上全部),並重新起算倒數
    RefreshNow(Option<String>),
    /// 視窗從最小化還原:補建被釋放的頁面、補刷新過期的內容
    Restored,
}

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct Schedule {
    /// 下一次自動刷新的時間(Unix 毫秒);None = 暫停或關閉
    next_at: Option<u64>,
    interval_sec: u64,
    paused: bool,
}

pub struct Scheduler {
    tx: Mutex<Sender<Msg>>,
    current: Mutex<Schedule>,
}

impl Scheduler {
    pub fn send(&self, msg: Msg) {
        let _ = self.tx.lock().unwrap_or_else(|e| e.into_inner()).send(msg);
    }

    pub fn current(&self) -> Schedule {
        self.current.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// 註冊排程器並啟動執行緒。需在 Settings 與 Shell 都已 manage 之後呼叫。
pub fn start(app: &AppHandle) {
    let (tx, rx) = mpsc::channel();
    app.manage(Scheduler {
        tx: Mutex::new(tx),
        current: Mutex::new(Schedule::default()),
    });
    let app = app.clone();
    std::thread::spawn(move || run(app, rx));
}

fn run(app: AppHandle, rx: Receiver<Msg>) {
    let shell = app.state::<Shell>();
    let mut next_refresh = deadline(&app);
    let mut next_maintain = Instant::now() + MAINTAIN_EVERY;
    publish(&app, next_refresh);

    loop {
        let wake = next_refresh.map_or(next_maintain, |t| t.min(next_maintain));
        let reset = match rx.recv_timeout(wake.saturating_duration_since(Instant::now())) {
            Ok(Msg::Reschedule) => true,
            Ok(Msg::RefreshNow(id)) => {
                let _ops = shell.ops();
                match id {
                    Some(id) => views::refresh(&app, &id),
                    None => views::refresh_on_screen(&app),
                }
                true
            }
            Ok(Msg::Restored) => {
                let cfg = app.state::<Settings>().get();
                let _ops = shell.ops();
                let _ = views::sync(&app, &cfg);
                false
            }
            Err(RecvTimeoutError::Timeout) => {
                let now = Instant::now();
                if now >= next_maintain {
                    let _ops = shell.ops();
                    views::maintain(&app, next_refresh.is_some());
                    next_maintain = now + MAINTAIN_EVERY;
                }
                let due = next_refresh.is_some_and(|t| now >= t);
                // 最小化時沒有人在看,跳過這一輪;還原時 sync 會補刷新過期的頁面
                if due && !shell.is_minimized() {
                    let _ops = shell.ops();
                    views::refresh_on_screen(&app);
                }
                due
            }
            Err(RecvTimeoutError::Disconnected) => return,
        };
        if reset {
            next_refresh = deadline(&app);
            publish(&app, next_refresh);
        }
    }
}

fn deadline(app: &AppHandle) -> Option<Instant> {
    let cfg = app.state::<Settings>().get();
    (cfg.interval_sec > 0 && !cfg.paused)
        .then(|| Instant::now() + Duration::from_secs(cfg.interval_sec))
}

fn publish(app: &AppHandle, next: Option<Instant>) {
    let cfg = app.state::<Settings>().get();
    let next_at = next.map(|t| {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        (now + t.saturating_duration_since(Instant::now())).as_millis() as u64
    });
    let schedule = Schedule {
        next_at,
        interval_sec: cfg.interval_sec,
        paused: cfg.paused,
    };
    *app.state::<Scheduler>()
        .current
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = schedule.clone();
    let _ = app.emit("schedule", schedule);
}
