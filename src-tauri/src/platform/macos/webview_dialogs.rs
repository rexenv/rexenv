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

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::runtime::{AnyClass, ClassBuilder};

    // The two imps must have DIFFERENT bodies: identical functions can be
    // merged by the linker into one address, which would make the
    // imp-identity assertion below a tautology (it could never observe a
    // replacement). The test also asserts they are distinct, so the guard
    // does not rest on the optimiser's mood.
    unsafe extern "C-unwind" fn imp_first(
        _this: *mut AnyObject,
        _cmd: Sel,
        _a: *mut AnyObject,
        _b: *mut AnyObject,
        _c: *mut AnyObject,
        _d: *mut AnyObject,
    ) {
        std::hint::black_box(1u8);
    }
    unsafe extern "C-unwind" fn imp_second(
        _this: *mut AnyObject,
        _cmd: Sel,
        _a: *mut AnyObject,
        _b: *mut AnyObject,
        _c: *mut AnyObject,
        _d: *mut AnyObject,
    ) {
        std::hint::black_box(2u8);
    }

    /// Ledger #166, leg A. The module's whole conflict story rests on ONE fact
    /// about the ObjC runtime: `class_addMethod` ADDS and never REPLACES, so a
    /// future wry that ships its own panel methods wins automatically and ours
    /// become dead code rather than a fight. That is a claim about Apple's
    /// runtime, not about our code — so it is measured against the real
    /// runtime, with a control first: adding to a class WITHOUT the selector
    /// must succeed, or "the second add failed" would also be what a broken
    /// registration looks like and the test would pass vacuously.
    #[test]
    fn class_add_method_is_additive_only_so_a_wry_with_native_panels_wins() {
        // Unique name: a class can be registered once per process, and the
        // runtime has no unregister for classes with registered subclasses.
        let name =
            std::ffi::CString::new(format!("RexDialogAdditiveTest{}", std::process::id()))
                .unwrap();
        let superclass = AnyClass::get(c"NSObject").expect("NSObject");
        let builder = ClassBuilder::new(&name, superclass).expect("fresh class name");
        let cls = builder.register();
        let sel = sel!(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:);

        type PanelImp = unsafe extern "C-unwind" fn(
            *mut AnyObject,
            Sel,
            *mut AnyObject,
            *mut AnyObject,
            *mut AnyObject,
            *mut AnyObject,
        );
        let add = |imp: PanelImp| unsafe {
            class_addMethod(
                cls as *const AnyClass as *mut _,
                sel,
                std::mem::transmute::<PanelImp, Imp>(imp),
                ENC_PANEL.as_ptr() as *const c_char,
            )
        };

        assert_ne!(
            imp_first as PanelImp as *const () as usize,
            imp_second as PanelImp as *const () as usize,
            "the two test imps were merged to one address — the identity assertion \
             below cannot see a replacement; give them different bodies"
        );
        // Control: the mechanism works at all.
        assert!(
            add(imp_first).as_bool(),
            "control broken: adding a panel method to a class without one failed — \
             every later assertion would be vacuous"
        );
        // The claim: a second add of the SAME selector fails…
        assert!(
            !add(imp_second).as_bool(),
            "class_addMethod REPLACED an existing selector — the 'a wry with native \
             panels wins automatically' story in this module's header is false"
        );
        // …and fails HARMLESSLY: the first implementation is still the one installed.
        let installed = cls
            .instance_method(sel)
            .expect("the selector answers after the first add")
            .implementation();
        assert_eq!(
            installed as usize, imp_first as PanelImp as *const () as usize,
            "the losing add clobbered the installed implementation — 'fails' without \
             'harmlessly'"
        );
    }
}
