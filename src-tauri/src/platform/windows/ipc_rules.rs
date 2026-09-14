//! The pure half of Windows' local IPC (ledger #611): what a failed AF_UNIX connect means.
//!
//! No Win32 here: `af_unix.rs` makes the calls, this file names their errors, and it is compiled
//! into the macOS test build so the rule runs in `verify.sh`.

/// Winsock's "connection refused".
pub(crate) const WSAECONNREFUSED: i32 = 10061;

/// The `std::io::ErrorKind` a failed AF_UNIX `connect` reports, as `LocalIpc::connect` promises:
/// `NotFound` when there is no socket file, `ConnectionRefused` when a file is there and nothing
/// listens. Windows answers WSAECONNREFUSED for BOTH (Caddy's own Windows reuse check says so, and
/// relies on it), so the file's existence is what tells them apart — asked by the caller, which has
/// the path.
pub(crate) fn connect_error_kind(code: i32, socket_file_exists: bool) -> std::io::ErrorKind {
    match (code, socket_file_exists) {
        (_, false) => std::io::ErrorKind::NotFound,
        (WSAECONNREFUSED, true) => std::io::ErrorKind::ConnectionRefused,
        _ => std::io::ErrorKind::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::ErrorKind;

    /// `dbsource::probe_socket` words its answer by these two kinds, and `proxy::admin_alive` only
    /// needs "not Ok" — so the distinction must survive Windows answering one code for both.
    #[test]
    fn no_file_is_not_found_and_a_file_nobody_serves_is_refused() {
        assert_eq!(connect_error_kind(WSAECONNREFUSED, false), ErrorKind::NotFound);
        assert_eq!(connect_error_kind(WSAECONNREFUSED, true), ErrorKind::ConnectionRefused);
        assert_eq!(connect_error_kind(10013, true), ErrorKind::Other, "access denied is neither");
        assert_eq!(connect_error_kind(10013, false), ErrorKind::NotFound);
    }
}
