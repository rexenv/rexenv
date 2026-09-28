#!/bin/bash
# The release notes of a tag = its annotated message minus the subject line.
#
#   scripts/tag-notes.sh <vX.Y.Z>     # prints the notes; exits 1 if there are none
#
# Reads the tag from GitHub (`gh api`, so GH_TOKEN or a `gh auth login`), not from a
# local clone: the publish job has no history checked out, and actions/checkout has
# been known to turn an annotated tag into a lightweight one — the message would be
# lost without a word. The repository is $GITHUB_REPOSITORY (set on a runner), else
# rexenv/rexenv.
#
# # Why the notes come from the tag
#
# The tag message is the one text written for a release, by a human, before any
# runner starts (RELEASING.md step 2 already requires it). release.yml used to draft
# every release with a fixed maintainer warning as its body ("Draft until
# PUBLISH-TESTING §A passes …"), and nothing in the publish step replaced it — so
# 0.8.8 and 0.8.9 went public with that warning as their release notes, and the
# website's changelog sync reads the same body. Refusing a lightweight or bodiless
# tag here, in the `versions` job, fails a release before the builds, not after.
set -euo pipefail

TAG="${1:?usage: scripts/tag-notes.sh <vX.Y.Z>}"
REPO="${GITHUB_REPOSITORY:-rexenv/rexenv}"

read -r kind sha < <(gh api "repos/$REPO/git/ref/tags/$TAG" --jq '.object.type + " " + .object.sha') ||
  { echo "tag-notes: $TAG does not exist on $REPO" >&2; exit 1; }
if [ "$kind" != "tag" ]; then
  echo "tag-notes: $TAG is a lightweight tag — it has no message, so the release would have no notes." >&2
  echo "  git tag -d $TAG && git push origin :refs/tags/$TAG" >&2
  echo "  git tag -a $TAG -m \"rexenv ${TAG#v}\" -m \"<what this release means>\" <commit> && git push origin $TAG" >&2
  exit 1
fi

# Subject, blank line, body: keep everything after the first blank line.
notes="$(gh api "repos/$REPO/git/tags/$sha" --jq .message | awk 'body; /^[[:space:]]*$/ { body = 1 }')"
if [ -z "${notes//[[:space:]]/}" ]; then
  echo "tag-notes: $TAG's message is only a subject line — the release would have no notes." >&2
  echo "  Re-tag with a body: git tag -a -f $TAG -m \"rexenv ${TAG#v}\" -m \"<what this release means>\" <commit>" >&2
  exit 1
fi
printf '%s\n' "$notes"
