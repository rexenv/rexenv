#!/bin/bash
# The receipt that binds a COMMIT to a recorded green verdict.
#
# WHY THIS EXISTS
#
# 3 Aug 2026: `live-checks.sh ... | tail -3 && git commit` landed a commit on a
# RED tier, because a pipe replaces the script's exit code with `tail`'s. The
# documented rule — "verify.sh is the bar; never a piped check" — was walked
# into by the person who wrote it, in the session it was written in. A rule that
# relies on memory is not a control.
#
# Half of that is already fixed: all three verdict-bearing scripts refuse to run
# with stdout piped. But the `&&` chain still reaches `git commit`, now after a
# refusal rather than after a false green. This is the other half — the commit
# itself asks whether a green verdict covers this exact tree.
#
# WHAT THE FINGERPRINT IS
#
# The CONTENT of every code file, not a diff against HEAD and not the output of
# `git status`. Both of those were considered and both are wrong here:
#
#   - `git status --porcelain` lists paths and status letters. Editing a file
#     that is ALREADY modified leaves that output byte-identical, so a changed
#     file would commit under a stale green. Status alone is not a content hash.
#   - a diff against HEAD moves when HEAD moves, so `git commit --amend` — which
#     changes nothing about the code — would fail every time. An override you
#     need routinely is not an override, it is the new habit, and the guard is
#     then dead.
#
# Hashing content is stable across both: amend passes because the code did not
# change, and any edit at all changes the hash.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
# The checkout's OWN git dir, not "$ROOT/.git": in a `git worktree`, `.git` is a file
# pointing elsewhere, and the hardcoded path made verify.sh fail at its last step
# and the pre-commit hook unpassable there (found cutting 0.7.1 from a worktree,
# 13 Sep 2026). Per-worktree is also right: the fingerprint is of THIS checkout.
RECEIPT="$(git -C "$ROOT" rev-parse --absolute-git-dir)/rexenv-verify-receipt"

# The paths the bar actually covers. `scripts/` is included deliberately: a
# change to verify.sh changes what "green" MEANS, so it must invalidate the
# receipt like any other code change. `docs/` is not — doc-only work is not
# gated, which is what keeps this from being in the way.
CODE_PATHS=(src src-tauri/src src-tauri/examples cli/src scripts)

fingerprint() {
  cd "$ROOT"
  # CONTENT ONLY — deliberately no `git status`, and this is the one thing that
  # took a real failure to get right. A first version mixed `git status
  # --porcelain` in to catch deletions, and it made `git add` change the
  # fingerprint: staging flips the status letters (`?? f` → `A  f`) without
  # touching a byte of code, so EVERY commit was blocked and `--no-verify`
  # would have become the habit within a day. An override you need routinely is
  # not an override.
  #
  # Deletions are caught anyway: a tracked file removed from the working tree
  # is still listed by `git ls-files` but cannot be hashed, so its line leaves
  # the input and the hash moves. A `git rm` removes it from `ls-files`, which
  # moves the hash too.
  #
  # The `sort` is load-bearing for the same reason, and was the second half of
  # the same bug: `git add` moves a file from the untracked list to the tracked
  # one, so the two streams below change ORDER without changing a byte. Sorting
  # the per-file hashes makes the fingerprint a property of the CONTENT SET
  # rather than of how git happens to enumerate it.
  {
    # `-z` + `xargs -0` so a path with a space is hashed rather than silently
    # skipped, and so this is a handful of processes rather than one per file.
    git ls-files -z -- "${CODE_PATHS[@]}"
    git ls-files -z --others --exclude-standard -- "${CODE_PATHS[@]}"
  } | { xargs -0 shasum 2>/dev/null || true; } | sort | shasum | cut -d' ' -f1
}

case "${1:-}" in
  fingerprint)
    fingerprint
    ;;
  write)
    # Called by verify.sh with the fingerprint taken BEFORE the run. If the tree
    # changed while the bar was running, the thing that passed is not the thing
    # on disk, and no receipt is written — see verify.sh's caller.
    printf 'green %s %s\n' "$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo none)" "${2:?fingerprint}" \
      > "$RECEIPT"
    ;;
  check)
    if [ ! -f "$RECEIPT" ]; then
      echo "no receipt — verify.sh has not passed on this clone since the last commit"
      exit 1
    fi
    recorded="$(cut -d' ' -f3 "$RECEIPT")"
    if [ "$recorded" != "$(fingerprint)" ]; then
      echo "the code changed since verify.sh last passed"
      exit 1
    fi
    ;;
  path)
    echo "$RECEIPT"
    ;;
  *)
    echo "usage: verify-receipt.sh fingerprint|write <fp>|check|path" >&2
    exit 2
    ;;
esac
