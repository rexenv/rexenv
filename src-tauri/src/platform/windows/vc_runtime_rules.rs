//! Is the Microsoft Visual C++ Redistributable (x64) on this PC — the pure half of
//! `vc_runtime.rs`, so every host proves it. php.net's PHP and Oracle's MySQL link
//! `VCRUNTIME140.dll`; a fresh Windows may not have it, and a user's first site died on it with a
//! bare exit code (8 Oct 2026, ledger #794). This answers BEFORE any start (#798).

use crate::platform::traits::RuntimeProblem;

/// Microsoft's own permanent link to the current x64 redistributable. `words.rs`' loader-failure
/// sentences name the same link (`the_words_name_the_same_download`).
pub const VC_REDIST_URL: &str = "https://aka.ms/vs/17/release/vc_redist.x64.exe";

/// The redistributable's own registry entry (HKLM, 64-bit view): `Installed`, `Major`, `Minor`, `Bld`.
pub const KEY: &str = r"SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\X64";

/// The banner's button.
pub const ACTION: &str = "Download from Microsoft";

/// The oldest runtime every pinned build runs on: 14.30, the first of the Visual Studio 2022
/// toolset that PHP 8.4 and 8.5 (`vs17`) are built with. MySQL and the `vs16` PHPs need less.
pub const MINIMUM: (u32, u32) = (14, 30);

/// What the redistributable's own registry entry says (`VC\Runtimes\X64`): `Installed = 1` and
/// its version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Registered {
    pub major: u32,
    pub minor: u32,
    pub build: u32,
}

/// The verdict from the redistributable's registry entry (`None` = no entry, or `Installed` is
/// not 1) and whether its DLLs are in System32.
///
/// - registered at [`MINIMUM`] or later → fine;
/// - registered but older → a WARNING: MySQL and PHP ≤ 8.3 run, PHP 8.4+ may not;
/// - not registered, but the DLLs are in System32 → fine: another installer can drop them without
///   the entry, and a version this cannot read is not a reason to block (a real failure is still
///   named by `core::proc::exit_text`);
/// - neither → BLOCKING: no database and no PHP can start.
pub fn judge(registered: Option<Registered>, dlls_present: bool) -> Option<RuntimeProblem> {
    match registered {
        Some(r) if (r.major, r.minor) >= MINIMUM => None,
        Some(r) => Some(RuntimeProblem {
            blocking: false,
            message: format!(
                "The Microsoft Visual C++ Redistributable on this PC is version {}.{}.{}; PHP 8.4 and newer \
                 need {}.{} or later and may not start. Install the current one from Microsoft.",
                r.major, r.minor, r.build, MINIMUM.0, MINIMUM.1
            ),
            url: VC_REDIST_URL.to_string(),
            action: ACTION.to_string(),
        }),
        None if dlls_present => None,
        None => Some(RuntimeProblem {
            blocking: true,
            message: "PHP and MySQL need the Microsoft Visual C++ Redistributable (x64), and this PC does not \
                      have it — no database or PHP can start until it is installed. Install it from \
                      Microsoft, then Start all."
                .to_string(),
            url: VC_REDIST_URL.to_string(),
            action: ACTION.to_string(),
        }),
    }
}

/// The DLLs whose presence counts when the registry entry is missing.
pub const DLLS: [&str; 3] = ["vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll"];

#[cfg(test)]
mod tests {
    use super::*;

    fn v(major: u32, minor: u32) -> Option<Registered> {
        Some(Registered { major, minor, build: 1 })
    }

    #[test]
    fn a_current_runtime_is_fine_an_older_one_warns_and_none_blocks() {
        assert_eq!(judge(v(14, 44), false), None);
        assert_eq!(judge(v(14, 30), false), None, "the minimum itself is enough");
        let old = judge(v(14, 29), true).expect("older than the minimum");
        assert!(!old.blocking && old.message.contains("14.29.1") && old.message.contains("14.30"), "{old:?}");
        assert_eq!(judge(None, true), None, "DLLs without the entry: not a reason to block");
        let missing = judge(None, false).expect("nothing at all");
        assert!(missing.blocking && missing.message.contains("no database or PHP can start"), "{missing:?}");
        assert_eq!(missing.url, VC_REDIST_URL);
    }

    /// The installer's offer (`nsis/hooks.nsh`, interactive installs only) asks the same registry
    /// entry and sends the user to the same link — one test, so the two cannot drift.
    #[test]
    fn the_installer_offers_the_same_runtime() {
        let hooks = include_str!("../../../nsis/hooks.nsh");
        // NSIS strings take a backslash literally, so the key is spelled the same in both files.
        assert!(hooks.contains(&format!("!define REXENV_VC_KEY \"{KEY}\"")), "the hook reads another key");
        assert!(hooks.contains(&format!("!define REXENV_VC_URL \"{VC_REDIST_URL}\"")), "the hook opens another link");
        assert!(hooks.contains("vcruntime140_1.dll") && DLLS.contains(&"vcruntime140_1.dll"));
        let post = &hooks[hooks.find("!macro NSIS_HOOK_POSTINSTALL").expect("post-install hook")..];
        let post = &post[..post.find("!macroend").expect("its end")];
        let offer = post.find("REXENV_OFFER_VC_RUNTIME").expect("the post-install hook makes the offer");
        assert!(post[..offer].contains("${Else}"), "the offer is a dialog: interactive installs only, never /S");
    }

    /// The exit-code sentences (`PlatformWords::loader_failures`) and this one send the user to ONE link.
    #[test]
    fn the_words_name_the_same_download() {
        for (_, why) in crate::platform::words::WINDOWS.loader_failures {
            assert!(why.contains(VC_REDIST_URL), "{why}");
        }
    }
}
