fn main() {
    // The `rex` sidecar (bundle.externalBin) must EXIST before tauri_build
    // validates the config — otherwise a fresh clone's bare `cargo build` /
    // `cargo test` dies before compiling a line. `tauri dev`/`tauri build`
    // stage it via beforeDevCommand/beforeBuildCommand anyway; this covers
    // every other entry point. The cli crate has its own target dir, so the
    // nested cargo can't deadlock this build; after the first run it's a
    // cache hit.
    #[cfg(target_os = "macos")]
    if !std::path::Path::new("binaries/rex-aarch64-apple-darwin").exists() {
        let ok = std::process::Command::new("sh")
            .arg("../scripts/build-cli.sh")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            println!("cargo:warning=could not stage the rex CLI sidecar (scripts/build-cli.sh)");
        }
    }
    tauri_build::build()
}
