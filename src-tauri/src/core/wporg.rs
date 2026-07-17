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
pub async fn plugin_icons(slugs: &[String]) -> std::collections::HashMap<String, Option<String>> {
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, Option<String>>>,
    > = std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);

    let mut out = std::collections::HashMap::new();
    let mut missing: Vec<String> = Vec::new();
    {
        let cached = cache.lock().expect("wporg icon cache");
        for slug in slugs {
            match cached.get(slug) {
                Some(icon) => {
                    out.insert(slug.clone(), icon.clone());
                }
                None => missing.push(slug.clone()),
            }
        }
    }
    missing.dedup();

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

#[cfg(test)]
mod tests {
    use super::*;

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
