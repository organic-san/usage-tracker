//! 服務子 webview 的生命週期:建立、排版、隱藏、重新整理與回收。
//!
//! 執行緒規則(違反會互相卡死):
//! - 在非主執行緒建立 webview 或查詢視窗狀態時,會等主執行緒處理完才返回,
//!   所以持有 `rt` 鎖的期間絕不能做這兩件事;`rt` 只用來短暫讀寫資料。
//! - `ops` 讓 command 與排程執行緒的多步驟操作(同步設定、刷新、回收)依序進行。
//!   主執行緒(視窗事件、頁面載入回呼)從不取用它。

use crate::config::{self, AppConfig, ServiceCfg};
use crate::layout::{self, Geometry, Rect};
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::webview::{PageLoadEvent, WebviewBuilder};
use tauri::window::Color;
use tauri::{AppHandle, LogicalPosition, LogicalSize, Manager, Url, WebviewUrl, Window};

// 偽裝成一般桌面 Chrome,避免 Google 因偵測到嵌入式 webview 而封鎖 OAuth 登入
const DESKTOP_UA: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) \
AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36";

/// 官方頁面是大型 SPA,在同一個 renderer 程序裡反覆 reload,記憶體不會完整還給系統
/// (2026-10 實測:常駐約 6 天後單一 renderer 程序達 5.5 GB)。
/// 存活超過這個時間的 webview 會整個換掉,讓舊程序結束、記憶體真正釋放:
/// 畫面上的在下一次刷新時於背景重建;不在畫面上的直接關閉,切換過去時再重新建立。
const MAX_AGE: Duration = Duration::from_secs(30 * 60);

/// 自動刷新暫停或關閉時,畫面上的頁面不會經過刷新流程,改在存活這麼久之後強制重建
const MAX_AGE_IDLE: Duration = Duration::from_secs(2 * 60 * 60);

/// 回收時最多等新頁面載入這麼久;逾時(例如斷線、載入事件沒觸發)就直接換上
const SWAP_TIMEOUT: Duration = Duration::from_secs(45);

const OFFSCREEN_X: f64 = -100000.0;

/// 頁面畫出來之前的底色,需與 styles.css 的 --bg 一致,避免閃白
const BG: Color = Color(16, 19, 24, 255);

struct View {
    label: String,
    url: String,
    profile: String,
    born: Instant,
    /// 上一次載入完整頁面的時間,用來判斷切換回來時內容是否過期
    loaded: Instant,
    /// 回收中:新的 webview 已在背景建立,等頁面載入完成後才換上 (label, 建立時間)
    pending: Option<(String, Instant)>,
}

impl View {
    fn new(label: String, svc: &ServiceCfg) -> Self {
        let now = Instant::now();
        Self {
            label,
            url: svc.url.clone(),
            profile: svc.profile.clone(),
            born: now,
            loaded: now,
            pending: None,
        }
    }

    fn into_labels(self) -> impl Iterator<Item = String> {
        std::iter::once(self.label).chain(self.pending.map(|(label, _)| label))
    }
}

/// 舞台上應該顯示的服務(依序)與各自的比例權重
#[derive(Default)]
struct Plan {
    ids: Vec<String>,
    weights: Vec<f64>,
    vertical: bool,
}

#[derive(Default)]
struct Runtime {
    /// service id → 目前的 webview
    views: HashMap<String, View>,
    plan: Plan,
    /// 前端回報舞台位置之前不擺放任何 webview,避免蓋住介面
    geometry: Option<Geometry>,
    /// 設定頁開啟中:官方頁面全部藏起來
    stage_hidden: bool,
    minimized: bool,
    seq: u64,
}

impl Runtime {
    /// 實際出現在畫面上的服務
    fn on_screen(&self) -> &[String] {
        if self.geometry.is_some() && !self.stage_hidden && !self.minimized {
            &self.plan.ids
        } else {
            &[]
        }
    }
}

#[derive(Default)]
pub struct Shell {
    rt: Mutex<Runtime>,
    ops: Mutex<()>,
}

impl Shell {
    fn rt(&self) -> MutexGuard<'_, Runtime> {
        self.rt.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 取得多步驟操作的執行權,見模組說明
    pub fn ops(&self) -> MutexGuard<'_, ()> {
        self.ops.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn set_geometry(&self, geometry: Geometry) {
        self.rt().geometry = Some(geometry);
    }

    pub fn set_stage_hidden(&self, hidden: bool) {
        self.rt().stage_hidden = hidden;
    }

    pub fn is_minimized(&self) -> bool {
        self.rt().minimized
    }

    /// 拖曳分隔線時暫時套用的權重(不存檔),需與畫面上的服務一一對應
    pub fn preview_weights(&self, weights: Vec<f64>) -> bool {
        let mut rt = self.rt();
        let ok = weights.len() == rt.plan.ids.len()
            && weights.iter().all(|w| w.is_finite() && *w > 0.0);
        if ok {
            rt.plan.weights = weights;
        }
        ok
    }
}

fn shell(app: &AppHandle) -> &Shell {
    app.state::<Shell>().inner()
}

fn window_size(win: &Window) -> Option<(f64, f64)> {
    let size = win.inner_size().ok()?;
    let logical = size.to_logical::<f64>(win.scale_factor().unwrap_or(1.0));
    Some((logical.width, logical.height))
}

fn plan_of(cfg: &AppConfig) -> Plan {
    let ids: Vec<String> = if cfg.split_mode {
        let chosen: Vec<&ServiceCfg> = cfg.services.iter().filter(|s| s.in_split).collect();
        let list = if chosen.is_empty() {
            cfg.services.iter().collect()
        } else {
            chosen
        };
        list.into_iter().map(|s| s.id.clone()).collect()
    } else {
        cfg.active_id.iter().cloned().collect()
    };
    let weights = ids
        .iter()
        .map(|id| cfg.split_weights.get(id).copied().unwrap_or(1.0))
        .collect();
    Plan {
        ids,
        weights,
        vertical: cfg.split_dir == "v",
    }
}

/// 每個 webview(含回收中的新頁面)該在的位置;None 代表藏起來
fn placements(rt: &Runtime, (w, h): (f64, f64)) -> Vec<(String, Option<Rect>)> {
    let on_screen = rt.on_screen();
    let rects = match rt.geometry {
        Some(g) if !on_screen.is_empty() => {
            layout::tiles(w, h, &g, &rt.plan.weights, rt.plan.vertical)
        }
        _ => Vec::new(),
    };
    let mut out = Vec::new();
    for (id, view) in &rt.views {
        let slot = on_screen
            .iter()
            .position(|x| x == id)
            .and_then(|i| rects.get(i).copied());
        out.push((view.label.clone(), slot));
        if let Some((label, _)) = &view.pending {
            out.push((label.clone(), None));
        }
    }
    out
}

fn apply(app: &AppHandle, list: Vec<(String, Option<Rect>)>) {
    for (label, rect) in list {
        let Some(wv) = app.get_webview(&label) else {
            continue;
        };
        match rect {
            Some(r) => {
                let _ = wv.set_bounds(tauri::Rect {
                    position: LogicalPosition::new(r.x, r.y).into(),
                    size: LogicalSize::new(r.w, r.h).into(),
                });
                let _ = wv.show();
            }
            None => {
                // 只移到畫面外時 WebView2 仍把頁面當成可見而全速執行;hide() 後才會被當成背景頁面節流
                let _ = wv.set_position(LogicalPosition::new(OFFSCREEN_X, 0.0));
                let _ = wv.hide();
            }
        }
    }
}

fn close(app: &AppHandle, labels: impl IntoIterator<Item = String>) {
    for label in labels {
        if let Some(wv) = app.get_webview(&label) {
            let _ = wv.close();
        }
    }
}

/// 依目前狀態重新擺放所有 webview。任何執行緒都可呼叫,但呼叫時不可持有 rt。
pub fn relayout(app: &AppHandle) {
    let Some(size) = app.get_window("main").as_ref().and_then(window_size) else {
        return;
    };
    let list = placements(&shell(app).rt(), size);
    apply(app, list);
}

/// 視窗尺寸改變(含最小化 / 還原)時由主執行緒呼叫。回傳是否剛從最小化還原。
pub fn on_resized(app: &AppHandle, win: &Window) -> bool {
    let minimized = win.is_minimized().unwrap_or(false);
    let Some(size) = window_size(win) else {
        return false;
    };
    let (list, was_minimized) = {
        let mut rt = shell(app).rt();
        let was = std::mem::replace(&mut rt.minimized, minimized);
        (placements(&rt, size), was)
    };
    if was_minimized == minimized {
        // 一般縮放只需要移動畫面上的 webview
        apply(app, list.into_iter().filter(|(_, r)| r.is_some()).collect());
    } else {
        // 最小化時全部藏起來交給瀏覽器節流,還原時再顯示
        apply(app, list);
    }
    was_minimized && !minimized
}

/// 以隔離的登入分身建立服務 webview,先藏在畫面外,由 relayout 決定擺放。
fn create(app: &AppHandle, svc: &ServiceCfg) -> Result<String, String> {
    let win = app.get_window("main").ok_or("main window not found")?;
    let url: Url = svc
        .url
        .parse()
        .map_err(|_| format!("網址無效:{}", svc.url))?;
    let label = {
        let mut rt = shell(app).rt();
        rt.seq += 1;
        // 每次建立都換新流水號,回收時新舊 webview 才能同時存在
        let id: String = svc
            .id
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect();
        format!("svc_{id}_{}", rt.seq)
    };
    let builder = WebviewBuilder::new(&label, WebviewUrl::External(url))
        .data_directory(config::profile_dir(app, &svc.profile))
        .user_agent(DESKTOP_UA)
        .background_color(BG)
        .on_page_load(|wv, payload| {
            if payload.event() == PageLoadEvent::Finished {
                promote(wv.app_handle(), wv.label());
            }
        });
    win.add_child(
        builder,
        LogicalPosition::new(OFFSCREEN_X, 0.0),
        LogicalSize::new(800.0, 600.0),
    )
    .map_err(|e| e.to_string())?;
    if let Some(wv) = app.get_webview(&label) {
        let _ = wv.hide();
    }
    Ok(label)
}

/// 回收中的新頁面載入完成(或逾時)後換上,再關閉舊的。不是回收中的頁面就什麼都不做。
fn promote(app: &AppHandle, label: &str) {
    let old = {
        let mut rt = shell(app).rt();
        let Some(view) = rt
            .views
            .values_mut()
            .find(|v| v.pending.as_ref().is_some_and(|(p, _)| p == label))
        else {
            return;
        };
        let (new, since) = view.pending.take().expect("matched above");
        view.born = since;
        view.loaded = since;
        std::mem::replace(&mut view.label, new)
    };
    // 新的建立得較晚,疊在舊的上面;先擺好新的再關舊的,畫面不會出現空白
    relayout(app);
    close(app, [old]);
}

/// 在背景建立新的 webview 取代舊的,載入完成後由 promote 換上。呼叫端須持有 ops。
fn recycle(app: &AppHandle, id: &str) -> Result<(), String> {
    let svc = {
        let rt = shell(app).rt();
        match rt.views.get(id) {
            Some(v) if v.pending.is_none() => ServiceCfg {
                id: id.to_string(),
                url: v.url.clone(),
                profile: v.profile.clone(),
                ..Default::default()
            },
            _ => return Ok(()),
        }
    };
    let label = create(app, &svc)?;
    let orphan = {
        let mut rt = shell(app).rt();
        match rt.views.get_mut(id) {
            Some(v) => {
                v.pending = Some((label, Instant::now()));
                None
            }
            None => Some(label),
        }
    };
    close(app, orphan);
    Ok(())
}

/// 讓 webview 符合設定:關閉已刪除或網址 / 分身改變的服務、建立畫面上需要但還沒有的、
/// 重新排版,最後補刷新「剛回到畫面上、內容已過期」的頁面。呼叫端須持有 ops。
pub fn sync(app: &AppHandle, cfg: &AppConfig) -> Result<(), String> {
    let stale: Vec<String> = {
        let mut rt = shell(app).rt();
        let gone: Vec<String> = rt
            .views
            .iter()
            .filter(|(id, v)| {
                !cfg.services
                    .iter()
                    .any(|s| &s.id == *id && s.url == v.url && s.profile == v.profile)
            })
            .map(|(id, _)| id.clone())
            .collect();
        gone.iter()
            .filter_map(|id| rt.views.remove(id))
            .flat_map(View::into_labels)
            .collect()
    };
    close(app, stale);

    // 只建立實際會出現在畫面上的;設定頁開啟或最小化期間等之後再建
    let missing: Vec<&ServiceCfg> = {
        let mut rt = shell(app).rt();
        rt.plan = plan_of(cfg);
        let on_screen = rt.on_screen();
        cfg.services
            .iter()
            .filter(|s| on_screen.contains(&s.id) && !rt.views.contains_key(&s.id))
            .collect()
    };
    let mut result = Ok(());
    for svc in missing {
        match create(app, svc) {
            Ok(label) => {
                shell(app).rt().views.insert(svc.id.clone(), View::new(label, svc));
            }
            Err(e) => {
                eprintln!("建立 {} 的頁面失敗:{e}", svc.name);
                result = Err(e);
            }
        }
    }
    relayout(app);

    if cfg.interval_sec > 0 && !cfg.paused {
        let interval = Duration::from_secs(cfg.interval_sec);
        let due: Vec<String> = {
            let rt = shell(app).rt();
            rt.on_screen()
                .iter()
                .filter(|id| rt.views.get(*id).is_some_and(|v| v.loaded.elapsed() >= interval))
                .cloned()
                .collect()
        };
        for id in due {
            refresh(app, &id);
        }
    }
    result
}

/// 重新整理單一服務:存活太久就在背景重建,否則一般 reload。呼叫端須持有 ops。
pub fn refresh(app: &AppHandle, id: &str) {
    let found = {
        let rt = shell(app).rt();
        rt.views
            .get(id)
            .map(|v| (v.label.clone(), v.born.elapsed(), v.pending.is_some()))
    };
    match found {
        // 尚未建立;或正在回收,新頁面載入完就是最新內容
        None | Some((_, _, true)) => {}
        Some((_, age, false)) if age >= MAX_AGE => {
            if let Err(e) = recycle(app, id) {
                eprintln!("回收 {id} 失敗:{e}");
            }
        }
        Some((label, _, false)) => {
            if let Some(wv) = app.get_webview(&label) {
                let _ = wv.reload();
            }
            if let Some(v) = shell(app).rt().views.get_mut(id) {
                v.loaded = Instant::now();
            }
        }
    }
}

/// 重新整理畫面上的所有服務。呼叫端須持有 ops。
pub fn refresh_on_screen(app: &AppHandle) {
    let ids = shell(app).rt().on_screen().to_vec();
    for id in ids {
        refresh(app, &id);
    }
}

/// 由排程執行緒定期呼叫的記憶體維護(見 MAX_AGE)。呼叫端須持有 ops。
/// `auto_refresh` 為 false(暫停或關閉)時,畫面上的頁面改用 MAX_AGE_IDLE 判斷是否重建。
pub fn maintain(app: &AppHandle, auto_refresh: bool) {
    let (overdue, discarded, idle) = {
        let mut rt = shell(app).rt();
        let on_screen = rt.on_screen().to_vec();
        let overdue: Vec<String> = rt
            .views
            .values()
            .filter_map(|v| v.pending.as_ref())
            .filter(|(_, since)| since.elapsed() >= SWAP_TIMEOUT)
            .map(|(label, _)| label.clone())
            .collect();
        let off: Vec<String> = rt
            .views
            .iter()
            .filter(|(id, v)| !on_screen.contains(id) && v.born.elapsed() >= MAX_AGE)
            .map(|(id, _)| id.clone())
            .collect();
        let discarded: Vec<String> = off
            .iter()
            .filter_map(|id| rt.views.remove(id))
            .flat_map(View::into_labels)
            .collect();
        let idle: Vec<String> = if auto_refresh {
            Vec::new()
        } else {
            on_screen
                .into_iter()
                .filter(|id| {
                    rt.views
                        .get(id)
                        .is_some_and(|v| v.pending.is_none() && v.born.elapsed() >= MAX_AGE_IDLE)
                })
                .collect()
        };
        (overdue, discarded, idle)
    };
    for label in overdue {
        promote(app, &label);
    }
    close(app, discarded);
    for id in idle {
        if let Err(e) = recycle(app, &id) {
            eprintln!("回收 {id} 失敗:{e}");
        }
    }
}
