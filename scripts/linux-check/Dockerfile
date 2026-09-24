# The Ubuntu image `scripts/linux-check.sh` lints the tree in — docs/PLAN-linux-port.md L0.
#
# Why a container and not a cross-compile: Tauri on Linux links against webkit2gtk, gtk3
# and libsoup through pkg-config, so a `--target x86_64-unknown-linux-gnu` from the Mac
# stops at the first `-sys` crate's build script for want of a sysroot. cargo-xwin could
# fetch Microsoft's SDK for Windows; nothing packages a GTK sysroot for a Mac host the
# same way. Ubuntu 22.04 is the floor rexenv supports on Linux (the first LTS that ships
# libwebkit2gtk-4.1, which Tauri 2 requires), so the check compiles against the OLDEST
# library set a user can have, never a newer one.
#
# The toolchain is pinned to the version the Mac runs (`rustc --version` on the dev
# machine, 24 Sep 2026) so a lint difference is a Linux difference, not a rustc one.
FROM ubuntu:22.04
ARG RUST_VERSION=1.93.1
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
      build-essential curl ca-certificates pkg-config file git \
      libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev \
      libssl-dev libxdo-dev libsoup-3.0-dev libjavascriptcoregtk-4.1-dev \
    && rm -rf /var/lib/apt/lists/*
ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --profile minimal \
      --default-toolchain "$RUST_VERSION" --component clippy \
    && chmod -R a+w /usr/local/rustup /usr/local/cargo
WORKDIR /work
