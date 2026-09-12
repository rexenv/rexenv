//! Ledger #166 leg B: `install_js_dialog_panels` against a REAL `WKWebView`
//! and a wry-shaped stub delegate (a class implementing ONLY the file-picker
//! selector, which is all wry ≤0.55 implements).
//!
//!   cargo run --example webview_dialogs_check      (sandbox tier)
//!
//! Spawns nothing, binds nothing, writes nothing, and — deliberately — shows
//! NO dialogs: driving a native NSAlert needs synthetic clicks, which are off
//! the table on a dev machine (see docs/archive/PLAN-webview-dialog-proofs.md §5).
//! What a real WebKit CAN prove headless is the installation mechanics:
//!
//!   1. before install, the delegate's class answers none of the three JS
//!      panel selectors (the negative control — without it, a stub that
//!      accidentally had them would make every later ✓ vacuous);
//!   2. after install, it answers all three;
//!   3. the file-picker method wry relies on is byte-identical before/after
//!      (ours ADD, never replace);
//!   4. the SAME delegate object is still installed after the re-set dance
//!      (a dropped delegate would silently kill the file picker);
//!   5. a second install is a no-op (the app may call it per-window).
//!
//! What this does NOT prove, stated as loudly: that a sheet renders, that
//! `confirm()` returns the button pressed, or that WebKit suspends the calling
//! frame — that is SMOKE's drop-table step (leg C), which needs an eye.

// macOS-only (WKWebView). Gated item by item rather than with a file-level
// `#![cfg]`, which on any other OS leaves a crate with no `main` (E0601) —
// found by the Windows compile check (docs/PLAN-windows-port.md W1).

#[cfg(target_os = "macos")]
use objc2::ffi::class_addMethod;
#[cfg(target_os = "macos")]
use objc2::rc::Retained;
#[cfg(target_os = "macos")]
use objc2::runtime::{AnyClass, AnyObject, ClassBuilder, Imp, Sel};
#[cfg(target_os = "macos")]
use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly};
#[cfg(target_os = "macos")]
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize};
#[cfg(target_os = "macos")]
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};

mod common;

#[cfg(target_os = "macos")]
const ENC_PANEL: &[u8] = b"v@:@@@@?\0";

/// The stand-in for wry's file-picker method. Never called — the example
/// opens no panels — it exists so the stub class has exactly wry's shape.
#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn stub_open_panel(
    _this: *mut AnyObject,
    _cmd: Sel,
    _webview: *mut AnyObject,
    _params: *mut AnyObject,
    _frame: *mut AnyObject,
    _completion: *mut AnyObject,
) {
    std::hint::black_box(0u8);
}

#[cfg(target_os = "macos")]
fn responds(cls: &AnyClass, sel: Sel) -> bool {
    cls.instance_method(sel).is_some()
}

#[cfg(target_os = "macos")]
fn imp_of(cls: &AnyClass, sel: Sel) -> Option<Imp> {
    cls.instance_method(sel).map(|m| m.implementation())
}

#[cfg(target_os = "macos")]
fn main() -> std::process::ExitCode {
    let mut checks = common::Check::new("webview_dialogs_check");
    let mtm = MainThreadMarker::new().expect("example main runs on the main thread");

    // A real WKWebView, never attached to a window — headless is the point.
    let config = unsafe { WKWebViewConfiguration::new(mtm) };
    let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(320.0, 240.0));
    let webview: Retained<WKWebView> = unsafe {
        WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config)
    };

    // The wry-shaped stub: NSObject subclass with ONLY the file-picker method.
    let superclass = AnyClass::get(c"NSObject").expect("NSObject");
    let builder =
        ClassBuilder::new(c"RexWryShapedStubDelegate", superclass).expect("fresh class name");
    let cls = builder.register();
    type PanelImp = unsafe extern "C-unwind" fn(
        *mut AnyObject,
        Sel,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
    );
    let picker_sel = sel!(webView:runOpenPanelWithParameters:initiatedByFrame:completionHandler:);
    let added = unsafe {
        class_addMethod(
            cls as *const AnyClass as *mut _,
            picker_sel,
            std::mem::transmute::<PanelImp, Imp>(stub_open_panel),
            ENC_PANEL.as_ptr() as *const std::ffi::c_char,
        )
    };
    assert!(added.as_bool(), "stub class setup failed — nothing below means anything");

    let delegate: Retained<NSObject> = unsafe { msg_send![cls, new] };
    let delegate_ptr = Retained::as_ptr(&delegate) as *const AnyObject;
    // Typed as AnyObject on purpose: the stub satisfies WKUIDelegate the way
    // wry's does — by responding to selectors, not by a Rust trait impl.
    let _: () = unsafe { msg_send![&*webview, setUIDelegate: &*delegate] };

    let alert_sel =
        sel!(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:);
    let confirm_sel =
        sel!(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:);
    let prompt_sel = sel!(
        webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:
    );

    // 1. Negative control: the wry-shaped stub answers NONE of the JS panels.
    checks.is(
        "before install, the delegate answers none of the three JS panel selectors",
        !responds(cls, alert_sel) && !responds(cls, confirm_sel) && !responds(cls, prompt_sel),
        "the stub already had panel methods — every later ✓ would be vacuous",
    );
    let picker_before = imp_of(cls, picker_sel);

    unsafe {
        rexenv_lib::platform::install_js_dialog_panels(
            Retained::as_ptr(&webview) as *mut std::ffi::c_void
        );
    }

    // 2. All three panels answer now.
    checks.is(
        "after install, alert/confirm/prompt all answer",
        responds(cls, alert_sel) && responds(cls, confirm_sel) && responds(cls, prompt_sel),
        "install_js_dialog_panels did not add the panel methods to the delegate's class",
    );
    // 3. The file-picker method is untouched.
    checks.is(
        "wry's file-picker implementation is byte-identical after install",
        imp_of(cls, picker_sel).map(|i| i as usize) == picker_before.map(|i| i as usize),
        "install replaced or dropped the file-picker method wry relies on",
    );
    // 4. The re-set dance left the SAME delegate installed.
    let after: Option<Retained<AnyObject>> = unsafe { msg_send![&*webview, UIDelegate] };
    checks.is(
        "the same delegate object is still installed after the re-set",
        after.as_ref().map(Retained::as_ptr) == Some(delegate_ptr),
        "the delegate changed or vanished — the file picker would die silently",
    );
    // 5. Idempotent: a second install changes nothing.
    let imps_first: Vec<Option<usize>> = [alert_sel, confirm_sel, prompt_sel, picker_sel]
        .iter()
        .map(|s| imp_of(cls, *s).map(|i| i as usize))
        .collect();
    unsafe {
        rexenv_lib::platform::install_js_dialog_panels(
            Retained::as_ptr(&webview) as *mut std::ffi::c_void
        );
    }
    let imps_second: Vec<Option<usize>> = [alert_sel, confirm_sel, prompt_sel, picker_sel]
        .iter()
        .map(|s| imp_of(cls, *s).map(|i| i as usize))
        .collect();
    let after2: Option<Retained<AnyObject>> = unsafe { msg_send![&*webview, UIDelegate] };
    checks.is(
        "a second install is a no-op (imps and delegate unchanged)",
        imps_first == imps_second
            && after2.as_ref().map(Retained::as_ptr) == Some(delegate_ptr),
        "re-installing moved an implementation or the delegate",
    );

    println!(
        "NOT proven here (needs an eye — SMOKE §Database drop-table): a sheet renders, \
         confirm() returns the button pressed, JS suspends."
    );
    checks.verdict()
}

/// WKWebView exists only on macOS; the other platforms' webviews draw their own
/// JS dialogs, so there is nothing here to check.
#[cfg(not(target_os = "macos"))]
fn main() -> std::process::ExitCode {
    eprintln!("webview_dialogs_check: skipped — macOS-only (WKWebView)");
    std::process::ExitCode::SUCCESS
}
