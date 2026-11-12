//! 主题探测与解析。
//!
//! 托盘图标跟随 `SystemUsesLightTheme`（任务栏），面板主题跟随
//! `AppsUseLightTheme`（应用模式）——这是两项独立的系统设置。

use serde::Serialize;

use crate::settings::Appearance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    Light,
    Dark,
}

pub fn resolve(appearance: Appearance) -> Theme {
    resolve_with(appearance, app_is_light())
}

fn resolve_with(appearance: Appearance, system_is_light: bool) -> Theme {
    match appearance {
        Appearance::Light => Theme::Light,
        Appearance::Dark => Theme::Dark,
        Appearance::System if system_is_light => Theme::Light,
        Appearance::System => Theme::Dark,
    }
}

fn app_is_light() -> bool {
    personalize_is_light("AppsUseLightTheme")
}

/// 任务栏是否为浅色。读不到设置时按深色处理。
pub fn taskbar_is_light() -> bool {
    personalize_is_light("SystemUsesLightTheme")
}

#[cfg(windows)]
fn personalize_is_light(value_name: &str) -> bool {
    use std::ffi::c_void;

    #[link(name = "advapi32")]
    extern "system" {
        fn RegGetValueW(
            hkey: isize,
            subkey: *const u16,
            value: *const u16,
            flags: u32,
            kind: *mut u32,
            data: *mut c_void,
            size: *mut u32,
        ) -> i32;
    }

    const HKEY_CURRENT_USER: isize = 0x8000_0001u32 as i32 as isize;
    const RRF_RT_REG_DWORD: u32 = 0x0000_0010;

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let value = wide(value_name);
    let mut data: u32 = 0;
    let mut size = std::mem::size_of::<u32>() as u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            value.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut data as *mut u32 as *mut c_void,
            &mut size,
        )
    };
    status == 0 && data != 0
}

#[cfg(not(windows))]
fn personalize_is_light(_value_name: &str) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_overrides_system() {
        assert_eq!(resolve_with(Appearance::Light, false), Theme::Light);
        assert_eq!(resolve_with(Appearance::Dark, true), Theme::Dark);
    }

    #[test]
    fn system_follows_the_app_theme() {
        assert_eq!(resolve_with(Appearance::System, true), Theme::Light);
        assert_eq!(resolve_with(Appearance::System, false), Theme::Dark);
    }
}
