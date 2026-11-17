//! `HD_WAT=1 hd build` writes hd's text of each module beside its `.wasm`
//! (#1973): every function and every call by its hd name, runtime helpers
//! included, so `scripts/ir-audit.py` reads it. Without `HD_WAT` it writes
//! none, and the module is the same either way.

use std::path::Path;

use hd_testkit::Scratch;

const MAIN: &str = "pub fn main() -> void $ Console:\n    xs := [for i in 0..10 => \"n$i\"]\n    println(\"${xs.len()} ${xs[3]}\")\n";

/// `hd build ARGS` in `root`, with `HD_WAT` when `wat`, and its own cache.
fn build(root: &Path, wat: bool, args: &[&str]) {
    let mut cmd = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"));
    cmd.env(
        "HD_CACHE",
        crate::shared_objects(root.with_extension("cache")),
    )
    .env_remove("HD_JOBS")
    .env_remove("HD_ENGINE")
    .env_remove("HD_WAT")
    .current_dir(root)
    .arg("build")
    .args(args);
    if wat {
        cmd.env("HD_WAT", "1");
    }
    let out = cmd.output().expect("run hd");
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn hd_wat_writes_the_named_text_beside_the_module() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "build-wat");
    std::fs::create_dir_all(root.join("src")).expect("src");
    std::fs::write(
        root.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("hd.toml");
    std::fs::write(root.join("src/main.hd"), MAIN).expect("main");
    for (args, dir) in [(&["--release"][..], "release"), (&[][..], "debug")] {
        let out = root.join("build").join(dir);
        build(&root, false, args);
        let plain = std::fs::read(out.join("shop.wasm")).expect("wasm");
        assert!(!out.join("shop.wat").exists(), "{dir}");
        build(&root, true, args);
        assert_eq!(
            std::fs::read(out.join("shop.wasm")).expect("wasm"),
            plain,
            "{dir}"
        );
        let text = std::fs::read_to_string(out.join("shop.wat")).expect("wat");
        assert!(text.contains("(func $shop/main/main"), "{dir}:\n{text}");
        assert!(text.contains("call $rt:"), "{dir}:\n{text}");
        let unnamed = text
            .lines()
            .filter(|l| {
                l.trim_start()
                    .strip_prefix("call ")
                    .is_some_and(|c| c.starts_with(|c: char| c.is_ascii_digit()))
            })
            .count();
        assert_eq!(unnamed, 0, "{dir}:\n{text}");
    }
}
