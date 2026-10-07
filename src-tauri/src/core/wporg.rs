//! core::wporg — WordPress.org directory search for the Add-plugin/theme flows
//! (P2-2/P2-3): users don't know slugs, so the UI offers wp-admin-style live
//! search and fills the slug from the picked result. Plain HTTPS GETs against
//! `api.wordpress.org` with a hard client timeout (never a forever spinner);
//! offline/API failure surfaces an honest error and the manual slug field
//! keeps working.

use crate::error::{Error, Result};

/// One plugin search hit (mirrors the frontend `WpOrgPlugin`).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpOrgPlugin {
    pub slug: String,
    pub name: String,
    /// Plain text (the API returns HTML like `<a href=…>Author</a>`).
    pub author: String,
    /// 0–100 (WordPress.org scale; ÷20 for stars).
    pub rating: f64,
    pub num_ratings: u64,
    pub active_installs: u64,
    /// Best available icon URL (svg → 2x → 1x → default), if any.
    pub icon: Option<String>,
    pub short_description: String,
}

/// Shared client: hard 10s timeout so a dead network can't hang the UI.
fn client() -> &'static reqwest::Client {
    static CLIENT: std::sync::OnceLock<reqwest::Client> = std::sync::OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            // core.svn.wordpress.org answers 403 to a request with NO User-Agent (measured
            // 8 Oct 2026: the same URL 200 with any UA) — `release_file` failed on it first.
            .user_agent(concat!("rexenv/", env!("CARGO_PKG_VERSION")))
            .build()
            .expect("wporg client")
    })
}

fn friendly(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Other("WordPress.org didn't answer within 10s — check your connection.".into())
    } else {
        Error::Other(format!("WordPress.org search failed: {e}"))
    }
}

/// Search the WordPress.org PLUGIN directory (the same API wp-admin's
/// "Add Plugin" screen uses). Returns up to 10 hits, relevance-ordered.
pub async fn search_plugins(query: &str) -> Result<Vec<WpOrgPlugin>> {
    let resp = client()
        .get("https://api.wordpress.org/plugins/info/1.2/")
        .query(&[
            ("action", "query_plugins"),
            ("request[search]", query),
            ("request[per_page]", "10"),
            ("request[fields][icons]", "1"),
            ("request[fields][short_description]", "1"),
            ("request[fields][active_installs]", "1"),
        ])
        .send()
        .await
        .map_err(friendly)?;
    let body: serde_json::Value = resp.json().await.map_err(friendly)?;
    Ok(parse_plugins(&body))
}

/// Best icon URL from a plugin object's `icons` map (svg → 2x → 1x → default).
fn best_icon(obj: &serde_json::Value) -> Option<String> {
    let icons = obj.get("icons");
    ["svg", "2x", "1x", "default"]
        .iter()
        .find_map(|k| icons?.get(k)?.as_str().map(str::to_string))
}

/// Icon URLs for INSTALLED plugins (the plugin list shows the same icons the
/// live search does). One `plugin_information` GET per slug, all concurrent,
/// with a process-lifetime cache — a slug is asked of wp.org at most once per
/// app run. Non-wp.org plugins (custom, mu, drop-ins) and any fetch failure
/// resolve to `None` (letter-tile fallback in the UI); the map is total over
/// the input, and this function never fails.
///
/// # Why a paid plugin borrows its free counterpart's art
///
/// wp-admin shows an icon for BetterDocs Pro, Elementor Pro, Rank Math Pro —
/// none of which exist in the wp.org directory. It reads those from the
/// `update_plugins` transient (`update-core.php`: `$plugin_data->update->icons`),
/// which the vendor's own updater fills in. **rexenv cannot read that**, and the
/// measurement is the reason this function derives instead: on a real 47-plugin
/// site (18 Aug 2026) the transient STORED in the database carried no premium
/// row at all, and a `wp eval` with every plugin loaded produced exactly one of
/// the nine installed premium plugins — the other eight inject their update data
/// on an `is_admin()` request, which no wp-cli run is. Faking `WP_ADMIN` to reach
/// them means running eight vendors' admin-only code paths (license HTTP calls
/// included) to decorate a list.
///
/// So a slug carrying a premium marker falls back to its FREE counterpart's
/// wp.org icon (`betterdocs-pro` → `betterdocs`, `fluentformpro` →
/// `fluentform`, `wp-security-audit-log-premium` → `wp-security-audit-log`).
/// That is the same artwork wp-admin ends up showing for them: the one premium
/// row rexenv could read pointed its `icons` at `ps.w.org/nelio-content/…`,
/// the free plugin's assets. It is a derivation and it is kept narrow —
/// [`free_counterpart`] refuses anything but a real premium marker, and a
/// counterpart wp.org has never heard of leaves the letter tile alone rather
/// than borrowing a stranger's logo.
pub async fn plugin_icons(slugs: &[String]) -> std::collections::HashMap<String, Option<String>> {
    let mut out = fetch_icons(slugs).await;

    // Only for slugs wp.org itself had nothing for: a premium plugin that IS in
    // the directory keeps its own art.
    let pairs = premium_pairs(&out);
    if pairs.is_empty() {
        return out;
    }
    // A free counterpart installed alongside its paid add-on — the usual case,
    // because the add-on needs it — was already fetched above, so the common
    // shape costs no extra request at all.
    let unasked: Vec<String> = pairs
        .iter()
        .map(|(_, base)| base.clone())
        .filter(|base| !out.contains_key(base))
        .collect();
    let extra = fetch_icons(&unasked).await;
    for (slug, icon) in premium_fills(&pairs, &out, &extra) {
        out.insert(slug, Some(icon));
    }
    out
}

/// The raw wp.org lookup behind [`plugin_icons`]: one `plugin_information` GET
/// per slug, concurrent, process-lifetime cached, total over the input.
async fn fetch_icons(slugs: &[String]) -> std::collections::HashMap<String, Option<String>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);

    let mut out = std::collections::HashMap::new();
    // A set, not a `Vec` + `dedup()`: `dedup` drops only ADJACENT repeats, so an
    // unsorted list with the same slug twice spawned the same GET twice.
    let mut missing: std::collections::HashSet<String> = std::collections::HashSet::new();
    {
        let cached = cache.lock().expect("wporg icon cache");
        for slug in slugs {
            match cached.get(slug) {
                Some(icon) => {
                    out.insert(slug.clone(), icon.clone());
                }
                None => {
                    missing.insert(slug.clone());
                }
            }
        }
    }

    let mut set = tokio::task::JoinSet::new();
    for slug in missing {
        set.spawn(async move {
            let icon = async {
                let resp = client()
                    .get("https://api.wordpress.org/plugins/info/1.2/")
                    .query(&[
                        ("action", "plugin_information"),
                        ("request[slug]", slug.as_str()),
                        ("request[fields][icons]", "1"),
                    ])
                    .send()
                    .await
                    .ok()?;
                let body: serde_json::Value = resp.json().await.ok()?;
                best_icon(&body)
            }
            .await;
            (slug, icon)
        });
    }
    while let Some(joined) = set.join_next().await {
        let Ok((slug, icon)) = joined else { continue };
        cache
            .lock()
            .expect("wporg icon cache")
            .insert(slug.clone(), icon.clone());
        out.insert(slug, icon);
    }
    out
}

/// Slug endings that mean "the paid build of another plugin". Deliberately
/// short: every extra marker is another way to hang the WRONG plugin's logo on
/// someone's private plugin, and `-pro`/`-premium` is what the ecosystem
/// actually ships. `-plus`, `-agency`, `-business` and friends stay out until a
/// real plugin needs one.
const PREMIUM_MARKERS: [&str; 2] = ["premium", "pro"];

/// The free counterpart's slug for a paid plugin's slug, if the slug carries a
/// premium marker. The separator is OPTIONAL — `betterdocs-pro` is one real
/// plugin's directory name and `fluentformpro` is another's.
///
/// `None` when what is left is too short to be a plugin slug (`ab-pro`): a
/// two-letter lookup is a coin flip, not a derivation.
fn free_counterpart(slug: &str) -> Option<String> {
    let base = PREMIUM_MARKERS.iter().find_map(|m| slug.strip_suffix(m))?;
    let base = base.trim_end_matches(['-', '_', '.']);
    (base.len() >= 3).then(|| base.to_string())
}

/// (paid slug, free counterpart) for every slug wp.org had no icon for.
/// Sorted, because a `HashMap`'s order is arbitrary and the fetch list this
/// drives — and the test that reads it — should not be.
fn premium_pairs(icons: &std::collections::HashMap<String, Option<String>>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = icons
        .iter()
        .filter(|(_, icon)| icon.is_none())
        .filter_map(|(slug, _)| free_counterpart(slug).map(|base| (slug.clone(), base)))
        .collect();
    pairs.sort();
    pairs
}

/// The icons to write back, from whichever map holds the counterpart. A
/// counterpart wp.org does not know — or that has no icon of its own — yields
/// nothing, so that row keeps its letter tile.
fn premium_fills(
    pairs: &[(String, String)],
    asked: &std::collections::HashMap<String, Option<String>>,
    extra: &std::collections::HashMap<String, Option<String>>,
) -> Vec<(String, String)> {
    pairs
        .iter()
        .filter_map(|(slug, base)| {
            let icon = asked.get(base).or_else(|| extra.get(base))?.clone()?;
            Some((slug.clone(), icon))
        })
        .collect()
}

/// Pure parser (unit-tested against a captured API shape). Skips malformed
/// entries instead of failing the whole search.
fn parse_plugins(body: &serde_json::Value) -> Vec<WpOrgPlugin> {
    let Some(items) = body.get("plugins").and_then(|p| p.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|p| {
            let slug = p.get("slug")?.as_str()?.to_string();
            let icon = best_icon(p);
            Some(WpOrgPlugin {
                slug,
                name: decode_entities(p.get("name").and_then(|v| v.as_str()).unwrap_or("")),
                author: decode_entities(&strip_tags(
                    p.get("author").and_then(|v| v.as_str()).unwrap_or(""),
                )),
                rating: p.get("rating").and_then(|v| v.as_f64()).unwrap_or(0.0),
                num_ratings: p.get("num_ratings").and_then(|v| v.as_u64()).unwrap_or(0),
                active_installs: p
                    .get("active_installs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                icon,
                short_description: decode_entities(
                    p.get("short_description").and_then(|v| v.as_str()).unwrap_or(""),
                ),
            })
        })
        .collect()
}

/// One theme search hit (mirrors the frontend `WpOrgTheme`). Themes have a
/// SCREENSHOT (4:3 preview) instead of an icon.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WpOrgTheme {
    pub slug: String,
    pub name: String,
    pub author: String,
    /// 0–100 (÷20 for stars).
    pub rating: f64,
    pub num_ratings: u64,
    pub active_installs: u64,
    pub screenshot: Option<String>,
}

/// Search the WordPress.org THEME directory (wp-admin "Add Theme" API).
pub async fn search_themes(query: &str) -> Result<Vec<WpOrgTheme>> {
    let resp = client()
        .get("https://api.wordpress.org/themes/info/1.2/")
        .query(&[
            ("action", "query_themes"),
            ("request[search]", query),
            ("request[per_page]", "10"),
            ("request[fields][screenshot_url]", "1"),
            ("request[fields][rating]", "1"),
            ("request[fields][active_installs]", "1"),
        ])
        .send()
        .await
        .map_err(friendly)?;
    let body: serde_json::Value = resp.json().await.map_err(friendly)?;
    Ok(parse_themes(&body))
}

/// Pure parser. The themes API returns `author` as either a plain nicename
/// string or an object with `display_name` — handle both. Protocol-relative
/// screenshot URLs (`//ts.w.org/…`) are normalized to https.
fn parse_themes(body: &serde_json::Value) -> Vec<WpOrgTheme> {
    let Some(items) = body.get("themes").and_then(|t| t.as_array()) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|t| {
            let slug = t.get("slug")?.as_str()?.to_string();
            let author = match t.get("author") {
                Some(serde_json::Value::String(s)) => s.clone(),
                Some(obj) => obj
                    .get("display_name")
                    .and_then(|v| v.as_str())
                    .or_else(|| obj.get("user_nicename").and_then(|v| v.as_str()))
                    .unwrap_or("")
                    .to_string(),
                None => String::new(),
            };
            let screenshot = t
                .get("screenshot_url")
                .and_then(|v| v.as_str())
                .map(|u| {
                    if let Some(rest) = u.strip_prefix("//") {
                        format!("https://{rest}")
                    } else {
                        u.to_string()
                    }
                });
            Some(WpOrgTheme {
                slug,
                name: decode_entities(t.get("name").and_then(|v| v.as_str()).unwrap_or("")),
                author: decode_entities(&strip_tags(&author)),
                rating: t.get("rating").and_then(|v| v.as_f64()).unwrap_or(0.0),
                num_ratings: t.get("num_ratings").and_then(|v| v.as_u64()).unwrap_or(0),
                active_installs: t
                    .get("active_installs")
                    .and_then(|v| v.as_u64())
                    .unwrap_or(0),
                screenshot,
            })
        })
        .collect()
}

/// Drop `<tag>`s (the author field is an anchor).
pub(crate) fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// Decode the handful of HTML entities the directory actually emits in
/// names/descriptions — display text only, not an HTML parser.
pub(crate) fn decode_entities(s: &str) -> String {
    let mut out = s
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#8211;", "–")
        .replace("&#8212;", "—")
        .replace("&#8216;", "'")
        .replace("&#8217;", "'")
        .replace("&#8220;", "\u{201C}")
        .replace("&#8221;", "\u{201D}");
    if out.contains("&nbsp;") {
        out = out.replace("&nbsp;", " ");
    }
    out
}

/// Every file a WordPress release ships, with its MD5, from wordpress.org's checksums API —
/// the list [`crate::core::wordpress::cut_name_casualties`] checks a docroot against and
/// [`crate::core::wordpress::restore_release_file`] verifies a download with. A release's list
/// never changes, so each version is asked at most once per app run (a site screen opened
/// twice and `rex doctor` over ten sites on the same version cost one GET). File NAMES are the
/// same in every locale, so en_US answers for all of them.
pub async fn core_file_list(version: &str) -> Result<std::sync::Arc<FileList>> {
    type Cache = std::sync::Mutex<std::collections::HashMap<String, std::sync::Arc<FileList>>>;
    static CACHE: std::sync::OnceLock<Cache> = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(version).cloned()) {
        return Ok(hit);
    }
    let body: serde_json::Value = client()
        .get("https://api.wordpress.org/core/checksums/1.0/")
        .query(&[("version", version), ("locale", "en_US")])
        .send()
        .await
        .map_err(|e| fetch_failed("checksums lookup", e))?
        .json()
        .await
        .map_err(|e| fetch_failed("checksums lookup", e))?;
    let list = std::sync::Arc::new(parse_core_file_list(&body, version)?);
    if let Ok(mut c) = cache.lock() {
        c.insert(version.to_string(), list.clone());
    }
    Ok(list)
}

/// Release path → MD5 hex.
pub type FileList = std::collections::BTreeMap<String, String>;

fn fetch_failed(what: &str, e: reqwest::Error) -> Error {
    Error::Other(if e.is_timeout() {
        "WordPress.org didn't answer within 10s — check your connection.".into()
    } else {
        format!("WordPress.org {what} failed: {e}")
    })
}

/// `{"checksums": {path: md5}}` — or `{"checksums": false}` for a version wordpress.org does
/// not know, which is an error: an empty list would read as "nothing is missing".
fn parse_core_file_list(body: &serde_json::Value, version: &str) -> Result<FileList> {
    match body.get("checksums").and_then(|c| c.as_object()) {
        Some(map) if !map.is_empty() => Ok(map
            .iter()
            .filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string())))
            .collect()),
        _ => Err(Error::Other(format!("WordPress.org has no file list for WordPress {version}"))),
    }
}

/// One file of a WordPress release, as built — from the release's tag in core's SVN mirror,
/// which serves each file at its release path (measured 8 Oct 2026: a 7.1.2 theme font
/// answered 200 with exactly the MD5 the checksums list names). One small GET per file beats
/// a 30 MB release zip for the handful a cut tarball lost. The caller verifies the MD5.
pub async fn release_file(version: &str, rel: &str) -> Result<Vec<u8>> {
    let safe = |c: char| c.is_ascii_alphanumeric() || "._-/".contains(c);
    if !crate::core::wordpress::valid_release_version(version) || !rel.chars().all(safe) || rel.contains("..") {
        return Err(Error::Other(format!("not fetching {rel:?} for WordPress {version:?}")));
    }
    let resp = client()
        .get(format!("https://core.svn.wordpress.org/tags/{version}/{rel}"))
        .send()
        .await
        .map_err(|e| fetch_failed("download", e))?;
    if !resp.status().is_success() {
        return Err(Error::Other(format!("WordPress.org answered {} for {rel}", resp.status())));
    }
    Ok(resp.bytes().await.map_err(|e| fetch_failed("download", e))?.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_unknown_version_is_an_error_never_an_empty_list() {
        let ok = serde_json::json!({"checksums": {"wp-load.php": "abc", "wp-includes/version.php": "def"}});
        let got = parse_core_file_list(&ok, "7.1").unwrap();
        assert_eq!(got.keys().collect::<Vec<_>>(), ["wp-includes/version.php", "wp-load.php"]);
        assert_eq!(got["wp-load.php"], "abc");
        for bad in [serde_json::json!({"checksums": false}), serde_json::json!({"checksums": {}}), serde_json::json!({})] {
            assert!(parse_core_file_list(&bad, "9.9").is_err(), "{bad}");
        }
    }

    #[test]
    fn strips_tags_and_decodes_entities() {
        assert_eq!(
            strip_tags("<a href=\"https://x\">Jane &amp; Co</a>"),
            "Jane &amp; Co"
        );
        assert_eq!(decode_entities("Jane &amp; Co &#8211; SEO"), "Jane & Co – SEO");
    }

    #[test]
    fn parses_the_query_plugins_shape() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"info":{"page":1},"plugins":[
                {"name":"Yoast SEO &#8211; fast","slug":"wordpress-seo",
                 "author":"<a href=\"https://yoa.st\">Team Yoast</a>",
                 "rating":92,"num_ratings":27000,"active_installs":10000000,
                 "short_description":"SEO plugin.",
                 "icons":{"1x":"https://ps.w.org/x/icon-128.png","2x":"https://ps.w.org/x/icon-256.png"}},
                {"slug":"minimal"},
                {"name":"no slug — skipped"}
            ]}"#,
        )
        .unwrap();
        let got = parse_plugins(&body);
        assert_eq!(got.len(), 2, "malformed entry skipped, minimal kept");
        let y = &got[0];
        assert_eq!(y.slug, "wordpress-seo");
        assert_eq!(y.name, "Yoast SEO – fast");
        assert_eq!(y.author, "Team Yoast");
        assert_eq!(y.rating, 92.0);
        assert_eq!(y.active_installs, 10_000_000);
        // svg absent → 2x preferred over 1x.
        assert_eq!(y.icon.as_deref(), Some("https://ps.w.org/x/icon-256.png"));
        // Minimal entry: defaults, no icon.
        assert_eq!(got[1].slug, "minimal");
        assert!(got[1].icon.is_none() && got[1].rating == 0.0);
    }

    /// The derivation itself: what counts as "the paid build of X", and what
    /// deliberately does not. `essential-addons-elementor` is the honest miss —
    /// its free counterpart is `essential-addons-for-elementor-lite`, which no
    /// suffix rule reaches, so that row keeps its letter tile.
    #[test]
    fn derives_the_free_counterpart_of_a_paid_slug() {
        for (paid, free) in [
            ("betterdocs-pro", "betterdocs"),
            ("betterlinks-pro", "betterlinks"),
            ("wp-analytify-pro", "wp-analytify"),
            ("seo-by-rank-math-pro", "seo-by-rank-math"),
            // No separator at all — one real plugin's directory name.
            ("fluentformpro", "fluentform"),
            ("nelio-content-premium", "nelio-content"),
            ("wp-security-audit-log-premium", "wp-security-audit-log"),
            ("wordpress-seo-premium", "wordpress-seo"),
        ] {
            assert_eq!(free_counterpart(paid).as_deref(), Some(free), "{paid}");
        }
        for plain in [
            "elementor",
            "wp-rocket",
            "query-monitor",
            "essential-addons-elementor",
            // Nothing left to look up, or too little to be a slug.
            "pro",
            "premium",
            "ab-pro",
        ] {
            assert_eq!(free_counterpart(plain), None, "{plain} is not a paid companion slug");
        }
    }

    /// The fill: a paid row borrows art ONLY from a counterpart that actually
    /// has some. `elementor-pro` here is the case that must stay a letter tile —
    /// its counterpart WAS asked and wp.org had no icon, so there is nothing to
    /// borrow and nothing to invent.
    #[test]
    fn a_paid_row_borrows_only_from_a_counterpart_that_has_an_icon() {
        let icons: std::collections::HashMap<String, Option<String>> = [
            ("betterdocs", Some("https://ps.w.org/betterdocs/icon-256x256.png")),
            ("betterdocs-pro", None),
            ("elementor", None),
            ("elementor-pro", None),
            ("wp-rocket", None),
            ("wp-security-audit-log-premium", None),
        ]
        .into_iter()
        .map(|(s, i)| (s.to_string(), i.map(str::to_string)))
        .collect();

        let pairs = premium_pairs(&icons);
        assert_eq!(
            pairs,
            vec![
                ("betterdocs-pro".to_string(), "betterdocs".to_string()),
                ("elementor-pro".to_string(), "elementor".to_string()),
                (
                    "wp-security-audit-log-premium".to_string(),
                    "wp-security-audit-log".to_string()
                ),
            ],
            "wp-rocket carries no premium marker, so it is never derived from"
        );

        // Only the counterpart nobody installed costs a second request.
        let extra: std::collections::HashMap<String, Option<String>> = [(
            "wp-security-audit-log".to_string(),
            Some("https://ps.w.org/wp-security-audit-log/icon-256x256.png".to_string()),
        )]
        .into_iter()
        .collect();

        assert_eq!(
            premium_fills(&pairs, &icons, &extra),
            vec![
                (
                    "betterdocs-pro".to_string(),
                    "https://ps.w.org/betterdocs/icon-256x256.png".to_string()
                ),
                (
                    "wp-security-audit-log-premium".to_string(),
                    "https://ps.w.org/wp-security-audit-log/icon-256x256.png".to_string()
                ),
            ]
        );
    }

    #[test]
    fn missing_plugins_key_is_empty_not_error() {
        let body: serde_json::Value = serde_json::from_str(r#"{"error":"down"}"#).unwrap();
        assert!(parse_plugins(&body).is_empty());
        assert!(parse_themes(&body).is_empty());
    }

    #[test]
    fn parses_the_query_themes_shape() {
        let body: serde_json::Value = serde_json::from_str(
            r#"{"themes":[
                {"name":"Astra","slug":"astra",
                 "author":{"user_nicename":"brainstormforce","display_name":"Brainstorm Force"},
                 "rating":98,"num_ratings":5000,"active_installs":1000000,
                 "screenshot_url":"//ts.w.org/wp-content/themes/astra/screenshot.jpg"},
                {"name":"Old Style","slug":"oldstyle","author":"someone"}
            ]}"#,
        )
        .unwrap();
        let got = parse_themes(&body);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].slug, "astra");
        assert_eq!(got[0].author, "Brainstorm Force");
        // Protocol-relative screenshot normalized to https.
        assert_eq!(
            got[0].screenshot.as_deref(),
            Some("https://ts.w.org/wp-content/themes/astra/screenshot.jpg")
        );
        // String-author variant handled too.
        assert_eq!(got[1].author, "someone");
        assert!(got[1].screenshot.is_none());
    }
}
