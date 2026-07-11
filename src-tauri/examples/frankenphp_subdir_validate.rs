//! M5 live check: the generated FrankenPHP **subdirectory-multisite** Caddyfile is
//! syntactically valid and provisions cleanly (the branch was never exercised before
//! M5). Prints the real `generate_config` output; pipe it to `frankenphp validate`:
//!
//!   cargo run --example frankenphp_subdir_validate > /tmp/fp.Caddyfile
//!   "<bin_dir>/frankenphp-<ver>/frankenphp" validate --config /tmp/fp.Caddyfile --adapter caddyfile
//!
//! Expected: `Valid configuration`. (A full WP request test needs a live :443 network.)

use rexenv_lib::core::frankenphp;
use rexenv_lib::core::services::RewriteMode;
use std::path::Path;

fn main() {
    let cfg = frankenphp::generate_config(
        Path::new("/tmp/fptest"),
        8200,
        RewriteMode::SubdirectoryMultisite,
        &[],
    );
    print!("{cfg}");
}
