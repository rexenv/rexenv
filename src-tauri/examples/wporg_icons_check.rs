//! Live check: the plugin list's icon column, including the PAID plugins that
//! are not in the wp.org directory at all (`core::wporg::plugin_icons`).
//!
//! The bug this exists for: rexenv asked wp.org for every installed slug, and
//! wp.org has never heard of `betterdocs-pro`, `elementor-pro`,
//! `wp-security-audit-log-premium` — so every premium row on a real site was a
//! letter tile while wp-admin's own update screen showed the vendor's logo.
//! wp-admin reads those from the `update_plugins` transient, which a wp-cli run
//! cannot see (most vendors inject it on an `is_admin()` request only), so
//! rexenv derives the icon from the FREE counterpart's slug instead.
//!
//! Asserted here against the REAL api.wordpress.org, because the derivation is
//! only worth anything if the counterpart slugs it invents actually resolve:
//!   - a paid slug whose free counterpart is installed alongside borrows that
//!     row's exact icon (no second request needed);
//!   - a paid slug whose counterpart is NOT installed still resolves (one extra
//!     lookup) — and the URL it lands on is a real, fetchable image;
//!   - a private plugin, and a paid plugin no suffix rule reaches, stay `None`
//!     rather than borrowing a stranger's logo;
//!   - the map stays total over the input.
//!
//! Touches nothing on the machine: no app data, no ports, no processes — HTTPS
//! GETs against api.wordpress.org and ps.w.org only.
//!
//! Run (needs the internet): `cargo run --example wporg_icons_check`

use rexenv_lib::core::wporg;

#[tokio::main]
async fn main() {
    // Shaped like a real site's plugin list (this one is bl.rex, 18 Aug 2026):
    // free plugins, their paid add-ons, a paid plugin whose free counterpart is
    // NOT installed, a paid plugin no rule can reach, and a private one.
    let slugs: Vec<String> = [
        "betterdocs",
        "betterdocs-pro",
        "elementor",
        "elementor-pro",
        "fluentform",
        "fluentformpro",
        "wp-security-audit-log-premium",
        "essential-addons-elementor",
        "wpdev-tweaks",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();

    let icons = wporg::plugin_icons(&slugs).await;

    assert_eq!(icons.len(), slugs.len(), "the map must be total over the input");
    for slug in &slugs {
        assert!(icons.contains_key(slug), "{slug} missing from the map");
    }

    let get = |slug: &str| icons.get(slug).cloned().flatten();

    // The free rows are the baseline — if these are empty the network is the
    // problem, not the derivation, and every assertion below would be noise.
    for slug in ["betterdocs", "elementor", "fluentform"] {
        assert!(
            get(slug).is_some(),
            "{slug} is in the wp.org directory — no icon here means the API leg is broken, \
             not the derivation"
        );
    }

    // The paid add-on borrows its free counterpart's art, exactly.
    for (paid, free) in [
        ("betterdocs-pro", "betterdocs"),
        ("elementor-pro", "elementor"),
        ("fluentformpro", "fluentform"),
    ] {
        assert_eq!(
            get(paid),
            get(free),
            "{paid} should show {free}'s icon (wp-admin shows the same artwork)"
        );
    }

    // The counterpart nobody installed: derived + fetched, one extra lookup.
    let premium = get("wp-security-audit-log-premium").unwrap_or_else(|| {
        panic!("wp-security-audit-log-premium should resolve to WP Activity Log's icon")
    });
    assert!(
        premium.contains("wp-security-audit-log"),
        "borrowed the wrong plugin's art: {premium}"
    );

    // The derived URL must be a real image, not a plausible string. A letter
    // tile beats a broken <img>, so this is the assertion that would catch a
    // counterpart slug that resolves in the API but has no asset behind it.
    // Retried, because the SUBJECT is "this derived URL is a real image" and the
    // INSTRUMENT is a third-party CDN this very check has just asked for a dozen
    // icons. On 21 Aug 2026 the network tier failed here on `403 Forbidden` and
    // the same URL returned 200 seconds later, from the same machine, with and
    // without a User-Agent — ps.w.org throttling a burst, not a broken
    // derivation. A single-shot assertion against someone else's rate limiter
    // measures the limiter.
    //
    // Three attempts, not "until it works": if the URL is genuinely wrong this
    // still fails, and it says how many times it asked so the next reader can
    // tell a dead link from a throttle.
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .unwrap();
    let mut attempts = 0;
    let resp = loop {
        attempts += 1;
        let r = client
            .get(&premium)
            .send()
            .await
            .unwrap_or_else(|e| panic!("derived icon {premium} is not fetchable: {e}"));
        if r.status().is_success() || attempts == 3 {
            break r;
        }
        println!("  attempt {attempts}: HTTP {} — retrying in 3s", r.status());
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;
    };
    assert!(
        resp.status().is_success(),
        "derived icon {premium} → HTTP {} after {attempts} attempts",
        resp.status()
    );
    let ctype = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    assert!(ctype.starts_with("image/"), "derived icon {premium} is {ctype}, not an image");
    let bytes = resp.bytes().await.expect("icon body");
    assert!(bytes.len() > 500, "derived icon {premium} is {} bytes — not an icon", bytes.len());

    // What must stay a letter tile: a private plugin (no marker at all), and a
    // paid plugin whose free counterpart is `essential-addons-for-elementor-lite`
    // — no suffix rule reaches that, and inventing one would hang someone's
    // logo on a stranger's plugin.
    for slug in ["wpdev-tweaks", "essential-addons-elementor"] {
        assert_eq!(get(slug), None, "{slug} must keep its letter tile");
    }

    println!("wporg_icons_check: OK");
    println!("  betterdocs-pro                 → {:?}", get("betterdocs-pro"));
    println!("  elementor-pro                  → {:?}", get("elementor-pro"));
    println!("  fluentformpro                  → {:?}", get("fluentformpro"));
    println!("  wp-security-audit-log-premium  → {premium} ({} bytes, {ctype})", bytes.len());
    println!("  wpdev-tweaks / essential-addons-elementor → letter tile (None)");
}
