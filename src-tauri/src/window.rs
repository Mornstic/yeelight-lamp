//! 面板窗口：右下角定位、按需创建、显示与释放。
//!
//! 面板不常驻：关闭时播放完退场动画就销毁窗口，WebView2 随窗口一起退出；
//! 下次打开再按 `tauri.conf.json` 里的 `panel` 配置重建，避免托盘常驻占几百 MB 内存。
//! 圆角完全由 CSS 绘制，窗口保持逐像素透明交给 DWM 合成，
//! 这样边缘才有抗锯齿；GDI region 裁切是 1-bit 的，高 DPI 下会呈锯齿。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

const EDGE_MARGIN: f64 = 16.0;
const HIDE_FALLBACK: Duration = Duration::from_millis(450);
const READY_FALLBACK: Duration = Duration::from_millis(1500);

static CLOSING: AtomicBool = AtomicBool::new(false);
static CREATING: AtomicBool = AtomicBool::new(false);
static AWAITING_READY: AtomicBool = AtomicBool::new(false);
static CLOSE_EPOCH: AtomicU64 = AtomicU64::new(0);
static SHOW_EPOCH: AtomicU64 = AtomicU64::new(0);
static CREATED_AT: Mutex<Option<Instant>> = Mutex::new(None);

/// 从任意线程请求显示面板：没有窗口就创建，创建后等首屏就绪再显示。
pub fn request_show(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move { show(&app) });
}

fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("panel") {
        if !AWAITING_READY.load(Ordering::SeqCst) {
            reveal(&window);
        }
        return;
    }
    if CREATING.swap(true, Ordering::SeqCst) {
        return;
    }
    let started = Instant::now();
    let created = create(app);
    CREATING.store(false, Ordering::SeqCst);
    let window = match created {
        Ok(window) => window,
        Err(err) => {
            log::warn!("创建面板窗口失败：{err}");
            return;
        }
    };
    log::info!("面板窗口创建 {} ms", started.elapsed().as_millis());
    *CREATED_AT.lock().unwrap() = Some(started);
    AWAITING_READY.store(true, Ordering::SeqCst);

    // 页面加载失败时兜底显示，避免热键/托盘点了没有任何反馈
    let epoch = SHOW_EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
    std::thread::spawn(move || {
        std::thread::sleep(READY_FALLBACK);
        if SHOW_EPOCH.load(Ordering::SeqCst) == epoch && AWAITING_READY.load(Ordering::SeqCst) {
            log::warn!(
                "面板未在 {} ms 内就绪，先行显示",
                READY_FALLBACK.as_millis()
            );
            reveal(&window);
        }
    });
}

fn create(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|config| config.label == "panel")
        .cloned()
        .expect("panel 窗口配置不存在");
    let window = tauri::WebviewWindowBuilder::from_config(app, &config)?.build()?;
    let _ = place_bottom_right(&window);
    Ok(window)
}

/// 前端把快照、设置、主题渲染完后调用，这时才把窗口显示出来。
pub fn ready(app: &AppHandle) {
    if !AWAITING_READY.swap(false, Ordering::SeqCst) {
        return;
    }
    SHOW_EPOCH.fetch_add(1, Ordering::SeqCst);
    if let Some(window) = app.get_webview_window("panel") {
        if !window.is_visible().unwrap_or(false) {
            if let Some(started) = *CREATED_AT.lock().unwrap() {
                log::info!("面板首屏就绪 {} ms", started.elapsed().as_millis());
            }
            reveal(&window);
        }
    }
}

pub fn toggle(app: &AppHandle) {
    let Some(window) = app.get_webview_window("panel") else {
        request_show(app);
        return;
    };
    if AWAITING_READY.load(Ordering::SeqCst) && !window.is_visible().unwrap_or(false) {
        // 还没显示就又被点了一次：放弃这次打开，直接释放
        finish_hide(app);
        return;
    }
    if CLOSING.load(Ordering::SeqCst) || window.is_visible().unwrap_or(false) {
        close(&window);
    } else {
        reveal(&window);
    }
}

fn reveal(window: &WebviewWindow) {
    CLOSE_EPOCH.fetch_add(1, Ordering::SeqCst);
    CLOSING.store(false, Ordering::SeqCst);
    AWAITING_READY.store(false, Ordering::SeqCst);
    let _ = place_bottom_right(window);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("panel-show", ());
}

pub fn close(window: &WebviewWindow) {
    if !window.is_visible().unwrap_or(false) {
        CLOSING.store(false, Ordering::SeqCst);
        return;
    }
    if CLOSING.swap(true, Ordering::SeqCst) {
        return;
    }

    let epoch = CLOSE_EPOCH.fetch_add(1, Ordering::SeqCst) + 1;
    let _ = window.emit("panel-hide", ());

    // 前端播放完退场动画会调用 hide_panel 销毁窗口；这里兜底，避免 WebView 忙碌时残留
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(HIDE_FALLBACK);
        if CLOSE_EPOCH.load(Ordering::SeqCst) == epoch {
            let _ = window.destroy();
            CLOSING.store(false, Ordering::SeqCst);
        }
    });
}

/// 销毁面板窗口；WebView2 的进程会随最后一个窗口退出。
pub fn finish_hide(app: &AppHandle) {
    CLOSE_EPOCH.fetch_add(1, Ordering::SeqCst);
    CLOSING.store(false, Ordering::SeqCst);
    AWAITING_READY.store(false, Ordering::SeqCst);
    *CREATED_AT.lock().unwrap() = None;
    if let Some(window) = app.get_webview_window("panel") {
        if let Err(err) = window.destroy() {
            log::warn!("销毁面板窗口失败：{err}");
            let _ = window.hide();
        } else {
            log::info!("面板窗口已销毁，WebView2 退出");
        }
    }
}

pub fn prepare(window: &WebviewWindow) {
    let _ = place_bottom_right(window);
}

fn place_bottom_right(window: &WebviewWindow) -> tauri::Result<()> {
    let cursor = window.cursor_position()?;
    let monitor = window
        .monitor_from_point(cursor.x, cursor.y)?
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else {
        return Ok(());
    };

    let area = monitor.work_area();
    let size = window.outer_size()?;
    let margin = (EDGE_MARGIN * monitor.scale_factor()).round() as i32;
    let x = area.position.x + area.size.width as i32 - size.width as i32 - margin;
    let y = area.position.y + area.size.height as i32 - size.height as i32 - margin;

    window.set_position(PhysicalPosition::new(x, y))
}
