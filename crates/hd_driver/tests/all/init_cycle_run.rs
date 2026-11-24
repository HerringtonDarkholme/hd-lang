//! Statements of a module cycle run in dependency order, then by module
//! identity and source position (spec 10 "Order Inside A Cycle":
//! `module.init.cycle.dependency`, `module.init.cycle.step`,
//! `module.init.cycle.earliest`), splitting a module's statements where
//! another module's must run between them; statements that wait on each
//! other are `top-level-read-before-initialization`
//! (`module.init.cycle.stuck`). The entry module follows the same order:
//! its statements, split or not, keep the entry's rights, and a script's
//! run with its entry providers. Programs build through the driver and run
//! on V8 (`host/run.ts`).

use std::path::Path;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;
use hd_testkit::Scratch;

/// Records lines in order; `main` prints them.
const LOG: &str = include_str!("programs/init_cycle_run/log.hd");

fn sources(files: &[(&str, &str)]) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("log/mod.hd", LOG);
    for (path, text) in files {
        s.insert(path, text);
    }
    s
}

fn build_in(store: &MemoryStore, src: &MemorySources) -> Output {
    let host = Host {
        render_tir: &[],
        sources: src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

fn run_wasm(name: &str, wasm: &[u8]) -> String {
    let ran = node_run(name, wasm);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

/// Runs `wasm` on V8 and returns how it exited.
fn node_run(name: &str, wasm: &[u8]) -> std::process::Output {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("init-cycle-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = hd_testkit::node::command()
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.ts"))
        .arg(&path)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// Builds cold, then warm from the same store, and runs the program: the
/// warm build reads `InitOrder`'s part and links the same Wasm.
fn run(name: &str, files: &[(&str, &str)]) -> String {
    run_wasm(name, &build_twice(name, files))
}

/// Like [`run`], but returns how the program exited, for one that fails.
fn run_status(name: &str, files: &[(&str, &str)]) -> std::process::Output {
    node_run(name, &build_twice(name, files))
}

/// Builds cold, then warm from the same store; both link the same Wasm.
fn build_twice(name: &str, files: &[(&str, &str)]) -> Vec<u8> {
    let store = MemoryStore::default();
    let src = sources(files);
    let cold = build_in(&store, &src);
    assert!(cold.diags.is_empty(), "{name}: {}", cold.render());
    let linked = cold.wasm.expect("wasm");
    let warm = build_in(&store, &src);
    assert!(warm.diags.is_empty(), "{name}: {}", warm.render());
    assert_eq!(warm.counters.computed("InitOrder"), 0, "{name}");
    assert_eq!(warm.wasm.as_deref(), Some(linked.as_slice()), "{name}");
    linked
}

#[test]
fn a_two_module_cycle_runs_a_statement_after_the_binding_it_reaches() {
    // `pkg.shop.catalog` orders first, but its statements wait on
    // `markup`, so prices' first statement runs before them and its
    // second after them.
    let catalog = include_str!("programs/init_cycle_run/two/catalog.hd");
    let prices = include_str!("programs/init_cycle_run/two/prices.hd");
    let main = include_str!("programs/init_cycle_run/two/main.hd");
    let out = run(
        "two",
        &[
            ("main.hd", main),
            ("shop/catalog.hd", catalog),
            ("shop/prices.hd", prices),
        ],
    );
    assert_eq!(out, "15\ncatalog 15\nprices\n");
}

#[test]
fn a_three_module_cycle_follows_calls_through_every_module() {
    // a's statement reaches c's binding through b, which has no
    // statement; c's statements are ready first.
    let a = include_str!("programs/init_cycle_run/three/a.hd");
    let b = include_str!("programs/init_cycle_run/three/b.hd");
    let c = include_str!("programs/init_cycle_run/three/c.hd");
    let main = include_str!("programs/init_cycle_run/three/main.hd");
    let out = run(
        "three",
        &[
            ("main.hd", main),
            ("ring/a.hd", a),
            ("ring/b.hd", b),
            ("ring/c.hd", c),
        ],
    );
    assert_eq!(out, "41\nc hello\na 41\n");
}

#[test]
fn statements_interleave_with_the_bindings_they_wait_on() {
    // Order: a's second statement (ready, earliest), b's `y`, a's `x`,
    // a's `z`, then b's rest. Both modules' inits split into runs.
    let a = include_str!("programs/init_cycle_run/interleave/a.hd");
    let b = include_str!("programs/init_cycle_run/interleave/b.hd");
    let main = include_str!("programs/init_cycle_run/interleave/main.hd");
    let out = run(
        "interleave",
        &[("main.hd", main), ("mix/a.hd", a), ("mix/b.hd", b)],
    );
    assert_eq!(out, "21\na second\nb y 1\nb w 21\n");
}

#[test]
fn statements_that_wait_on_each_other_stay_an_error() {
    let a = include_str!("programs/init_cycle_run/stuck/a.hd");
    let b = include_str!("programs/init_cycle_run/stuck/b.hd");
    let main = include_str!("programs/init_cycle_run/stuck/main.hd");
    let store = MemoryStore::default();
    let src = sources(&[("main.hd", main), ("shop/a.hd", a), ("shop/b.hd", b)]);
    // Cold, then warm: every run reports the cycle, once, at a's statement.
    for _ in 0..2 {
        let out = build_in(&store, &src);
        assert_eq!(
            out.diags.code,
            vec![Code::TopLevelReadBeforeInitialization],
            "{}",
            out.render()
        );
        assert!(out.wasm.is_none());
        let shown = out.render();
        assert!(
            shown.contains("shop/a.hd:25..41") && shown.contains("`b`"),
            "{shown}"
        );
    }
}

#[test]
fn an_entry_module_in_a_cycle_runs_its_statements_around_another_modules() {
    // The entry module `pkg.main` orders before `pkg.zed`: its first
    // statement runs first, its second waits on zed's `h`, so zed's `h`
    // splits the entry module's statements; zed's `record` comes last,
    // after every earlier ready statement.
    let main = include_str!("programs/init_cycle_run/entry/main.hd");
    let zed = include_str!("programs/init_cycle_run/entry/zed.hd");
    let out = run("entry", &[("main.hd", main), ("zed.hd", zed)]);
    assert_eq!(out, "21\nmain first\nmain m 21\nzed h 20\n");
}

#[test]
fn a_script_in_a_cycle_runs_its_statements_among_another_modules() {
    // The script's second statement waits on zed's `h`, so zed's first
    // statement runs between the script's; zed's last statement is ready
    // only after the script's last, so it runs after the script's
    // statements, and its panic shows that it ran.
    let main = include_str!("programs/init_cycle_run/script/main.hd");
    let zed = include_str!("programs/init_cycle_run/script/zed.hd");
    let ran = run_status("script", &[("main.hd", main), ("zed.hd", zed)]);
    assert!(!ran.status.success(), "script: the panic exits non-zero");
    assert_eq!(
        String::from_utf8_lossy(&ran.stdout),
        "main first\nmain m 21\n"
    );
    let stderr = String::from_utf8_lossy(&ran.stderr);
    assert!(stderr.contains("zed after 21"), "script: {stderr}");
}
