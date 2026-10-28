//! 面板窗口：右下角定位、显示与隐藏。
//!
//! 圆角完全由 CSS 绘制，窗口保持逐像素透明交给 DWM 合成，
//! 这样边缘才有抗锯齿；GDI region 裁切是 1-bit 的，高 DPI 下会呈锯齿。

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{Emitter, PhysicalPosition, WebviewWindow};

const EDGE_MARGIN: f64 = 16.0;
const HIDE_FALLBACK: Duration = Duration::from_millis(450);

static CLOSING: AtomicBool = AtomicBool::new(false);
static CLOSE_EPOCH: AtomicU64 = AtomicU64::new(0);

pub fn prepare(window: &WebviewWindow) -> tauri::Result<()> {
    place_bottom_right(window)
}

pub fn show(window: &WebviewWindow) {
    CLOSE_EPOCH.fetch_add(1, Ordering::SeqCst);
    CLOSING.store(false, Ordering::SeqCst);
    let _ = place_bottom_right(window);
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("panel-show", ());
}

pub fn toggle(window: &WebviewWindow) {
    if CLOSING.load(Ordering::SeqCst) || window.is_visible().unwrap_or(false) {
        close(window);
    } else {
        show(window);
    }
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

    // 前端播放完退场动画会调用 finish_hide；这里兜底，避免 WebView 忙碌时窗口残留
    let window = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(HIDE_FALLBACK);
        if CLOSE_EPOCH.load(Ordering::SeqCst) == epoch {
            let _ = window.hide();
            CLOSING.store(false, Ordering::SeqCst);
        }
    });
}

pub fn finish_hide(window: &WebviewWindow) {
    CLOSE_EPOCH.fetch_add(1, Ordering::SeqCst);
    CLOSING.store(false, Ordering::SeqCst);
    let _ = window.hide();
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
