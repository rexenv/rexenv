//! Native JavaScript dialog panels (alert/confirm/prompt) for the app webview.
//!
//! wry (≤0.55) implements NO WKUIDelegate JS-dialog methods on macOS (its UI
//! delegate only handles the file-picker panel), so WebKit resolves every
//! `window.confirm()` to `false` and swallows `alert()`/`prompt()` — observed
//! live as Adminer's confirm-gated delete/drop buttons silently no-oping.
//!
//! Fix at the container level: add the three panel methods to wry's EXISTING
//! UI-delegate class at runtime. `class_addMethod` is additive-only — it fails
//! (harmlessly) if the selector already exists, so a future wry that ships its
//! own panels wins automatically, and wry's file-picker method is untouched.
//! The delegate is re-set afterwards so WebKit re-reads which optional
//! delegate methods are available.
//!
//! Dialogs are shown as window SHEETS (`beginSheetModalForWindow:`) — native
//! look, the app event loop keeps running, and WebKit itself suspends the
//! calling frame's JS until the completion handler fires, which is exactly
//! `confirm()`'s blocking contract.

use std::ffi::{c_char, c_void};

use block2::{Block, RcBlock};
use objc2::ffi::class_addMethod;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Imp, Sel};
use objc2::{sel, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{NSAlert, NSAlertFirstButtonReturn, NSModalResponse, NSTextField};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_web_kit::WKWebView;

/// ObjC type encodings: void return, `self` + `_cmd`, then the object args and
/// a trailing block (`@?`). Panels take (webView, message, frame, completion);
/// the prompt panel has one extra NSString (defaultText).
const ENC_PANEL: &[u8] = b"v@:@@@@?\0";
const ENC_PROMPT_PANEL: &[u8] = b"v@:@@@@@?\0";

/// Build the sheet for one dialog: message text + OK (first button ⇒
/// `NSAlertFirstButtonReturn`), optionally Cancel.
unsafe fn make_alert(
    mtm: MainThreadMarker,
    message: *mut AnyObject,
    cancelable: bool,
) -> Retained<NSAlert> {
    let alert = NSAlert::new(mtm);
    if !message.is_null() {
        alert.setMessageText(&*(message as *const NSString));
    }
    alert.addButtonWithTitle(&NSString::from_str("OK"));
    if cancelable {
        alert.addButtonWithTitle(&NSString::from_str("Cancel"));
    }
    alert
}

/// `webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:`
unsafe extern "C-unwind" fn run_alert_panel(
    _this: *mut AnyObject,
    _cmd: Sel,
    webview: *mut AnyObject,
    message: *mut AnyObject,
    _frame: *mut AnyObject,
    completion: *mut AnyObject,
) {
    // The handler outlives this callback — copy it off WebKit's stack.
    let done = (&*(completion as *const Block<dyn Fn()>)).copy();
    let Some(mtm) = MainThreadMarker::new() else {
        return done.call(()); // never leak the handler — WebKit traps that
    };
    let alert = make_alert(mtm, message, false);
    match (&*(webview as *const WKWebView)).window() {
        Some(win) => {
            let handler = RcBlock::new(move |_resp: NSModalResponse| done.call(()));
            alert.beginSheetModalForWindow_completionHandler(&win, Some(&handler));
        }
        None => {
            alert.runModal();
            done.call(());
        }
    }
}

/// `webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:`
unsafe extern "C-unwind" fn run_confirm_panel(
    _this: *mut AnyObject,
    _cmd: Sel,
    webview: *mut AnyObject,
    message: *mut AnyObject,
    _frame: *mut AnyObject,
    completion: *mut AnyObject,
) {
    let done = (&*(completion as *const Block<dyn Fn(Bool)>)).copy();
    let Some(mtm) = MainThreadMarker::new() else {
        return done.call((Bool::NO,));
    };
    let alert = make_alert(mtm, message, true);
    match (&*(webview as *const WKWebView)).window() {
        Some(win) => {
            let handler = RcBlock::new(move |resp: NSModalResponse| {
                done.call((Bool::new(resp == NSAlertFirstButtonReturn),))
            });
            alert.beginSheetModalForWindow_completionHandler(&win, Some(&handler));
        }
        None => {
            let resp = alert.runModal();
            done.call((Bool::new(resp == NSAlertFirstButtonReturn),));
        }
    }
}

/// `webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:`
unsafe extern "C-unwind" fn run_text_input_panel(
    _this: *mut AnyObject,
    _cmd: Sel,
    webview: *mut AnyObject,
    prompt: *mut AnyObject,
    default_text: *mut AnyObject,
    _frame: *mut AnyObject,
    completion: *mut AnyObject,
) {
    let done = (&*(completion as *const Block<dyn Fn(*mut NSString)>)).copy();
    let Some(mtm) = MainThreadMarker::new() else {
        return done.call((std::ptr::null_mut(),));
    };
    let alert = make_alert(mtm, prompt, true);
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(240.0, 24.0)),
    );
    if !default_text.is_null() {
        field.setStringValue(&*(default_text as *const NSString));
    }
    alert.setAccessoryView(Some(&field));
    let finish = move |resp: NSModalResponse| {
        if resp == NSAlertFirstButtonReturn {
            let text = field.stringValue();
            done.call((Retained::as_ptr(&text) as *mut NSString,));
        } else {
            done.call((std::ptr::null_mut(),)); // prompt() -> null on cancel
        }
    };
    match (&*(webview as *const WKWebView)).window() {
        Some(win) => {
            let handler = RcBlock::new(finish);
            alert.beginSheetModalForWindow_completionHandler(&win, Some(&handler));
        }
        None => {
            let resp = alert.runModal();
            finish(resp);
        }
    }
}

/// Install the three JS dialog panels on the webview's UI-delegate class.
///
/// `wk_webview` is the raw `WKWebView` handle from tauri's
/// [`PlatformWebview::inner`]. Must run on the main thread (tauri's
/// `with_webview` guarantees that).
///
/// # Safety
/// `wk_webview` must be a live `WKWebView` pointer.
pub unsafe fn install_js_dialog_panels(wk_webview: *mut c_void) {
    let Some(webview) = (wk_webview as *mut WKWebView).as_ref() else {
        log::warn!("webview dialogs: null WKWebView handle");
        return;
    };
    // wry always sets a UI delegate (file-picker support); extend its class.
    let Some(delegate) = webview.UIDelegate() else {
        log::warn!("webview dialogs: webview has no UI delegate to extend");
        return;
    };
    let obj = &*(Retained::as_ptr(&delegate) as *const AnyObject);
    let cls = obj.class() as *const _ as *mut _;

    type PanelImp = unsafe extern "C-unwind" fn(
        *mut AnyObject,
        Sel,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
    );
    type PromptImp = unsafe extern "C-unwind" fn(
        *mut AnyObject,
        Sel,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
    );
    let added = [
        class_addMethod(
            cls,
            sel!(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:),
            std::mem::transmute::<PanelImp, Imp>(run_alert_panel),
            ENC_PANEL.as_ptr() as *const c_char,
        ),
        class_addMethod(
            cls,
            sel!(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:),
            std::mem::transmute::<PanelImp, Imp>(run_confirm_panel),
            ENC_PANEL.as_ptr() as *const c_char,
        ),
        class_addMethod(
            cls,
            sel!(
                webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:
            ),
            std::mem::transmute::<PromptImp, Imp>(run_text_input_panel),
            ENC_PROMPT_PANEL.as_ptr() as *const c_char,
        ),
    ];
    if added.iter().any(|b| !b.as_bool()) {
        // Selector already present — a wry with native panels; ours defer.
        log::info!("webview dialogs: delegate already implements some JS dialog panels");
    }
    // WebKit checks which optional delegate methods exist when the delegate is
    // SET — re-set it so the just-added panels are picked up.
    webview.setUIDelegate(None);
    webview.setUIDelegate(Some(&delegate));
    log::info!("webview dialogs: JS alert/confirm/prompt panels installed");
}
