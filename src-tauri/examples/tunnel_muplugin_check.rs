//! Phase-3 §9.2 check: the tunnel URL-rewrite mu-plugin, executed by the REAL
//! bundled PHP. `wp_tunnel::enable` writes the plugin into a temp docroot, then a
//! small harness (WP function stubs) includes it three ways:
//!   1. `local`  — no Cloudflare headers → completely inert (no filters, no
//!      buffer, `$_SERVER` untouched);
//!   2. `tunnel` — CF-marked request → `HTTP_HOST`/`HTTPS` overridden and the
//!      `option_siteurl`/`option_home`/`content_url`/`upload_dir` filters return
//!      the public origin (lookalike hosts untouched);
//!   3. `buffer` — the shutdown flush rewrites plain, JSON-escaped, and
//!      %-encoded local URLs in the output, leaving `mysite.tester.com` and
//!      `sub.mysite.test` alone.
//!
//! Run: `cargo run --example tunnel_muplugin_check`

use rexenv_lib::core::{binaries, wp_tunnel};
use rexenv_lib::platform;

const ORIGIN: &str = "https://blue-cat-runs-fast.trycloudflare.com";

const HARNESS: &str = r#"<?php
// Minimal WP stubs: collect filters, apply them on demand.
$GLOBALS['rex_filters'] = [];
function add_filter($tag, $cb, $prio = 10) { $GLOBALS['rex_filters'][$tag][] = $cb; }
function apply_rex($tag, $value) {
    foreach ($GLOBALS['rex_filters'][$tag] ?? [] as $cb) { $value = $cb($value); }
    return $value;
}
function ok($cond, $msg) {
    if (!$cond) { fwrite(STDERR, "FAIL: $msg\n"); exit(1); }
    fwrite(STDERR, "ok: $msg\n");
}

$mode = $argv[1];
$plugin = $argv[2];
$_SERVER['HTTP_HOST'] = 'mysite.test';
if ($mode !== 'local') { $_SERVER['HTTP_CF_RAY'] = '8f0000000000-EWR'; }
$level = ob_get_level();

require $plugin;

if ($mode === 'local') {
    ok($GLOBALS['rex_filters'] === [], 'local request registers no filters');
    ok($_SERVER['HTTP_HOST'] === 'mysite.test', 'local HTTP_HOST untouched');
    ok(empty($_SERVER['HTTPS']), 'local HTTPS untouched');
    ok(ob_get_level() === $level, 'local request starts no output buffer');
    exit(0);
}

$origin = 'https://blue-cat-runs-fast.trycloudflare.com';
ok($_SERVER['HTTP_HOST'] === 'blue-cat-runs-fast.trycloudflare.com', 'HTTP_HOST -> public host');
ok($_SERVER['HTTPS'] === 'on', 'HTTPS forced on');
ok(ob_get_level() === $level + 1, 'output buffer installed');
ok(apply_rex('option_siteurl', 'https://mysite.test') === $origin, 'siteurl -> origin');
ok(apply_rex('option_home', 'https://mysite.test') === $origin, 'home -> origin');
ok(apply_rex('content_url', 'https://mysite.test/wp-content/x.css') === "$origin/wp-content/x.css",
    'content_url host swapped');
ok(apply_rex('content_url', 'https://mysite.tester.com/x') === 'https://mysite.tester.com/x',
    'lookalike host untouched');
$up = apply_rex('upload_dir', [
    'url' => 'https://mysite.test/wp-content/uploads/2026/07',
    'baseurl' => 'http://mysite.test/wp-content/uploads',
    'path' => '/var/x',
]);
ok($up['url'] === "$origin/wp-content/uploads/2026/07", 'upload_dir url swapped');
ok($up['baseurl'] === "$origin/wp-content/uploads", 'upload_dir baseurl swapped (http too)');
ok($up['path'] === '/var/x', 'upload_dir filesystem path untouched');

if ($mode === 'buffer') {
    // Everything echoed below passes through the plugin's ob callback on the
    // shutdown flush; the Rust side asserts the transformed stdout.
    echo "plain https://mysite.test/wp-content/uploads/a.png\n";
    echo "plain-http http://mysite.test/page/\n";
    echo "json {\"link\":\"https:\\/\\/mysite.test\\/hello-world\\/\"}\n";
    echo "enc back=https%3A%2F%2Fmysite.test%2Fwp-admin%2F\n";
    echo "lookalike https://mysite.tester.com/x\n";
    echo "subhost https://sub.mysite.test/y\n";
}
exit(0);
"#;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let php = binaries::resolve(&*plat, "php", binaries::PHP_VERSION).await.expect("php");

    let dir = std::env::temp_dir().join("rexenv-tunnel-muplugin-check");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    wp_tunnel::enable(&dir, ORIGIN).expect("enable");
    let plugin = dir.join("wp-content/mu-plugins/rexenv-tunnel.php");
    assert!(plugin.is_file(), "mu-plugin written");
    let harness = dir.join("harness.php");
    std::fs::write(&harness, HARNESS).unwrap();

    // 0) The generated plugin is valid PHP.
    let lint = std::process::Command::new(&php).arg("-l").arg(&plugin).output().unwrap();
    assert!(
        lint.status.success(),
        "php -l failed: {}{}",
        String::from_utf8_lossy(&lint.stdout),
        String::from_utf8_lossy(&lint.stderr)
    );
    println!("✓ php -l: no syntax errors");

    let run = |mode: &str| {
        let out = std::process::Command::new(&php)
            .arg(&harness)
            .arg(mode)
            .arg(&plugin)
            .output()
            .unwrap();
        let stdout = String::from_utf8_lossy(&out.stdout).to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        assert!(out.status.success(), "harness '{mode}' failed:\n{stderr}\n{stdout}");
        (stdout, stderr)
    };

    // 1) A local request (no CF headers) is completely untouched.
    let (_, err) = run("local");
    assert!(err.contains("ok: local request registers no filters"), "{err}");
    println!("✓ local requests: mu-plugin inert");

    // 2) A CF-marked request gets host/https overrides + URL filters.
    let (_, err) = run("tunnel");
    for needle in ["HTTP_HOST -> public host", "siteurl -> origin", "lookalike host untouched"] {
        assert!(err.contains(&format!("ok: {needle}")), "missing '{needle}':\n{err}");
    }
    println!("✓ tunnel requests: host override + siteurl/home/content/upload filters");

    // 3) The output buffer rewrites plain + JSON-escaped + %-encoded local URLs.
    let (out, _) = run("buffer");
    assert!(out.contains(&format!("plain {ORIGIN}/wp-content/uploads/a.png")), "plain:\n{out}");
    assert!(out.contains(&format!("plain-http {ORIGIN}/page/")), "http scheme:\n{out}");
    assert!(
        out.contains(r#"json {"link":"https:\/\/blue-cat-runs-fast.trycloudflare.com\/hello-world\/"}"#),
        "json-escaped:\n{out}"
    );
    assert!(
        out.contains("enc back=https%3A%2F%2Fblue-cat-runs-fast.trycloudflare.com%2Fwp-admin%2F"),
        "%-encoded:\n{out}"
    );
    assert!(out.contains("lookalike https://mysite.tester.com/x"), "lookalike rewritten!\n{out}");
    assert!(out.contains("subhost https://sub.mysite.test/y"), "subhost rewritten!\n{out}");
    assert!(!out.contains("://mysite.test/"), "a local URL survived:\n{out}");
    println!("✓ output buffer: plain / JSON-escaped / %-encoded rewritten; lookalikes kept");

    let _ = std::fs::remove_dir_all(&dir);
    println!("\nALL GOOD — tunnel requests get public URLs, local requests stay untouched.");
}
