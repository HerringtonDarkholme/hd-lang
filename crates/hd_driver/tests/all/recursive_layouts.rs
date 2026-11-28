//! Recursive type layouts (#120; `crates/hd_wasm/DESIGN.md` §15.2, §15.3): an
//! enum recurs through `&` (`data.enum.recursive`), and data types and
//! trait values that name themselves have finite RC layouts through counted pointers.
//! Programs build through the driver and run on V8 (`host/run.ts`), as the
//! conformance runner does, so each module also validates there.

use std::fmt::Write as _;
use std::path::Path;

use hd_testkit::Scratch;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// Builds `main` as the entry module, the same bytes from the serial and
/// pool executors and from a warm cache (whose code entries decode their
/// recursion groups), and returns its standard output on V8.
fn output_of(name: &str, main: &str) -> String {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let wasm = |store: &MemoryStore, executor| {
        let host = Host {
            render_tir: &[],
            sources: &src,
            store,
            clock: &NoClock,
            executor,
        };
        let out = build(
            &host,
            "app",
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(out.diags.is_empty(), "{name}: {}", out.render());
        (out.wasm.expect("wasm"), out.counters.emitted)
    };
    let store = MemoryStore::default();
    let (serial, _) = wasm(&store, Executor::Serial(SerialOrder::Priority));
    let (pool, _) = wasm(&MemoryStore::default(), Executor::Pool(4));
    assert_eq!(pool, serial, "{name}: pool bytes");
    let (again, emitted) = wasm(&store, Executor::Serial(SerialOrder::Priority));
    assert_eq!(emitted, 0, "{name}: re-emitted");
    assert_eq!(again, serial, "{name}: warm bytes");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("recursive-{name}.wasm"),
    );
    std::fs::write(&path, &serial).expect("write wasm");
    let ran = hd_testkit::node::command()
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

#[test]
fn a_generic_binary_tree_inserts_walks_and_derives() {
    let main = include_str!("programs/recursive_layouts/tree.hd");
    assert_eq!(
        output_of("tree", main),
        "[1, 2, 3, 5, 8, 9]\ntrue\nfalse\n\
         Tree.Node(left=Tree.Node(left=Tree.Leaf, value=1, right=Tree.Leaf), value=2, right=Tree.Leaf)\n\
         [\"a\", \"m\", \"z\"]\n"
    );
}

#[test]
fn a_json_like_value_holds_lists_and_maps_of_itself() {
    // Five payload fields: the box is a base and one subtype per variant.
    let main = include_str!("programs/recursive_layouts/value.hd");
    assert_eq!(
        output_of("value", main),
        "8\n{name:'hd',tags:[true,null,25],nested:{name:null}}\n[]\n"
    );
}

#[test]
fn a_linked_list_through_an_optional_field() {
    let main = include_str!("programs/recursive_layouts/list.hd");
    assert_eq!(output_of("list", main), "6\n3\n1\n");
}

#[test]
fn mutually_recursive_expressions_and_statements() {
    let main = include_str!("programs/recursive_layouts/expr.hd");
    assert_eq!(
        output_of("expr", main),
        "42\ntrue\nfalse\n\
         Expr.Block(body=Stmt.Let(name=\"a\", value=Expr.Lit(value=1)), result=Expr.Var(name=\"a\"))\n"
    );
}

#[test]
fn closures_trait_values_and_boxed_tuples_that_name_their_data() {
    // Each recursion runs through a type that is not data: a closure's
    // code type, a vtable's slot type, a tuple's box.
    let main = include_str!("programs/recursive_layouts/groups.hd");
    assert_eq!(output_of("groups", main), "42\na>b>c\n3\n");
}

#[test]
fn an_enum_reference_at_growing_arguments() {
    // `E[T]` holds `&E[T?]`: the reference ends every value layout.
    let main = include_str!("programs/recursive_layouts/grow.hd");
    assert_eq!(output_of("grow", main), "1\n0\n");
}

/// One group of `n` struct types (a ring through `&`), a chain of `n`
/// struct types and a chain of `n` enums each held by value, and a `main`
/// that prints `2`, `4` and `1`.
fn long_chains(n: usize) -> String {
    let mut main = String::new();
    for i in 0..n {
        let ring = (i + 1) % n;
        // The chain's last type holds an `i32` where the others hold the next.
        let (chain, held) = if i + 1 < n {
            (format!("C{}", i + 1), format!("E{}", i + 1))
        } else {
            ("i32".to_owned(), "i32".to_owned())
        };
        writeln!(main, "struct R{i}:\n    v: i32\n    next: *R{ring}?\n").expect("write");
        writeln!(main, "struct C{i}:\n    v: i32\n    next: {chain}?\n").expect("write");
        writeln!(main, "enum E{i}:\n    Stop\n    Go(next: {held})\n").expect("write");
    }
    main.push_str(
        "\
fn around(r: R0) -> i32:
    match r.next:
        .Some(n) => n.v
        .None => 0

fn along(c: C0) -> i32:
    match c.next:
        .Some(n) => n.v
        .None => 0

fn tag(e: E0) -> i32:
    match e:
        .Stop => 0
        .Go(_n) => 1

pub fn main() -> void $ Console:
    println(around(R0 { v: +1, next: .Some(box(R1 { v: +2, next: .None })) }))
    println(along(C0 { v: +3, next: .Some(C1 { v: +4, next: .None }) }))
    println(tag(E0.Go(E1.Stop)))
",
    );
    main
}

#[test]
fn a_large_recursion_group_and_long_chains_of_types() {
    // Layout, encoding and link walk the types with loops, so their depth
    // is no native stack depth. A chain value is thousands of Wasm values,
    // more than an engine takes as a function's parameters, so it passes
    // through the shadow stack (`crates/hd_wasm/DESIGN.md` §23.29).
    assert_eq!(output_of("long", &long_chains(1500)), "2\n4\n1\n");
}

/// #2423: a pool build runs its tasks on its own threads, each at the
/// bottom of a `hd_sched::pool::WORKER_STACK` stack, wherever its caller
/// runs. Here the caller is a 64 KiB rayon worker, and the long chains'
/// build needs about 100 KiB: a run that stayed on a rayon caller, as
/// nested runs do, overflowed it.
#[test]
fn a_pool_build_from_a_small_rayon_worker_runs_on_the_pool_stacks()
-> Result<(), rayon::ThreadPoolBuildError> {
    let mut src = MemorySources::default();
    src.insert("main.hd", &long_chains(1500));
    let small = rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .stack_size(64 << 10)
        .build()?;
    let out = small.install(|| {
        let host = Host {
            render_tir: &[],
            sources: &src,
            store: &MemoryStore::default(),
            clock: &NoClock,
            executor: Executor::Pool(2),
        };
        build(
            &host,
            "app",
            &Goal::Program {
                entry: "main".into(),
            },
        )
    });
    assert!(out.diags.is_empty(), "{}", out.render());
    assert!(out.wasm.is_some());
    Ok(())
}
