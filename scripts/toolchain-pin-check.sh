#!/bin/bash
# Is every place that installs Rust taking rust-toolchain.toml's version? (ledger #792)
#
# The pin only holds if nothing routes around it. On 2 Oct 2026 the CI runners moved to
# Rust 1.99 because the workflows said `rustup toolchain install stable`; its clippy
# fired on code untouched since 13 Sep and Windows verify stayed red for four days while
# every dev machine (1.93.1) was green. So this fails when:
#   - rust-toolchain.toml's channel is not an exact X.Y.Z (a channel name floats);
#   - a workflow names a toolchain to rustup (`stable`, `beta`, a version) or uses a
#     setup-rust action, either of which picks a version this file did not;
#   - scripts/linux-check/Dockerfile's default RUST_VERSION differs from the pin.
# Offline, no toolchain needed. Part of verify.sh.
set -uo pipefail
cd "$(dirname "$0")/.."

fail=0
channel="$(sed -n 's/^channel *= *"\(.*\)"/\1/p' rust-toolchain.toml 2>/dev/null)"
if ! printf '%s' "$channel" | grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "toolchain-pin-check: rust-toolchain.toml channel is '${channel}', not an exact X.Y.Z" >&2
  fail=1
fi

# `rustup toolchain install` / `rustup target add` / `rustup component add` with no
# toolchain argument resolve through rust-toolchain.toml; anything naming one does not.
bad="$(grep -nE 'rustup +(toolchain +install|default|override +set|install|update) +[A-Za-z0-9]|rustup +[^ ]* *\+[A-Za-z0-9]|(dtolnay|actions-rs|actions-rust-lang)/(rust-)?toolchain|setup-rust-toolchain' .github/workflows/*.yml)"
if [ -n "$bad" ]; then
  echo "toolchain-pin-check: a workflow picks its own Rust version — install with a bare \`rustup toolchain install\` so rust-toolchain.toml decides:" >&2
  printf '%s\n' "$bad" | sed 's/^/    /' >&2
  fail=1
fi

docker_default="$(sed -n 's/^ARG RUST_VERSION=//p' scripts/linux-check/Dockerfile)"
if [ "$docker_default" != "$channel" ]; then
  echo "toolchain-pin-check: scripts/linux-check/Dockerfile defaults RUST_VERSION=$docker_default, rust-toolchain.toml pins $channel" >&2
  fail=1
fi

[ "$fail" -eq 0 ] && echo "toolchain-pin-check: every Rust install takes $channel from rust-toolchain.toml"
exit "$fail"
