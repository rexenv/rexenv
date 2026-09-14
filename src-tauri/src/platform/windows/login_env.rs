//! The user's environment for streamed steps on Windows — Composer, git, npm — as a fresh logon
//! would hand it to a new program, read from the registry every time it is asked for (owner
//! ruling 14 Sep 2026, ledger #609).
//!
//! The macOS answer runs the login shell, because a Finder-launched app inherits launchd's bare
//! environment. A Windows app started from Explorer inherits the user's whole environment — but
//! as it was at LOGON: a developer who installs git or Node while rexenv runs, or edits their
//! `Path`, is invisible to rexenv's own copy until it restarts. The registry holds the current
//! answer: `HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment` (the system's) and
//! `HKCU\Environment` (the user's). Some variables a program needs live in neither —
//! `SystemRoot`, `USERPROFILE`, `APPDATA`, `COMPUTERNAME` are set by the logon itself — so the
//! process's own environment is the base the registry is laid over.
//!
//! No Win32 here: `process.rs` reads the keys, this file merges, and the file is compiled into
//! the macOS test build so the rules run in `verify.sh`.

/// One registry environment value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RegValue {
    pub name: String,
    pub data: String,
    /// `REG_EXPAND_SZ`: `%NAME%` references are expanded against the merged environment.
    pub expand: bool,
}

fn find(env: &[(String, String)], name: &str) -> Option<usize> {
    env.iter().position(|(k, _)| k.eq_ignore_ascii_case(name))
}

/// Set `name`, keeping the first spelling of a name already present (Windows names are
/// case-insensitive: `PATH` from the process and `Path` from the registry are one variable).
fn set(env: &mut Vec<(String, String)>, name: &str, value: String) {
    match find(env, name) {
        Some(i) => env[i].1 = value,
        None => env.push((name.to_string(), value)),
    }
}

/// Expand `%NAME%` references against `env`, ignoring case; an unknown name is left as written,
/// as Windows leaves it, and a lone `%` is kept. One pass — an expanded value is not re-read.
pub(crate) fn expand(template: &str, env: &[(String, String)]) -> String {
    let mut out = String::new();
    let mut rest = template;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match find(env, name) {
                    Some(i) => out.push_str(&env[i].1),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The environment a new logon would give a program: `process` as the base, the `system` values
/// over it, then the `user` values over those — except `Path`, where the user's value is APPENDED
/// to the system's, as Windows builds it. In each layer plain values are set before expandable
/// ones, so an expandable value can name a plain one from the same layer. `None` for a layer that
/// could not be read leaves the base as it was for that layer.
pub(crate) fn merge_login_env(
    process: &[(String, String)],
    system: Option<&[RegValue]>,
    user: Option<&[RegValue]>,
) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = Vec::new();
    for (k, v) in process {
        set(&mut env, k, v.clone());
    }
    let mut system_path: Option<String> = None;
    for (layer, is_user) in [(system, false), (user, true)] {
        let Some(values) = layer else { continue };
        let ordered = values.iter().filter(|v| !v.expand).chain(values.iter().filter(|v| v.expand));
        for v in ordered {
            let data = if v.expand { expand(&v.data, &env) } else { v.data.clone() };
            if !v.name.eq_ignore_ascii_case("Path") {
                set(&mut env, &v.name, data);
            } else if !is_user {
                system_path = Some(data.clone());
                set(&mut env, "Path", data);
            } else {
                // Without the system's value, the base is the process's — which at logon was
                // already system + user, so a stale user part may repeat; better than dropping it.
                let base = system_path.clone().or_else(|| find(&env, "Path").map(|i| env[i].1.clone())).unwrap_or_default();
                let joined = match (base.trim_end_matches(';'), data.as_str()) {
                    ("", d) => d.to_string(),
                    (b, "") => b.to_string(),
                    (b, d) => format!("{b};{d}"),
                };
                set(&mut env, "Path", joined);
            }
        }
    }
    env
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reg(name: &str, data: &str, expand: bool) -> RegValue {
        RegValue { name: name.into(), data: data.into(), expand }
    }

    fn get<'a>(env: &'a [(String, String)], name: &str) -> Option<&'a str> {
        find(env, name).map(|i| env[i].1.as_str())
    }

    fn process() -> Vec<(String, String)> {
        vec![
            ("SystemRoot".into(), r"C:\Windows".into()),
            ("USERPROFILE".into(), r"C:\Users\John Smith".into()),
            // The logon-time Path: stale — git was installed after rexenv started.
            ("PATH".into(), r"C:\Windows\system32;C:\Users\John Smith\AppData\Local\old".into()),
            ("TEMP".into(), r"C:\stale-temp".into()),
        ]
    }

    /// The case the ruling is for: a tool installed while rexenv runs is on the Path it hands a
    /// step, the Path is the system's then the user's, and the process's stale copy is gone.
    #[test]
    fn the_path_is_the_systems_then_the_users_read_now_not_the_stale_one() {
        let system = [reg("Path", r"%SystemRoot%\system32;C:\Program Files\Git\cmd;", true)];
        let user = [reg("Path", r"%USERPROFILE%\AppData\Local\Programs\nodejs", true)];
        let env = merge_login_env(&process(), Some(&system), Some(&user));
        assert_eq!(
            get(&env, "path"),
            Some(r"C:\Windows\system32;C:\Program Files\Git\cmd;C:\Users\John Smith\AppData\Local\Programs\nodejs")
        );
        assert_eq!(env.iter().filter(|(k, _)| k.eq_ignore_ascii_case("path")).count(), 1, "PATH and Path are one variable");
    }

    #[test]
    fn logon_only_variables_stay_and_registry_values_win_user_over_system() {
        let system = [reg("COMPOSER_HOME", r"C:\ProgramData\Composer", false), reg("TEMP", r"%SystemRoot%\TEMP", true)];
        let user = [reg("TEMP", r"%USERPROFILE%\AppData\Local\Temp", true), reg("REXENV_NEW", "set after launch", false)];
        let env = merge_login_env(&process(), Some(&system), Some(&user));
        assert_eq!(get(&env, "SystemRoot"), Some(r"C:\Windows"), "a logon-only variable is kept");
        assert_eq!(get(&env, "TEMP"), Some(r"C:\Users\John Smith\AppData\Local\Temp"), "the user's value wins, expanded");
        assert_eq!(get(&env, "COMPOSER_HOME"), Some(r"C:\ProgramData\Composer"));
        assert_eq!(get(&env, "REXENV_NEW"), Some("set after launch"), "a variable set after launch is seen");
    }

    #[test]
    fn an_expandable_value_can_name_a_plain_one_from_its_own_layer() {
        let user = [reg("TOOLS", r"%DEVROOT%\bin", true), reg("DEVROOT", r"D:\dev", false)];
        let env = merge_login_env(&process(), None, Some(&user));
        assert_eq!(get(&env, "TOOLS"), Some(r"D:\dev\bin"));
    }

    #[test]
    fn expansion_leaves_unknown_names_and_lone_percents_as_written() {
        let env = process();
        assert_eq!(expand(r"%systemroot%\x", &env), r"C:\Windows\x", "names ignore case");
        assert_eq!(expand("%NOPE%;100%", &env), "%NOPE%;100%");
        assert_eq!(expand("%%", &env), "%%");
        assert_eq!(expand("plain", &env), "plain");
    }

    /// A layer that could not be read changes nothing for that layer — and without the system's
    /// Path, the user's is appended to what the process had.
    #[test]
    fn an_unreadable_layer_leaves_the_base() {
        assert_eq!(merge_login_env(&process(), None, None), process());
        let user = [reg("Path", r"C:\user\bin", false)];
        let env = merge_login_env(&process(), None, Some(&user));
        assert_eq!(get(&env, "Path"), Some(r"C:\Windows\system32;C:\Users\John Smith\AppData\Local\old;C:\user\bin"));
    }
}
