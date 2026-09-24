//! The pure half of the Linux process and socket reads — `ss` rows and `/proc/<pid>/stat`
//! fields parsed as TEXT, so the macOS host proves them in `verify.sh` (docs/PLAN-linux-port.md
//! L1). The reads themselves (`/proc` walks, spawning `ss`) live in `mod.rs`.
//!
//! Why `ss` and not `lsof`: Ubuntu Server does not install `lsof`, every Ubuntu has iproute2,
//! and `ss -p` prints the owning pid for this user's sockets in one call. `-H` drops the
//! header, `-n` keeps ports numeric, `-t`/`-u` pick the family, `-l` listeners only.

/// One socket row of `ss -H…np`: the LOCAL port and every pid `ss` could attribute it to.
/// The pids are empty for a socket another user owns — `ss` prints no `users:` column then.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SocketRow {
    pub local_port: u16,
    /// `(process name, pid)` pairs from `users:(("nginx",pid=12,fd=6),…)`.
    pub users: Vec<(String, u32)>,
}

/// Parse the rows of `ss -Hltnp` / `ss -Hlunp` / `ss -Htnp state established`.
///
/// Column layout differs by one between the listener form (`State Recv-Q Send-Q Local Peer
/// Process`) and the `state established` form, which DROPS the State column. Rather than
/// count columns, the local address is found as the first field that ends in `:<port>` —
/// both `127.0.0.1:18088` and `[::]:18088` and `*:18088` do.
pub(crate) fn parse_ss(output: &str) -> Vec<SocketRow> {
    output.lines().filter_map(parse_ss_line).collect()
}

fn parse_ss_line(line: &str) -> Option<SocketRow> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let local_port = line.split_whitespace().find_map(local_port_of)?;
    let users = match line.find("users:(") {
        Some(i) => parse_users(&line[i + "users:(".len()..]),
        None => Vec::new(),
    };
    Some(SocketRow { local_port, users })
}

/// The port of an `addr:port` token; `None` for `*`, `0.0.0.0:*` and anything else.
fn local_port_of(token: &str) -> Option<u16> {
    let (_, port) = token.rsplit_once(':')?;
    port.parse().ok()
}

/// `("nginx",pid=1234,fd=6),("nginx",pid=1235,fd=6))` → the pairs.
fn parse_users(s: &str) -> Vec<(String, u32)> {
    let mut out = Vec::new();
    let mut rest = s;
    while let Some(start) = rest.find("(\"") {
        let after = &rest[start + 2..];
        let Some(name_end) = after.find('"') else { break };
        let name = after[..name_end].to_string();
        let tail = &after[name_end..];
        let pid = tail
            .find("pid=")
            .and_then(|i| tail[i + 4..].split(|c: char| !c.is_ascii_digit()).next())
            .and_then(|d| d.parse::<u32>().ok());
        if let Some(pid) = pid {
            out.push((name, pid));
        }
        let Some(close) = tail.find(')') else { break };
        rest = &tail[close + 1..];
    }
    out
}

/// The fields of `/proc/<pid>/stat` rexenv reads: the state letter and the parent pid.
///
/// The second field, `comm`, is in parentheses and may itself contain spaces and parentheses
/// (`(php-fpm: master process (/x/y.conf))` is real), so the split is at the LAST `)`, never the
/// first — a `split_whitespace` reads the wrong column for every php-fpm master.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StatFields {
    pub state: char,
    pub ppid: u32,
}

pub(crate) fn parse_stat(stat: &str) -> Option<StatFields> {
    let rest = &stat[stat.rfind(')')? + 1..];
    let mut it = rest.split_whitespace();
    let state = it.next()?.chars().next()?;
    let ppid = it.next()?.parse().ok()?;
    Some(StatFields { state, ppid })
}

/// Field 22 of `/proc/<pid>/stat`, `starttime` — clock ticks since boot, the process's identity
/// beyond its pid. Fields are counted AFTER the `)` that ends `comm` (state is field 3).
pub(crate) fn start_time_of(stat: &str) -> Option<String> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(22 - 3).map(str::to_string)
}

/// `/proc/<pid>/cmdline` is NUL-separated; the space-joined form is what `pid_command`
/// promises. A trailing NUL is the normal ending, not an empty last argument.
pub(crate) fn cmdline_to_string(raw: &[u8]) -> Option<String> {
    let s = String::from_utf8_lossy(raw);
    let joined = s.trim_end_matches('\0').split('\0').collect::<Vec<_>>().join(" ");
    (!joined.trim().is_empty()).then_some(joined)
}

/// The copy-paste line that frees a port on Linux: `fuser` (psmisc, on every Ubuntu) kills
/// every process bound to the port, master and workers alike, so nothing is left to respawn a
/// worker into the socket. `sudo` because a root edge is the usual holder.
pub(crate) fn free_port_command(port: u16, udp: bool) -> String {
    format!("sudo fuser -k {port}/{}", if udp { "udp" } else { "tcp" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listener_rows_yield_the_local_port_and_every_attributed_pid() {
        let out = "LISTEN 0 511 127.0.0.1:18088 0.0.0.0:* users:((\"nginx\",pid=1235,fd=6),(\"nginx\",pid=1234,fd=6))\n\
                   LISTEN 0 4096 [::]:443 [::]:*\n\
                   LISTEN 0 128 *:15353 *:* users:((\"rexenv\",pid=77,fd=9))\n";
        let rows = parse_ss(out);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].local_port, 18088);
        assert_eq!(rows[0].users, vec![("nginx".to_string(), 1235), ("nginx".to_string(), 1234)]);
        assert_eq!(rows[1].local_port, 443);
        assert!(rows[1].users.is_empty(), "a root socket has no users column for a plain user");
        assert_eq!(rows[2].local_port, 15353);
        assert_eq!(rows[2].users, vec![("rexenv".to_string(), 77)]);
    }

    /// `state established` drops the State column; the port is still found.
    #[test]
    fn established_rows_without_a_state_column_parse_too() {
        let out = "0 0 127.0.0.1:9783 127.0.0.1:51234 users:((\"php-fpm8.3\",pid=9,fd=3))\n";
        let rows = parse_ss(out);
        assert_eq!(rows[0].local_port, 9783);
        assert_eq!(rows[0].users[0].1, 9);
    }

    #[test]
    fn a_wildcard_peer_never_reads_as_a_port() {
        assert_eq!(local_port_of("0.0.0.0:*"), None);
        assert_eq!(local_port_of("*"), None);
        assert_eq!(local_port_of("[::1]:53"), Some(53));
    }

    /// A php-fpm master's comm carries spaces and parentheses; the split is at the LAST `)`.
    #[test]
    fn stat_is_split_at_the_last_paren() {
        let s = "4242 (php-fpm: master process (/home/u/.local/share/rexenv/config/php-fpm-8.3.conf)) S 1 4242 4242 0 -1";
        assert_eq!(parse_stat(s), Some(StatFields { state: 'S', ppid: 1 }));
        assert_eq!(parse_stat("9 (nginx) Z 8 9 9"), Some(StatFields { state: 'Z', ppid: 8 }));
        assert_eq!(parse_stat("garbage"), None);
    }

    #[test]
    fn the_start_time_is_field_22_counted_past_the_comm() {
        // pid (comm) state ppid pgrp session tty tpgid flags minflt cminflt majflt cmajflt utime
        // stime cutime cstime priority nice num_threads itrealvalue starttime …
        let s = "4242 (php-fpm: master (x)) S 1 4242 4242 0 -1 4194560 100 0 0 0 5 3 0 0 20 0 3 0 987654 12345 0";
        assert_eq!(start_time_of(s), Some("987654".into()));
        assert_eq!(start_time_of("9 (x) S 1"), None);
    }

    #[test]
    fn cmdline_joins_on_spaces_and_drops_the_trailing_nul() {
        assert_eq!(cmdline_to_string(b"/usr/bin/x\0--flag\0v\0"), Some("/usr/bin/x --flag v".into()));
        assert_eq!(cmdline_to_string(b""), None, "a kernel thread has no command line");
    }

    #[test]
    fn the_free_command_names_the_port_and_the_family() {
        assert_eq!(free_port_command(443, false), "sudo fuser -k 443/tcp");
        assert_eq!(free_port_command(15353, true), "sudo fuser -k 15353/udp");
    }
}
