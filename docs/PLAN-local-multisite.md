# Import Local multisite networks — Q2 of the Local import

**Status:** IN FLIGHT 12 Sep 2026 — owner: "Q2 o kore felo multisite". T0 (this plan),
T1 (scan + adopt, ledger #580), T2 (the network URL pass, #573) and T3 (the connect moves
`DOMAIN_CURRENT_SITE`, #581) and T4 (L1 leg C on a real network — which measured the
override's `DOMAIN_CURRENT_SITE` pin unnecessary and removed it) done; T5 PASSED
owner-run 13 Sep 2026 on `multisite.local` (`docs/PUBLISH-TESTING.md` §N, N10–N14) after
three fixes its first attempt found (#586, #587, #588). SHIPPED — moves to
`docs/archive/` with the next reconcile.
Planned against `626b685`. Parent design record: `docs/archive/PLAN-local-import.md`
(§3 listed networks as unsupported, §9 Q2 parked them).

## 1. What a Local network looks like (measured on the owner's Mac)

`~/Library/Application Support/Local/sites.json`, site `multi`:

```json
"multiSite": "ms-subdomain",
"multiSiteDomains": ["http://multi.local/", "http://ea1.multi.local/", "http://ea2.multi.local/"]
```

Its `app/public/wp-config.php` (read by hand for this plan — the scan never opens it):

```php
define( 'WP_ALLOW_MULTISITE', true );
define( 'MULTISITE', true );
define( 'SUBDOMAIN_INSTALL', true );
define( 'DOMAIN_CURRENT_SITE', 'multi.local' );
define( 'PATH_CURRENT_SITE', '/' );
define( 'SITE_ID_CURRENT_SITE', 1 );
define( 'BLOG_ID_CURRENT_SITE', 1 );
```

`multiSite` is `ms-subdir` for a subdirectory network, `""` for a single site. The
network was halted at planning time (no mysqld socket), so the live leg needs the
owner to start it in Local.

## 2. What rexenv already has, and what blocks adoption (research, 12 Sep 2026)

Already there — nothing to build:
- `sites.multisite` (`MultisiteMode { None, Subdomain, Subdirectory }`, schema v3) drives
  the three templates: both network modes carry WordPress's network rewrites;
  subdomain adds `*.domain` to nginx `server_name` and the Caddy addresses.
- Every site's certificate already carries `*.domain`; the embedded DNS answers every
  name under a TLD, so `ea1.multi.rex` resolves and is covered.
- The WordPress tab's network UI keys off the column (`isNetwork`).

The blockers:
1. **No way to set the mode without converting.** Every setter runs
   `wp core multisite-convert` first; `NewSite` has no mode. An adopted network left at
   `none` is served as a single site (subsites 404 / unanswered) AND is offered
   "Convert to multisite" — which would run convert on a live network.
2. **The URL pass can't move a network.** The scheme pairs match only the network's
   own name, so `http://ea1.multi.local` becomes `http://ea1.multi.rex` (the bare pair
   renames it, the scheme never moves). And every wp-cli run boots with the project's
   `DOMAIN_CURRENT_SITE = 'multi.local'`: after the bare pass rewrites `wp_site` /
   `wp_blogs`, bootstrap can't find the network, so the `siteurl` re-check fails.
3. **Serving needs `DOMAIN_CURRENT_SITE` moved in their wp-config.** With it left at
   `multi.local`, WordPress builds the network's cookie domain and network URLs from
   it — logins at `multi.rex` fail. That file is theirs: the ONLY sanctioned write is
   the connection rewrite (preview → fingerprint → backup → verify → revert), whose
   `RewriteKey` vocabulary has no such key.

## 3. Design

- **Adopt, never convert.** A Local network row is Importable. The candidate carries
  its `multisite` mode (from the registry) and its re-homed subsite names (display).
  After the ordinary provision succeeds, the import records the mode with a
  dedicated core function whose contract is "record what is already on disk — never
  run convert", then the batch's single config reload (today's alias reload) also
  runs when a network was imported.
- **Refused, with the reason:** a subsite on a MAPPED domain (a `multiSiteDomains`
  host that is neither the network's domain nor under it). rexenv would have to serve
  an unrelated name as an extra domain and re-home it separately — a later cut.
- **The URL pass learns networks** (rexenv's copy only, as #573):
  - Before any replace, list the copy's blog domains (`wp site list`); a mapped
    domain there fails the job before a byte is written.
  - Each subsite's `http(s)://sub.old` pairs run first, then the network's pairs,
    bare network name LAST (it renames `wp_blogs.domain`, `wp_site.domain`, and every
    `wp_N_options` row — substrings, so `ea1.multi.local` → `ea1.multi.rex`).
  - ~~The override file also defines `DOMAIN_CURRENT_SITE`: the old name during the
    replaces, the new one for the re-check.~~ **Measured wrong in T4:** with the pin
    removed the real network still moved; with `--url` removed from the proof it failed
    ("Site 'multi.local/' not found"). The proof runs with `--url=https://<new>/` and the
    override keeps only the four connection constants.
  - Proof: the network's `siteurl` AND every blog's URL must read `https://…` on the
    new name, or the job fails (Retry re-imports).
- **Connect moves the network's domain.** `RewriteKey::NetworkDomain`
  (`DOMAIN_CURRENT_SITE`, wp-config only — a `.env` has no such key), staged when the
  site is a network and the file's value differs from the site's domain. Same
  charset check, same diff-is-the-write, same byte-identical revert. `PATH_CURRENT_SITE`
  never changes (re-homing moves the name, not the path).
- **Say what it costs.** The connect preview flags that the network's domain moves:
  while connected, Local's own `multi.local` stops loading the network (the folder is
  shared — it already serves rexenv's copy while connected); Revert brings it back.

Not in this cut: mapped-domain subsites; Change domain on networks (still refused);
Valet/Herd networks (see §6).

## 4. Task list (one commit each)

- **T0** — this plan; TODO row out of Parked into Now; the Valet/Herd finding as its own row.
- **T1** — scan + adopt: Local network rows importable (mapped domains refused);
  `multisite` + `subsites` on the row (Rust, TS, MCP JSON); the Import row says it's a
  network and lists the subsites; the import records the mode without converting and
  the batch reload covers it. L0 + ledger row.
- **T2** — the network-aware URL pass (pairs, override `DOMAIN_CURRENT_SITE`, blog list
  guard, every-blog proof). L0 + ledger #573 extended.
- **T3** — `RewriteKey::NetworkDomain` + preview flag + the DbImportCard/MCP wording.
  L0 (confedit bytes, resolve staging) + ledger row.
- **T4** — L1: `local_import_check` gains a network leg (a fixture subdomain network
  with a subsite, re-homed on rexenv's engine, every blog proved; the connect plan's
  bytes on its wp-config copy).
- **T5** — ARCHITECTURE, PUBLISH-TESTING §N network steps, TESTING; the live pass on
  `multi.local` (owner starts it in Local).

## 5. Invariants this adds

- An import never runs `multisite-convert` / `multisite-install`; the mode is recorded
  from the source's own registry.
- The network URL pass writes only rexenv's copy and fails the job unless every blog
  reads `https://` on its new name.
- `DOMAIN_CURRENT_SITE` is written only by the connection rewrite, only for a network,
  and reverts byte-identically.

## 6. Found while researching (not this plan's to fix)

A Valet or Herd **network imports silently as a single site**: nothing in
`core/valet.rs` or `commands/valet_import.rs` detects multisite, and the scan's
never-open-a-project-file rule (#570's Valet twin) means it can't read `MULTISITE` from
wp-config at scan time. Its own TODO row. **Fixed 13 Sep 2026** by reading it at IMPORT
time instead (as text, like the database import's own read), plus a Convert guard that
records an on-disk network rather than converting it — ledger #591.
