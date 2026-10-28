use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::image::Image;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager, State, WindowEvent};
use tokio::sync::Mutex;

use crate::device::{Lamp, LampError, Status, PRESETS};
use crate::hotkeys::HotkeyBindings;
use crate::settings::Settings;

const ONLINE_POLL: Duration = Duration::from_millis(2500);
const OFFLINE_POLL: Duration = Duration::from_millis(4000);

const TRAY_OFFLINE: &[u8] = include_bytes!("../icons/tray_offline.png");
const TRAY_OFF: &[u8] = include_bytes!("../icons/tray_off.png");
const TRAY_ON: &[u8] = include_bytes!("../icons/tray_on.png");
const TRAY_OFFLINE_LIGHT: &[u8] = include_bytes!("../icons/tray_offline_light.png");
const TRAY_OFF_LIGHT: &[u8] = include_bytes!("../icons/tray_off_light.png");
const TRAY_ON_LIGHT: &[u8] = include_bytes!("../icons/tray_on_light.png");

pub type SharedState = Arc<Mutex<AppState>>;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub connection: &'static str,
    pub configured: bool,
    pub power: bool,
    pub brightness: u8,
    pub color_temp: u16,
    pub mode: u8,
    pub auto_fill: bool,
    pub ip: Option<String>,
    pub latency_ms: Option<u32>,
    pub error: Option<String>,
    pub device_name: String,
    pub model: String,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            connection: "connecting",
            configured: false,
            power: false,
            brightness: 58,
            color_temp: 4000,
            mode: 0,
            auto_fill: false,
            ip: None,
            latency_ms: None,
            error: None,
            device_name: "屏幕挂灯".into(),
            model: "yeelink.light.lamp43".into(),
        }
    }
}

pub struct AppState {
    lamp: Option<Lamp>,
    pub(crate) snapshot: Snapshot,
    settings: Settings,
}

impl AppState {
    fn new() -> Self {
        let settings = Settings::load();
        match Lamp::load() {
            Ok(lamp) => {
                let config = lamp.config();
                let snapshot = Snapshot {
                    configured: true,
                    device_name: config.name.clone(),
                    model: config.model.clone(),
                    ip: config.ip.map(|ip| ip.to_string()),
                    ..Snapshot::default()
                };
                Self {
                    lamp: Some(lamp),
                    snapshot,
                    settings,
                }
            }
            Err(err) => Self {
                lamp: None,
                snapshot: Snapshot {
                    connection: "offline",
                    error: Some(err.to_string()),
                    ..Snapshot::default()
                },
                settings,
            },
        }
    }

    fn apply_status(&mut self, status: Status, latency: u32, ip: Option<std::net::Ipv4Addr>) {
        self.snapshot.connection = "online";
        self.snapshot.power = status.power;
        self.snapshot.brightness = status.brightness;
        self.snapshot.color_temp = status.color_temp;
        self.snapshot.mode = status.mode;
        self.snapshot.auto_fill = status.auto_fill;
        self.snapshot.latency_ms = Some(latency);
        self.snapshot.error = None;
        if let Some(ip) = ip {
            self.snapshot.ip = Some(ip.to_string());
        }
    }

    fn apply_error(&mut self, err: &LampError) {
        self.snapshot.mark_offline(&err.to_string());
    }
}

impl Snapshot {
    fn mark_offline(&mut self, message: &str) {
        self.connection = "offline";
        self.latency_ms = None;
        self.error = Some(message.to_owned());
    }
}

struct TrayHandles {
    tray: TrayIcon,
    power: MenuItem<tauri::Wry>,
    autofill: CheckMenuItem<tauri::Wry>,
    icon_key: AtomicU8,
}

#[derive(Clone, Copy)]
pub(crate) enum Request {
    SetPower(bool),
    SetBrightness(u8),
    SetColorTemp(u16),
    SetMode(u8),
    SetAutoFill(bool),
    Reconnect,
}

pub fn run() {
    let state = AppState::new();
    let initial = state.snapshot.clone();
    let initial_settings = state.settings.clone();
    let show_on_launch = initial_settings.show_on_launch;

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, args, _cwd| {
            if args.iter().any(|arg| arg == "--autostart") {
                return;
            }
            if let Some(window) = app.get_webview_window("panel") {
                crate::window::show(&window);
            }
        }))
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .arg("--autostart")
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(crate::hotkeys::handler)
                .build(),
        )
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .targets([
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::LogDir {
                        file_name: Some("yeelight-lamp".into()),
                    }),
                    tauri_plugin_log::Target::new(tauri_plugin_log::TargetKind::Stdout),
                ])
                .build(),
        )
        .manage(Arc::new(Mutex::new(state)))
        .manage(HotkeyBindings::default())
        .invoke_handler(tauri::generate_handler![
            lamp_snapshot,
            refresh_lamp,
            set_power,
            set_brightness,
            set_color_temp,
            set_mode,
            set_auto_fill,
            reconnect_lamp,
            close_panel,
            hide_panel,
            get_settings,
            save_settings,
        ])
        .setup(move |app| {
            log::info!("Yeelight 屏幕挂灯 {} 启动", env!("CARGO_PKG_VERSION"));
            let window = app.get_webview_window("panel").expect("panel 窗口不存在");
            crate::window::prepare(&window)?;

            setup_tray(app.handle())?;
            update_tray(app.handle(), &initial);

            let launch_by_autostart = std::env::args().any(|arg| arg == "--autostart");
            let force_show =
                cfg!(debug_assertions) || std::env::var_os("YEELIGHT_LAMP_KEEP_OPEN").is_some();
            if force_show || !launch_by_autostart || show_on_launch {
                crate::window::show(&window);
            }

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(poll_loop(handle));

            // 热键注册会切到主线程执行，放到 setup 之外再触发
            let handle = app.handle().clone();
            let settings = initial_settings.clone();
            tauri::async_runtime::spawn(async move {
                crate::hotkeys::sync(&handle, &settings);
                if !cfg!(debug_assertions) {
                    if let Err(err) = sync_autostart(&handle, &settings) {
                        log::warn!("{err}");
                    }
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::Focused(false) => {
                if std::env::var_os("YEELIGHT_LAMP_KEEP_OPEN").is_none() {
                    if let Some(panel) = window.app_handle().get_webview_window("panel") {
                        crate::window::close(&panel);
                    }
                }
            }
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if let Some(panel) = window.app_handle().get_webview_window("panel") {
                    crate::window::close(&panel);
                }
            }
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(panel) = window.app_handle().get_webview_window("panel") {
                    let _ = crate::window::prepare(&panel);
                }
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("Tauri 启动失败");
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示面板", true, None::<&str>)?;
    let power = MenuItem::with_id(app, "power", "开灯", true, None::<&str>)?;
    let autofill = CheckMenuItem::with_id(app, "autofill", "自动补光", true, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(
        app,
        &[
            &show,
            &PredefinedMenuItem::separator(app)?,
            &power,
            &autofill,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;

    let initial_icon = if crate::theme::taskbar_is_light() {
        TRAY_OFFLINE_LIGHT
    } else {
        TRAY_OFFLINE
    };
    let tray = TrayIconBuilder::with_id("main")
        .icon(Image::from_bytes(initial_icon)?)
        .tooltip("Yeelight 屏幕挂灯")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| handle_menu(app, event.id().as_ref()))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                if let Some(window) = tray.app_handle().get_webview_window("panel") {
                    crate::window::toggle(&window);
                }
            }
        })
        .build(app)?;

    app.manage(TrayHandles {
        tray,
        power,
        autofill,
        icon_key: AtomicU8::new(u8::MAX),
    });
    Ok(())
}

fn handle_menu(app: &AppHandle, id: &str) {
    match id {
        "show" => {
            if let Some(window) = app.get_webview_window("panel") {
                crate::window::show(&window);
            }
        }
        "power" | "autofill" => {
            let app = app.clone();
            let kind = if id == "power" { "power" } else { "autofill" };
            tauri::async_runtime::spawn(async move {
                let state = app.state::<SharedState>().inner().clone();
                let request = {
                    let guard = state.lock().await;
                    if kind == "power" {
                        Request::SetPower(!guard.snapshot.power)
                    } else {
                        Request::SetAutoFill(!guard.snapshot.auto_fill)
                    }
                };
                let _ = apply_request(&app, &state, request).await;
            });
        }
        "quit" => app.exit(0),
        _ => {}
    }
}

async fn poll_loop(app: AppHandle) {
    loop {
        let state = app.state::<SharedState>().inner().clone();
        let online = poll_once(&app, &state).await;
        tokio::time::sleep(if online { ONLINE_POLL } else { OFFLINE_POLL }).await;
    }
}

async fn poll_once(app: &AppHandle, state: &SharedState) -> bool {
    let mut guard = state.lock().await;

    if guard.lamp.is_none() {
        match Lamp::load() {
            Ok(lamp) => {
                let config = lamp.config();
                guard.snapshot.configured = true;
                guard.snapshot.device_name = config.name.clone();
                guard.snapshot.model = config.model.clone();
                guard.lamp = Some(lamp);
            }
            Err(err) => {
                guard.snapshot.connection = "offline";
                guard.snapshot.error = Some(err.to_string());
                let snapshot = guard.snapshot.clone();
                drop(guard);
                publish(app, &snapshot);
                return false;
            }
        }
    }

    let Some(lamp) = guard.lamp.as_mut() else {
        return false;
    };
    let outcome = lamp.status_with_latency().await;
    let ip = lamp.ip();

    match outcome {
        Ok((status, latency)) => {
            guard.apply_status(status, latency, ip);
            let snapshot = guard.snapshot.clone();
            drop(guard);
            publish(app, &snapshot);
            true
        }
        Err(err) => {
            guard.apply_error(&err);
            let snapshot = guard.snapshot.clone();
            drop(guard);
            publish(app, &snapshot);
            false
        }
    }
}

pub(crate) async fn apply_request(
    app: &AppHandle,
    state: &SharedState,
    request: Request,
) -> Result<Snapshot, String> {
    let mut guard = state.lock().await;
    let AppState { lamp, snapshot, .. } = &mut *guard;
    let Some(lamp) = lamp.as_mut() else {
        return Err("尚未配置设备，请先运行 tools/py/get_token.py".into());
    };

    if snapshot.connection != "online" {
        snapshot.connection = "connecting";
        snapshot.error = None;
        let _ = app.emit("lamp-state", &*snapshot);
    }

    let started = Instant::now();

    if let Request::Reconnect = request {
        lamp.invalidate();
        match lamp.status_with_latency().await {
            Ok((status, latency)) => {
                guard_status(snapshot, status, latency);
                snapshot.ip = lamp.ip().map(|ip| ip.to_string());
            }
            Err(err) => snapshot.mark_offline(&err.to_string()),
        }
        let clone = snapshot.clone();
        drop(guard);
        publish(app, &clone);
        return Ok(clone);
    }

    let wake = !snapshot.power
        && matches!(
            request,
            Request::SetBrightness(_) | Request::SetColorTemp(_) | Request::SetMode(_)
        );
    let outcome = if wake {
        // 固件会丢弃关灯状态下的亮度、色温写入，先开灯再写
        match lamp.set_power(true).await {
            Ok(()) => run_request(lamp, request).await,
            Err(err) => Err(err),
        }
    } else {
        run_request(lamp, request).await
    };

    // 补光开启后由灯自己决定亮度和色温，写入完成后再读一次真实状态
    let adapted = matches!(request, Request::SetAutoFill(true)) && outcome.is_ok();
    let refreshed = if adapted {
        lamp.status().await.ok()
    } else {
        None
    };

    let latency = started.elapsed().as_millis() as u32;

    match outcome {
        Ok(()) => {
            snapshot.connection = "online";
            snapshot.latency_ms = Some(latency);
            snapshot.error = None;
            snapshot.ip = lamp.ip().map(|ip| ip.to_string());
            if wake {
                snapshot.power = true;
            }
            if let Some(status) = refreshed {
                snapshot.brightness = status.brightness;
                snapshot.color_temp = status.color_temp;
            }
            match request {
                Request::SetPower(on) => snapshot.power = on,
                Request::SetBrightness(value) => {
                    snapshot.brightness = value.clamp(1, 100);
                    snapshot.mode = 0;
                    snapshot.auto_fill = false;
                }
                Request::SetColorTemp(value) => {
                    snapshot.color_temp = value.clamp(2700, 6500);
                    snapshot.mode = 0;
                    snapshot.auto_fill = false;
                }
                Request::SetMode(mode) => {
                    snapshot.mode = mode.min(4);
                    snapshot.auto_fill = false;
                    if let Some(preset) = PRESETS.iter().find(|preset| preset.mode == mode) {
                        snapshot.brightness = preset.brightness;
                        snapshot.color_temp = preset.color_temp;
                    }
                }
                Request::SetAutoFill(on) => snapshot.auto_fill = on,
                Request::Reconnect => {}
            }
        }
        Err(err) => snapshot.mark_offline(&err.to_string()),
    }

    let clone = snapshot.clone();
    drop(guard);
    publish(app, &clone);
    Ok(clone)
}

async fn run_request(lamp: &mut Lamp, request: Request) -> Result<(), LampError> {
    match request {
        Request::SetPower(on) => lamp.set_power(on).await,
        Request::SetBrightness(value) => lamp.set_brightness(value).await,
        Request::SetColorTemp(value) => lamp.set_color_temp(value).await,
        Request::SetMode(mode) => lamp.set_mode(mode).await,
        Request::SetAutoFill(on) => lamp.set_auto_fill(on).await,
        Request::Reconnect => Ok(()),
    }
}

fn guard_status(snapshot: &mut Snapshot, status: Status, latency: u32) {
    snapshot.connection = "online";
    snapshot.power = status.power;
    snapshot.brightness = status.brightness;
    snapshot.color_temp = status.color_temp;
    snapshot.mode = status.mode;
    snapshot.auto_fill = status.auto_fill;
    snapshot.latency_ms = Some(latency);
    snapshot.error = None;
}

fn publish(app: &AppHandle, snapshot: &Snapshot) {
    let _ = app.emit("lamp-state", snapshot);
    update_tray(app, snapshot);
}

fn update_tray(app: &AppHandle, snapshot: &Snapshot) {
    let Some(handles) = app.try_state::<TrayHandles>() else {
        return;
    };
    let light = crate::theme::taskbar_is_light();
    let state: u8 = if snapshot.connection != "online" {
        0
    } else if snapshot.power {
        2
    } else {
        1
    };
    let key: u8 = state + if light { 3 } else { 0 };
    if handles.icon_key.swap(key, Ordering::Relaxed) != key {
        let icons = if light {
            [TRAY_OFFLINE_LIGHT, TRAY_OFF_LIGHT, TRAY_ON_LIGHT]
        } else {
            [TRAY_OFFLINE, TRAY_OFF, TRAY_ON]
        };
        if let Ok(image) = Image::from_bytes(icons[state as usize]) {
            let _ = handles.tray.set_icon(Some(image));
        }
    }

    let tooltip = match snapshot.connection {
        "online" if snapshot.power => {
            format!("Yeelight 屏幕挂灯 · 开灯 {}%", snapshot.brightness)
        }
        "online" => "Yeelight 屏幕挂灯 · 已关灯".to_owned(),
        "connecting" => "Yeelight 屏幕挂灯 · 正在连接".to_owned(),
        _ => "Yeelight 屏幕挂灯 · 设备离线".to_owned(),
    };
    let _ = handles.tray.set_tooltip(Some(&tooltip));
    let _ = handles
        .power
        .set_text(if snapshot.power { "关灯" } else { "开灯" });
    let _ = handles.autofill.set_checked(snapshot.auto_fill);
}

#[tauri::command]
async fn lamp_snapshot(state: State<'_, SharedState>) -> Result<Snapshot, String> {
    Ok(state.lock().await.snapshot.clone())
}

#[tauri::command]
async fn refresh_lamp(app: AppHandle, state: State<'_, SharedState>) -> Result<Snapshot, String> {
    let shared = state.inner().clone();
    poll_once(&app, &shared).await;
    let snapshot = shared.lock().await.snapshot.clone();
    Ok(snapshot)
}

#[tauri::command]
async fn set_power(
    app: AppHandle,
    state: State<'_, SharedState>,
    on: bool,
) -> Result<Snapshot, String> {
    apply_request(&app, state.inner(), Request::SetPower(on)).await
}

#[tauri::command]
async fn set_brightness(
    app: AppHandle,
    state: State<'_, SharedState>,
    value: u8,
) -> Result<Snapshot, String> {
    apply_request(&app, state.inner(), Request::SetBrightness(value)).await
}

#[tauri::command]
async fn set_color_temp(
    app: AppHandle,
    state: State<'_, SharedState>,
    value: u16,
) -> Result<Snapshot, String> {
    apply_request(&app, state.inner(), Request::SetColorTemp(value)).await
}

#[tauri::command]
async fn set_mode(
    app: AppHandle,
    state: State<'_, SharedState>,
    mode: u8,
) -> Result<Snapshot, String> {
    apply_request(&app, state.inner(), Request::SetMode(mode)).await
}

#[tauri::command]
async fn set_auto_fill(
    app: AppHandle,
    state: State<'_, SharedState>,
    on: bool,
) -> Result<Snapshot, String> {
    apply_request(&app, state.inner(), Request::SetAutoFill(on)).await
}

#[tauri::command]
async fn reconnect_lamp(app: AppHandle, state: State<'_, SharedState>) -> Result<Snapshot, String> {
    apply_request(&app, state.inner(), Request::Reconnect).await
}

#[tauri::command]
fn close_panel(app: AppHandle) {
    if let Some(window) = app.get_webview_window("panel") {
        crate::window::close(&window);
    }
}

#[tauri::command]
fn hide_panel(app: AppHandle) {
    if let Some(window) = app.get_webview_window("panel") {
        crate::window::finish_hide(&window);
    }
}

#[tauri::command]
async fn get_settings(state: State<'_, SharedState>) -> Result<Settings, String> {
    Ok(state.lock().await.settings.clone())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SaveOutcome {
    settings: Settings,
    hotkey_error: Option<String>,
}

#[tauri::command]
async fn save_settings(
    app: AppHandle,
    state: State<'_, SharedState>,
    settings: Settings,
) -> Result<SaveOutcome, String> {
    {
        let mut guard = state.lock().await;
        settings.save().map_err(|err| err.to_string())?;
        guard.settings = settings.clone();
    }

    let failures = crate::hotkeys::sync(&app, &settings);
    if let Err(err) = sync_autostart(&app, &settings) {
        log::warn!("{err}");
    }

    Ok(SaveOutcome {
        settings,
        hotkey_error: (!failures.is_empty()).then(|| failures.join("；")),
    })
}

fn sync_autostart(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    use tauri_plugin_autostart::ManagerExt;

    let manager = app.autolaunch();
    // 每次启动都重写一次，exe 换位置后开机项才能跟着更新
    let result = if settings.autostart {
        manager.enable()
    } else if manager.is_enabled().unwrap_or(false) {
        manager.disable()
    } else {
        return Ok(());
    };
    result.map_err(|err| format!("同步开机自启失败：{err}"))
}
