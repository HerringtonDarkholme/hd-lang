//! A warm `hd check` trusts a file's stamp (size, modification and change
//! times, inode) only once both times are settled, and then reads no
//! unchanged file (`crates/hd_cache/DESIGN.md` §5.5, #1503). These cases
//! wait for the stamps to settle, so each check below runs on trusted
//! stamps: an edit that keeps the size and the modification time, a file
//! replaced the way `git checkout` replaces it, and a renamed folder are
//! each seen.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, SystemTime};

use hd_testkit::Scratch;

/// Longer than `disk::SETTLE`, so every stamp of the package is trusted.
const SETTLED: Duration = Duration::from_millis(2_200);

const MAIN: &str = "use pkg.a.x.{x}\n\npub fn main() -> void:\n    let v: i32 = x() + price()\n";
const GOOD: &str = "pub fn price() -> i32:\n    return 41\n";
const BAD: &str = "pub fn price() -> i32:\n    return no\n";

/// `hd check` in `root`; whether it found no error.
fn check(root: &Path) -> bool {
    let out = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            crate::shared_objects(root.with_extension("cache")),
        )
        .env_remove("HD_JOBS")
        .current_dir(root)
        .arg("check")
        .output()
        .expect("run hd");
    match out.status.code() {
        Some(0) => true,
        Some(101) => false,
        _ => panic!("{}", String::from_utf8_lossy(&out.stderr)),
    }
}

fn mtime(file: &Path) -> SystemTime {
    std::fs::metadata(file)
        .and_then(|m| m.modified())
        .expect("mtime")
}

fn set_mtime(file: &Path, at: SystemTime) {
    std::fs::File::options()
        .write(true)
        .open(file)
        .and_then(|f| f.set_modified(at))
        .expect("set mtime");
}

#[test]
fn settled_stamps_still_see_same_size_edits_replacements_and_renames() {
    assert_eq!(GOOD.len(), BAD.len());
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "stat-manifest");
    std::fs::create_dir_all(root.join("src/a")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(
        root.join("src/main.hd"),
        format!("use pkg.price.{{price}}\n{MAIN}"),
    )
    .expect("main");
    std::fs::write(root.join("src/a/x.hd"), "pub fn x() -> i32: 1\n").expect("x");
    let price = root.join("src/price.hd");
    std::fs::write(&price, GOOD).expect("price");
    let written = mtime(&price);
    std::thread::sleep(SETTLED);
    assert!(check(&root), "cold");
    assert!(check(&root), "warm, every stamp trusted");

    // Same size, same modification time: only the change time moves.
    std::fs::write(&price, BAD).expect("edit");
    set_mtime(&price, written);
    std::thread::sleep(SETTLED);
    assert!(!check(&root), "an edit that keeps size and mtime");

    // `git checkout` writes a new file and renames it into place: a new
    // inode, though size and modification time match the last run's.
    let next = root.join("src/price.hd.new");
    std::fs::write(&next, GOOD).expect("new");
    set_mtime(&next, written);
    std::fs::rename(&next, &price).expect("replace");
    std::thread::sleep(SETTLED);
    assert!(check(&root), "a replaced file");

    // A renamed folder: its files keep their stamps under new paths.
    std::fs::rename(root.join("src/a"), root.join("src/b")).expect("rename");
    std::thread::sleep(SETTLED);
    assert!(!check(&root), "`pkg.a.x` is gone after the rename");
}
