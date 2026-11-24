//! A warm `hd build` never reuses a generic instance's stored owned form
//! after a body it read changed (#2106): `keep`'s instances fold in
//! `util.bump`, and an edit to `bump` alone, which neither `keep`'s TIR
//! nor any declaration shows, rebuilds them through the disk store.

use std::path::Path;
use std::time::{Duration, Instant};

use hd_testkit::Scratch;

const MAIN: &str = include_str!("programs/instance_cache_edit/main.hd");

const GEN: &str = include_str!("programs/instance_cache_edit/gen.hd");

fn package(name: &str, bump: &str) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(
        root.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("hd.toml");
    std::fs::write(root.join("src/main.hd"), MAIN).expect("main");
    std::fs::write(root.join("src/gen.hd"), GEN).expect("gen");
    std::fs::write(
        root.join("src/util.hd"),
        format!("pub fn bump(x: usize) -> usize:\n    {bump}\n"),
    )
    .expect("util");
    root
}

/// `hd build` in `root`, with its own cache, killed after two minutes; the
/// debug Wasm it wrote.
fn build(root: &Path) -> Vec<u8> {
    let mut child = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            crate::shared_objects(root.with_extension("cache")),
        )
        .env_remove("HD_JOBS")
        .env_remove("HD_ENGINE")
        .current_dir(root)
        .arg("build")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("run hd");
    let deadline = Instant::now() + Duration::from_mins(2);
    let status = loop {
        if let Some(status) = child.try_wait().expect("wait") {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("hd build overran two minutes in {}", root.display());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(status.code(), Some(0), "hd build in {}", root.display());
    std::fs::read(root.join("build/debug/shop.wasm")).expect("wasm")
}

#[test]
fn an_edit_to_a_body_an_instance_folded_in_rebuilds_the_instance() {
    let root = package("instance-cache-edit", "x + 1");
    let old = build(&root);
    std::fs::write(
        root.join("src/util.hd"),
        include_str!("programs/instance_cache_edit/an_edit_to_a_body_an_instance_folded_in_rebuilds_the_instance.hd"),
    )
    .expect("edit");
    let new = build(&root);
    assert_ne!(new, old, "the edit is seen");
    let clean = package("instance-cache-clean", "x + 20");
    assert_eq!(new, build(&clean), "incremental == clean");
}
