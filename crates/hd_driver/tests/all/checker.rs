//! M4a's exit: every `lib/std` body checks through `InferTable`, the
//! solver and `TirBuilder` to verified TIR with no diagnostic when std is
//! the root package; a few bodies' TIR is pinned by golden files; and the
//! checker's own errors (rows, exhaustiveness, mismatches) are reported in
//! user programs.

use hd_base::SliceSort as _;
use std::path::{Path, PathBuf};

use hd_base::Stage;
use hd_cache::{CacheStore, MemoryStore};
use hd_diag::Code;
use hd_driver::{
    Dependency, Executor, Goal, Host, NoClock, Output, Packages, build, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn walk(root: &Path, dir: &Path, out: &mut MemorySources) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort_stable();
    for p in paths {
        if p.is_dir() {
            walk(root, &p, out);
        } else if p.extension().is_some_and(|x| x == "hd") {
            let rel = p
                .strip_prefix(root)
                .expect("root")
                .to_string_lossy()
                .replace('\\', "/");
            out.insert(&rel, &std::fs::read_to_string(&p).expect("read"));
        }
    }
}

fn std_sources() -> MemorySources {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../lib/std");
    let mut s = MemorySources::default();
    walk(&root, &root, &mut s);
    s
}

fn analyze_std(render: &[&str]) -> Output {
    let src = std_sources();
    let store = MemoryStore::default();
    let host = Host {
        render_tir: render,
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "std", &Goal::Analyze)
}

fn program(main: &str) -> Output {
    build_program(main, &crate::warm_store())
}

/// `program` with its own empty store, for the tests that read the
/// build's report, which a cache hit leaves empty.
fn cold_program(main: &str) -> Output {
    build_program(main, &MemoryStore::default())
}

fn build_program(main: &str, store: &dyn CacheStore) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
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

#[test]
fn foreign_bare_loop_names_the_endless_loop_spelling() {
    let source = "pub fn main() -> void:\n    loop:\n        pass\n";
    let out = program(source);
    assert_eq!(out.diags.code, [Code::UnknownName], "{}", out.render());
    assert_eq!(
        out.diags.get_text(out.diags.message[0]),
        "`loop` is not defined; write 'while true:' for an endless loop"
    );
    let start = u32::try_from(source.find("loop:").unwrap()).unwrap();
    assert_eq!(out.diags.primary[0].lo, start);
    assert_eq!(out.diags.primary[0].hi, start + 4);
}

#[test]
fn foreign_loop_hint_does_not_change_other_unresolved_calls() {
    for call in ["loop()", "loop(1):\n        pass", "missing:\n        pass"] {
        let source = format!("pub fn main() -> void:\n    {call}\n");
        let out = program(&source);
        assert_eq!(out.diags.code, [Code::UnknownName], "{}", out.render());
        let name = if call.starts_with("loop") {
            "loop"
        } else {
            "missing"
        };
        assert_eq!(
            out.diags.get_text(out.diags.message[0]),
            format!("`{name}` is not defined")
        );
    }
}

/// Every module of `lib/std`, plus the compiler-supplied `std.rt`, and the
/// trait default bodies among the bodies: 43 modules.
#[test]
fn every_std_body_checks_with_no_diagnostic() {
    let out = analyze_std(&[]);
    let body = out.report.tally(Stage::Body);
    assert_eq!(
        (body.ok, body.not_implemented, body.blocked),
        (43, 0, 0),
        "{}\n{:?}",
        out.report.render(),
        out.report.body_failures
    );
    assert_eq!(out.report.tally(Stage::ModuleFinish).ok, 43);
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    assert!(out.report.body_ok > 1000, "{}", out.report.body_ok);
    assert_eq!(out.diags.len(), 0, "{}", out.render());
}

/// The golden TIR of a few std bodies: `println` (rows, `$.use`, a cold
/// suspending call, `block_on`, interpolation, `match` on a `BlockOn` of a
/// `Result`),
/// `simple_escape` (integer patterns), `Option.map` (an `Option` match with a
/// closure call) and `Iterator.take` (a trait default returning an owned
/// named adapter).
/// `HD_BLESS=1` rewrites the files.
#[test]
fn std_bodies_match_golden_tir() {
    let bodies = [
        ("std/console/println", "println"),
        ("std/text/simple_escape", "simple_escape"),
        ("std/option/impl [ T ] T ?.map", "option_map"),
        ("std/iter/Iterator.take", "iterator_take"),
    ];
    let paths: Vec<&str> = bodies.iter().map(|b| b.0).collect();
    let out = analyze_std(&paths);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tir");
    let update = std::env::var_os("HD_BLESS").is_some();
    for (path, file) in bodies {
        let got = out
            .tir_text
            .get(path)
            .unwrap_or_else(|| panic!("no body {path}; have {:?}", out.tir_text.keys()));
        let golden = dir.join(format!("{file}.tir"));
        if update {
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(&golden, got).expect("write golden");
            continue;
        }
        let want = std::fs::read_to_string(&golden).unwrap_or_default();
        assert_eq!(got, &want, "{path}: TIR differs from {}", golden.display());
    }
}

/// `use a.b as c` and `use a.{b as c}` (grammar.use) reach the checker:
/// the renamed module, type and trait resolve in bodies.
#[test]
fn renamed_uses_resolve_in_bodies() {
    let mut src = MemorySources::default();
    src.insert(
        "main.hd",
        "use std.cmp as order\nuse std.cmp.Ordering as Order\nuse std.format.{Display as Show}\n\nfn pick(o: Order) -> i32:\n    match o:\n        .Less => 1\n        .Equal => 2\n        .Greater => 3\n\nfn other(o: order.Ordering) -> i32:\n    pick(o)\n\nfn show[T < Show](x: T) -> string:\n    x.to_string()\n\npub fn main() -> void $ Console:\n    println(other(.Less))\n",
    );
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Analyze);
    assert_eq!(out.diags.len(), 0, "{}", out.render());
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
}

/// Module initialization (spec/lang/10-modules.md): top-level bindings are
/// visible to function bodies; a statement whose transitive read set
/// holds a later binding is `top-level-read-before-initialization`; a
/// non-entry module's top level is requirement-free.
#[test]
fn top_level_statements_are_checked_in_order() {
    let ok = cold_program(
        "limit := +10\n\nfn below(n: i32) -> bool:\n    n < limit\n\npub fn main() -> void $ Console:\n    println(below(3))\n",
    );
    assert!(
        !codes(&ok).iter().any(|c| matches!(
            c,
            Code::TopLevelReadBeforeInitialization | Code::UnknownName | Code::TypeMismatch
        )),
        "{}",
        ok.render()
    );
    assert_eq!(ok.report.body_failed, 0, "{:?}", ok.report.body_failures);
    let late = cold_program(
        "first := apply(first_name)\nlet names: List[string] = [\"Ada\"]\n\nfn apply(callback: fn() -> string) -> string:\n    callback()\n\nfn first_name() -> string:\n    names[0]\n\npub fn main() -> void $ Console:\n    println(first)\n",
    );
    assert!(
        codes(&late).contains(&Code::TopLevelReadBeforeInitialization),
        "{}",
        late.render()
    );
    // A closure, or a plain statement, that names a binding declared
    // below it reads it before initialization; a name nothing declares
    // stays unknown, and a local of the same name is not the binding.
    for (text, want) in [
        (
            "f := fn() -> i32:\n    later\nlater := +1\n",
            Some(Code::TopLevelReadBeforeInitialization),
        ),
        (
            "x := later\nlater := +1\n",
            Some(Code::TopLevelReadBeforeInitialization),
        ),
        (
            "f := fn() -> i32:\n    nope\nlater := +1\n",
            Some(Code::UnknownName),
        ),
        (
            "f := fn(later: i32) -> i32:\n    later\nlater := +1\n",
            None,
        ),
    ] {
        let out = cold_program(&format!(
            "{text}\npub fn main() -> void $ Console:\n    println(1)\n"
        ));
        match want {
            Some(code) => {
                assert!(codes(&out).contains(&code), "{text}: {}", out.render());
                if code == Code::TopLevelReadBeforeInitialization {
                    assert!(
                        !codes(&out).contains(&Code::UnknownName),
                        "{}",
                        out.render()
                    );
                }
            }
            None => assert_eq!(out.diags.len(), 0, "{text}: {}", out.render()),
        }
    }
    let mut src = MemorySources::default();
    src.insert("lib.hd", "println(1)\n\npub fn one() -> i32:\n    1\n");
    src.insert(
        "main.hd",
        "use pkg.lib.{one}\n\npub fn main() -> void $ Console:\n    println(one())\n",
    );
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(
        codes(&out).contains(&Code::MissingRequirement),
        "{}",
        out.render()
    );
}

/// A top-level binding that holds a function value, plain or suspending,
/// is callable by name from a function body (`names.module.declarations`,
/// `module.init.binding`), with `f(x)`, `f!(x)` or as a value.
#[test]
fn top_level_function_values_are_callable() {
    let out = cold_program(
        "twice := fn(n: i32) -> i32:\n    n * +2\n\nfetch := fn!(n: i32) -> i32:\n    n\n\npub fn main!() -> void $ Console:\n    println(twice(+3))\n    println(fetch!(+4))\n    pending := fetch(+5)\n    println(pending!())\n",
    );
    assert!(
        !codes(&out)
            .iter()
            .any(|c| matches!(c, Code::UnknownName | Code::TypeMismatch)),
        "{}",
        out.render()
    );
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
}

/// `$.with` covers its keys in its block, and each provider must
/// implement its key (`req.with.type-or-ref`): a value that does not convert to
/// the key's trait value is `type-mismatch`, as the fixture
/// `provider-value-type-mismatch` expects.
#[test]
fn with_blocks_provide_requirements() {
    let out = cold_program(
        "use std.console.MemoryConsole\n\nfn show() -> void:\n    let mut console = MemoryConsole.new()\n    $.with(Console = console):\n        println(1)\n\npub fn main() -> void $ Console:\n    show()\n",
    );
    assert!(
        codes(&out).iter().all(|c| *c == Code::Unsupported),
        "{}",
        out.render()
    );
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    let out = cold_program(
        "fn show() -> void:\n    $.with(Console = 5):\n        println(1)\n\npub fn main() -> void $ Console:\n    show()\n",
    );
    assert!(
        codes(&out).contains(&Code::TypeMismatch),
        "{}",
        out.render()
    );
}

/// Pipes and comprehensions check (an Emit stub may stop the build).
#[test]
fn pipes_and_comprehensions_check() {
    let out = cold_program(
        "fn add(a: i32, b: i32) -> i32:\n    a + b\n\npub fn main() -> void $ Console:\n    xs := [+1, 2, 3, 4]\n    y := 3 |> add(_, 1)\n    evens := [for x in xs if x % 2 == 0 => x * 10]\n    m := {for x in xs => \"$x\": x + y}\n    println(evens.len() + m.len())\n",
    );
    assert!(
        codes(&out).iter().all(|c| *c == Code::Unsupported),
        "{}",
        out.render()
    );
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    let out = cold_program(
        "fn add(a: i32, b: i32) -> i32:\n    a + b\n\npub fn main() -> void $ Console:\n    println(add(_, 1))\n",
    );
    assert!(
        codes(&out).contains(&Code::PlaceholderOutsidePipe),
        "{}",
        out.render()
    );
}

#[test]
fn an_unresolved_impl_trait_only_poisons_matching_target_arguments() {
    let input = r"
struct Envelope[T]:
    value: T

impl From[string] for Envelope[i32]:
    fn from(text: string) -> Envelope[i32]: Envelope { value: +1 }

fn parse(text: string) -> Result[i32, string]: .Err(text)

fn matching(text: string) -> Result[i32, Envelope[i32]]:
    value := parse(text)?
    .Ok(value)

fn unrelated(text: string) -> Result[i32, Envelope[string]]:
    value := parse(text)?
    .Ok(value)
";
    let out = program(input);
    assert_eq!(
        codes(&out),
        [Code::UnknownTrait, Code::InvalidResultPropagation],
        "{}",
        out.render()
    );
    assert_eq!(
        out.diags.primary[1].lo as usize,
        input.rfind("parse(text)?").unwrap(),
        "the unrelated target must still report its missing conversion"
    );
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

/// A row parameter takes the row of the callback a call passes
/// (`req.poly.least`): a pure closure needs nothing, a closure printing
/// needs `Console` from its caller, and a caller whose row lacks it is
/// rejected at the call.
#[test]
fn row_polymorphic_callbacks() {
    const CALL: &str = "fn call[T, $R](f: fn() -> T $ R) -> T $ R:\n    f()\n\n";
    let ok = cold_program(&format!(
        "{CALL}pub fn main() -> void $ Console:\n    n := call(fn() -> i32: 7)\n    call(fn() -> void: println(n))\n"
    ));
    assert!(codes(&ok).is_empty(), "{}", ok.render());
    // Each call's instance gets its row's providers; the closure reads
    // its own from the context the call passes.
    assert!(ok.wasm.is_some(), "{:?}", ok.report.body_failures);
    let bad = cold_program(&format!(
        "{CALL}pub fn quiet() -> void:\n    call(fn() -> void: println(1))\n\npub fn main() -> void $ Console:\n    quiet()\n"
    ));
    assert!(
        codes(&bad).contains(&Code::MissingRequirement),
        "{}",
        bad.render()
    );
    // A closure fits a wider expected row, never a narrower one.
    let narrow = cold_program(
        "fn run(f: fn() -> void) -> void:\n    f()\n\npub fn main() -> void $ Console:\n    run(fn() -> void: println(1))\n",
    );
    assert!(
        codes(&narrow).contains(&Code::TypeMismatch),
        "{}",
        narrow.render()
    );
}

fn program_of(files: &[(&str, &str)]) -> Output {
    let mut src = MemorySources::default();
    for (path, text) in files {
        src.insert(path, text);
    }
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
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

/// An error in a dependency's file is located in that file's own text, not
/// at `1:1` for want of the root package's source of it.
#[test]
fn a_dependency_diagnostic_is_located_in_its_file() {
    let mut root = MemorySources::default();
    root.insert(
        "src/main.hd",
        "use dep.ext.f\n\npub fn main() -> void $ Console:\n    println(f())\n",
    );
    let mut dep = MemorySources::default();
    dep.insert("src/lib.hd", "pub fn f() -> i32:\n    _xs := []\n    1\n");
    let store = MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &root,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        requires: vec![("ext".to_owned(), 1)],
        deps: vec![Dependency {
            name: "ext".to_owned(),
            identity: "ext".to_owned(),
            sources: &dep,
            requires: Vec::new(),
            fetched: false,
        }],
        ..Packages::default()
    };
    let out = build_packages(
        &host,
        "app",
        &packages,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert_eq!(codes(&out), [Code::CannotInferType], "{}", out.render());
    let at = out.diags.content_order()[0];
    let (file, line, column) = out.locate(out.diags.primary[at]);
    assert_eq!((file.as_str(), line, column), ("<ext>/src/lib.hd", 2, 12));
}

/// A `println` written directly in a non-entry module's initialization
/// reports both `missing-requirement` (`module.init.requirement-free.non-script`) and
/// `suspension-forbidden-context` (`module.console.println-block-on.forbidden-direct`).
/// A conformance case holds one error, so this is the check of the pair.
#[test]
fn println_in_non_entry_module_init_reports_both_errors() {
    let out = program_of(&[
        (
            "main.hd",
            "use pkg.catalog.item_count\n\npub fn main() -> void:\n    _ := item_count()\n",
        ),
        (
            "catalog.hd",
            "println(\"loading\")\n\npub fn item_count() -> i32:\n    3\n",
        ),
    ]);
    let mut found = codes(&out);
    found.sort_stable_by_key(|c| c.as_str());
    assert_eq!(
        found,
        [Code::MissingRequirement, Code::SuspensionForbiddenContext],
        "{}",
        out.render()
    );
}

/// Sibling bodies share interface methods, but retain their own private traits.
/// The public trait occurs both in its declaring body's own items and its interface.
#[test]
fn shared_method_index_keeps_private_traits_and_deduplicates_own_items() {
    let out = program_of(&[
        (
            "main.hd",
            "use pkg.left.one\nuse pkg.right.two\n\npub fn main() -> void:\n    _ := one(1) + two(2)\n",
        ),
        (
            "left.hd",
            r"trait Private:
    fn stamp(self) -> i32
impl Private for i32:
    fn stamp(self) -> i32: self
pub trait Shared:
    fn shared(self) -> i32
impl Shared for i32:
    fn shared(self) -> i32: self
pub fn one(n: i32) -> i32:
    n.stamp() + n.shared()
",
        ),
        (
            "right.hd",
            r"use pkg.left.Shared
trait Private:
    fn stamp(self) -> i32
impl Private for i32:
    fn stamp(self) -> i32: self
pub fn two(n: i32) -> i32:
    n.stamp() + n.shared()
",
        ),
    ]);
    assert!(!out.diags.has_errors(), "{}", out.render());
}

#[test]
fn associated_function_output_infers_its_requirement_row() {
    let header = r#"
trait Make:
    type Out
    fn make(self) -> Self::Out
struct Hello: pass
impl Make for Hello:
    type Out = fn() -> void $ Console
    fn make(self) -> (fn() -> void $ Console):
        fn() -> void $ Console: println("hello")
fn call[$S, P < Make[Out = fn() -> void $ S]](maker: P) -> void $ S:
    callback := maker.make()
    callback()
"#;
    let missing = program(&format!(
        "{header}\npub fn main() -> void:\n    call(Hello {{}})\n"
    ));
    assert!(
        missing.diags.code.contains(&Code::MissingRequirement),
        "{}",
        missing.render()
    );
    let mismatched = program(&format!(
        "{header}\npub fn main() -> void:\n    call::[$(), Hello](Hello {{}})\n"
    ));
    assert!(
        mismatched.diags.code.contains(&Code::UnsatisfiedTraitBound),
        "{}",
        mismatched.render()
    );
}

#[test]
fn opaque_trait_methods_report_dynamic_calls_with_generic_bound_advice() {
    for name in ["items", "custom_result"] {
        for (receiver, expression) in [
            ("&dyn Source", format!("source.{name}()")),
            ("&mut dyn Source", format!("source.{name}()")),
            ("*dyn Source", format!("source::{name}")),
        ] {
            let source = format!(
                "trait Source:\n    fn {name}(&self) -> impl Display\n\nfn read(source: {receiver}) -> void:\n    _result := {expression}\n"
            );
            let out = program(&source);
            assert_eq!(
                out.diags.code,
                [Code::DynMemberUnavailable],
                "{}",
                out.render()
            );
            let message = out.diags.get_text(out.diags.message[0]);
            assert!(
                message.contains(&format!("`{name}` returns `impl Trait`")),
                "{message}"
            );
            assert!(message.contains("use a generic bound instead"), "{message}");
            let span = out.diags.primary[0];
            assert_eq!(&source[span.lo as usize..span.hi as usize], expression);
        }
    }
}
