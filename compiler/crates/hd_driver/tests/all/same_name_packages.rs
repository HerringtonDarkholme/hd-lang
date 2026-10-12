//! Package identity in the module table (spec 10 `module.dep.identity`,
//! `module.dep.key-name`, `module.package.no-sharing`,
//! `module.package.identity-part`): two packages whose manifests share a
//! name are two packages. Each depender reaches one by its own key, their
//! declarations stay apart, and their cached outputs never mix.

use hd_cache::MemoryStore;
use hd_driver::{Dependency, Executor, Goal, Host, NoClock, Output, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

const MAIN: &str = "\
use dep.euro
use dep.yen
use dep.euro.{Coin as Euro}
use dep.yen.{Coin as Yen}

pub fn main() -> void $ Console:
    let e: Euro = euro.make(1)
    let y: Yen = yen.make(1)
    println(e.cents + y.cents + euro.value() + yen.value())
";

/// A library named `money` whose declarations are spelled like the other
/// one's, with different bodies.
fn money(scale: i32, value: i32) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert(
        "src/lib.hd",
        &format!(
            "pub data Coin:\n    pub cents: i32\n\n\
             pub fn make(n: i32) -> Coin:\n    Coin {{ cents: n * {scale} }}\n\n\
             pub fn value() -> i32:\n    {value}\n"
        ),
    );
    s
}

fn root(main: &str) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("src/main.hd", main);
    s
}

/// `app` with `deps`, each by its key, name `money` and identity.
fn run(store: &MemoryStore, main: &MemorySources, deps: &[(&str, &str, &MemorySources)]) -> Output {
    let host = Host {
        render_tir: &[],
        sources: main,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        requires: deps
            .iter()
            .enumerate()
            .map(|(i, (key, _, _))| ((*key).to_owned(), u16::try_from(i + 1).expect("deps")))
            .collect(),
        deps: deps
            .iter()
            .map(|(_, identity, sources)| Dependency {
                name: "money".to_owned(),
                identity: (*identity).to_owned(),
                sources: *sources,
                requires: Vec::new(),
                fetched: false,
            })
            .collect(),
        ..Packages::default()
    };
    build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

#[track_caller]
fn ok(out: Output) -> Output {
    assert!(out.diags.is_empty(), "{}", out.render());
    assert!(out.wasm.is_some());
    out
}

#[test]
fn same_named_dependencies_are_two_packages() {
    let store = MemoryStore::default();
    let (a, b) = (money(100, 1), money(1000, 2));
    let main = root(MAIN);
    let deps = [("euro", "/w/a", &a), ("yen", "/w/b", &b)];
    let cold = ok(run(&store, &main, &deps));
    // Each package has its own module root; the first keeps its name.
    assert!(cold.modules.contains(&"money".to_owned()));
    assert!(cold.modules.contains(&"money$2".to_owned()));
    // A file of a package whose name another shares is named by identity.
    assert!(cold.files.contains(&"</w/b>/src/lib.hd".to_owned()));
    // User text names both packages `money`.
    assert_eq!(
        cold.user_text("`money$2.make` and `money.make`"),
        "`money.make` and `money.make`"
    );

    // Warm, no edit: everything hits.
    let warm = ok(run(&store, &main, &deps));
    assert!(warm.counters.modules_checked.is_empty());
    assert_eq!(warm.counters.hit("link"), 1);
    assert_eq!(warm.wasm, cold.wasm);

    // An edit of the root's body reuses both packages' entries.
    let edited = root(&MAIN.replace("yen.value())", "yen.value() + 1)"));
    let r = ok(run(&store, &edited, &deps));
    assert_eq!(r.counters.modules_checked, ["app.main"]);
    assert!(
        r.counters.ifaces_built.is_empty(),
        "{:?}",
        r.counters.ifaces_built
    );
}

/// A package's cached outputs are keyed by its identity, so another
/// package of the same name and the same text builds its own, even at the
/// same module root.
#[test]
fn same_named_packages_never_share_entries() {
    let store = MemoryStore::default();
    let a = money(100, 1);
    let main =
        root("use dep.euro\n\npub fn main() -> void $ Console:\n    println(euro.value())\n");
    let first = ok(run(&store, &main, &[("euro", "/w/a", &a)]));
    assert!(first.counters.ifaces_built.contains(&"money".to_owned()));
    let again = ok(run(&store, &main, &[("euro", "/w/a", &a)]));
    assert!(again.counters.ifaces_built.is_empty());
    let other = ok(run(&store, &main, &[("euro", "/w/b", &a)]));
    assert!(
        other.counters.ifaces_built.contains(&"money".to_owned()),
        "{:?}",
        other.counters.ifaces_built
    );
    assert!(
        other.counters.modules_checked.contains(&"money".to_owned()),
        "{:?}",
        other.counters.modules_checked
    );
}

/// Two packages of one name are two declarations each: a `Coin` of one is
/// no `Coin` of the other (`module.package.identity-part`).
#[test]
fn same_named_declarations_stay_apart() {
    let store = MemoryStore::default();
    let (a, b) = (money(100, 1), money(1000, 2));
    let main = root(
        "use dep.euro\nuse dep.yen.{Coin as Yen}\n\n\
         pub fn main() -> void $ Console:\n    let y: Yen = euro.make(1)\n    println(y.cents)\n",
    );
    let out = run(&store, &main, &[("euro", "/w/a", &a), ("yen", "/w/b", &b)]);
    assert!(out.wasm.is_none());
    let text = out.user_text(&out.render());
    assert!(text.contains("type-mismatch"), "{text}");
    assert!(!text.contains('$'), "{text}");
}

/// Two packages of one name print alike but have two identities
/// (`trait.typeid.name.package`, `trait.typeid.equality`): the second is
/// `money.Coin`, not `money$2.Coin`.
#[test]
fn same_named_packages_print_alike_and_compare_unequal() {
    let store = MemoryStore::default();
    let (a, b) = (money(100, 1), money(1000, 2));
    let main = root(
        "use dep.euro.{Coin as Euro}\nuse dep.yen.{Coin as Yen}\nuse std.inspect.TypeId\n\n\
         pub fn main() -> void $ Console:\n\
         \x20   println(\"${TypeId::of::[Euro]()} ${TypeId::of::[Yen]()}\")\n\
         \x20   println(\"${TypeId::of::[Euro]() == TypeId::of::[Yen]()}\")\n\
         \x20   println(\"${TypeId::of::[List[Yen?]]()}\")\n",
    );
    let out = ok(run(
        &store,
        &main,
        &[("euro", "/w/a", &a), ("yen", "/w/b", &b)],
    ));
    let path = hd_testkit::Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), "same-name-packages.wasm");
    std::fs::write(&path, out.wasm.expect("wasm")).expect("write wasm");
    let ran = std::process::Command::new("node")
        .arg(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{}",
        String::from_utf8_lossy(&ran.stderr)
    );
    assert_eq!(
        String::from_utf8(ran.stdout).expect("UTF-8"),
        "money.Coin money.Coin\nfalse\nList[money.Coin?]\n"
    );
}
