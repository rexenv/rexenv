# Security policy

rexenv manages real system state: a root LaunchDaemon serving `:443`,
`/etc/resolver/*` files, a locally-trusted certificate authority, per-user
LaunchAgents, a private CLI socket, and on-demand public exposure of local
sites through Cloudflare quick tunnels. A vulnerability here can matter beyond
the app itself, so please report privately first.

## Reporting

- **Preferred:** GitHub's private vulnerability reporting on this repository
  (Security → "Report a vulnerability").
- **Email fallback:** rudlinkon@gmail.com — subject starting `[rexenv security]`.

**Please do not open a public issue or PR for a suspected vulnerability before
reporting it privately.** No response-time SLA is promised — this is a
maintainer-run project — but reports are read and acknowledged.

Helpful in a report: the build/commit (`Settings → About` or `rex version`),
macOS version, what you did, what happened, and why you believe it crosses a
trust boundary (the boundaries we defend are described in
`docs/ARCHITECTURE.md` and the invariant inventory in `docs/CLAIM-LEDGER.md`).

## Scope notes

- The app is currently ad-hoc signed (not notarized); the Homebrew cask's
  quarantine-stripping postflight is deliberate and documented — that design
  itself is not a finding, but escapes it enables would be.
- Fixed historical findings are published in `docs/archive/FINDINGS.md` and
  `docs/archive/CODEBASE-REVIEW.md`; there has been no public release carrying
  the pre-fix code.

## Supported versions

Pre-1.0: the latest `master` only.
