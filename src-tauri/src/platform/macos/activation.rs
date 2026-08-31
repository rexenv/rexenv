//! Bringing an app with NO dock icon to the front.
//!
//! rexenv runs as an **accessory** app (`NSApplicationActivationPolicy`
//! `Accessory`): a menu-bar item, no dock tile, no entry in the app switcher.
//! That is what was asked for, and it costs something specific — an accessory
//! app is never made active by the things that normally activate one. Clicking
//! a dock tile is not available, and a window it merely shows or focuses can
//! come up BEHIND the browser the developer was reading, which reads as a menu
//! item that did nothing. A modal it opens can land behind too, and a native
//! prompt nobody sees is worse than no prompt: the app looks hung.
//!
//! So every path that draws our own UI while no window is up activates first.
//! Prompts run through `osascript` are NOT in that set — those are drawn by a
//! separate process (SecurityAgent), which fronts itself.
use objc2::MainThreadMarker;
use objc2_app_kit::NSApplication;

/// Make rexenv the active app. No-op off the main thread — AppKit activation is
/// main-thread-only, and a wrong-thread call is undefined behaviour, not an
/// error, so this checks rather than assumes.
///
/// `activate` (not the deprecated `activateIgnoringOtherApps:`) because the
/// app's minimum system version is macOS 15 — the newer call is available on
/// every machine that can run rexenv at all, and the old one warns.
pub fn activate_app() {
    let Some(mtm) = MainThreadMarker::new() else {
        log::debug!("activation: not the main thread — skipping activate");
        return;
    };
    NSApplication::sharedApplication(mtm).activate();
}
