//! 面板窗口：右下角定位、显示与隐藏。
//!
//! 圆角完全由 CSS 绘制，窗口保持逐像素透明交给 DWM 合成，
//! 这样边缘才有抗锯齿；GDI region 裁切是 1-bit 的，高 DPI 下会呈锯齿。

use tauri::{PhysicalPosition, WebviewWindow};

const EDGE_MARGIN: f64 = 16.0;

pub fn prepare(window: &WebviewWindow) -> tauri::Result<()> {
    place_bottom_right(window)
}

pub fn show(window: &WebviewWindow) {
    let _ = place_bottom_right(window);
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn toggle(window: &WebviewWindow) {
    if window.is_visible().unwrap_or(false) {
        let _ = window.hide();
    } else {
        show(window);
    }
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
