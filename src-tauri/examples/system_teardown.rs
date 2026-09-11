//! Reverse the batched system setup (task 3.4): remove the `.test` resolver file
//! and untrust the local CA. Run: `cargo run --example system_teardown`.

use rexenv_lib::core::setup;
use rexenv_lib::platform;
use rexenv_lib::state::db;

fn main() {
    let plat = platform::current();
    // The real app database: teardown consults it for resolver files we
    // BORROWED from Valet/Herd, which are restored rather than removed.
    let conn = match db::open_for_platform(plat.paths()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("could not open the app database: {e}");
            return;
        }
    };
    // Teardown takes the database itself and locks it per step, never across its
    // prompts (#569).
    match setup::run_system_teardown(&std::sync::Mutex::new(conn), &*plat) {
        Ok(r) => println!(
            "system teardown OK — removed {:?}, restored {:?}, left alone {:?}, backup missing {:?}; CA untrusted",
            r.removed, r.restored, r.left_alone, r.backup_missing
        ),
        Err(e) => eprintln!("system teardown failed: {e}"),
    }
}
