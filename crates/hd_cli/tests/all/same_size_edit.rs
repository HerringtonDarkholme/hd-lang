//! A warm `hd build` sees an edit that keeps a file's size and its
//! modification time (#49): a file is never trusted by its metadata alone,
//! so the build is the clean build of the edited text, byte for byte.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

const MAIN: &str =
    "use pkg.price.price\n\npub fn main() -> void $ Console:\n    println(price())\n";

fn package(name: &str, price: &str) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(root.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("hd.toml");
    std::fs::write(root.join("src/main.hd"), MAIN).expect("main");
    std::fs::write(root.join("src/price.hd"), price).expect("price");
    root
}

/// `hd build` in `root`, with its own cache; the debug Wasm it wrote.
fn build(root: &Path) -> Vec<u8> {
    let out = Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", root.with_extension("cache"))
        .env_remove("HD_JOBS")
        .env_remove("HD_ENGINE")
        .current_dir(root)
        .arg("build")
        .output()
        .expect("run hd");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    std::fs::read(root.join("build/debug/shop.wasm")).expect("wasm")
}

#[test]
fn an_edit_that_keeps_size_and_mtime_is_rebuilt() {
    let before = "pub fn price() -> i32:\n    return 41\n";
    let after = "pub fn price() -> i32:\n    return 42\n";
    assert_eq!(before.len(), after.len());
    let root = package("same-size-edit", before);
    let file = root.join("src/price.hd");
    let old = build(&root);
    assert_eq!(build(&root), old, "a warm build with no edit");
    let mtime = std::fs::metadata(&file)
        .and_then(|m| m.modified())
        .expect("mtime");
    std::fs::write(&file, after).expect("edit");
    std::fs::File::options()
        .write(true)
        .open(&file)
        .and_then(|f| f.set_modified(mtime))
        .expect("restore mtime");
    let meta = std::fs::metadata(&file).expect("metadata");
    assert_eq!(meta.modified().expect("mtime"), mtime);
    assert_eq!(meta.len(), before.len() as u64);
    let new = build(&root);
    assert_ne!(new, old, "the edit is seen");
    let clean = package("same-size-clean", after);
    assert_eq!(new, build(&clean), "incremental == clean");
}
