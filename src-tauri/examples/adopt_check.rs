//! Live check: services OUTLIVE the app and the next launch ADOPTS them.
//!
//! Run in two phases (separate processes, like two app sessions):
//!   cargo run --example adopt_check -- phase1   # start MySQL, exit WITHOUT stopping
//!   cargo run --example adopt_check -- phase2   # adopt the survivor, verify, stop
//!
//! Phase 2 asserts: `adopt_startup` finds the surviving mysqld (ownership-gated),
//! status() reports it running with a pid, and `stop_all` actually stops it.

use rexenv_lib::core::db::DbEngine;
use rexenv_lib::core::service_manager::ServiceManager;
use rexenv_lib::platform;

#[tokio::main]
async fn main() {
    // Deliberate real-stack control: this utility exists to adopt/stop the
    // shared stack. Without this, core::stack_guard skips adopted services.
    rexenv_lib::core::stack_guard::allow_real_stack_control();
    let phase = std::env::args().nth(1).unwrap_or_default();
    let plat = platform::current();

    match phase.as_str() {
        "phase1" => {
            let mut mgr = ServiceManager::default();
            mgr.ensure_db(&*plat, DbEngine::Mysql).await.expect("start mysql");
            assert!(DbEngine::Mysql.running(), "mysql should accept connections");
            println!("phase1: mysql up on {} — exiting WITHOUT stop", DbEngine::Mysql.port());
            // Exit without stop_all / Drop cleanup: process::exit skips Drop,
            // exactly like the real app quitting while services run.
            std::process::exit(0);
        }
        "phase2" => {
            assert!(
                DbEngine::Mysql.running(),
                "precondition: phase1's mysqld should still be running"
            );
            let mut mgr = ServiceManager::default();
            let adopted = mgr.adopt_startup(&*plat, &[]);
            println!("phase2: adopted {adopted} service(s)");
            assert!(adopted >= 1, "should adopt at least the surviving mysqld");
            let infos = mgr.status(&*plat, &[]);
            let mysql = infos.iter().find(|i| i.name == "MySQL").expect("MySQL row");
            assert!(mysql.running, "adopted MySQL should report running");
            assert!(mysql.pid.is_some(), "adopted MySQL should have a pid");
            println!("phase2: status shows MySQL running (pid {:?})", mysql.pid);
            mgr.stop_all(&*plat).expect("stop_all");
            // Poll: SIGTERM → mysqld shutdown takes a moment.
            for _ in 0..30 {
                if !DbEngine::Mysql.running() {
                    break;
                }
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            assert!(!DbEngine::Mysql.running(), "stop_all should stop the adopted mysqld");
            println!("✓ adopt_check: survivor adopted, reported running, stopped by stop_all");
        }
        other => panic!("usage: adopt_check <phase1|phase2> (got {other:?})"),
    }
}
