//! Not shipped with `ehai` — a disposable helper binary used only by
//! `tests/self_replace_flow.rs` to exercise the real `self_replace`-based
//! `swap_in`/`rollback` path without ever touching the shared build artifact
//! or the real `ehai` binary (see `lib.rs`'s module doc for why that matters).
//!
//! Usage: `update_test_harness swap <new-content-file>` | `rollback` | `version`

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("version") => println!("v1"),
        Some("swap") => {
            let new_content = std::fs::read(&args[2]).expect("read new content");
            let exe = std::env::current_exe().expect("current_exe");
            ehai_update::swap_in(&new_content, &exe).expect("swap_in");
            println!("swapped");
        }
        Some("rollback") => {
            let exe = std::env::current_exe().expect("current_exe");
            ehai_update::rollback(&exe).expect("rollback");
            println!("rolled back");
        }
        _ => {
            eprintln!("usage: update_test_harness swap <file> | rollback | version");
            std::process::exit(2);
        }
    }
}
