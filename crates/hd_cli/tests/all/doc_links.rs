//! `` [`NAME`] `` links in `hd doc` pages (`cli.doc.link.*`): resolved in
//! the module scope of the documented item, relative to the page that holds
//! them. A name that resolves to no item, or to a private item the run does
//! not document, is a `broken-doc-link` finding from `hd lint`. `hd doc`
//! still writes the pages but reports compiler errors only.

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

fn read(root: &Path, path: &str) -> String {
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"))
}

const MANIFEST: &str =
    "[package]\nname = \"shop\"\n\n[dependencies]\nmoney = { path = \"money\" }\n";

const MONEY: &[(&str, &str)] = &[
    ("money/hd.toml", "[package]\nname = \"money\"\n"),
    (
        "money/src/lib.hd",
        "## Money helpers.\n\n## Turns euros into cents.\npub fn cents(euros: i32) -> i32:\n    euros * 100\n\n## A sum.\npub struct Sum:\n    pub n: i32\n\nimpl Sum:\n    ## Doubles.\n    pub fn double(self) -> Sum: self\n",
    ),
];

const LIB: &str = "\
## The shop. See [`total`] and [`std.text.split`].

## Sums a [`helper`] result, like [`Cart.add`] does.
pub fn total() -> i32:
    helper()

fn helper() -> i32: 1
";

const CART: &str = "\
## Carts, summed by [`total`].

use pkg.{total}
use dep.money.{Sum, cents}

## A cart that costs [`cents`], [`Sum.double`], [`Sum.n`] and [`sluggify`], not [`Sum.nope`].
pub struct Cart:
    pub n: i32

impl Cart:
    ## Adds to a [`Cart`].
    pub fn add(self) -> Cart: self
";

const DEEP: &str = "\
## Deep. See [`total`] and [`Cart.add`].

use pkg.{total}
use pkg.cart.{Cart}
";

fn project(name: &str) -> (Scratch, Scratch) {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    let cache = Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), &format!("{name}-cache"));
    write(&root, &[("hd.toml", MANIFEST)]);
    write(&root, MONEY);
    write(
        &root,
        &[
            ("src/lib.hd", &LIB.replace("[`Cart.add`]", "`Cart.add`")),
            ("src/cart.hd", CART),
            ("src/shop/deep.hd", DEEP),
        ],
    );
    (root, cache)
}

#[test]
fn links_resolve_across_modules_and_nested_pages() {
    let (root, cache) = project("hd-doc-links");
    let out = hd(&root, &cache, &["doc"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let doc = "build/doc";
    let cart = read(&root, &format!("{doc}/cart.md"));
    assert!(
        cart.contains("summed by [`total`](pkg.md#total)."),
        "{cart}"
    );
    assert!(cart.contains("Adds to a [`Cart`](#Cart)."), "{cart}");
    let deep_md = read(&root, &format!("{doc}/shop/deep.md"));
    assert!(
        deep_md.contains("See [`total`](../pkg.md#total) and [`Cart.add`](../cart.md#Cart.add)."),
        "{deep_md}"
    );
    let deep_html = read(&root, &format!("{doc}/shop/deep.html"));
    assert!(
        deep_html.contains("<a href=\"../pkg.html#total\"><code>total</code></a>"),
        "{deep_html}"
    );
    let pkg = read(&root, &format!("{doc}/pkg.md"));
    assert!(
        pkg.contains("[`std.text.split`](spec/std/text.md#r-std-text.string.split)"),
        "{pkg}"
    );
}

#[test]
fn a_dependency_name_is_checked_and_shown_as_code() {
    let (root, cache) = project("hd-doc-links-dep");
    let out = hd(&root, &cache, &["doc"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let cart = read(&root, "build/doc/cart.md");
    assert!(
        cart.contains("costs `cents`, `Sum.double`, `Sum.n` and `sluggify`, not `Sum.nope`."),
        "{cart}"
    );
    // Only the names that resolve to nothing are reported by lint.
    let lint = hd(&root, &cache, &["lint"]);
    assert_eq!(lint.status.code(), Some(0), "{}", text(&lint.stderr));
    let err = text(&lint.stderr) + &text(&lint.stdout);
    assert!(err.contains("broken-doc-link"), "{err}");
    assert!(
        err.contains("sluggify") && err.contains("Sum.nope"),
        "{err}"
    );
    for ok in ["`[`cents`]`", "[`Sum.double`]", "[`Sum.n`]"] {
        assert!(!err.contains(ok), "{ok} in {err}");
    }
}

#[test]
fn a_broken_link_warns_and_the_pages_are_written() {
    let (root, cache) = project("hd-doc-links-broken");
    let out = hd(&root, &cache, &["doc", "--out", "site"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let doc_output = text(&out.stderr) + &text(&out.stdout);
    assert!(!doc_output.contains("broken-doc-link"), "{doc_output}");
    let lint = hd(&root, &cache, &["lint"]);
    assert_eq!(lint.status.code(), Some(0), "{}", text(&lint.stderr));
    let all = text(&lint.stderr) + &text(&lint.stdout);
    assert!(
        all.contains("warning") && all.contains("broken-doc-link") && all.contains("src/cart.hd"),
        "{all}"
    );
    for page in ["pkg.md", "pkg.html", "cart.md", "cart.html", "shop/deep.md"] {
        assert!(root.join("site").join(page).exists(), "{page}");
    }
}

#[test]
fn a_private_target_warns_unless_private_items_are_documented() {
    let (root, cache) = project("hd-doc-links-private");
    let out = hd(&root, &cache, &["doc", "--out", "site"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let pkg = read(&root, "site/pkg.md");
    assert!(pkg.contains("Sums a `helper` result"), "{pkg}");
    let lint = hd(&root, &cache, &["lint"]);
    assert_eq!(lint.status.code(), Some(0), "{}", text(&lint.stderr));
    let all = text(&lint.stderr) + &text(&lint.stdout);
    assert!(all.contains("helper") && all.contains("private"), "{all}");

    let out = hd(&root, &cache, &["doc", "--private", "--out", "site"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stderr));
    let pkg = read(&root, "site/pkg.md");
    assert!(pkg.contains("Sums a [`helper`](#helper) result"), "{pkg}");
    let all = text(&out.stderr) + &text(&out.stdout);
    assert!(!all.contains("helper"), "{all}");
}

#[test]
fn broken_link_policy_is_applied_by_lint() {
    let (root, cache) = project("hd-doc-links-policy");
    let allowed = hd(&root, &cache, &["lint", "--allow", "broken-doc-link"]);
    assert_eq!(allowed.status.code(), Some(0), "{}", text(&allowed.stderr));
    let all = text(&allowed.stderr) + &text(&allowed.stdout);
    assert!(!all.contains("broken-doc-link"), "{all}");

    let denied = hd(&root, &cache, &["lint", "--deny", "broken-doc-link"]);
    assert_eq!(denied.status.code(), Some(101), "{}", text(&denied.stderr));
    let all = text(&denied.stderr) + &text(&denied.stdout);
    assert!(
        all.contains("error") && all.contains("broken-doc-link"),
        "{all}"
    );
}

#[test]
fn linting_one_file_limits_broken_links_to_its_reachable_modules() {
    let (root, cache) = project("hd-doc-links-file");
    let lint = hd(&root, &cache, &["lint", "src/lib.hd"]);
    assert_eq!(lint.status.code(), Some(0), "{}", text(&lint.stderr));
    let all = text(&lint.stderr) + &text(&lint.stdout);
    assert!(all.contains("helper"), "{all}");
    assert!(
        !all.contains("sluggify") && !all.contains("Sum.nope"),
        "{all}"
    );
}
