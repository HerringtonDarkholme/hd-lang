//! REPL inputs (repl-plan.md §2.2, job C4): each input is a synthetic
//! module `repl#N` outside every package folder, built with
//! `Goal::ReplInput` as one more module of a session. Its scope is
//! `src/lib.hd`'s, private declarations included (`cli.repl.package.lib`),
//! with each earlier input's top-level names nested in it. So only the
//! newest input is checked, emitted and linked. A later input reads and
//! assigns an earlier input's top-level bindings by name
//! (`cli.repl.declarations.bindings`, job C5).

use hd_cache::{CacheStore, OverlayStore};
use hd_driver::{Executor, Goal, Host, NoClock, Output, Packages, Session, build_packages};
use hd_project::{MemorySources, repl_file};
use hd_sched::SerialOrder;

const LIB: &str = "fn bonus() -> i32:\n    +10\n";

/// The spec's Redefinition example without its redeclaration: `price`
/// reads input 1's `rate`, and `count` is assigned by the input after the
/// one that binds it.
const INPUTS: [&str; 9] = [
    "rate := +3\n",
    "fn price(n: i32) -> i32:\n    n * rate\n",
    "fn total() -> i32:\n    price(+2)\n",
    "println(total())\n",
    "use std.cmp.{max}\n\nfn with_bonus() -> i32:\n    total() + bonus()\n",
    "println(max(with_bonus(), +0))\n",
    "let count = +1\n",
    "count = count + price(+1)\n",
    "println(count)\n",
];

/// A session: the package's files, the inputs that joined it, and what
/// their modules export.
struct Repl<'s> {
    src: MemorySources,
    store: &'s dyn CacheStore,
    session: Session,
}

impl Repl<'_> {
    /// Builds input `n` over the earlier inputs.
    fn build(&mut self, n: u32, text: &str) -> Output {
        self.src.insert(&repl_file(n), text);
        let host = Host {
            render_tir: &[],
            sources: &self.src,
            store: self.store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let packages = Packages {
            session: Some(&self.session),
            ..Packages::default()
        };
        build_packages(&host, "app", &packages, &Goal::ReplInput { input: n })
    }

    /// Builds input `n` and adds it to the session, as an input that runs
    /// without a panic does.
    fn input(&mut self, n: u32, text: &str) -> Output {
        let out = self.build(n, text);
        assert!(out.diags.is_empty(), "repl#{n}: {}", out.render());
        self.session
            .add(out.session_added.clone().expect("what the input adds"));
        out
    }

    /// Drops input `n` from the source set, as after a panic
    /// (`cli.repl.panic.binding`).
    fn forget(&mut self, n: u32) {
        let file = repl_file(n);
        self.src.retain(|p| p != file);
    }
}

fn repl(store: &dyn CacheStore) -> Repl<'_> {
    let mut src = MemorySources::default();
    src.insert("src/lib.hd", LIB);
    Repl {
        src,
        store,
        session: Session::start(),
    }
}

/// The session globals a module imports, by session key; validates it
/// first.
fn global_imports(wasm: &[u8]) -> Vec<String> {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut out = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::ImportSection(r) = payload.expect("payload") {
            for i in r.into_imports() {
                let i = i.expect("import");
                if i.module == hd_wasm::SESSION_MODULE
                    && matches!(i.ty, wasmparser::TypeRef::Global(_))
                {
                    out.push(i.name.to_owned());
                }
            }
        }
    }
    out
}

/// The debug names of a module's functions; validates it first.
fn function_names(wasm: &[u8]) -> Vec<String> {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut out = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::CustomSection(c) = payload.expect("payload")
            && let wasmparser::KnownCustom::Name(r) = c.as_known()
        {
            for n in r {
                if let wasmparser::Name::Function(m) = n.expect("name") {
                    out.extend(m.into_iter().map(|f| f.expect("name").name.to_owned()));
                }
            }
        }
    }
    out
}

#[test]
fn each_input_is_one_module_and_rechecks_only_itself() {
    // A build that leaves `src/lib.hd` and std checked in the inner
    // store, as an earlier session leaves the disk cache.
    let inner = crate::warm_store();
    repl(&inner).input(1, "use std.cmp.{max}\n\nprintln(max(+1, +0))\n");
    let overlay = OverlayStore::new(&inner);
    let mut repl = repl(&overlay);
    // The session's functions, each emitted by the first input that
    // reaches it and imported by every later one.
    let mut emitted: Vec<String> = Vec::new();
    for (i, text) in INPUTS.iter().enumerate() {
        let n = u32::try_from(i + 1).expect("inputs");
        let out = repl.input(n, text);
        let path = format!("app.$repl{n}");
        assert_eq!(out.counters.modules_checked, [path], "repl#{n}");
        let names = function_names(out.wasm.as_deref().expect("a module"));
        for f in names.iter().filter(|f| f.starts_with("app/")) {
            assert!(!emitted.contains(f), "repl#{n} emits {f} again");
            emitted.push(f.clone());
        }
    }
    // Input 4 calls input 3's `total`, which calls input 2's `price`;
    // input 8 calls `price` again, through an import.
    for f in [
        "app/$repl2/price#",
        "app/$repl3/total#",
        "app/$repl5/with_bonus#",
    ] {
        assert!(emitted.iter().any(|e| e.starts_with(f)), "{f}: {emitted:?}");
    }
    // The inputs' entries stay in the overlay.
    assert!(!overlay.memory().is_empty());
}

#[test]
fn a_panicked_input_leaves_the_session() {
    let store = crate::warm_store();
    let mut repl = repl(&store);
    repl.input(1, "fn kept() -> i32:\n    +1\n");
    // Input 2 is built, then panics when it runs: it joins nothing.
    let out = repl.build(2, "fn lost() -> i32:\n    +2\n\nprintln(kept())\n");
    assert!(out.diags.is_empty(), "{}", out.render());
    repl.forget(2);
    let out = repl.build(3, "println(kept() + lost())\n");
    let text = out.render();
    assert!(
        text.contains("repl#3:") && text.contains("`lost` is not defined"),
        "{text}"
    );
    assert!(out.wasm.is_none());
    // A rejected input leaves the source set too.
    repl.forget(3);
    repl.input(4, "println(kept())\n");
}

/// The `prices` example of live-execution.md §2.2 (std's `Map` stores by
/// index, not `insert`): input 2's function reads input 1's binding, and
/// input 3's statement changes its value. Each reaches the binding through
/// the global input 1 exports.
#[test]
fn later_inputs_read_an_earlier_binding_through_its_global() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    let out = repl.input(1, "let prices: mut Map[string, u32] = {}\n");
    let wasm = out.wasm.as_deref().expect("a module");
    assert!(global_imports(wasm).iter().all(|g| !g.starts_with("bind.")));
    // A function is emitted by the first input that reaches it: input 4.
    repl.input(
        2,
        "fn price(item: string) -> u32:\n    prices.get(item).unwrap_or(+0)\n",
    );
    let out = repl.input(3, "prices[\"tea\"] = +3\n");
    let imports = global_imports(out.wasm.as_deref().expect("a module"));
    let bind: Vec<&String> = imports.iter().filter(|g| g.starts_with("bind.")).collect();
    assert_eq!(bind.len(), 1, "{imports:?}");
    let out = repl.input(4, "println(price(\"tea\"))\n");
    assert_eq!(out.counters.modules_checked, ["app.$repl4"]);
    let wasm = out.wasm.as_deref().expect("a module");
    assert!(global_imports(wasm).contains(bind[0]));
    assert!(
        function_names(wasm)
            .iter()
            .any(|f| f.starts_with("app/$repl2/price#"))
    );
}

/// `cli.repl.declarations.bindings`: a declaration of a later input reads
/// an earlier input's binding, with the type that input fixed
/// (`cli.repl.input-types`). A `:=` binding stays not reassignable.
#[test]
fn a_later_declaration_reads_an_earlier_binding() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.input(1, "rate := +3\n");
    repl.input(2, "fn price(n: i32) -> i32:\n    n * rate\n");
    repl.input(3, "println(price(+2))\n");
    let out = repl.build(4, "rate = +4\n");
    let text = out.render();
    assert!(
        text.contains("repl#4:") && text.contains("non-reassignable-binding"),
        "{text}"
    );
    repl.forget(4);
    // An `i64` value does not fit the `i32` that input 1 fixed.
    let out = repl.build(5, "fn wide(n: i64) -> i32:\n    n * rate\n");
    let text = out.render();
    assert!(text.contains("type-mismatch"), "{text}");
    assert!(out.wasm.is_none());
}

/// An earlier input's bindings are in a later input's check key: the same
/// text after another first input is checked again.
#[test]
fn a_later_input_is_checked_again_after_another_earlier_input() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let later = "println(level + +1)\n";
    let mut first = repl(&store);
    first.input(1, "level := +3\n");
    first.input(2, later);
    let mut second = repl(&store);
    second.input(1, "level := \"high\"\n");
    let out = second.build(2, later);
    assert_eq!(out.counters.modules_checked, ["app.$repl2"]);
    let text = out.render();
    assert!(text.contains("type-mismatch"), "{text}");
    assert!(out.wasm.is_none());
}
