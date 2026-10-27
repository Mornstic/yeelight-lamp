//! 全局快捷键：把设置里的组合键注册成系统热键，并把按键事件转成挂灯指令。

use std::collections::HashMap;
use std::str::FromStr;
use std::sync::Mutex;

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{
    Code, GlobalShortcutExt, Modifiers, Shortcut, ShortcutEvent, ShortcutState,
};

use crate::app::{apply_request, Request, SharedState};
use crate::settings::{Hotkey, Settings};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    Toggle,
    BrightnessUp,
    BrightnessDown,
}

#[derive(Default)]
pub struct HotkeyBindings(Mutex<HashMap<u32, HotkeyAction>>);

pub fn handler(app: &AppHandle, shortcut: &Shortcut, event: ShortcutEvent) {
    if event.state != ShortcutState::Pressed {
        return;
    }
    let action = app
        .state::<HotkeyBindings>()
        .0
        .lock()
        .unwrap()
        .get(&shortcut.id())
        .copied();
    let Some(action) = action else {
        return;
    };

    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<SharedState>().inner().clone();
        let request = {
            let guard = state.lock().await;
            match action {
                HotkeyAction::Toggle => Request::SetPower(!guard.snapshot.power),
                HotkeyAction::BrightnessUp => Request::SetBrightness(
                    guard.snapshot.brightness.saturating_add(5).clamp(1, 100),
                ),
                HotkeyAction::BrightnessDown => Request::SetBrightness(
                    guard.snapshot.brightness.saturating_sub(5).clamp(1, 100),
                ),
            }
        };
        if let Err(err) = apply_request(&app, &state, request).await {
            log::warn!("快捷键指令未执行：{err}");
        }
    });
}

/// 按当前设置重新注册全部快捷键，返回给用户看的失败原因。
pub fn sync(app: &AppHandle, settings: &Settings) -> Vec<String> {
    app.state::<HotkeyBindings>().0.lock().unwrap().clear();
    if let Err(err) = app.global_shortcut().unregister_all() {
        log::warn!("清除旧快捷键失败：{err}");
    }
    if !settings.hotkeys_enabled {
        return Vec::new();
    }

    let rows = [
        (HotkeyAction::Toggle, &settings.hotkeys.toggle),
        (HotkeyAction::BrightnessUp, &settings.hotkeys.brightness_up),
        (
            HotkeyAction::BrightnessDown,
            &settings.hotkeys.brightness_down,
        ),
    ];
    let mut failures = Vec::new();
    for (action, hotkey) in rows {
        if !hotkey.enabled {
            continue;
        }
        let Some(shortcut) = shortcut_for(hotkey) else {
            failures.push(format!("{} 不是有效的按键组合", display_chord(hotkey)));
            continue;
        };
        let duplicate = app
            .state::<HotkeyBindings>()
            .0
            .lock()
            .unwrap()
            .get(&shortcut.id())
            .copied();
        if let Some(existing) = duplicate {
            failures.push(format!(
                "{} 与「{}」重复",
                display_chord(hotkey),
                action_label(existing)
            ));
            continue;
        }
        match app.global_shortcut().register(shortcut) {
            Ok(()) => {
                app.state::<HotkeyBindings>()
                    .0
                    .lock()
                    .unwrap()
                    .insert(shortcut.id(), action);
                log::info!(
                    "已注册快捷键 {} → {}",
                    display_chord(hotkey),
                    action_label(action)
                );
            }
            Err(_) => failures.push(format!("{} 可能已被其他程序占用", display_chord(hotkey))),
        }
    }
    for failure in &failures {
        log::warn!("快捷键注册失败：{failure}");
    }
    failures
}

fn action_label(action: HotkeyAction) -> &'static str {
    match action {
        HotkeyAction::Toggle => "开 / 关挂灯",
        HotkeyAction::BrightnessUp => "亮度增加",
        HotkeyAction::BrightnessDown => "亮度降低",
    }
}

fn shortcut_for(hotkey: &Hotkey) -> Option<Shortcut> {
    let mut modifiers = Modifiers::empty();
    if hotkey.ctrl {
        modifiers |= Modifiers::CONTROL;
    }
    if hotkey.alt {
        modifiers |= Modifiers::ALT;
    }
    if hotkey.shift {
        modifiers |= Modifiers::SHIFT;
    }
    if hotkey.win {
        modifiers |= Modifiers::SUPER;
    }
    if modifiers.is_empty() {
        return None;
    }
    Some(Shortcut::new(Some(modifiers), code_for(&hotkey.key)?))
}

/// 设置里存的是浏览器 `KeyboardEvent.key`，先映射到 `Code` 名称。
fn code_for(key: &str) -> Option<Code> {
    let name = match key {
        " " | "Space" => "Space".to_owned(),
        _ if key.chars().count() == 1 => {
            let ch = key.chars().next()?;
            match ch.to_ascii_uppercase() {
                letter @ 'A'..='Z' => format!("Key{letter}"),
                digit @ '0'..='9' => format!("Digit{digit}"),
                _ => punctuation(ch)?.to_owned(),
            }
        }
        _ => key.to_owned(),
    };
    Code::from_str(&name).ok()
}

fn punctuation(ch: char) -> Option<&'static str> {
    Some(match ch {
        '`' => "Backquote",
        '-' => "Minus",
        '=' => "Equal",
        '[' => "BracketLeft",
        ']' => "BracketRight",
        '\\' => "Backslash",
        ';' => "Semicolon",
        '\'' => "Quote",
        ',' => "Comma",
        '.' => "Period",
        '/' => "Slash",
        _ => return None,
    })
}

pub fn display_chord(hotkey: &Hotkey) -> String {
    let mut parts = Vec::new();
    if hotkey.ctrl {
        parts.push("Ctrl".to_owned());
    }
    if hotkey.alt {
        parts.push("Alt".to_owned());
    }
    if hotkey.shift {
        parts.push("Shift".to_owned());
    }
    if hotkey.win {
        parts.push("Win".to_owned());
    }
    parts.push(
        match hotkey.key.as_str() {
            "ArrowUp" => "↑",
            "ArrowDown" => "↓",
            "ArrowLeft" => "←",
            "ArrowRight" => "→",
            other => other,
        }
        .to_owned(),
    );
    parts.join(" + ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hotkey(key: &str) -> Hotkey {
        Hotkey {
            enabled: true,
            ctrl: true,
            alt: true,
            shift: false,
            win: false,
            key: key.into(),
        }
    }

    #[test]
    fn maps_browser_keys_to_codes() {
        assert_eq!(code_for("L"), Some(Code::KeyL));
        assert_eq!(code_for("ArrowUp"), Some(Code::ArrowUp));
        assert_eq!(code_for("Space"), Some(Code::Space));
        assert_eq!(code_for("7"), Some(Code::Digit7));
        assert_eq!(code_for("`"), Some(Code::Backquote));
        assert_eq!(code_for("F5"), Some(Code::F5));
        assert_eq!(code_for("Unicorn"), None);
    }

    #[test]
    fn requires_a_modifier() {
        let mut bare = hotkey("B");
        bare.ctrl = false;
        bare.alt = false;
        assert!(shortcut_for(&bare).is_none());
        assert!(shortcut_for(&hotkey("B")).is_some());
    }

    #[test]
    fn chord_is_human_readable() {
        assert_eq!(display_chord(&hotkey("ArrowUp")), "Ctrl + Alt + ↑");
    }
}
