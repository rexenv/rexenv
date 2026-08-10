//! Live check: browser + editor detection, the OS default-`https` handler, and
//! REAL app-icon extraction — the parts of the "open in <browser>" preference
//! that only a machine with apps installed can prove.
//! Read-only: reads app bundles, runs `defaults`/`sips`, writes only its own
//! temp PNG (deleted). It never OPENS anything — no browser is launched, so it
//! is safe with the stack running and safe on a machine someone is using.
//! Run: `cargo run --example browser_detect_check`
//!
//! What to eyeball on a real machine:
//! - Every browser you actually have is listed, and exactly one (or zero) is
//!   marked `system default` — the one your Mac really opens links with.
//! - `icon` says `PNG <n> bytes` for the mainstream browsers. `none` is an
//!   allowed, honest outcome (icon only in a compiled asset catalog) — the UI
//!   draws its own glyph there — but `none` for EVERY app means the extraction
//!   path is broken, not that your Mac is unusual.
//! - The icon must be a real PNG: the check decodes the data URI and asserts
//!   the PNG magic bytes, so a `sips` that "succeeded" into an empty or HTML
//!   file can't pass as an icon.

use base64::Engine;
use rexenv_lib::platform;

fn main() {
    let plat = platform::current();
    let shell = plat.shell();

    let browsers = shell.detect_browsers();
    println!("browsers detected: {}", browsers.len());
    let mut icons_ok = 0usize;
    let mut defaults = 0usize;
    for b in &browsers {
        if b.system_default {
            defaults += 1;
        }
        let icon = describe_icon(b.icon.as_deref(), &mut icons_ok);
        println!(
            "  {:<28} id={:<14} {:<22} icon={icon}",
            b.name,
            b.id,
            if b.system_default { "SYSTEM DEFAULT" } else { "" }
        );
    }

    let editors = shell.detect_editors();
    println!("\neditors detected: {}", editors.len());
    for e in &editors {
        let icon = describe_icon(e.icon.as_deref(), &mut icons_ok);
        println!("  {:<28} id={:<14} icon={icon}", e.name, e.id);
    }

    // The guard that matters: a chosen browser takes URLs, never local paths.
    // `open -a Safari /etc/passwd` would happily display the file, and every
    // caller of open_in_browser is a link affordance.
    println!();
    let id = browsers.first().map(|b| b.id.clone()).unwrap_or_else(|| "safari".into());
    let mut failed = false;
    for path in ["/etc/hosts", "file:///etc/hosts", "/Applications"] {
        match shell.open_in_browser(&id, path) {
            Ok(()) => {
                eprintln!("FAIL: open_in_browser({id}, {path}) OPENED A PATH — the URL guard is gone");
                failed = true;
            }
            Err(e) => println!("path refused, as it must: {path} → {e}"),
        }
    }

    println!();
    if browsers.is_empty() {
        eprintln!("FAIL: no browsers detected at all — a Mac has Safari");
        failed = true;
    }
    if defaults > 1 {
        eprintln!("FAIL: {defaults} browsers claim to be the system default; at most one can be");
        failed = true;
    }
    if icons_ok == 0 && !(browsers.is_empty() && editors.is_empty()) {
        eprintln!("FAIL: not one app icon extracted — icon reading is broken, not just unlucky");
        failed = true;
    }
    if failed {
        std::process::exit(1);
    }
    println!("browser_detect_check: OK ({icons_ok} icons, {defaults} system default)");
}

/// Describe an icon field AND verify it is really a PNG — an icon that decodes
/// to something else would render as a broken image in the UI, so "present"
/// isn't the bar.
fn describe_icon(icon: Option<&str>, ok: &mut usize) -> String {
    let Some(uri) = icon else { return "none (UI draws its own glyph)".into() };
    let Some(b64) = uri.strip_prefix("data:image/png;base64,") else {
        return format!("MALFORMED data URI ({} chars)", uri.len());
    };
    match base64::engine::general_purpose::STANDARD.decode(b64) {
        Ok(bytes) if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => {
            *ok += 1;
            format!("PNG {} bytes", bytes.len())
        }
        Ok(bytes) => format!("NOT A PNG ({} bytes)", bytes.len()),
        Err(e) => format!("UNDECODABLE base64: {e}"),
    }
}
