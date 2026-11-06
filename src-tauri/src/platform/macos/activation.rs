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
use objc2::runtime::NSObjectProtocol;
use objc2::{sel, MainThreadMarker};
use objc2_app_kit::NSApplication;

/// Make rexenv the active app. No-op off the main thread — AppKit activation is
/// main-thread-only, and a wrong-thread call is undefined behaviour, not an
/// error, so this checks rather than assumes.
///
/// **`-activate` exists from macOS 14; `-activateIgnoringOtherApps:` from 10.0.**
/// The first version of this called `activate` unconditionally, justified by a
/// stated floor of 15 — and the day the floor moved to 13 (ledger #713) the very
/// first launch on a macOS 13.6 VM died in `applicationDidFinishLaunching`:
/// `-[TaoApp activate]: unrecognized selector`, an ObjC exception that crossed
/// tao's nounwind frame and aborted the process before a window existed. The
/// choice is made by ASKING the object (`respondsToSelector:`), not by comparing
/// a version: it is exactly the selector's presence that decides, and a version
/// check would have to be kept in step with Apple's tables by hand. An
/// availability comment is not a gate; this is.
pub fn activate_app() {
    let Some(mtm) = MainThreadMarker::new() else {
        log::debug!("activation: not the main thread — skipping activate");
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    if app.respondsToSelector(sel!(activate)) {
        app.activate();
    } else {
        // macOS 13: the older call, which the floor admits. The deprecation is
        // a warning about the FUTURE, and 13 has none of that future.
        #[allow(deprecated)]
        app.activateIgnoringOtherApps(true);
    }
}

/// Keep a headless rexenv out of macOS's "reopen windows when logging back in" list (T5 M3). Its
/// LaunchAgent starts it at login; when the list ALSO reopened it, the list's copy came first as a plain
/// desktop launch and the agent's copy handed off to it and exited — measured on the 15.8 VM, 6 Nov 2026:
/// after a reboot with automatic login, `TALAppsToRelaunchAtLogin` held `dev.rexenv.rexenv`, the app ran
/// with no arguments and `headless.log` said "handed this login launch to it". Apple's own advice for an
/// app "launched via launchd": call `disableRelaunchOnLogin` once and never pair it with `enable`.
/// Thread-safe per AppKit; the marker is still required to reach `sharedApplication`.
pub fn disable_relaunch_on_login() {
    let Some(mtm) = MainThreadMarker::new() else {
        log::debug!("relaunch on login: not the main thread — skipping");
        return;
    };
    NSApplication::sharedApplication(mtm).disableRelaunchOnLogin();
}

#[cfg(test)]
mod tests {
    /// Ledger #713 — the newer selector is never sent without asking first. A
    /// source tripwire, because AppKit activation cannot run on a test thread:
    /// the gate has to be visible in the text or it is not there.
    #[test]
    fn activation_asks_before_sending_the_macos_14_selector() {
        const SRC: &str = include_str!("activation.rs");
        let body = &SRC[SRC.find("pub fn activate_app()").expect("the function")..];
        let gate = body.find("app.respondsToSelector(sel!(activate))").expect("the respondsToSelector gate is gone");
        let call = body.find("app.activate();").expect("the activate call");
        assert!(gate < call, "the gate must come before the call it guards");
        assert!(body.contains("activateIgnoringOtherApps(true)"), "no fallback for macOS 13");
    }
}
