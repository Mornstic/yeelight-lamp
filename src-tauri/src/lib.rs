pub mod app;
pub mod config;
pub mod device;
pub mod hotkeys;
pub mod miio;
pub mod settings;
pub mod theme;
pub mod window;

pub fn run() {
    app::run();
}
