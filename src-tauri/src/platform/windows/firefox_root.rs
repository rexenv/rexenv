//! Where Firefox keeps this user's profiles on Windows (ledger #612). Pure file-system rule, compiled
//! into the macOS test build so it runs in `verify.sh` too.

use std::path::{Path, PathBuf};

/// `<Roaming AppData>\Mozilla\Firefox`, when it holds a `profiles.ini` FILE — Firefox creates both on
/// its first run, so without one there is no Firefox to configure for this user. Measured on the Dell
/// (Windows 10, Firefox 105, 14 Sep 2026): `%APPDATA%\Mozilla\Firefox\profiles.ini`, its profile
/// `IsRelative=1` under `Profiles/`. The Microsoft Store build keeps a virtualized copy under
/// `%LOCALAPPDATA%\Packages`, which this does not look in — none was on the Dell to measure.
pub(crate) fn profiles_root_in(roaming_app_data: &Path) -> Option<PathBuf> {
    let root = roaming_app_data.join("Mozilla").join("Firefox");
    root.join("profiles.ini").is_file().then_some(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_root_is_roaming_mozilla_firefox_and_only_with_a_profiles_ini_file() {
        let roaming = std::env::temp_dir().join("rexenv-firefox-root Roaming");
        let _ = std::fs::remove_dir_all(&roaming);
        let root = roaming.join("Mozilla").join("Firefox");
        std::fs::create_dir_all(&root).unwrap();
        assert_eq!(profiles_root_in(&roaming), None, "a Firefox folder with no profiles.ini is no Firefox");
        std::fs::create_dir_all(root.join("profiles.ini")).unwrap();
        assert_eq!(profiles_root_in(&roaming), None, "a directory named profiles.ini is not the file");
        std::fs::remove_dir(root.join("profiles.ini")).unwrap();
        std::fs::write(root.join("profiles.ini"), "[General]\r\nVersion=2\r\n").unwrap();
        assert_eq!(profiles_root_in(&roaming), Some(root));
        let _ = std::fs::remove_dir_all(&roaming);
    }
}
