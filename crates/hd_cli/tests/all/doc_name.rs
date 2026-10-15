//! `hd doc NAME` with a path dependency (`cli.doc.name.dependency`):
//! `dep.KEY.item` finds the dependency's `pub` items only, prints to
//! standard output, and writes no file.

use std::path::Path;
use std::process::{Command, Output};

use hd_testkit::Scratch;

fn write(root: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().expect("parent")).expect("mkdir");
        std::fs::write(at, text).expect("write");
    }
}

fn hd(root: &Path, cache: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", cache)
        .env_remove("HD_JOBS")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run hd")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn a_dependency_name_finds_only_its_pub_items() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-doc-name-dep");
    let cache = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "hd-doc-name-dep-cache");
    write(
        &root,
        &[
            (
                "hd.toml",
                "[package]\nname = \"shop\"\n\n[dependencies]\nmoney = { path = \"money\" }\n",
            ),
            (
                "src/main.hd",
                "use dep.money.{cents}\n\npub fn main() -> void $ Console:\n    println(cents(12))\n",
            ),
            ("money/hd.toml", "[package]\nname = \"money\"\n"),
            (
                "money/src/lib.hd",
                "## Money helpers.\n\n## Turns euros into cents.\npub fn cents(euros: i32) -> i32:\n    euros * 100\n\nfn helper() -> i32: 1\n",
            ),
        ],
    );
    let out = hd(&root, &cache, &["doc", "--out", "site", "dep.money.cents"]);
    assert_eq!(
        text(&out.stdout),
        "## `cents`\n\n```hd\npub fn cents(euros: i32) -> i32\n```\n\nTurns euros into cents.\n",
        "{}",
        text(&out.stderr)
    );
    assert_eq!(out.status.code(), Some(0));
    assert!(!root.join("site").exists());
    let out = hd(&root, &cache, &["doc", "dep.money.pkg"]);
    assert!(
        text(&out.stdout).starts_with("# Module `pkg`\n"),
        "{}",
        text(&out.stderr)
    );
    // A private item is not found, even with `--private`, which applies to
    // the package only (`cli.doc.private`).
    for args in [
        &["doc", "dep.money.helper"][..],
        &["doc", "--private", "dep.money.helper"][..],
        &["doc", "dep.nope.cents"][..],
        &["doc", "dep.money"][..],
    ] {
        let out = hd(&root, &cache, args);
        assert_eq!(out.status.code(), Some(101), "{args:?}");
        assert_eq!(text(&out.stdout), "", "{args:?}");
        assert!(text(&out.stderr).contains("no documented item"), "{args:?}");
    }
    let out = hd(&root, &cache, &["doc", "dep.money.helper"]);
    assert!(
        text(&out.stderr).contains("candidates: dep.money.pkg, dep.money.cents"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn a_std_name_prints_the_lib_std_declaration() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-doc-name-std");
    let cache = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "hd-doc-name-std-cache");
    write(
        &root,
        &[
            ("hd.toml", "[package]\nname = \"shop\"\n"),
            ("src/lib.hd", "pub fn one() -> i32: 1\n"),
        ],
    );
    let out = hd(&root, &cache, &["doc", "std.text.split"]);
    assert_eq!(
        text(&out.stdout),
        "```hd\npub fn split(self, separator: string) -> List[string]\n```\n\nSplits the string around each occurrence of `separator`, in order.\n\nReference: [std.text.string.split](spec/std/text.md#r-std-text.string.split)\n",
        "{}",
        text(&out.stderr)
    );
    let out = hd(&root, &cache, &["doc", "std.text.pad_start"]);
    assert!(text(&out.stdout).contains("Pads the string on the left"));
    let out = hd(&root, &cache, &["doc", "std.text"]);
    let page = text(&out.stdout);
    assert!(page.starts_with("# Module `std.text`\n"), "{page}");
    assert!(page.contains("- [`join`](#join): "), "{page}");
    let out = hd(&root, &cache, &["doc", "std.text.nope"]);
    assert_eq!(out.status.code(), Some(101));
    assert_eq!(text(&out.stdout), "");
    assert!(text(&out.stderr).contains("no documented item"));
    assert!(text(&out.stderr).contains("candidates: std.text."));
}
