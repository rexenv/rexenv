//! The page a STOPPED site answers with (v44).
//!
//! # Why this is a real page and not a line of text
//!
//! The first version was `respond "This site is stopped in rexenv…" 503`, and
//! the person who reads it is a developer whose own site just stopped loading.
//! A bare line of monospace on a white page reads like a server that fell over
//! — which is the one thing this response must NOT say, because nothing is
//! broken and the fix is a button they already own. So it is a full page in
//! rexenv's own clothes: the mark they see in the app's sidebar, the app's
//! palette, the site's own hostname, and both ways to start it again.
//!
//! # How it reaches the browser
//!
//! Caddy cannot `respond` with a file, and a Caddyfile string is the wrong home
//! for HTML: every `{` in the CSS would be read as a placeholder. So the page is
//! WRITTEN to the config dir and served by the edge's error handler —
//! `error 503` + `handle_errors { rewrite * /stopped.html; file_server }` —
//! which keeps the 503 status (verified against the pinned Caddy build before
//! this was written: status 503, `Content-Type: text/html`, braces intact).
//!
//! # One page for every stopped site
//!
//! The file is generic and the HOSTNAME is filled in by two lines of script from
//! `location.hostname`, rather than writing one file per stopped site. A file
//! per site would put the site's name in a path on disk for no gain, and would
//! have to be re-written on every rename. With scripting off the page still says
//! everything that matters — the hostname is the one part the reader already
//! knows, since they typed it.

use crate::error::Result;
use crate::platform::traits::Platform;
use std::path::PathBuf;

/// The directory (under the config dir) holding the page. A directory of its
/// own because Caddy's `file_server` roots there: anything else beside it would
/// become reachable at the address of every stopped site.
const DIR: &str = "stopped";
const FILE: &str = "stopped.html";

/// The headline, exported so tests and the live check assert on the SAME string
/// the page renders rather than on a copy of it.
pub const STOPPED_HEADLINE: &str = "This site is stopped";

/// The rexenv brand mark, inlined at build time.
///
/// A build-time read across the crate boundary, deliberately: the page must
/// carry the mark the user sees in the app, and a second copy of the logo — or
/// a link the browser would have to fetch from somewhere — is how the two drift
/// apart. If the asset moves, the BUILD fails, which is the loud failure.
const LOGO_SVG: &str = include_str!("../../../src/assets/rexenv-logo.svg");

/// Render the page. Pure, so its content is unit-testable without a filesystem.
pub fn html() -> String {
    // The logo file carries an XML prolog and an Inkscape comment; inline SVG in
    // HTML wants neither. Cutting at the first `<svg` is exact for this asset
    // and harmless for any replacement that has no prolog.
    let logo = match LOGO_SVG.find("<svg") {
        Some(i) => &LOGO_SVG[i..],
        None => LOGO_SVG,
    };
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{STOPPED_HEADLINE} · rexenv</title>
<style>
  /* rexenv's own tokens, dark-first with a light theme for a light OS — the
     values are the app's (src/styles/tokens.css). Kept small and literal: this
     page must render with no network, no fonts to fetch and no build step. */
  :root {{
    color-scheme: dark light;
    --bg: #0d0e12;
    --surface: #15171d;
    --border: #262a33;
    --text: #e7e9ee;
    --muted: #8a90a0;
    --dim: #6e7681;
    --brand: #6b4ae8;
    --brand-tint: #c9bcff;
    --well: #0b0c10;
    --chip-from: #20232c;
    --chip-to: #13151b;
    --chip-border: #2c303b;
  }}
  @media (prefers-color-scheme: light) {{
    :root {{
      --bg: #f4f5f8;
      --surface: #ffffff;
      --border: #d6dae3;
      --text: #1b1e26;
      --muted: #5c6373;
      --dim: #767d8c;
      --well: #eef0f5;
      --chip-from: #ffffff;
      --chip-to: #eef0f5;
      --chip-border: #d6dae3;
    }}
  }}
  * {{ box-sizing: border-box; }}
  body {{
    margin: 0;
    min-height: 100vh;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 32px;
    background:
      radial-gradient(1100px 520px at 50% -8%, rgba(107, 74, 232, 0.16), transparent 70%),
      var(--bg);
    color: var(--text);
    font: 400 15px/1.55 -apple-system, BlinkMacSystemFont, "Segoe UI", Inter, system-ui, sans-serif;
    -webkit-font-smoothing: antialiased;
  }}
  main {{ width: 100%; max-width: 560px; text-align: center; }}
  .mark {{
    width: 62px; height: 62px; margin: 0 auto 20px;
    display: flex; align-items: center; justify-content: center;
    border: 1px solid var(--chip-border); border-radius: 16px;
    background: linear-gradient(160deg, var(--chip-from), var(--chip-to));
    box-shadow: 0 10px 30px rgba(107, 74, 232, 0.18);
  }}
  .mark svg {{ width: 32px; height: 32px; display: block; }}
  .brand {{
    font-size: 0.8125rem; letter-spacing: 0.14em; text-transform: uppercase;
    color: var(--muted); margin-bottom: 22px;
  }}
  h1 {{ font-size: 1.75rem; line-height: 1.25; margin: 0 0 10px; letter-spacing: -0.015em; }}
  .host {{
    display: inline-block; margin: 0 0 18px;
    font-family: ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    font-size: 0.875rem; color: var(--brand-tint);
    background: var(--well); border: 1px solid var(--border);
    border-radius: 8px; padding: 5px 11px;
  }}
  p {{ margin: 0 auto 22px; max-width: 460px; color: var(--muted); }}
  .card {{
    text-align: left; background: var(--surface); border: 1px solid var(--border);
    border-radius: 14px; padding: 18px 20px;
  }}
  .card h2 {{
    margin: 0 0 12px; font-size: 0.6875rem; letter-spacing: 0.12em;
    text-transform: uppercase; color: var(--dim); font-weight: 600;
  }}
  ol {{ margin: 0; padding-left: 20px; }}
  li {{ margin-bottom: 9px; color: var(--text); }}
  li:last-child {{ margin-bottom: 0; }}
  code {{
    font-family: ui-monospace, "JetBrains Mono", SFMono-Regular, Menlo, monospace;
    font-size: 0.8125rem; background: var(--well); border: 1px solid var(--border);
    border-radius: 6px; padding: 2px 7px; color: var(--text);
  }}
  .foot {{ margin-top: 20px; font-size: 0.78125rem; color: var(--dim); }}
</style>
</head>
<body>
<main>
  <div class="mark">{logo}</div>
  <div class="brand">rexenv</div>
  <h1>{STOPPED_HEADLINE}</h1>
  <div class="host" id="host">this site</div>
  <p>
    Nothing is broken. You stopped this one site in rexenv, so it is served by
    nothing — every other site on this machine is still running.
  </p>
  <div class="card">
    <h2>Start it again</h2>
    <ol>
      <li>Open rexenv, find the site in <strong>Sites</strong>, and choose <strong>Start site</strong>.</li>
      <li>Or, in a terminal: <code id="cli">rex site start</code></li>
    </ol>
  </div>
  <div class="foot">Served by rexenv on this machine · HTTP 503</div>
</main>
<script>
  // The hostname is the ONE per-site fact on this page, filled in here so a
  // single file can serve every stopped site. With scripting off the fallbacks
  // above still read correctly.
  var h = location.hostname;
  if (h) {{
    document.getElementById("host").textContent = h;
    document.getElementById("cli").textContent = "rex site start " + h;
  }}
</script>
</body>
</html>
"##
    )
}

/// Write the page under the config dir and return the DIRECTORY Caddy roots at.
///
/// Rewritten on every config rebuild rather than only when missing: the page is
/// generated, so a rexenv update that changes its wording must reach a machine
/// whose file was written by the previous version — and "only if absent" is how
/// that update silently never lands.
pub fn ensure(platform: &dyn Platform) -> Result<PathBuf> {
    let dir = platform.paths().config_dir()?.join(DIR);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(FILE), html())?;
    Ok(dir)
}

/// The path Caddy's error handler rewrites to, relative to [`ensure`]'s dir.
pub fn request_path() -> String {
    format!("/{FILE}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The page says the three things a stopped site has to say.**
    ///
    /// Not a snapshot test — the wording will change and should be free to. The
    /// assertions are the load-bearing content: that nothing is broken (the
    /// reader's first assumption, and the reason a bare 503 was wrong), that the
    /// OTHER sites are unaffected (the fear this feature creates), and both ways
    /// back — the app's own words for the menu item, and the CLI verb.
    #[test]
    fn the_stopped_page_says_it_is_deliberate_and_how_to_undo_it() {
        let page = html();
        assert!(page.contains(STOPPED_HEADLINE));
        assert!(page.contains("Nothing is broken"), "the page must not read as a crash");
        assert!(
            page.contains("every other site"),
            "the page must say the rest of the machine is unaffected"
        );
        assert!(page.contains("Start site"), "the app's own menu wording");
        assert!(page.contains("rex site start"), "the CLI's verb");
    }

    /// **It renders standalone: rexenv's mark, rexenv's palette, no network.**
    ///
    /// A page served to a browser that cannot reach anything (the site it wanted
    /// is down, and this is a local edge) must fetch NOTHING: a webfont or a
    /// remote logo would render as a flash of nothing on the one screen whose
    /// job is to look deliberate. So the mark is inline SVG and every URL in the
    /// document must be same-document.
    #[test]
    fn the_page_fetches_nothing_and_carries_the_brand_mark_inline() {
        let page = html();
        assert!(page.contains("<svg"), "the brand mark must be inline, not a link");
        // Namespace URIs inside the inlined SVG are not fetches, so the check is
        // on what a browser would actually REQUEST: a src, an href, or a CSS url().
        for fetch in ["src=\"http", "href=\"http", "url(http", "@import"] {
            assert!(!page.contains(fetch), "the page would fetch something remote: {fetch}");
        }
        assert!(!page.contains("<link"), "no stylesheet may be fetched");
        assert!(page.contains("#6b4ae8"), "rexenv's brand violet (tokens.css)");
        assert!(page.contains("prefers-color-scheme"), "a light-mode reader gets a light page");
        // The CSS survived `format!` — a literal brace bug would produce a page
        // whose rules are gone and whose braces are missing, which still LOOKS
        // like HTML in a test that only greps for words.
        assert!(page.contains("body {"), "the stylesheet's braces did not survive formatting");
    }
}
