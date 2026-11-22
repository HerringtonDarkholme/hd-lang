//! Incremental soundness by edit script (#1801 draft 79,
//! `docs/compiler/testing-the-compiler.md` §8.2): a package goes through
//! one edit of each kind the cache must see, and after each one a warm
//! `hd check` and `hd check --tests` over the cache every earlier step
//! filled report exactly what a clean one reports, from a cache that holds
//! std alone. Each fact a module's row in the last-run record holds is
//! read by some step after an edit elsewhere: diagnostics, test
//! registrations, init summaries, generic edges, and the meta flags that
//! make a run read the whole entry.
//!
//! Not scripted here: edits to a user derivation template and to a
//! dependency's version, which need a second package, and to a top-level
//! statement of an init group.

use std::path::{Path, PathBuf};

use hd_testkit::Scratch;

/// Copies the tree at `from` to `to`.
fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).expect("dir");
    for entry in std::fs::read_dir(from).expect("read dir") {
        let entry = entry.expect("entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).expect("copy");
        }
    }
}

type Outcome = (Option<i32>, String, String);

/// `hd check` with `args` in `dir` over the cache `cache`: its status and
/// output.
fn check_with(dir: &Path, cache: &Path, args: &[&str]) -> Outcome {
    let out = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", cache)
        .env_remove("HD_JOBS")
        .current_dir(dir)
        .arg("check")
        .args(args)
        .output()
        .expect("run hd");
    let text = |b: &[u8]| String::from_utf8_lossy(b).into_owned();
    (out.status.code(), text(&out.stdout), text(&out.stderr))
}

/// `hd check`, then `hd check --tests`, in `dir` over the cache `cache`.
fn check(dir: &Path, cache: &Path) -> [Outcome; 2] {
    [&[][..], &["--tests"]].map(|args| check_with(dir, cache, args))
}

const MAIN: &str = "use pkg.api.{total}\nuse pkg.shop.items.{Item}\nuse pkg.shop.labels.{label}\n\n\
pub fn main() -> void $ Console:\n    \
println(label(total([Item { name: \"a\", price: 2, count: 3, status: .Pending }])))\n";

const ITEMS: &str = "## The state of an order.\n@derive(Eq)\npub enum Status:\n    Pending\n    \
Paid(amount: i64)\n\n## A line item.\npub struct Item:\n    pub name: string\n    pub price: i64\n    \
pub count: i32\n    pub status: Status\n\n## Something with a cost.\npub trait Costed:\n    \
fn cost(self) -> i64\n\nimpl Costed for Item:\n    fn cost(self) -> i64:\n        \
self.price * i64(self.count)\n\n## The total cost of the items.\n\
pub fn total(items: List[Item]) -> i64:\n    let sum: i64 = 0\n    for item in items:\n        \
sum = sum + item.cost() + bump()\n    sum\n\n## Twice the total.\n\
pub fn double(items: List[Item]) -> i64:\n    total(items) * 2\n\nfn bump() -> i64:\n    0\n";

const LABELS: &str =
    "## The label of a total.\npub fn label(n: i64) -> string:\n    \"total ${n}\"\n";

const SUMMARY: &str = "use pkg.shop.items.{Item, Status}\n\n## The first item's name.\n\
pub fn first_name(item: Item) -> string:\n    item.name\n\n## Whether the item is paid.\n\
pub fn paid(item: Item) -> bool:\n    item.status != .Pending\n";

/// The package's files, written fresh.
fn package(root: &Path) {
    let files = [
        (
            "hd.toml",
            "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
        ),
        ("src/main.hd", MAIN),
        (
            "src/api.hd",
            "## The package's front door.\npub use pkg.shop.items.{total}\n",
        ),
        ("src/shop/items.hd", ITEMS),
        ("src/shop/labels.hd", LABELS),
        ("src/report/summary.hd", SUMMARY),
        (
            "tests/smoke.hd",
            "use pkg.shop.labels.{label}\n\nit(\"labels\"):\n    println(label(1))\n",
        ),
    ];
    for (path, text) in files {
        let at = root.join(path);
        std::fs::create_dir_all(at.parent().expect("parent")).expect("dir");
        std::fs::write(at, text).expect("write");
    }
}

/// Replaces the one `from` in the file at `path` with `to`.
fn replace(root: &Path, path: &str, from: &str, to: &str) {
    let at = root.join(path);
    let text = std::fs::read_to_string(&at).expect("read");
    assert_eq!(text.matches(from).count(), 1, "{path}: {from:?}");
    std::fs::write(at, text.replacen(from, to, 1)).expect("write");
}

fn append(root: &Path, path: &str, more: &str) {
    let at = root.join(path);
    let text = std::fs::read_to_string(&at).expect("read");
    std::fs::write(at, text + more).expect("write");
}

type Edit = (&'static str, fn(&Path));

const HIDDEN: &str = "\nuse pkg.shop.items.{Costed}\n\nstruct Hidden:\n    n: i64\n\n\
impl Costed for Hidden:\n    fn cost(self) -> i64:\n        self.n\n";
const BROKEN: &str = "\npub fn broken(x: Missing) -> i32:\n    1\n";

/// Files whose findings the run makes across modules, after the module
/// checks: a growing generic call cycle, an `@inline(.Always)` cycle,
/// recursive hidden results and an initialization cycle; and a body error.
const WHOLE: &[(&str, &str)] = &[
    ("src/more/bad.hd", "pub fn bad() -> i32:\n    true\n"),
    (
        "src/more/grow.hd",
        "fn even[T](value: T, n: i32) -> i32:\n    if n == 0: 0 else: odd(value, n - 1)\n\n\
         fn odd[T](value: T, n: i32) -> i32:\n    if n == 0: 1 else: even(.Some(value), n - 1)\n\n\
         pub fn grow() -> i32:\n    even(+1, +3)\n",
    ),
    (
        "src/more/inline.hd",
        "use std.annotation.inline\n\n@inline(.Always)\nfn depth(n: i32) -> i32:\n    \
         if n == 0:\n        return +0\n    depth(n - 1) + 1\n\npub fn deep() -> i32:\n    depth(2)\n",
    ),
    (
        "src/more/hidden.hd",
        "fn first() -> impl Display:\n    second()\n\nfn second() -> impl Display:\n    first()\n",
    ),
    (
        "src/cycle/a.hd",
        "use pkg.cycle.b.{read_b}\n\nlet a = read_b()\n\npub fn read_a() -> i32:\n    a\n",
    ),
    (
        "src/cycle/b.hd",
        "use pkg.cycle.a.{read_a}\n\nlet b = read_a()\n\npub fn read_b() -> i32:\n    b\n",
    ),
];

/// Each edit kind, in order; each step edits the package as the step
/// before it left it.
const EDITS: &[Edit] = &[
    ("a private function body", |r| {
        replace(
            r,
            "src/shop/items.hd",
            "fn bump() -> i64:\n    0",
            "fn bump() -> i64:\n    1",
        );
    }),
    ("a save of the same bytes", |r| {
        let text = std::fs::read_to_string(r.join("src/shop/items.hd")).expect("read");
        std::fs::write(r.join("src/shop/items.hd"), text).expect("write");
    }),
    ("an edit that keeps the file's size", |r| {
        replace(
            r,
            "src/shop/items.hd",
            "fn bump() -> i64:\n    1",
            "fn bump() -> i64:\n    2",
        );
    }),
    ("a new private item", |r| {
        append(
            r,
            "src/shop/items.hd",
            "\nfn spare(x: i32) -> i32:\n    x\n",
        );
    }),
    ("a private signature and its call", |r| {
        replace(
            r,
            "src/shop/items.hd",
            "fn bump() -> i64:\n    2",
            "fn bump(n: i64) -> i64:\n    n",
        );
        replace(r, "src/shop/items.hd", "bump()", "bump(1)");
    }),
    ("a public signature that breaks a caller", |r| {
        replace(r, "src/shop/labels.hd", "label(n: i64)", "label(n: i32)");
    }),
    ("the public signature restored", |r| {
        replace(r, "src/shop/labels.hd", "label(n: i32)", "label(n: i64)");
    }),
    ("a field type a reader reaches through a signature", |r| {
        replace(r, "src/shop/items.hd", "pub name: string", "pub name: i32");
    }),
    ("the field type restored", |r| {
        replace(r, "src/shop/items.hd", "pub name: i32", "pub name: string");
    }),
    ("a re-export redirected", |r| {
        replace(r, "src/api.hd", "{total}", "{double as total}");
    }),
    ("a re-export removed", |r| {
        replace(
            r,
            "src/api.hd",
            "pub use pkg.shop.items.{double as total}\n",
            "",
        );
    }),
    ("the re-export restored", |r| {
        append(r, "src/api.hd", "pub use pkg.shop.items.{total}\n");
    }),
    ("a derivation removed", |r| {
        replace(r, "src/shop/items.hd", "@derive(Eq)\n", "");
    }),
    ("the derivation restored", |r| {
        replace(
            r,
            "src/shop/items.hd",
            "pub enum Status:",
            "@derive(Eq)\npub enum Status:",
        );
    }),
    ("an impl for a private type added", |r| {
        append(r, "src/shop/labels.hd", HIDDEN);
    }),
    ("the private impl removed", |r| {
        replace(r, "src/shop/labels.hd", HIDDEN, "");
    }),
    ("an overlapping impl for a public type added", |r| {
        append(
            r,
            "src/shop/labels.hd",
            "\nuse pkg.shop.items.{Costed, Item}\n\nimpl Costed for Item:\n    \
             fn cost(self) -> i64:\n        0\n",
        );
    }),
    ("the overlapping impl removed", |r| {
        std::fs::write(r.join("src/shop/labels.hd"), LABELS).expect("write");
    }),
    ("a doc comment", |r| {
        replace(r, "src/shop/labels.hd", "of a total", "of a sum");
    }),
    ("a comment", |r| {
        replace(
            r,
            "src/shop/items.hd",
            "## A line item.",
            "# Kept short.\n## A line item.",
        );
    }),
    ("a header error", |r| {
        append(r, "src/shop/labels.hd", BROKEN);
    }),
    ("blank lines before the header error", |r| {
        replace(
            r,
            "src/shop/labels.hd",
            "\npub fn broken",
            "\n\n\n\npub fn broken",
        );
    }),
    ("the header error removed", |r| {
        replace(
            r,
            "src/shop/labels.hd",
            "\n\n\n\npub fn broken(x: Missing) -> i32:\n    1\n",
            "",
        );
    }),
    ("files with findings across modules added", |r| {
        for (path, text) in WHOLE {
            let at = r.join(path);
            std::fs::create_dir_all(at.parent().expect("parent")).expect("dir");
            std::fs::write(at, text).expect("write");
        }
    }),
    ("a private body edit beside them", |r| {
        replace(
            r,
            "src/shop/items.hd",
            "fn bump(n: i64) -> i64:\n    n",
            "fn bump(n: i64) -> i64:\n    n * 1",
        );
    }),
    ("the files with findings across modules removed", |r| {
        std::fs::remove_dir_all(r.join("src/more")).expect("rm");
        std::fs::remove_dir_all(r.join("src/cycle")).expect("rm");
    }),
    ("a file deleted", |r| {
        std::fs::remove_file(r.join("src/report/summary.hd")).expect("rm");
    }),
    ("the file restored", |r| {
        std::fs::write(r.join("src/report/summary.hd"), SUMMARY).expect("write");
    }),
    ("a file moved to another folder", |r| {
        std::fs::rename(r.join("src/shop/labels.hd"), r.join("src/report/labels.hd")).expect("mv");
        replace(r, "src/main.hd", "pkg.shop.labels", "pkg.report.labels");
    }),
    ("the file moved back", |r| {
        std::fs::rename(r.join("src/report/labels.hd"), r.join("src/shop/labels.hd")).expect("mv");
        replace(r, "src/main.hd", "pkg.report.labels", "pkg.shop.labels");
    }),
    ("x.hd moved to x/mod.hd", |r| {
        std::fs::create_dir_all(r.join("src/shop/labels")).expect("dir");
        std::fs::rename(
            r.join("src/shop/labels.hd"),
            r.join("src/shop/labels/mod.hd"),
        )
        .expect("mv");
    }),
    ("x/mod.hd moved back to x.hd", |r| {
        std::fs::rename(
            r.join("src/shop/labels/mod.hd"),
            r.join("src/shop/labels.hd"),
        )
        .expect("mv");
        std::fs::remove_dir(r.join("src/shop/labels")).expect("rmdir");
    }),
    ("a capability dropped from the manifest", |r| {
        replace(r, "hd.toml", "\n[capabilities]\nConsole = true\n", "");
    }),
    ("a body edit while the capability is dropped", |r| {
        replace(r, "src/shop/items.hd", "    n * 1", "    n * 2");
    }),
    ("the capability restored", |r| {
        append(r, "hd.toml", "\n[capabilities]\nConsole = true\n");
    }),
    ("the package renamed", |r| {
        replace(r, "hd.toml", "name = \"shop\"", "name = \"store\"");
    }),
    ("the name restored", |r| {
        replace(r, "hd.toml", "name = \"store\"", "name = \"shop\"");
    }),
];

#[test]
fn every_edit_kind_checks_warm_as_it_checks_clean() {
    let base = env!("CARGO_TARGET_TMPDIR");
    // A cache holding std alone, prepared once and copied for each clean run.
    let empty = Scratch::new(base, "edit-script-std");
    std::fs::create_dir_all(empty.join("src")).expect("src");
    std::fs::write(empty.join("hd.toml"), "[package]\nname = \"empty\"\n").expect("write");
    std::fs::write(empty.join("src/lib.hd"), "pub fn one() -> i32: 1\n").expect("write");
    assert_eq!(check_with(&empty, &empty.cache(), &[]).0, Some(0));
    let dir = Scratch::new(base, "edit-script");
    let root = dir.join("shop");
    package(&root);
    let warm = dir.cache();
    let clean = |at: &Path, step: usize| -> [Outcome; 2] {
        [&[][..], &["--tests"]].map(|args| {
            let cache: PathBuf = dir.join(format!("clean-{step}"));
            copy_tree(&empty.cache(), &cache);
            let out = check_with(at, &cache, args);
            std::fs::remove_dir_all(&cache).expect("rm clean cache");
            out
        })
    };
    let first = check(&root, &warm);
    assert_eq!(first, clean(&root, 0), "the unedited package");
    assert!(first.iter().all(|o| o.0 == Some(0)), "{first:?}");
    let mut failed = 0;
    for (step, (name, edit)) in EDITS.iter().enumerate() {
        edit(&root);
        let (incremental, fresh) = (check(&root, &warm), clean(&root, step + 1));
        assert_eq!(incremental, fresh, "after {name}");
        failed += usize::from(fresh[1].0 != Some(0));
    }
    // The script reaches both outcomes, so a check that always passes or
    // always fails would not match it.
    assert!(failed >= 6, "{failed} steps failed to check");
    // A second checkout of the same package, over the same warm cache.
    let other = dir.join("checkout");
    copy_tree(&root, &other);
    assert_eq!(
        check(&other, &warm),
        clean(&other, EDITS.len() + 1),
        "a second checkout"
    );
}
