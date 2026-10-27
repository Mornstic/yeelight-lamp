//! 任务栏主题探测。
//!
//! 浅色任务栏上浅色图标几乎看不见，所以托盘图标跟着 `SystemUsesLightTheme`
//! 在两套配色之间切换——注意它和窗口用的 `AppsUseLightTheme` 是两项独立设置。

/// 任务栏是否为浅色。读不到设置时按深色处理。
#[cfg(windows)]
pub fn taskbar_is_light() -> bool {
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
    let value = wide("SystemUsesLightTheme");
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
pub fn taskbar_is_light() -> bool {
    false
}
