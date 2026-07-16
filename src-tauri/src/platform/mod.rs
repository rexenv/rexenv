//! Platform selection. `core/` calls `platform::current()` to get a
//! `&dyn Platform` and never names a concrete OS type. The right implementation
//! is chosen at compile time via `#[cfg(target_os = "...")]`.

pub mod traits;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

use traits::Platform;

/// Native JS dialog panels (alert/confirm/prompt) for the app webview — a
/// container gap, not a service concern, so it lives beside the traits rather
/// than behind one. macOS only: wry implements no WKUIDelegate JS-dialog
/// methods there, so `confirm()` silently returns `false`. Windows (WebView2)
/// and Linux (webkitgtk) webviews render their own JS dialogs, and tauri's
/// `PlatformWebview::inner` is macOS/iOS-only — so no cross-platform shim.
#[cfg(target_os = "macos")]
pub use macos::webview_dialogs::install_js_dialog_panels;

/// Construct the platform implementation for the current OS.
pub fn current() -> Box<dyn Platform> {
    #[cfg(target_os = "macos")]
    {
        Box::new(macos::MacosPlatform::new())
    }
    #[cfg(target_os = "windows")]
    {
        Box::new(windows::WindowsPlatform::new())
    }
    #[cfg(target_os = "linux")]
    {
        Box::new(linux::LinuxPlatform::new())
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        compile_error!("unsupported target OS")
    }
}
