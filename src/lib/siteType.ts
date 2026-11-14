import type { Site } from "@/types";

/**
 * Per-site-type avatar tokens: a letter on a tinted square.
 * WordPress = blue, Laravel = red, Blank-PHP = purple (per the design comps).
 */
export const SITE_TYPE_META: Record<
  string,
  { letter: string; bg: string; color: string; border: string }
> = {
  wordpress: { letter: "W", bg: "var(--rex-accent-blue-bg)", color: "var(--rex-accent-blue)", border: "var(--rex-accent-blue-border)" },
  laravel: { letter: "L", bg: "var(--rex-accent-red-bg)", color: "var(--rex-accent-red)", border: "var(--rex-accent-red-border)" },
  php: { letter: "P", bg: "var(--rex-accent-periwinkle-bg)", color: "var(--rex-accent-periwinkle)", border: "var(--rex-accent-periwinkle-border)" },
};

export function siteTypeMeta(type: string) {
  return SITE_TYPE_META[type] ?? SITE_TYPE_META.php;
}

/** **THE scratch predicate — the recorded fact, and nothing else.**
 *
 * `origin === "agent"` is written at creation and is the only thing any policy
 * reads (#204). The `.scratch.<tld>` suffix is UX so a human can scan the list;
 * a site the USER hand-created at `foo.scratch.rex` is an ordinary site of
 * theirs and must appear in their own group. Matching on the name here would be
 * the obvious shortcut and would quietly put someone's real site in the
 * agent's disposable section. Pinned on both sides: `Site::is_scratch` in Rust
 * (`a_stored_origin_that_isnt_exactly_agent_reads_as_the_users_site`) and the L2
 * `scratch-rows` probe on the Sites list, whose fixture includes a KEPT site and a
 * hand-named `mine.scratch.rex` — both `origin: "user"`, both ending in
 * `.scratch.rex`, so the suffix shortcut fails the probe by name.
 */
export const isScratch = (s: Pick<Site, "origin">) => s.origin === "agent";
