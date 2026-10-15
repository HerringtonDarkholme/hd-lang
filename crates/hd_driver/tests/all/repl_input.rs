//! REPL inputs (`f1c55020:future-work/compiler/repl-plan.md` §2.2, job C4): each input is a synthetic
//! module `repl#N` outside every package folder, built with
//! `Goal::ReplInput` as one more module of a session. Its scope is
//! `src/lib.hd`'s, private declarations included (`cli.repl.package.lib`),
//! with each earlier input's top-level names nested in it. So only the
//! newest input is checked, emitted and linked. A later input reads and
//! assigns an earlier input's top-level bindings by name
//! (`cli.repl.declarations.bindings`, job C5). An input that is one
//! expression writes its value line, and an input's top level is a driver
//! context (job C6). A session starts with its prelude, input 0, which
//! holds what showing a value needs, so small inputs stay small (job C8).

use hd_cache::{CacheStore, OverlayStore};
use hd_driver::{
    BuildProfile, Dependency, EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Output,
    Packages, Session, build_packages, build_repl_prelude,
};
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
/// their modules export; with `dep`, the package requires the package
/// `money` of those files.
struct Repl<'s> {
    src: MemorySources,
    store: &'s dyn CacheStore,
    session: Session,
    dep: Option<&'s MemorySources>,
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
        let deps: Vec<Dependency<'_>> = self
            .dep
            .iter()
            .map(|sources| Dependency {
                name: "money".to_owned(),
                identity: "/w/money".to_owned(),
                sources: *sources,
                requires: Vec::new(),
                fetched: false,
            })
            .collect();
        let packages = Packages {
            session: Some(&self.session),
            requires: deps.iter().map(|_| ("money".to_owned(), 1)).collect(),
            deps,
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
        dep: None,
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

/// Runtime helpers are named by kind, never by a `Debug` dump: no `{`, `[`
/// or `Hash128`, at most 64 characters, and unique within the module.
#[test]
fn runtime_helper_names_are_short_and_readable() {
    let inner = crate::warm_store();
    let overlay = OverlayStore::new(&inner);
    let mut repl = repl(&overlay);
    let mut helpers = 0;
    for (i, text) in INPUTS.iter().enumerate() {
        let n = u32::try_from(i + 1).expect("inputs");
        let out = repl.input(n, text);
        let names = function_names(out.wasm.as_deref().expect("a module"));
        let rt: Vec<&String> = names.iter().filter(|f| f.starts_with("rt:")).collect();
        helpers += rt.len();
        for name in &rt {
            assert!(
                !name.contains(['{', '[', ' ']) && !name.contains("Hash128") && name.len() <= 64,
                "repl#{n}: {name}"
            );
        }
        let mut sorted = rt.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), rt.len(), "repl#{n}: a helper name repeats");
    }
    assert!(helpers > 0, "no runtime helper was checked");
}

#[test]
fn each_input_is_one_module_and_rechecks_only_itself() {
    // A build that leaves `src/lib.hd` and std checked in the inner
    // store, as an earlier session leaves the disk cache.
    let inner = crate::warm_store();
    repl(&inner).input(1, "use std.cmp.{max}\n\nprintln(max(bonus(), +0))\n");
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

/// The `prices` example of `docs/future-work/live-execution.md` §2.2 (std's `Map` stores by
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

/// Builds `text` as input `n` of a session holding `first` as input 1 and
/// returns its diagnostics.
fn redeclared(first: &str, text: &str) -> String {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.input(1, first);
    let out = repl.build(2, text);
    let text = out.render();
    assert!(out.wasm.is_none(), "{first:?} then {text:?}");
    text
}

/// `cli.repl.redeclare.binding`: an input declares no function or
/// top-level binding that an earlier input declared, whichever kinds.
#[test]
fn an_input_does_not_redeclare_a_function_or_binding() {
    let f = "fn price() -> i32:\n    +1\n";
    for (first, text) in [
        (f, "fn price() -> i32:\n    +2\n"),
        ("rate := +3\n", "fn rate() -> i32:\n    +2\n"),
        ("let count = +1\n", "fn count() -> i32:\n    +2\n"),
        (f, "price := +2\n"),
        ("rate := +3\n", "rate := +4\n"),
        ("rate := +3\n", "let rate = +4\n"),
        ("let count = +1\n", "let count = +2\n"),
        ("let count = +1\n", "count := +2\n"),
    ] {
        let text = redeclared(first, text);
        assert_eq!(text.matches("duplicate-binding").count(), 1, "{text}");
        assert!(text.contains("is already declared by repl#1"), "{text}");
    }
}

/// `cli.repl.redeclare.type`: nor a type, trait, or type alias.
#[test]
fn an_input_does_not_redeclare_a_type() {
    for (first, text) in [
        ("data Point:\n    x: i32\n", "data Point:\n    y: i32\n"),
        ("enum Dir:\n    Up\n    Down\n", "data Dir:\n    y: i32\n"),
        (
            "trait Shape:\n    fn area(self) -> i32\n",
            "trait Shape:\n    fn size(self) -> i32\n",
        ),
        ("type Id = i32\n", "type Id = i64\n"),
    ] {
        let text = redeclared(first, text);
        assert_eq!(text.matches("duplicate-type").count(), 1, "{text}");
        assert!(text.contains("is already declared by repl#1"), "{text}");
    }
}

/// A `src/lib.hd` name is a name of the session too: one name, one
/// declaration.
#[test]
fn an_input_does_not_redeclare_a_library_name() {
    let text = redeclared("println(bonus())\n", "fn bonus() -> i32:\n    +1\n");
    assert!(text.contains("duplicate-binding"), "{text}");
    assert!(text.contains("declared by src/lib.hd"), "{text}");
}

/// A `let` binding is assigned, not declared again; a panicked input's
/// names stay free (`cli.repl.panic.binding`).
#[test]
fn assignment_and_the_names_of_a_panicked_input_are_fine() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.input(1, "let count = +1\n");
    repl.input(2, "count = count + 1\n");
    let out = repl.build(3, "fn lost() -> i32:\n    +2\n\nlost_total := +5\n");
    assert!(out.diags.is_empty(), "{}", out.render());
    repl.forget(3);
    repl.input(4, "fn lost() -> i32:\n    +3\n\nlost_total := +6\n");
}

/// A program is a plain module: the same names in two inputs do not
/// matter there.
#[test]
fn a_program_is_unaffected() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut src = MemorySources::default();
    src.insert("src/lib.hd", LIB);
    src.insert("src/other.hd", "fn bonus() -> i32:\n    +1\n");
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_packages(&host, "app", &Packages::default(), &Goal::Analyze);
    assert!(out.diags.is_empty(), "{}", out.render());
}

/// Whether `needle` occurs in `wasm`, as a string constant's bytes do.
fn holds(wasm: &[u8], needle: &str) -> bool {
    wasm.windows(needle.len()).any(|w| w == needle.as_bytes())
}

/// Whether an input module writes a value line (`cli.repl.value`).
fn shows_a_value(wasm: &[u8]) -> bool {
    function_names(wasm)
        .iter()
        .any(|f| f.starts_with("std/format/repl_value#"))
}

/// `cli.repl.value`: an input that is one expression writes its value as
/// `dbg` prints it, then its type, which the module holds as diagnostics
/// show it: the literal's default `usize`, a function's signature, and no
/// `mut`.
#[test]
fn an_expression_input_writes_its_value_line() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    let out = repl.input(1, "[3, 6]\n");
    let wasm = out.wasm.as_deref().expect("a module");
    assert!(shows_a_value(wasm), "{:?}", function_names(wasm));
    assert!(holds(wasm, "List[usize]"));
    // The value line goes to standard output (`cli.repl.value.stream`),
    // through the runtime's `stdout` import, not `Console`.
    let (imports, _) = imports_and_exports(wasm);
    assert!(
        imports.contains(&("hd:rt".to_owned(), "stdout".to_owned())),
        "{imports:?}"
    );
    repl.input(2, "fn twice(x: i32) -> i32:\n    x * +2\n");
    let out = repl.input(3, "twice\n");
    let wasm = out.wasm.as_deref().expect("a module");
    assert!(holds(wasm, "<fn twice(i32) -> i32>"));
    assert!(holds(wasm, "fn(i32) -> i32"));
    repl.input(4, "let names: mut List[string] = []\n");
    let out = repl.input(5, "names\n");
    let wasm = out.wasm.as_deref().expect("a module");
    assert!(holds(wasm, "List[string]") && !holds(wasm, "mut List"));
}

/// An input of declarations and statements writes no value line, nor
/// does a `void` call or an expression after a declaration.
#[test]
fn other_inputs_write_no_value_line() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    for (n, text) in [
        "xs := [+1, +2]\n",
        "let total = +0\n",
        "total = total + xs[0]\n",
        "println(total)\n",
        "fn half(x: i32) -> i32:\n    x / +2\n\nhalf(+4)\n",
        "fn more() -> i32:\n    +1\n",
    ]
    .iter()
    .enumerate()
    {
        let n = u32::try_from(n + 1).expect("inputs");
        let out = repl.input(n, text);
        let wasm = out.wasm.as_deref().expect("a module");
        assert!(!shows_a_value(wasm), "repl#{n}");
    }
}

/// An input's top level is a driver context, as `main!`'s body is: it
/// may make bang calls. A program's top level still may not
/// (`module.init.script-not-driver`).
#[test]
fn an_input_may_make_bang_calls_at_top_level() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    let text = "use std.time.{sleep, Duration}\n\nsleep!(Duration.milliseconds(+1))\n";
    repl.input(1, text);
    let mut src = MemorySources::default();
    src.insert("src/main.hd", text);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = build_packages(&host, "app", &Packages::default(), &goal);
    assert!(
        out.render().contains("bang-call-outside-suspension"),
        "{}",
        out.render()
    );
}

/// Builds the session's prelude over `store` and adds it to `repl`'s
/// session, as a session that starts does once the prelude has run.
fn start_with_prelude(repl: &mut Repl<'_>, store: &dyn CacheStore) -> Output {
    let host = Host {
        render_tir: &[],
        sources: &repl.src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build_repl_prelude(&host, BuildProfile::Checked, EngineTarget::default());
    assert!(out.diags.is_empty(), "{}", out.render());
    repl.session
        .add(out.session_added.clone().expect("what the prelude adds"));
    out
}

/// A module's imports and exports as (module, name), (name); validates it
/// first.
fn imports_and_exports(wasm: &[u8]) -> (Vec<(String, String)>, Vec<String>) {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut imports = Vec::new();
    let mut exports = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    let i = i.expect("import");
                    imports.push((i.module.to_owned(), i.name.to_owned()));
                }
            }
            wasmparser::Payload::ExportSection(r) => {
                for e in r {
                    exports.push(e.expect("export").name.to_owned());
                }
            }
            _ => {}
        }
    }
    (imports, exports)
}

/// The prelude owns the session's memory and runtime globals, and the
/// first real input imports them with what its value line calls instead
/// of linking its own copies: a `1 + 2` input stays under 2 KB (it is
/// about 9 KB without the prelude), and so does `[3, 6]`.
#[test]
fn the_first_input_links_against_the_prelude() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    let prelude = start_with_prelude(&mut repl, &inner);
    let (imports, exports) = imports_and_exports(prelude.wasm.as_deref().expect("a module"));
    assert!(imports.iter().all(|(m, _)| m != hd_wasm::SESSION_MODULE));
    assert!(exports.iter().any(|e| e == hd_wasm::MEMORY_KEY));
    for g in hd_wasm::rt::runtime_globals() {
        let key = hd_wasm::global_key(&g);
        assert!(exports.contains(&key), "{key}");
    }
    for (n, text) in ["1 + 2\n", "[3, 6]\n"].iter().enumerate() {
        let n = u32::try_from(n + 1).expect("inputs");
        let out = repl.input(n, text);
        let wasm = out.wasm.as_deref().expect("a module");
        assert!(wasm.len() < 2048, "repl#{n} is {} bytes", wasm.len());
        let (imports, _) = imports_and_exports(wasm);
        let session = |f: &dyn Fn(&str) -> bool| {
            imports
                .iter()
                .any(|(m, i)| m == hd_wasm::SESSION_MODULE && f(i))
        };
        assert!(session(&|i| i == hd_wasm::MEMORY_KEY), "{imports:?}");
        assert!(session(&|i| i.starts_with("inst.")), "{imports:?}");
        let names = function_names(wasm);
        assert!(
            names.iter().all(|f| !f.starts_with("std/")),
            "repl#{n}: {names:?}"
        );
    }
}

/// The prelude is the same for every session of a toolchain: the second
/// session reads it from the store, with no stage run, and adds the same
/// to its session.
#[test]
fn the_prelude_is_cached_per_toolchain() {
    let store = hd_cache::MemoryStore::default();
    let mut first = repl(&store);
    let built = start_with_prelude(&mut first, &store);
    assert_eq!(built.counters.misses.get("link"), Some(&1));
    let mut second = repl(&store);
    let cached = start_with_prelude(&mut second, &store);
    assert_eq!(cached.counters.hits.get("link"), Some(&1));
    assert!(cached.counters.modules_checked.is_empty());
    assert_eq!(cached.wasm, built.wasm);
    assert_eq!(cached.session_added, built.session_added);
    // Another build profile is another prelude.
    let host = Host {
        render_tir: &[],
        sources: &second.src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let release = build_repl_prelude(&host, BuildProfile::Release, EngineTarget::default());
    assert_eq!(release.counters.misses.get("link"), Some(&1));
    assert_ne!(release.wasm, built.wasm);
    // The prelude is built for the session's target, in the dev pipeline.
    // `Wasmtime` lowers closures unlike the default target, so its key
    // differs: its prelude is another entry, cached in its turn.
    let wasmtime = build_repl_prelude(&host, BuildProfile::Checked, EngineTarget::Wasmtime);
    assert_eq!(
        (wasmtime.target, wasmtime.level),
        (EngineTarget::Wasmtime, OptLevel::Dev)
    );
    assert_eq!(wasmtime.counters.misses.get("link"), Some(&1));
    let again = build_repl_prelude(&host, BuildProfile::Checked, EngineTarget::Wasmtime);
    assert_eq!(again.counters.hits.get("link"), Some(&1));
    assert_eq!(again.wasm, wasmtime.wasm);
}

/// A session is one scope, as a module is (`cli.repl.package.lib`): a
/// later input uses the private fields and methods of an earlier input's
/// type as its own module's, constructing it included.
#[test]
fn an_input_sees_the_private_members_of_earlier_inputs() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.input(
        1,
        "data Point:\n    x: i32\n    y: i32\n\nimpl Point:\n    fn sum(self) -> i32:\n        self.x + self.y\n",
    );
    repl.input(2, "p := Point { x: +12, y: +23 }\n");
    repl.input(3, "println(p.sum() + p.x)\n");
    repl.input(4, "moved := Point { ...p, x: +1 }\n");
}

/// The session acts as code inside `src/lib.hd` (`cli.repl.package.lib`):
/// an input reads, constructs and calls the private members of its types.
#[test]
fn an_input_sees_the_private_members_of_the_library() {
    let lib = "data Secret:\n    code: i32\n\nimpl Secret:\n    fn twice(self) -> i32:\n        self.code * +2\n\nfn secret() -> Secret:\n    Secret { code: +7 }\n";
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/lib.hd", lib);
    repl.input(1, "println(secret().code + secret().twice())\n");
    repl.input(2, "s := Secret { code: +1 }\n");
}

/// Security check: the session is `src/lib.hd`'s scope only. The private
/// members of the package's other modules and of its dependencies stay
/// private to an input, as they are to `src/lib.hd`.
#[test]
fn other_modules_and_dependencies_keep_their_private_members() {
    let util = "pub data Boxed:\n    v: i32\n\nimpl Boxed:\n    fn peek(self) -> i32:\n        self.v\n\npub fn boxed() -> Boxed:\n    Boxed { v: +1 }\n";
    let mut money = MemorySources::default();
    money.insert(
        "src/lib.hd",
        "pub data Coin:\n    cents: i32\n\npub fn coin() -> Coin:\n    Coin { cents: +5 }\n",
    );
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/util.hd", util);
    repl.dep = Some(&money);
    repl.input(
        1,
        "use self.util.{Boxed, boxed}\nuse dep.money.{Coin, coin}\n",
    );
    for (n, text) in [
        (2, "boxed().v\n"),
        (3, "boxed().peek()\n"),
        (4, "Boxed { v: +2 }\n"),
        (5, "coin().cents\n"),
        (6, "Coin { cents: +2 }\n"),
    ] {
        let out = repl.build(n, text);
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(codes, ["private-member"], "{text:?}: {}", out.render());
        repl.forget(n);
    }
}

/// A build reads the earlier inputs from the session, not from the store,
/// and gives them no task but their skim: its store reads and its other
/// tasks are the same at input 12 as at input 3 (`f1c55020:future-work/compiler/repl-plan.md` §4).
#[test]
fn a_build_reads_the_earlier_inputs_from_the_session() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    let mut work = Vec::new();
    for n in 1..=12 {
        let out = repl.input(n, &format!("w{n} := +{n}\n"));
        // Store reads, hit or miss: the warm store is shared by tests.
        let mut reads = out.counters.hits.clone();
        for (kind, n) in &out.counters.misses {
            *reads.entry(kind).or_default() += n;
        }
        let mut tasks = out.counters.tasks.clone();
        tasks.remove("Skim");
        work.push((reads, tasks));
    }
    assert_eq!(work[2], work[11]);
    // Input 12 still reads input 1's binding.
    repl.input(13, "println(w1 + w12)\n");
}

/// A single use binds its name for later inputs too, as a grouped one does.
#[test]
fn a_single_use_reaches_later_inputs() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.input(1, "use std.cmp.max\nuse std.cmp.min as least\n");
    repl.input(2, "println(max(+1, +2) + least(+3, +4))\n");
}

/// A use that names a module binds it for later inputs too
/// (`cli.repl.uses.other`), under its alias when it has one.
#[test]
fn a_module_use_reaches_later_inputs() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.input(1, "use std.path\nuse std.cmp as c\n");
    repl.input(
        2,
        "println(path.Path(\"a\").join(\"b\").parent().is_some())\n",
    );
    repl.input(3, "println(c.max(+1, +2))\n");
}

/// `cli.repl.redeclare.binding`: a later input binds no name that an
/// earlier module use bound.
#[test]
fn an_input_does_not_redeclare_a_module_name() {
    for (first, text) in [
        ("use std.path\n", "fn path() -> i32:\n    +2\n"),
        ("use std.path as p\n", "p := +2\n"),
    ] {
        let text = redeclared(first, text);
        assert_eq!(text.matches("duplicate-binding").count(), 1, "{text}");
        assert!(text.contains("is already declared by repl#1"), "{text}");
    }
}

/// A redeclared module name is reported against the input that used the
/// module, not against a later input that merely reads the name.
#[test]
fn a_redeclared_module_name_names_the_first_input() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.input(1, "use std.path\n");
    repl.input(2, "println(path.Path(\"a\").parent().is_some())\n");
    repl.input(3, "use std.cmp as c\n");
    let out = repl.build(4, "fn path() -> i32:\n    +2\n");
    let text = out.render();
    assert!(out.wasm.is_none(), "{text}");
    assert!(text.contains("is already declared by repl#1"), "{text}");
}

/// A session is one module for implementation placement (`trait.own.*`):
/// an input implements a trait or adds methods for a type that an earlier
/// input or `src/lib.hd` declares, and a later input calls them.
#[test]
fn an_input_implements_for_an_earlier_input_s_or_the_library_s_type() {
    let lib = "trait Named:\n    fn name(self) -> string\n\ndata Tag:\n    pub x: i32\n";
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/lib.hd", lib);
    repl.input(1, "data Point:\n    pub x: i32\n    pub y: i32\n");
    repl.input(
        2,
        "impl Display for Point:\n    fn to_string(self) -> string: \"(${self.x}, ${self.y})\"\n",
    );
    repl.input(
        3,
        "impl Point:\n    fn sum(self) -> i32:\n        self.x + self.y\n",
    );
    repl.input(
        4,
        "impl Named for Point:\n    fn name(self) -> string: \"point\"\n",
    );
    repl.input(
        5,
        "impl Named for Tag:\n    fn name(self) -> string: \"tag\"\n",
    );
    repl.input(6, "impl Tag:\n    fn twice(self) -> i32: self.x * +2\n");
    let p = "p := Point { x: +1, y: +2 }\n\nprintln(\"${p} ${p.sum()} ${p.name()} ${Tag { x: +3 }.name()} ${Tag { x: +3 }.twice()}\")\n";
    let out = repl.input(7, p);
    assert!(out.wasm.is_some(), "{}", out.render());
}

/// Placement keeps its rule for the package's other modules and its
/// dependencies: an input is not their module.
#[test]
fn an_input_does_not_implement_for_another_module_s_type() {
    let util =
        "pub trait Shown:\n    fn shown(self) -> string\n\npub data Boxed:\n    pub v: i32\n";
    let mut money = MemorySources::default();
    money.insert("src/lib.hd", "pub data Coin:\n    pub cents: i32\n");
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/util.hd", util);
    repl.dep = Some(&money);
    repl.input(
        1,
        "use self.util.{Shown, Boxed}\nuse dep.money.{Coin}\n\ndata Local:\n    pub n: i32\n",
    );
    for (n, text, code) in [
        (
            2,
            "impl Boxed:\n    fn peek(self) -> i32: self.v\n",
            "nonlocal-impl",
        ),
        (
            3,
            "impl Shown for Boxed:\n    fn shown(self) -> string: \"b\"\n",
            "nonlocal-impl",
        ),
        (
            4,
            "impl Coin:\n    fn peek(self) -> i32: self.cents\n",
            "orphan-impl",
        ),
        (
            5,
            "impl Display for Coin:\n    fn to_string(self) -> string: \"c\"\n",
            "orphan-impl",
        ),
    ] {
        let out = repl.build(n, text);
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(codes, [code], "{text:?}: {}", out.render());
        repl.forget(n);
    }
    // The input's own type may implement the other module's trait.
    repl.input(
        6,
        "impl Shown for Local:\n    fn shown(self) -> string: \"l\"\n",
    );
}

/// `cli.repl.redefine.impl`: an implementation that overlaps one of an
/// earlier input or of `src/lib.hd` is `overlapping-impl`, as in one
/// module, and the earlier one stays.
#[test]
fn an_input_does_not_overlap_an_earlier_implementation() {
    let lib = "trait Named:\n    fn name(self) -> string\n\ndata Tag:\n    pub x: i32\n\nimpl Named for Tag:\n    fn name(self) -> string: \"lib\"\n";
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/lib.hd", lib);
    repl.input(1, "data Point:\n    pub x: i32\n\nimpl Named for Point:\n    fn name(self) -> string: \"one\"\n");
    for (n, text) in [
        (
            2,
            "impl Named for Tag:\n    fn name(self) -> string: \"repl\"\n",
        ),
        (
            3,
            "impl Named for Point:\n    fn name(self) -> string: \"two\"\n",
        ),
    ] {
        let out = repl.build(n, text);
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(codes, ["overlapping-impl"], "{text:?}: {}", out.render());
        assert!(
            out.render().starts_with(&format!("error: repl#{n}:")),
            "{}",
            out.render()
        );
        repl.forget(n);
    }
    repl.input(
        4,
        "println(Point { x: +1 }.name() + Tag { x: +1 }.name())\n",
    );
}

/// The later implementation is the one reported, by input number: input
/// 10 comes after input 9.
#[test]
fn the_newer_input_s_implementation_is_the_overlapping_one() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.input(
        1,
        "trait Named:\n    fn name(self) -> string\n\ndata Point:\n    pub x: i32\n",
    );
    for n in 2..9 {
        repl.input(n, &format!("v{n} := +{n}\n"));
    }
    repl.input(
        9,
        "impl Named for Point:\n    fn name(self) -> string: \"nine\"\n",
    );
    let out = repl.build(
        10,
        "impl Named for Point:\n    fn name(self) -> string: \"ten\"\n",
    );
    let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
    assert_eq!(codes, ["overlapping-impl"], "{}", out.render());
    assert!(
        out.render().starts_with("error: repl#10:"),
        "{}",
        out.render()
    );
}

/// An earlier input read from the session has no task, so a later
/// input's interface waits for the modules that input reaches: here
/// `src/util.hd`, which only input 1 uses. On the pool a task runs as soon
/// as its dependencies are done.
#[test]
fn a_later_input_waits_for_the_modules_an_earlier_input_uses() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut src = MemorySources::default();
    src.insert("src/util.hd", "pub fn double(x: i32) -> i32: x * +2\n");
    let mut repl = Repl {
        src,
        store: &store,
        session: Session::start(),
        dep: None,
    };
    repl.input(1, "use self.util.{double}\n");
    repl.src.insert(&repl_file(2), "println(double(+4))\n");
    let host = Host {
        render_tir: &[],
        sources: &repl.src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Pool(4),
    };
    let packages = Packages {
        session: Some(&repl.session),
        ..Packages::default()
    };
    let out = build_packages(&host, "app", &packages, &Goal::ReplInput { input: 2 });
    assert!(out.diags.is_empty(), "{}", out.render());
    assert!(out.wasm.is_some(), "{:?}", out.report);
}

/// A session is one module for derivation blocks and templates
/// (`annot.block.module`, `annot.template.module`): a block in a later
/// input derives for a type that an earlier input or `src/lib.hd`
/// declares, a template in an input is for a trait of the library, and a
/// later input uses what they derive.
#[test]
fn an_input_derives_for_an_earlier_input_s_or_the_library_s_type() {
    let lib = "use std.structure.{Structure, Field, Variant, Describer}\n\ntrait Named:\n    fn title() -> string\n\ndata MemberCount:\n    count: i32\n\nimpl[S] Describer[S] for MemberCount:\n    type Error = never\n\n    fn variant(mut self, v: Variant[S]) -> Result[void, never]:\n        .Ok(())\n\n    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:\n        self.count = self.count + 1\n        .Ok(())\n\ndata Tag:\n    pub x: i32\n";
    let template = "use std.structure.Structure\n\nimpl[T] Named for T by Structure:\n    fn title() -> string:\n        let mut d = MemberCount { count: 0 }\n        _ := T::describe(d)\n        \"${Structure::name()}/${d.count}\"\n";
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/lib.hd", lib);
    repl.input(
        1,
        "data Point:\n    pub x: i32\n    pub y: i32\n\n@derive(Eq)\ndata Pair:\n    pub a: i32\n",
    );
    repl.input(2, template);
    repl.input(
        3,
        "use std.structure.Structure\n\nimpl Eq for Point by Structure\nimpl Hash for Point by Structure\nimpl Named for Point by Structure\nimpl Named for Tag by Structure\nimpl Hash for Pair by Structure\n",
    );
    let p = "println(\"${Point { x: +1, y: +2 } == Point { x: +1, y: +2 }} ${Point::title()} ${Tag::title()} ${Pair { a: +1 } == Pair { a: +1 }}\")\n";
    let out = repl.input(4, p);
    assert!(out.wasm.is_some(), "{}", out.render());
}

/// Derivation placement keeps its rule for the package's other modules
/// and its dependencies: an input is not their module.
#[test]
fn an_input_does_not_derive_for_another_module_s_type() {
    let util =
        "pub trait Shown:\n    fn shown(self) -> string\n\npub data Boxed:\n    pub v: i32\n";
    let mut money = MemorySources::default();
    money.insert("src/lib.hd", "pub data Coin:\n    pub cents: i32\n");
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/util.hd", util);
    repl.dep = Some(&money);
    let first = "use std.structure.{Structure, Field, Variant, Describer}\nuse self.util.{Shown, Boxed}\nuse dep.money.{Coin}\n\ndata Local:\n    pub n: i32\n\ntrait Enc:\n    fn enc() -> string\n\ndata Count:\n    count: i32\n\nimpl[S] Describer[S] for Count:\n    type Error = never\n\n    fn variant(mut self, v: Variant[S]) -> Result[void, never]:\n        .Ok(())\n\n    fn member[F](mut self, h: Field[S, F]) -> Result[void, never]:\n        self.count = self.count + 1\n        .Ok(())\n\nimpl[T] Enc for T by Structure:\n    fn enc() -> string:\n        let mut d = Count { count: 0 }\n        _ := T::describe(d)\n        \"${d.count}\"\n";
    repl.input(1, first);
    for (n, text, code) in [
        // The trait and the target both belong to other modules.
        (2, "impl Eq for Boxed by Structure\n", "nonlocal-impl"),
        (3, "impl Eq for Coin by Structure\n", "orphan-impl"),
        // The input's own trait derives for another module's type.
        (
            4,
            "impl Enc for Boxed by Structure\n",
            "misplaced-derivation",
        ),
        (
            5,
            "impl Enc for Coin by Structure\n",
            "misplaced-derivation",
        ),
        // A template for another module's trait.
        (
            6,
            "impl[T] Shown for T by Structure:\n    fn shown(self) -> string:\n        T::name()\n",
            "misplaced-derivation",
        ),
    ] {
        let out = repl.build(n, text);
        let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
        assert_eq!(codes, [code], "{text:?}: {}", out.render());
        repl.forget(n);
    }
    // The input's own type may derive.
    repl.input(
        7,
        "impl Eq for Local by Structure\nimpl Enc for Local by Structure\n",
    );
}

/// `src/lib.hd` for the member-line tests: a `Column` fact and a `Columns`
/// template whose walker writes each member's column name, its `Column`
/// fact or else its own name.
const COLUMNS: &str = "use std.structure.{Structure, Field, Variant, Walker}

data Column:
    name: string

fn column(name: string) -> Column:
    Column { name: name }

trait Columns:
    fn columns(self) -> string

data Header:
    out: string

impl[S] Walker[S] for Header:
    type Error = never

    fn variant(mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](mut self, h: Field[S, F], value: F) -> Result[void, never]:
        name := match h.info.facts.find::[Column]():
            .Some(c) => c.name
            .None => h.info.name
        self.out = self.out + name + \";\"
        .Ok(())

impl[T] Columns for T by Structure:
    fn columns(self) -> string:
        let mut w = Header { out: \"\" }
        _ := Structure::walk(self, w)
        w.out

data Tag:
    x: i32
";

/// A derivation block's member lines edit the facts of an earlier input's
/// type as within one module (`annot.block.lines`, `annot.line.extend`,
/// `annot.line.replace`, `annot.line.omit`).
#[test]
fn an_input_s_member_lines_edit_an_earlier_type_s_facts() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/lib.hd", COLUMNS);
    repl.input(
        1,
        "data Row:\n    @column(\"row_id\")\n    id: i32\n    total: i32\n    cache: i32 = +0\n",
    );
    repl.input(
        2,
        "use std.structure.Structure\n\nimpl Columns for Row by Structure:\n    total = [column(\"sum_total\")]\n\nimpl Hash for Row by Structure:\n    cache = pass\n",
    );
    let out = repl.input(3, "println(Row { id: +1, total: +2 }.columns())\n");
    let wasm = out.wasm.as_deref().expect("wasm");
    assert!(holds(wasm, "row_id") && holds(wasm, "sum_total"));
}

/// A block with no trait in one input writes declaration facts of an
/// earlier type that a later input's derivation sees
/// (`annot.no-trait.declaration-facts`, `annot.no-trait.then-blocks`).
#[test]
fn a_later_derivation_sees_an_input_s_block_with_no_trait() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/lib.hd", COLUMNS);
    repl.input(
        1,
        "data Row:\n    @column(\"row_id\")\n    id: i32\n    total: i32\n",
    );
    repl.input(
        2,
        "use std.structure.Structure\n\nimpl Row by Structure:\n    total += [column(\"sum_total\")]\n",
    );
    repl.input(3, "impl Columns for Row by Structure\n");
    let out = repl.input(4, "println(Row { id: +1, total: +2 }.columns())\n");
    let wasm = out.wasm.as_deref().expect("wasm");
    assert!(holds(wasm, "row_id") && holds(wasm, "sum_total"));
    // Both blocks in one input, for a type of the library.
    repl.input(
        5,
        "impl Tag by Structure:\n    x += [column(\"tag_x\")]\n\nimpl Columns for Tag by Structure\n",
    );
    let out = repl.input(6, "println(Tag { x: +1 }.columns())\n");
    assert!(holds(out.wasm.as_deref().expect("wasm"), "tag_x"));
    // A type has one block with no trait in the whole session.
    let out = repl.build(7, "impl Row by Structure:\n    id += [+1]\n");
    let codes: Vec<&str> = out.diags.code.iter().map(|c| c.as_str()).collect();
    assert_eq!(codes, ["overlapping-impl"], "{}", out.render());
}

/// The codes of the diagnostics of REPL input `n` with source `text`.
fn input_codes(repl: &mut Repl<'_>, n: u32, text: &str) -> Vec<String> {
    let out = repl.build(n, text);
    out.diags
        .code
        .iter()
        .map(|c| c.as_str().to_owned())
        .collect()
}

/// A type's fact of one concrete type may not repeat across inputs, as
/// within one module: the declaration's and a later block's, or a block
/// with no trait's and a derivation block's (`annot.line.duplicate`).
#[test]
fn a_fact_type_repeated_across_inputs_is_a_duplicate() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let decl = "data Row:\n    @\"cents\"\n    total: i32\n    id: i32\n";
    let block =
        "use std.structure.Structure\n\nimpl Row by Structure:\n    total += [\"checked\"]\n";
    // The declaration and a later input's block with no trait.
    let mut r = repl(&store);
    r.input(1, decl);
    assert_eq!(
        input_codes(&mut r, 2, block),
        ["duplicate-fact"],
        "across inputs"
    );
    // As one module.
    let mut one = repl(&store);
    assert_eq!(
        input_codes(&mut one, 1, &format!("{decl}\n{block}")),
        ["duplicate-fact"],
        "one module"
    );
    // Distinct fact types are fine, on the same and on another member.
    let mut r = repl(&store);
    r.input(1, decl);
    r.input(
        2,
        "use std.structure.Structure\n\nimpl Row by Structure:\n    total += [+5]\n    id += [\"checked\"]\n",
    );
    // A `=` line drops the declaration's fact first.
    let mut r = repl(&store);
    r.input(1, decl);
    r.input(
        2,
        "use std.structure.Structure\n\nimpl Row by Structure:\n    total = [\"checked\"]\n",
    );
}

/// A block with no trait in one input and a derivation block in a later
/// one write facts of one type (`annot.no-trait.then-blocks`).
#[test]
fn a_fact_type_repeated_across_two_blocks_is_a_duplicate() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let decl = "data Row:\n    total: i32\n";
    let traitless =
        "use std.structure.Structure\n\nimpl Row by Structure:\n    total += [\"cents\"]\n";
    let derived = "impl Eq for Row by Structure:\n    total += [\"checked\"]\n";
    let mut r = repl(&store);
    r.input(1, decl);
    r.input(2, traitless);
    assert_eq!(
        input_codes(&mut r, 3, derived),
        ["duplicate-fact"],
        "across inputs"
    );
    let mut one = repl(&store);
    assert_eq!(
        input_codes(&mut one, 1, &format!("{decl}\n{traitless}\n{derived}")),
        ["duplicate-fact"],
        "one module"
    );
    // A distinct fact type is fine.
    let mut r = repl(&store);
    r.input(1, decl);
    r.input(2, traitless);
    r.input(3, "impl Eq for Row by Structure:\n    total += [+5]\n");
}

/// The `Severity::Warning` codes of a build.
fn warning_codes(out: &Output) -> Vec<String> {
    out.diags
        .content_order()
        .into_iter()
        .filter(|i| out.diags.severity[*i] == hd_diag::Severity::Warning)
        .map(|i| out.diags.code[i].as_str().to_owned())
        .collect()
}

/// Derivation blocks for one type in different inputs whose member lines
/// differ warn `derivation-line-drift` on the later block, as within one
/// module (`annot.line.drift`); the warning does not stop the input.
#[test]
fn blocks_across_inputs_with_different_lines_drift() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let decl = "data Row:\n    total: i32 = 0\n    id: i32 = 0\n";
    let eq = "use std.structure.Structure\n\nimpl Eq for Row by Structure:\n    total = pass\n";
    let hash = "use std.structure.Structure\n\nimpl Hash for Row by Structure:\n    id = pass\n";
    let same = "use std.structure.Structure\n\nimpl Hash for Row by Structure:\n    total = pass\n";
    // Different lines warn on the later block, which still joins.
    let mut r = repl(&store);
    r.input(1, decl);
    r.input(2, eq);
    let out = r.build(3, hash);
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert_eq!(
        warning_codes(&out),
        ["derivation-line-drift"],
        "{}",
        out.render()
    );
    assert!(out.wasm.is_some(), "{}", out.render());
    r.session
        .add(out.session_added.clone().expect("what the input adds"));
    let out = r.build(4, "println(+1)\n");
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert!(out.wasm.is_some(), "{}", out.render());
    // Identical lines do not warn.
    let mut r = repl(&store);
    r.input(1, decl);
    r.input(2, eq);
    let out = r.build(3, same);
    assert!(out.diags.is_empty(), "{}", out.render());
    // As one module.
    let mut one = repl(&store);
    let out = one.build(1, &format!("{decl}\n{eq}\n{hash}"));
    assert!(!out.diags.has_errors(), "{}", out.render());
    assert_eq!(
        warning_codes(&out),
        ["derivation-line-drift"],
        "{}",
        out.render()
    );
}

/// The modules of the package that a build checked: std's depend on what
/// the shared warm store holds already.
fn package_checked(out: &Output) -> Vec<&str> {
    out.counters
        .modules_checked
        .iter()
        .map(String::as_str)
        .filter(|m| m.starts_with("app"))
        .collect()
}

/// A source set that records the files a build reads.
struct Reading<'s> {
    inner: &'s MemorySources,
    read: std::sync::Mutex<Vec<String>>,
}

impl hd_project::SourceSet for Reading<'_> {
    fn list(&self) -> Vec<hd_project::SourceEntry> {
        self.inner.list()
    }
    fn read(&self, path: &str) -> Option<std::sync::Arc<[u8]>> {
        self.read.lock().expect("reads").push(path.to_owned());
        self.inner.read(path)
    }
}

/// A package file that does not check, `src/main.hd` or a module no input
/// uses, stops no input: an input builds only the modules it reaches
/// (`cli.repl.package.lib`, `cli.repl.uses.other`), and reads no other.
#[test]
fn a_broken_module_no_input_reaches_is_never_read() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert(
        "src/main.hd",
        "pub fn main() -> void:\n    let n: i32 = \"no\"\n",
    );
    repl.src
        .insert("src/junk.hd", "pub fn junk() -> i32:\n    \"no\"\n\nfn (\n");
    repl.src
        .insert("src/util.hd", "pub fn double(x: i32) -> i32:\n    x * +2\n");
    let out = repl.input(1, "1 + 2\n");
    assert_eq!(package_checked(&out), ["app.$repl1"]);
    repl.input(2, "bonus() + +1\n");
    repl.input(3, "use self.util.{double}\n");
    repl.src.insert(&repl_file(4), "double(bonus())\n");
    let reading = Reading {
        inner: &repl.src,
        read: std::sync::Mutex::default(),
    };
    let host = Host {
        render_tir: &[],
        sources: &reading,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        session: Some(&repl.session),
        ..Packages::default()
    };
    let out = build_packages(&host, "app", &packages, &Goal::ReplInput { input: 4 });
    assert!(out.diags.is_empty(), "{}", out.render());
    let read = reading.read.into_inner().expect("reads");
    for file in ["src/lib.hd", "src/util.hd", "repl#3", "repl#4"] {
        assert!(read.iter().any(|r| r == file), "{file}: {read:?}");
    }
    for file in ["src/main.hd", "src/junk.hd"] {
        assert!(!read.iter().any(|r| r == file), "{file}: {read:?}");
    }
}

/// A session loads `src/lib.hd` lazily: an input that names none of its
/// names neither builds it nor sees it, so a library that does not check
/// stops only the first input that names it, with the library's error.
/// Once the library is fixed, the session rebuilt over it, as `:reload`
/// rebuilds it, runs that input.
#[test]
fn a_broken_library_stops_only_the_inputs_that_name_it() {
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src
        .insert("src/lib.hd", "fn bonus() -> i32:\n    \"ten\"\n");
    let out = repl.input(1, "1 + 2\n");
    assert_eq!(package_checked(&out), ["app.$repl1"]);
    repl.input(2, "x := +5\n");
    let out = repl.build(3, "x + bonus()\n");
    assert!(out.diags.has_errors(), "{}", out.render());
    assert!(
        out.render().starts_with("error: src/lib.hd:"),
        "{}",
        out.render()
    );
    repl.forget(3);
    repl.input(4, "x * +2\n");
    // Fixed, and rebuilt from its inputs.
    repl.src.insert("src/lib.hd", LIB);
    repl.session = Session::start();
    repl.input(1, "1 + 2\n");
    repl.input(2, "x := +5\n");
    repl.input(4, "x * +2\n");
    repl.input(5, "x + bonus()\n");
}

/// A method of a library trait, implemented for a std type, is named by
/// the method alone, and loads the library too: the session sees the
/// trait (`names.member.trait-available`).
#[test]
fn a_library_trait_s_method_loads_the_library() {
    let lib = "trait Twice:\n    fn twice(self) -> i32\n\nimpl Twice for i32:\n    fn twice(self) -> i32:\n        self * +2\n";
    let inner = crate::warm_store();
    let store = OverlayStore::new(&inner);
    let mut repl = repl(&store);
    repl.src.insert("src/lib.hd", lib);
    repl.input(1, "n := +3\n");
    repl.input(2, "println(n.twice())\n");
}
