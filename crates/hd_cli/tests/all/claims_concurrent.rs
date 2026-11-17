//! Several `hd check` commands over one reused cache, round after round
//! (#2018). Each round edits every module, so every command misses each
//! module's `check` entry together with the others, and claims or waits
//! for it. A module of many bodies checks them on the pool, whose join
//! lets the worker run another module's check while the first one's claim
//! is held: that task must not wait for another command's claim. Before
//! #2018 a debug `hd` panicked there ("waiting for a claim while holding
//! ..."), and a release one let the held claim go.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hd_testkit::Scratch;

/// Commands per round, modules per package, functions per module (past
/// the 32 bodies at which a module checks its bodies in parallel), rounds.
const COMMANDS: usize = 4;
const MODULES: usize = 16;
const FUNCTIONS: usize = 200;
const ROUNDS: usize = 8;

/// Writes the package `round` into `dir`: every module's text names the
/// round, so its `check` key is new.
fn write_package(dir: &Path, round: usize) {
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"claims\"\n").expect("hd.toml");
    let mut lib = String::new();
    for m in 0..MODULES {
        let mut text = String::new();
        for f in 0..FUNCTIONS {
            let _ = write!(
                text,
                "pub fn f{m}_{f}(x: i32) -> i32:\n    return x * {round} + {f}\n\n"
            );
        }
        std::fs::write(dir.join(format!("src/m{m}.hd")), text).expect("module");
        let _ = writeln!(lib, "use pkg.m{m}.f{m}_0");
    }
    lib.push_str("\npub fn total() -> i32:\n    return ");
    let calls: Vec<String> = (0..MODULES).map(|m| format!("f{m}_0({m})")).collect();
    lib.push_str(&calls.join(" + "));
    lib.push('\n');
    std::fs::write(dir.join("src/lib.hd"), lib).expect("lib");
}

#[test]
fn concurrent_checks_over_one_reused_cache_never_wait_inside_a_claim() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "claims-concurrent");
    let cache = root.join("cache");
    let dirs: Vec<PathBuf> = (0..COMMANDS).map(|i| root.join(format!("p{i}"))).collect();
    for round in 0..ROUNDS {
        for dir in &dirs {
            write_package(dir, round);
        }
        // cost: COMMANDS child processes per round, run at once.
        let children: Vec<_> = dirs
            .iter()
            .map(|dir| {
                hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
                    .env("HD_CACHE", &cache)
                    .env_remove("HD_JOBS")
                    .current_dir(dir)
                    .arg("check")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .expect("run hd")
            })
            .collect();
        for child in children {
            let out = child.wait_with_output().expect("hd output");
            assert!(
                out.status.success(),
                "round {round}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
    }
}
