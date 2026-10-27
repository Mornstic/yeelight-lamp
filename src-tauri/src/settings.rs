//! 应用设置的读写（`%APPDATA%\yeelight-lamp\settings.json`）。
//!
//! 文件不存在或字段缺失时回落到默认值，损坏时不会阻塞启动。

use std::env;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkey {
    /// 单个快捷键的开关，总开关关闭时全部不生效。
    pub enabled: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: String,
}

impl Default for Hotkey {
    fn default() -> Self {
        Self::chord("L")
    }
}

impl Hotkey {
    fn chord(key: &str) -> Self {
        Self {
            enabled: true,
            ctrl: true,
            alt: true,
            shift: false,
            win: false,
            key: key.to_owned(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Hotkeys {
    pub toggle: Hotkey,
    pub brightness_up: Hotkey,
    pub brightness_down: Hotkey,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            toggle: Hotkey::chord("L"),
            brightness_up: Hotkey::chord("ArrowUp"),
            brightness_down: Hotkey::chord("ArrowDown"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// 全局快捷键总开关，默认关闭。
    pub hotkeys_enabled: bool,
    pub hotkeys: Hotkeys,
    pub autostart: bool,
    pub show_on_launch: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            hotkeys_enabled: false,
            hotkeys: Hotkeys::default(),
            autostart: true,
            show_on_launch: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum SettingsError {
    #[error("读写设置失败：{0}")]
    Io(#[from] std::io::Error),
    #[error("设置文件不是合法 JSON：{0}")]
    Json(#[from] serde_json::Error),
}

impl Settings {
    pub fn load() -> Self {
        match Self::load_from(default_path()) {
            Ok(settings) => settings,
            Err(err) => {
                log::warn!("读取设置失败，使用默认值：{err}");
                Settings::default()
            }
        }
    }

    pub fn load_from(path: impl AsRef<std::path::Path>) -> Result<Self, SettingsError> {
        let text = fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    pub fn save(&self) -> Result<(), SettingsError> {
        let path = default_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let temp = path.with_extension("json.tmp");
        fs::write(&temp, serde_json::to_string_pretty(self)?)?;
        fs::rename(&temp, &path)?;
        Ok(())
    }
}

pub fn default_path() -> PathBuf {
    if let Some(path) = env::var_os("YEELIGHT_LAMP_SETTINGS") {
        return PathBuf::from(path);
    }
    let base = env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("yeelight-lamp").join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_fields_fall_back_to_defaults() {
        let settings: Settings = serde_json::from_str(r#"{"autostart": false}"#).unwrap();
        assert!(!settings.hotkeys_enabled);
        assert!(!settings.autostart);
        assert_eq!(settings.hotkeys.toggle.key, "L");
        assert_eq!(settings.hotkeys.brightness_up.key, "ArrowUp");
    }

    #[test]
    fn hotkey_roundtrips_through_json() {
        let mut settings = Settings::default();
        settings.hotkeys_enabled = true;
        settings.hotkeys.toggle = Hotkey {
            enabled: false,
            ctrl: true,
            alt: false,
            shift: true,
            win: false,
            key: "K".into(),
        };
        let text = serde_json::to_string(&settings).unwrap();
        let restored: Settings = serde_json::from_str(&text).unwrap();
        assert_eq!(restored, settings);
    }
}
