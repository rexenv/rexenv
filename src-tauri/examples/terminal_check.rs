//! Phase-3 §4.1 check: PTY backend. Opens a `core::terminal` session in a chosen
//! cwd with the bundled PHP + `wp` wrapper on PATH, writes shell commands, and
//! collects PTY output over a channel (no Tauri). Verifies: `php -v` reports the
//! BUNDLED version (not a system/Homebrew php), `pwd` is the cwd, and the `wp`
//! wrapper runs WP-CLI.
//!
//! Runs under a LAUNCHD-SHAPED PATH (`/usr/bin:/bin:/usr/sbin:/sbin`), because a
//! .app is started by launchd, not by a terminal: this process's own rich PATH is
//! precisely the fixture that hid the bug where the session spawned a NON-login
//! shell and `code` / `rex` / brew tools were "command not found" in the app while
//! working in Terminal.app.
//!
//! Run: `cargo run --example terminal_check`

use rexenv_lib::core::terminal::{self, PtyConfig, TerminalSession};
use rexenv_lib::core::binaries;
use rexenv_lib::platform;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    let plat = platform::current();

    // Bundled PHP 8.3.31 + WP-CLI + the `wp` wrapper.
    let php_bin = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.expect("php");
    let wp_phar = binaries::resolve_file(&*plat, "wp-cli", binaries::WP_CLI_VERSION).await.expect("wp-cli");
    let wp_dir = terminal::ensure_wp_wrapper(&*plat, &php_bin, &wp_phar).expect("wp wrapper");
    let php_dir = php_bin.parent().unwrap().to_path_buf();

    // A docroot to start in.
    let cwd = std::env::temp_dir().join("rexenv-term-cwd");
    let _ = std::fs::create_dir_all(&cwd);
    let cwd_real = std::fs::canonicalize(&cwd).unwrap();

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    println!("shell={shell}\ncwd={}\nphp_dir={}", cwd_real.display(), php_dir.display());

    // Shrink PATH to what launchd hands a GUI .app BEFORE opening the session —
    // the session inherits this process's PATH, so a developer-shell PATH here
    // would fake a pass for the very thing under test.
    const BARE: &str = "/usr/bin:/bin:/usr/sbin:/sbin";
    std::env::set_var("PATH", BARE);

    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let session = TerminalSession::open(
        PtyConfig {
            // The mail catch-all's terminal half; empty is honest for a probe that
            // never sends mail.
            env: Vec::new(),
            cwd: cwd_real.clone(),
            shell,
            path_prepend: vec![php_dir, wp_dir],
            rows: 24,
            // Wide: PATH is long, and an 80-col PTY hard-wraps it mid-word, which
            // would split the dir this check looks for across two lines.
            cols: 400,
        },
        move |chunk| {
            let _ = tx.send(chunk);
        },
    )
    .expect("open session");

    // Drive the shell.
    session.write(b"php -v\n").unwrap();
    session.write(b"pwd\n").unwrap();
    session.write(b"wp --version\n").unwrap();
    session.write(b"echo PATHIS:$PATH\n").unwrap();
    session.write(b"exit\n").unwrap();

    // Generous deadline: a LOGIN shell runs /etc/zprofile + ~/.zprofile + ~/.zshrc
    // (prompt frameworks, nvm, …) before the first command echoes — seconds.
    let mut out = String::new();
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(300)) {
            Ok(chunk) => out.push_str(&String::from_utf8_lossy(&chunk)),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if out.contains("PATHIS:/") {
                    break;
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    println!("\n────── PTY output ──────\n{out}\n────────────────────────");

    let bundled = format!("PHP {}", binaries::PHP_VERSION); // "PHP 8.3.31"
    assert!(out.contains(&bundled), "php -v did not report the bundled version ({bundled})");
    println!("✓ `php -v` → bundled {bundled}");

    let cwd_str = cwd_real.display().to_string();
    assert!(out.contains(&cwd_str), "pwd did not show the docroot ({cwd_str})");
    println!("✓ cwd is the docroot");

    assert!(out.contains("WP-CLI"), "wp wrapper did not run WP-CLI");
    println!("✓ `wp --version` runs the bundled WP-CLI");

    // The shell must be a LOGIN shell: only /etc/zprofile runs path_helper, which
    // is where /usr/local/bin (stock /etc/paths) and a brew prefix come from. A
    // non-login shell would echo back the four bare dirs and nothing else.
    let echoed: String = out
        .lines()
        // rsplit: the echoed COMMAND (`echo PATHIS:$PATH`) and the shell's title
        // escape both carry the marker on the same line as the answer; the value
        // is whatever follows the LAST marker.
        .filter(|l| l.contains("PATHIS:"))
        .filter_map(|l| l.rsplit("PATHIS:").next())
        .find(|v| v.starts_with('/'))
        .map(|v| v.trim().to_string())
        .expect("shell never echoed its PATH");
    assert!(
        echoed.contains("/usr/local/bin"),
        "login shell did not restore the system PATH (/etc/zprofile → path_helper); got {echoed}"
    );
    println!("✓ login shell restores the full system PATH from a bare launchd PATH");

    let _ = session.kill();
    println!("\nALL GOOD — PTY shell runs in the docroot with bundled PHP + wp on PATH.");
}
