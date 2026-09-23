//! Phase-3 §9.2 check: the tunnel URL-rewrite mu-plugin, executed by the REAL
//! bundled PHP. `wp_tunnel::enable` writes the plugin into a temp docroot, then a
//! small harness (WP function stubs) includes it three ways:
//!   1. `local`  — no Cloudflare headers → completely inert (no filters, no
//!      buffer, `$_SERVER` untouched);
//!   2. `tunnel` — CF-marked request → `HTTP_HOST`/`HTTPS` overridden and the
//!      `option_siteurl`/`option_home`/`content_url`/`upload_dir` filters return
//!      the public origin (lookalike hosts untouched), keeping any blog PATH so
//!      a subdirectory-multisite sub-site still resolves;
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
// Subdirectory multisite: a sub-site's siteurl/home carry the blog PATH. Losing
// it sent /sub1/wp-login.php to the MAIN site's dashboard (26 Aug 2026).
ok(apply_rex('option_siteurl', 'https://mysite.test/sub1') === "$origin/sub1",
    'sub-site siteurl keeps the blog path');
// Only the scheme+host prefix is replaced, so the tail survives verbatim —
// that is what keeps a ?ver= query on an asset URL intact too.
ok(apply_rex('option_home', 'https://mysite.test/sub1/') === "$origin/sub1/",
    'sub-site home keeps the blog path, tail verbatim');
ok(apply_rex('content_url', 'https://mysite.test/wp-content/x.css?ver=6.9') === "$origin/wp-content/x.css?ver=6.9",
    'a query string on an asset URL survives');
// A SUBDOMAIN network's sub-site lives on another host. The tunnel pins ONE
// Host, so rewriting it would not make it reachable — it would point at the
// MAIN site while looking like the sub-site.
ok(apply_rex('option_home', 'http://s1.mysite.test/') === 'http://s1.mysite.test/',
    'another blog on another host is left alone');
ok(apply_rex('option_siteurl', 'https://mysite.tester.com/') === 'https://mysite.tester.com/',
    'lookalike host is left alone here too');
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

/// Harness for the wp-config cookie-scope block. Includes the generated config
/// exactly as PHP would and reports what COOKIE_DOMAIN ended up as — the block
/// is only worth anything if it fires under the real interpreter, in the real
/// four combinations.
const CONFIG_HARNESS: &str = r#"<?php
if (($argv[1] ?? '') === 'cf') {
    $_SERVER['HTTP_CF_RAY'] = '8f0000000000-EWR';
}
require $argv[2];
echo defined('COOKIE_DOMAIN') ? "COOKIE_DOMAIN=[" . COOKIE_DOMAIN . "]\n" : "COOKIE_DOMAIN=undefined\n";
"#;

/// The wp-config a subdomain network carries when `ensure_subdomain_cookie_scope`
/// meets it — the constants WP-CLI writes plus the anchor comment it leaves.
const SUBDOMAIN_CONFIG: &str = "<?php\ndefine( 'MULTISITE', true );\n\
define( 'SUBDOMAIN_INSTALL', true );\n\
/* That's all, stop editing! */\n";

/// Harness for the sunrise drop-in. Stubs the two things it touches — the CF
/// headers and `$wpdb` — and prints what the request looked like afterwards.
const SUNRISE_HARNESS: &str = r#"<?php
define('SUBDOMAIN_INSTALL', true);
define('DOMAIN_CURRENT_SITE', 'mysite.test');
class RexWpdbStub {
    public $blogs = 'wp_blogs';
    public $asked = [];
    public function prepare($q, ...$a) { return [$q, $a]; }
    public function get_var($q) {
        $this->asked[] = $q[1][0];
        return $q[1][0] === 's1.mysite.test' ? 2 : null;
    }
}
$GLOBALS['wpdb'] = new RexWpdbStub();
$mode = $argv[1];
$_SERVER['REQUEST_URI'] = $argv[3];
$_SERVER['HTTP_HOST'] = 'mysite.test';
if ($mode === 'tunnel') { $_SERVER['HTTP_CF_RAY'] = '8f0000000000-EWR'; }
require $argv[2];
echo "HOST={$_SERVER['HTTP_HOST']} URI={$_SERVER['REQUEST_URI']}\n";
"#;

#[tokio::main]
async fn main() {
    let plat = platform::current();
    let php = binaries::resolve(&*plat, "php", binaries::pins().php).await.expect("php");

    let dir = std::env::temp_dir().join("rexenv-tunnel-muplugin-check");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    wp_tunnel::enable(&dir, "wp-content", ORIGIN).expect("enable");
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

    // 4) The sunrise drop-in: the REQUEST half. A tunnel pins one Host, so a
    //    subdomain network's sub-sites are reachable only as subdirectories —
    //    /s1/... has to become a request for s1.<network>, and it has to happen
    //    before ms-settings resolves the blog.
    let sunrise = dir.join("wp-content/sunrise.php");
    assert!(sunrise.is_file(), "sunrise written next to the mu-plugin");
    let lint = std::process::Command::new(&php).arg("-l").arg(&sunrise).output().unwrap();
    assert!(lint.status.success(), "sunrise php -l failed: {}", String::from_utf8_lossy(&lint.stderr));
    let sun_harness = dir.join("sunrise-harness.php");
    std::fs::write(&sun_harness, SUNRISE_HARNESS).unwrap();
    let run_sun = |mode: &str, uri: &str| {
        let out = std::process::Command::new(&php)
            .arg(&sun_harness)
            .arg(mode)
            .arg(&sunrise)
            .arg(uri)
            .output()
            .unwrap();
        assert!(out.status.success(), "sunrise harness failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    // The Host moves; the URI does NOT. WP strips home_url()'s /s1 itself, and
    // everything it builds from REQUEST_URI needs the prefix still there.
    assert_eq!(run_sun("tunnel", "/s1/wp-admin/"), "HOST=s1.mysite.test URI=/s1/wp-admin/");
    assert_eq!(run_sun("tunnel", "/s1"), "HOST=s1.mysite.test URI=/s1");
    assert_eq!(run_sun("tunnel", "/s1/?p=7"), "HOST=s1.mysite.test URI=/s1/?p=7");
    // A label no blog owns belongs to the MAIN site — a page called /about must
    // not be swallowed by this.
    assert_eq!(run_sun("tunnel", "/about/"), "HOST=mysite.test URI=/about/");
    // Assets never even reach the lookup.
    assert_eq!(run_sun("tunnel", "/wp-content/x.css"), "HOST=mysite.test URI=/wp-content/x.css");
    // Local requests keep the network a real subdomain network.
    assert_eq!(run_sun("local", "/s1/wp-admin/"), "HOST=mysite.test URI=/s1/wp-admin/");
    println!("✓ sunrise: /s1/… -> s1.<network> through the tunnel only; main site and assets untouched");

    // 5) The wp-config block: a SUBDOMAIN network's auth cookies survive the
    //    tunnel only if COOKIE_DOMAIN is emptied BEFORE wp-settings runs, and
    //    only while the share is rexenv's and live. Four combinations, because
    //    the second condition is the one a reviewer would be tempted to drop.
    let cfg_dir = dir.join("subdomain-network");
    std::fs::create_dir_all(&cfg_dir).unwrap();
    std::fs::write(cfg_dir.join("wp-config.php"), SUBDOMAIN_CONFIG).unwrap();
    assert!(
        wp_tunnel::ensure_subdomain_cookie_scope(&cfg_dir, "wp-content").expect("cookie scope"),
        "block written"
    );
    let config = cfg_dir.join("wp-config.php");
    let cfg_harness = dir.join("config-harness.php");
    std::fs::write(&cfg_harness, CONFIG_HARNESS).unwrap();
    let lint = std::process::Command::new(&php).arg("-l").arg(&config).output().unwrap();
    assert!(lint.status.success(), "wp-config php -l failed: {}", String::from_utf8_lossy(&lint.stderr));

    let mu = cfg_dir.join("wp-content/mu-plugins/rexenv-tunnel.php");
    let run_cfg = |mode: &str| {
        let out = std::process::Command::new(&php)
            .arg(&cfg_harness)
            .arg(mode)
            .arg(&config)
            .output()
            .unwrap();
        assert!(out.status.success(), "config harness failed: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).to_string()
    };
    // No share live yet: a CF-marked request must NOT empty the constant, or a
    // wp-config copied to a Cloudflare-fronted production network would break
    // cross-subdomain SSO there.
    assert!(run_cfg("cf").contains("COOKIE_DOMAIN=undefined"), "no live share, yet it fired");
    assert!(run_cfg("local").contains("COOKIE_DOMAIN=undefined"), "local request fired it");
    // Share live.
    std::fs::create_dir_all(mu.parent().unwrap()).unwrap();
    std::fs::write(&mu, "<?php // stand-in for a live share\n").unwrap();
    assert!(run_cfg("local").contains("COOKIE_DOMAIN=undefined"), "local request fired it");
    assert!(run_cfg("cf").contains("COOKIE_DOMAIN=[]"), "tunnel request did NOT empty COOKIE_DOMAIN");
    println!("✓ wp-config cookie scope: empty ONLY for a CF request with a live share");

    // 6) Stopping the share takes BOTH halves away — a sunrise left behind would
    //    keep remapping requests with no rewriter to match.
    wp_tunnel::disable(&dir).expect("disable");
    assert!(!plugin.exists(), "mu-plugin removed");
    assert!(!sunrise.exists(), "sunrise removed");
    println!("✓ disable removes both halves");

    let _ = std::fs::remove_dir_all(&dir);
    println!("\nALL GOOD — tunnel requests get public URLs, local requests stay untouched.");
}
