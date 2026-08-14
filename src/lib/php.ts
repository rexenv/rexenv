/** PHP-version presentation helpers.
 *
 *  The FACTS live in core (`core::php::eol_since`, derived from php.net's
 *  published security-support end dates against today). Nothing here decides
 *  whether a version is dead — these only turn the date core sent into words.
 *  rexenv already paid for the other arrangement: the Xdebug support rule was
 *  hand-copied into the UI as a version literal, free to disagree with core. */

/** "2023-11-26" → "November 2023". Month + year is the honest resolution: the
 *  exact day matters to nobody choosing a runtime, and a full date invites the
 *  reader to check whether it is right rather than to read what it means. */
export function eolWhen(eolSince: string): string {
  const [y, m] = eolSince.split("-");
  const month = new Date(Number(y), Number(m) - 1, 1).toLocaleString(undefined, { month: "long" });
  return `${month} ${y}`;
}

/** The short tag next to a version — for a `<select>` option, where markup
 *  cannot go. Deliberately not "old" or "legacy": the fact is that security
 *  fixes stopped. */
export function eolTag(eolSince: string | null): string {
  return eolSince ? " — end of life" : "";
}

/** The sentence in front of the button that starts it (docs/DESIGN.md).
 *
 *  `wordpress` adds what the user WILL see and would otherwise report as a
 *  rexenv bug: WordPress checks the running PHP against its own support data
 *  and puts a dashboard notice plus a Site Health critical on every site below
 *  the recommended version. Saying it first is the difference between an
 *  informed choice and a surprise. */
export function eolNote(minor: string, eolSince: string, opts?: { wordpress?: boolean }): string {
  const base = `PHP ${minor} stopped receiving upstream security fixes in ${eolWhen(eolSince)}. It still runs — use it to work on a legacy project, not to build a new one.`;
  return opts?.wordpress
    ? `${base} WordPress will show its own "outdated PHP" notice and a Site Health critical on sites using it.`
    : base;
}
