//! Live check: resolve the user's login-shell environment + system dev tools
//! (git/node/npm/pnpm/yarn/composer) exactly the way the repo-install feature
//! will. Read-only — spawns only the user's shell and `--version` probes; no
//! services, no app-data writes, no stack-guard concerns.
//! Run: `cargo run --example devtools_check`
//!
//! What to eyeball on a real machine:
//! - PATH should contain the version-manager dirs (nvm/fnm/asdf/Homebrew) a
//!   terminal would have — the whole point over the bare launchd PATH.
//! - node should resolve to the SAME binary `which node` prints in a terminal.
//! - SSH_AUTH_SOCK should be present (private-repo clones ride the agent).

use rexenv_lib::core::devtools;
use rexenv_lib::platform;

fn main() {
    let plat = platform::current();
    println!("SHELL = {}", std::env::var("SHELL").unwrap_or_else(|_| "(unset)".into()));

    let env = match plat.shell().login_shell_env() {
        Ok(env) => env,
        Err(e) => {
            eprintln!("login_shell_env FAILED: {e}");
            std::process::exit(1);
        }
    };
    println!("login-shell env: {} vars", env.len());
    match devtools::env_var(&env, "PATH") {
        Some(path) => {
            println!("PATH ({} entries):", path.split(':').count());
            for dir in path.split(':') {
                println!("  {dir}");
            }
        }
        None => println!("PATH: MISSING (unexpected — login_shell_env guards this)"),
    }
    println!(
        "SSH_AUTH_SOCK = {}",
        devtools::env_var(&env, "SSH_AUTH_SOCK").unwrap_or("(absent — SSH clones would need it)")
    );
    println!();

    // git goes through the preflight (quiet CLT check — no GUI dialog).
    match devtools::resolve_git(&*plat, &env) {
        Ok(t) => println!("git      → {} ({})", t.path.display(), t.version.as_deref().unwrap_or("?")),
        Err(e) => println!("git      → MISSING:\n{e}"),
    }
    match devtools::resolve_node(&env) {
        Ok(t) => println!("node     → {} ({})", t.path.display(), t.version.as_deref().unwrap_or("?")),
        Err(e) => println!("node     → MISSING:\n{e}"),
    }
    for pm in ["npm", "pnpm", "yarn", "bun"] {
        match devtools::find_optional(&env, pm) {
            Some(t) => {
                println!("{pm:<8} → {} ({})", t.path.display(), t.version.as_deref().unwrap_or("?"))
            }
            None => println!("{pm:<8} → not installed (fine — resolved per-repo by lockfile)"),
        }
    }
    // Composer is optional by design: absence falls back to the pinned
    // composer.phar on the site's bundled PHP (lands with the install step).
    match devtools::find_optional(&env, "composer") {
        Some(t) => {
            println!("composer → {} ({})", t.path.display(), t.version.as_deref().unwrap_or("?"))
        }
        None => println!("composer → not installed (fine — bundled composer.phar fallback)"),
    }
}
