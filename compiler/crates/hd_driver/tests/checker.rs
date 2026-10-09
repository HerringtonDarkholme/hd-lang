//! M4a's exit: every `lib/std` body checks through `InferTable`, the
//! solver and `TirBuilder` to verified TIR with no diagnostic when std is
//! the root package; a few bodies' TIR is pinned by golden files; and the
//! checker's own errors (rows, exhaustiveness, mismatches) are reported in
//! user programs.

use std::path::{Path, PathBuf};

use hd_base::Stage;
use hd_cache::MemoryStore;
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
    paths.sort();
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
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib/std");
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
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
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

/// Every module of `lib/std`, plus the compiler-supplied `std.rt`, and the
/// trait default bodies among the bodies.
#[test]
fn every_std_body_checks_with_no_diagnostic() {
    let out = analyze_std(&[]);
    let body = out.report.tally(Stage::Body);
    assert_eq!(
        (body.ok, body.not_implemented, body.blocked),
        (40, 0, 0),
        "{}\n{:?}",
        out.report.render(),
        out.report.body_failures
    );
    assert_eq!(out.report.tally(Stage::ModuleFinish).ok, 40);
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    assert!(out.report.body_ok > 1000, "{}", out.report.body_ok);
    assert_eq!(out.diags.len(), 0, "{}", out.render());
}

/// The golden TIR of a few std bodies: `println` (rows, `$.use`, a cold
/// suspending call, `block_on`, interpolation, `match` on a `Result`),
/// `digit_text` (integer patterns), `Option.map` (an `Option` match with a
/// closure call) and `Iterator.take` (a closure capturing and assigning).
/// `HD_UPDATE_GOLDEN=1` rewrites the files.
#[test]
fn std_bodies_match_golden_tir() {
    let bodies = [
        ("std/console/println", "println"),
        ("std/format/digit_text", "digit_text"),
        ("std/option/impl [ T ] T ?.map", "option_map"),
        ("std/iter/impl [ T ] Iterator [ T ].take", "iterator_take"),
    ];
    let paths: Vec<&str> = bodies.iter().map(|b| b.0).collect();
    let out = analyze_std(&paths);
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tir");
    let update = std::env::var_os("HD_UPDATE_GOLDEN").is_some();
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
    let ok = program(
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
    let late = program(
        "first := apply(first_name)\nlet names: List[string] = [\"Ada\"]\n\nfn apply(callback: fn() -> string) -> string:\n    callback()\n\nfn first_name() -> string:\n    names[0]\n\npub fn main() -> void $ Console:\n    println(first)\n",
    );
    assert!(
        codes(&late).contains(&Code::TopLevelReadBeforeInitialization),
        "{}",
        late.render()
    );
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

/// `$.with` covers its keys in its block, and each provider must
/// implement its key (`req.with.type`).
#[test]
fn with_blocks_provide_requirements() {
    let out = program(
        "use std.console.BufferConsole\n\nfn show() -> void:\n    let mut console = BufferConsole.new()\n    $.with(Console = console):\n        println(1)\n\npub fn main() -> void $ Console:\n    show()\n",
    );
    assert!(
        codes(&out).iter().all(|c| *c == Code::Unsupported),
        "{}",
        out.render()
    );
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    let out = program(
        "fn show() -> void:\n    $.with(Console = 5):\n        println(1)\n\npub fn main() -> void $ Console:\n    show()\n",
    );
    assert!(
        codes(&out).contains(&Code::UnsatisfiedTraitBound),
        "{}",
        out.render()
    );
}

/// Pipes and comprehensions check (an Emit stub may stop the build).
#[test]
fn pipes_and_comprehensions_check() {
    let out = program(
        "fn add(a: i32, b: i32) -> i32:\n    a + b\n\npub fn main() -> void $ Console:\n    xs := [+1, 2, 3, 4]\n    y := 3 |> add(_, 1)\n    evens := [for x in xs if x % 2 == 0 => x * 10]\n    m := {for x in xs => \"$x\": x + y}\n    println(evens.len() + m.len())\n",
    );
    assert!(
        codes(&out).iter().all(|c| *c == Code::Unsupported),
        "{}",
        out.render()
    );
    assert_eq!(out.report.body_failed, 0, "{:?}", out.report.body_failures);
    let out = program(
        "fn add(a: i32, b: i32) -> i32:\n    a + b\n\npub fn main() -> void $ Console:\n    println(add(_, 1))\n",
    );
    assert!(
        codes(&out).contains(&Code::PlaceholderOutsidePipe),
        "{}",
        out.render()
    );
}

fn codes(out: &Output) -> Vec<Code> {
    out.diags.code.clone()
}

#[test]
fn a_missing_requirement_is_an_error() {
    let out = program("pub fn main():\n    println(42)\n");
    assert!(
        codes(&out).contains(&Code::MissingRequirement),
        "{}",
        out.render()
    );
    let ok = program("pub fn main() -> void $ Console:\n    println(42)\n");
    assert!(
        !codes(&ok).contains(&Code::MissingRequirement),
        "{}",
        ok.render()
    );
    // A callee's own row reaches its callers.
    let out = program(
        "fn show(x: i32) -> void $ Console:\n    println(x)\n\npub fn main():\n    show(1)\n",
    );
    assert!(
        codes(&out).contains(&Code::MissingRequirement),
        "{}",
        out.render()
    );
}

/// A row parameter takes the row of the callback a call passes
/// (`req.poly.least`): a pure closure needs nothing, a closure printing
/// needs `Console` from its caller, and a caller whose row lacks it is
/// rejected at the call.
#[test]
fn row_polymorphic_callbacks() {
    const CALL: &str = "fn call[T, $R](f: fn() -> T $ R) -> T $ R:\n    f()\n\n";
    let ok = program(&format!(
        "{CALL}pub fn main() -> void $ Console:\n    n := call(fn() -> i32: 7)\n    call(fn() -> void: println(n))\n"
    ));
    assert!(codes(&ok).is_empty(), "{}", ok.render());
    // Each call's instance gets its row's providers; the closure reads
    // its own from the context the call passes.
    assert!(ok.wasm.is_some(), "{:?}", ok.report.body_failures);
    let bad = program(&format!(
        "{CALL}pub fn quiet() -> void:\n    call(fn() -> void: println(1))\n\npub fn main() -> void $ Console:\n    quiet()\n"
    ));
    assert!(
        codes(&bad).contains(&Code::MissingRequirement),
        "{}",
        bad.render()
    );
    // A closure fits a wider expected row, never a narrower one.
    let narrow = program(
        "fn run(f: fn() -> void) -> void:\n    f()\n\npub fn main() -> void $ Console:\n    run(fn() -> void: println(1))\n",
    );
    assert!(
        codes(&narrow).contains(&Code::TypeMismatch),
        "{}",
        narrow.render()
    );
}

#[test]
fn an_impl_must_write_every_required_trait_method() {
    let missing = "trait Named:\n    fn name(self) -> string\n    fn id(self) -> i32\n\ndata User: pass\n\nimpl Named for User:\n    fn name(self) -> string:\n        \"u\"\n\npub fn main() -> void $ Console:\n    println(1)\n";
    let out = program(missing);
    assert!(
        codes(&out).contains(&Code::MissingTraitMethod),
        "{}",
        out.render()
    );
    // A method with a default body need not be written.
    let defaulted = "trait Named:\n    fn name(self) -> string:\n        \"named\"\n\ndata User: pass\n\nimpl Named for User\n\npub fn main() -> void $ Console:\n    println(1)\n";
    let out = program(defaulted);
    assert!(
        !codes(&out).contains(&Code::MissingTraitMethod),
        "{}",
        out.render()
    );
}

#[test]
fn a_match_must_cover_every_variant() {
    let src = "enum Color:\n    Red\n    Green\n    Blue\n\nfn code(c: Color) -> i32:\n    match c:\n        .Red => 1\n        .Green => 2\n\npub fn main() -> void $ Console:\n    println(code(.Red))\n";
    let out = program(src);
    assert!(
        codes(&out).contains(&Code::NonexhaustiveMatch),
        "{}",
        out.render()
    );
    let full = src.replace(
        "        .Green => 2\n",
        "        .Green => 2\n        .Blue => 3\n",
    );
    let out = program(&full);
    assert!(
        !codes(&out).contains(&Code::NonexhaustiveMatch),
        "{}",
        out.render()
    );
    // Nested: `.Some(.Red)` alone leaves `.None` and the other colors.
    let nested = "enum Color:\n    Red\n    Green\n\nfn f(c: Color?) -> i32:\n    match c:\n        .Some(.Red) => 1\n        .None => 0\n\npub fn main() -> void $ Console:\n    println(f(.None))\n";
    let out = program(nested);
    assert!(
        codes(&out).contains(&Code::NonexhaustiveMatch),
        "{}",
        out.render()
    );
}

#[test]
fn checker_errors_are_reported() {
    let cases: &[(&str, Code)] = &[
        (
            "pub fn main() -> void $ Console:\n    let x: i32 = \"a\"\n    println(x)\n",
            Code::TypeMismatch,
        ),
        (
            "pub fn main() -> void $ Console:\n    x := 1\n    println(x.nope())\n",
            Code::UnknownMethod,
        ),
        (
            "fn f(o: i32?) -> i32:\n    let .Some(v) = o else:\n        println(0)\n    v\n\npub fn main() -> void $ Console:\n    println(f(.None))\n",
            Code::LetElseFallsThrough,
        ),
        (
            "fn f(x: i32) -> i32:\n    y := x?\n    y\n\npub fn main() -> void $ Console:\n    println(f(1))\n",
            Code::InvalidResultPropagation,
        ),
        (
            "data P:\n    x: i32\n\npub fn main() -> void $ Console:\n    p := P { x: 1 }\n    println(p.y)\n",
            Code::UnknownDataField,
        ),
        (
            "fn get!() -> i32:\n    1\n\npub fn main() -> void $ Console:\n    println(get!())\n",
            Code::BangCallOutsideSuspension,
        ),
        (
            "fn get() -> i32:\n    1\n\npub fn main!() -> void $ Console:\n    println(get!())\n",
            Code::NotSuspending,
        ),
        (
            "fn f(o: i32?) -> i32:\n    match o:\n        _ => 0\n        .Some(x) => x\n\npub fn main() -> void $ Console:\n    println(f(.None))\n",
            Code::UnreachableMatchArm,
        ),
        (
            "pub fn main() -> void $ Console:\n    x := .Red\n    println(1)\n",
            Code::MissingContextualEnumType,
        ),
        (
            "fn f(a: i32, b: i32) -> i32:\n    a + b\n\npub fn main() -> void $ Console:\n    println(f(1, c = 2))\n",
            Code::UnknownNamedArgument,
        ),
        (
            "fn f(a: i32, b: i32) -> i32:\n    a + b\n\npub fn main() -> void $ Console:\n    println(f(1, a = 2))\n",
            Code::DuplicateArgument,
        ),
        (
            "pub fn main() -> void $ Console:\n    let x: u8 = 300\n    println(x)\n",
            Code::IntegerLiteralRange,
        ),
        (
            "pub fn main() -> void $ Console:\n    while true:\n        break 1\n",
            Code::BreakValueContext,
        ),
        (
            "fn f() -> i32:\n    defer:\n        return 1\n    2\n\npub fn main() -> void $ Console:\n    println(f())\n",
            Code::DeferControlFlow,
        ),
        (
            "data P:\n    x: i32\n\npub fn main() -> void $ Console:\n    q := P\n    println(1)\n",
            Code::TypeUsedAsValue,
        ),
        (
            "fn f(t: (i32, i32)) -> i32:\n    let (a, a) = t\n    a\n\npub fn main() -> void $ Console:\n    println(f((1, 2)))\n",
            Code::DuplicateBinding,
        ),
        (
            "pub fn main() -> void $ Console:\n    xs := []\n    println(1)\n",
            Code::CannotInferType,
        ),
        (
            "pub fn main() -> void $ Console:\n    let mut n = 0\n    println(n)\n",
            Code::MutOnPrimitive,
        ),
        (
            "pub fn main() -> void $ Console:\n    a := (1, 2)\n    println(a is a)\n",
            Code::IdentityRequiresReferences,
        ),
        (
            "pub fn main() -> void $ Console:\n    let n: mut i32 = 0\n    println(n)\n",
            Code::MutOnPrimitive,
        ),
        (
            "pub fn main() -> void $ Console:\n    let mut pair = (1, 2)\n    println(pair._0)\n",
            Code::MutOnTuple,
        ),
    ];
    for (src, code) in cases {
        let out = program(src);
        assert!(codes(&out).contains(code), "{src}\n{}", out.render());
    }
}

/// `mut T` over an unconstrained parameter reports `mut-on-type-parameter`
/// alone, also in a method of a covariant impl (`types.generic.no-mut-t`).
#[test]
fn mut_on_unconstrained_type_parameter() {
    let ok = program(
        "fn keep[T < mut Any](value: mut T) -> void:\n    pass\n\npub fn main() -> void:\n    pass\n",
    );
    assert!(
        !codes(&ok).contains(&Code::MutOnTypeParameter),
        "{}",
        ok.render()
    );
    let bad = program(
        "fn reset[T](value: mut T) -> void:\n    pass\n\npub fn main() -> void:\n    pass\n",
    );
    assert!(
        codes(&bad).contains(&Code::MutOnTypeParameter),
        "{}",
        bad.render()
    );
    let method = program(
        "data Feed[T]:\n    latest: T\n\nimpl[T] Feed[T]:\n    pub fn swap(self, other: mut T) -> void:\n        pass\n\npub fn main() -> void:\n    pass\n",
    );
    let found = codes(&method);
    assert!(
        found.contains(&Code::MutOnTypeParameter),
        "{}",
        method.render()
    );
    assert!(
        !found.contains(&Code::InvalidVariance),
        "{}",
        method.render()
    );
}

/// `mut Self` in a trait and in an impl names no declared type parameter.
#[test]
fn mut_self_is_not_a_type_parameter() {
    let out = program(
        "trait Dup:\n    fn dup(mut self) -> mut Self\n\ndata Box:\n    n: i32\n\nimpl Dup for Box:\n    fn dup(mut self) -> mut Self:\n        self\n\nimpl[T < mut Any] Dup for T:\n    fn dup(mut self) -> mut Self:\n        self\n\npub fn main() -> void:\n    pass\n",
    );
    assert!(
        !codes(&out).contains(&Code::MutOnTypeParameter),
        "{}",
        out.render()
    );
}

const LIMITED_FACT: &str = "use std.annotation.annotate\n\n@annotate(.Field)\ndata MaxLen:\n    value: i32\n\nfn max_len(value: i32) -> MaxLen:\n    MaxLen { value: value }\n\ndata Profile:\n    @max_len(80)\n    name: string\n\n";

/// A fact type limited to `.Field` is accepted on a field and rejected
/// before a function (`annot.target.limit.kind-error`).
#[test]
fn decorator_target_kind_is_checked() {
    let ok = program(&format!("{LIMITED_FACT}pub fn main() -> void:\n    pass\n"));
    assert!(
        !codes(&ok).contains(&Code::DecoratorTargetKind),
        "{}",
        ok.render()
    );
    let bad = program(&format!(
        "{LIMITED_FACT}@max_len(3)\nfn greet() -> string:\n    \"hi\"\n\npub fn main() -> void:\n    pass\n"
    ));
    let hits = codes(&bad)
        .into_iter()
        .filter(|c| *c == Code::DecoratorTargetKind)
        .count();
    assert_eq!(hits, 1, "{}", bad.render());
}

/// A bare integer literal defaults to `usize`; a signed one to `i32`
/// (`types.literal.local.default`).
#[test]
fn integer_literals_take_their_default_types() {
    let ok = program(
        "pub fn main() -> void:\n    x := 3\n    y := +3\n    z := -1\n    let a: usize = x\n    let b: i32 = y\n    let c: i32 = z\n    pass\n",
    );
    assert!(!ok.render().contains("error"), "{}", ok.render());
    for (name, want) in [("x", "i32"), ("y", "usize"), ("z", "usize")] {
        let bad = program(&format!(
            "pub fn main() -> void:\n    x := 3\n    y := +3\n    z := -1\n    let w: {want} = {name}\n    pass\n"
        ));
        assert!(
            codes(&bad).contains(&Code::TypeMismatch),
            "{name}: {}",
            bad.render()
        );
    }
}

const MONEY_ADD: &str = "use std.ops.Add\n\ndata Money:\n    cents: i64\n\nimpl Add for Money:\n    type Out = Money\n    fn add(self, rhs: Money) -> Money:\n        Money { cents: self.cents + rhs.cents }\n\n";

/// A bound that omits a defaulted trait argument takes the default, as an
/// impl head does: `T < Add` is `T < Add[T]`, which `impl Add for Money`
/// (`Add[Money]`) satisfies (`types.generic.default.written`,
/// `expr.op.trait.rhs-self`).
#[test]
fn a_bound_takes_the_trait_argument_default() {
    for twice in [
        "fn twice[T < Add](x: T) -> T::Out:\n    x + x\n\n",
        "fn twice[T < Add[Out = T]](x: T) -> T:\n    x + x\n\n",
    ] {
        let out = program(&format!(
            "{MONEY_ADD}{twice}pub fn main() -> void:\n    _m := twice(Money {{ cents: 2 }})\n    pass\n"
        ));
        assert!(codes(&out).is_empty(), "{twice}{}", out.render());
    }
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

const SAME: &str = "trait Same[Other = Self]:\n    fn same(self, other: Other) -> bool\n";
const CONVERT: &str = "trait Convert[A, B = A]:\n    fn convert(self, a: A) -> B\n";
const SAME_USES: &str = "data Money:\n    cents: i64\n\nimpl Same for Money:\n    fn same(self, other: Money) -> bool:\n        self.cents == other.cents\n\nimpl Convert[i64] for Money:\n    fn convert(self, a: i64) -> i64:\n        self.cents + a\n\nfn check[T < Same](left: T, right: T) -> bool:\n    left.same(right)\n\nfn plus[T < Convert[i64]](x: T) -> i64:\n    x.convert(1)\n\npub fn main() -> void:\n    _a := check(Money { cents: 1 }, Money { cents: 1 })\n    _b := plus(Money { cents: 1 })\n    pass\n";

/// A trait of the same folder fills its defaults in impl heads and bounds
/// whatever the declaration order, in the trait's own module (private)
/// and in a sibling module: `impl Same for Money` is `Same[Money]` and
/// `T < Same` is `T < Same[T]` (a default naming `Self`); `Convert[i64]`
/// is `Convert[i64, i64]` (a default naming an earlier parameter).
#[test]
fn same_folder_trait_defaults_fill_in_any_order() {
    let one = program(&format!("{SAME_USES}\n{SAME}\n{CONVERT}"));
    assert!(codes(&one).is_empty(), "{}", one.render());
    let sibling = format!("pub {SAME}\npub {CONVERT}");
    let two = program_of(&[
        (
            "main.hd",
            &format!("use pkg.zz.{{Same, Convert}}\n\n{SAME_USES}"),
        ),
        ("zz.hd", &sibling),
    ]);
    assert!(codes(&two).is_empty(), "{}", two.render());
}

/// The written and the defaulted form of one trait reference are one
/// impl head, so two impls of them overlap.
#[test]
fn a_filled_default_and_the_written_argument_overlap() {
    let out = program(&format!(
        "data Money:\n    cents: i64\n\nimpl Same for Money:\n    fn same(self, other: Money) -> bool:\n        true\n\nimpl Same[Money] for Money:\n    fn same(self, other: Money) -> bool:\n        false\n\n{SAME}\npub fn main() -> void:\n    pass\n"
    ));
    assert!(
        codes(&out).contains(&Code::OverlappingImpl),
        "{}",
        out.render()
    );
}

/// The diagnostic codes of a program made of `items` and an empty `main`.
fn item_codes(items: &str) -> Vec<Code> {
    let out = program(&format!("{items}\npub fn main() -> void:\n    pass\n"));
    codes(&out)
}

/// `**` takes an unsigned exponent for an integer base, an exponent of the
/// base's type for a float base, and gives the base's type
/// (`expr.power.int.exponent`, `expr.power.int.literal`,
/// `expr.power.float.same-type`).
#[test]
fn power_operands_follow_the_exponent_rules() {
    for ok in [
        "fn f(b: i32, e: u32) -> i32: b ** e",
        "fn f(b: i64, e: u8) -> i64: b ** e",
        "fn f(b: u8, e: u16) -> u8: b ** e",
        "fn f(b: i32, e: u64) -> i32: b ** e",
        "fn f(b: i32, e: usize) -> i32: b ** e",
        "fn f(b: i32) -> i32: b ** 3",
        "fn f() -> i32: 2 ** 3 ** 2",
        "fn f() -> i32: -2 ** 2",
        "fn f(b: f64, e: f64) -> f64: b ** e",
        "fn f(b: f32, e: f32) -> f32: b ** e",
        "fn f(b: f32) -> f32: b ** 2.0",
        "fn f() -> f64: 2.0 ** -1.0",
        "fn f(b: i32) -> i64: i64(b ** 2)",
    ] {
        assert!(item_codes(ok).is_empty(), "{ok}");
    }
}

/// Each rejected form is one `type-mismatch` and nothing more
/// (`expr.power.int.signed`, `expr.power.negated-literal`,
/// `expr.power.float.same-type`, `expr.power.mixed`,
/// `expr.op.not-overloaded`).
#[test]
fn power_operand_errors_are_one_type_mismatch() {
    for bad in [
        // A signed exponent, in a variable or as a negated literal.
        "fn f(b: i32, e: i32) -> i32: b ** e",
        "fn f(b: u32, e: i64) -> u32: b ** e",
        "fn f() -> i32: 2 ** -1",
        "fn f(b: i32) -> i32: b ** -3",
        // Integer and floating operands do not mix.
        "fn f() -> f64: 2 ** 2.0",
        "fn f(b: i32, e: f64) -> i32: b ** e",
        "fn f(b: f64, e: i32) -> f64: b ** e",
        "fn f(b: f64) -> f64: b ** 2",
        // A float exponent has the base's type.
        "fn f(b: f32, e: f64) -> f32: b ** e",
        "fn f(b: f64, e: f32) -> f64: b ** e",
        // `**` has no trait: any other base is an error.
        "data Money:\n    cents: i64\n\nfn f(m: Money) -> Money: m ** 2",
        "fn f(b: bool) -> bool: b ** 2",
        "fn f(c: char) -> char: c ** 2",
        "fn f(s: string) -> string: s ** 2",
    ] {
        assert_eq!(item_codes(bad), [Code::TypeMismatch], "{bad}");
    }
}

/// An error in the exponent leaves no second error in its context.
#[test]
fn a_rejected_power_is_an_error_value() {
    assert_eq!(
        item_codes("fn f(e: i32) -> f64: 2 ** e"),
        [Code::TypeMismatch]
    );
    assert_eq!(
        item_codes("fn f(e: i32) -> i32: 2 ** e + 1"),
        [Code::TypeMismatch]
    );
}

const WALKS: &str = "trait Show:\n    fn show(self) -> i32\n\nimpl Show for i32:\n    fn show(self) -> i32:\n        self\n\nimpl[T < Show] Show for List[T]:\n    fn show(self) -> i32:\n        0\n\ntrait Sink:\n    type Error\n    fn put(mut self, n: i32) -> Result[void, Self::Error]\n\ntrait Walk:\n    type Error\n    fn one(mut self) -> Result[void, Self::Error]\n    fn item[F < Show](mut self, value: F) -> Result[void, Self::Error]\n    fn two(mut self) -> Result[void, Self::Error]\n    fn three[T < Show](mut self, v: List[T]) -> Result[void, Self::Error]\n\ndata W[K < mut Sink]:\n    pub out: mut K\n\nimpl[K < Sink] Walk for W[K]:\n    type Error = K::Error\n\n    fn one(mut self) -> Result[void, K::Error]:\n        self.out.put(1)\n\n    fn item[F < Show](mut self, value: F) -> Result[void, K::Error]:\n        self.out.put(value.show())\n\n";
const MAIN: &str = "\npub fn main() -> void $ Console:\n    println(1)\n";

/// Inside `impl[K < Sink] Walk for W[K]`, the goal `W[K]: Walk` names the
/// impl's own parameter `K`, and the head `W[K]` is the same type. The
/// impl's arguments are still bound (`K := K`), so `Self::Error` of a
/// sibling call normalizes to `K::Error` instead of an unbound variable
/// (`cannot-infer-type`), also when the sibling has a bound of its own.
#[test]
fn an_impl_body_calls_its_siblings_through_a_projection_binding() {
    for (two, three) in [
        ("self.one()", "self.item(v)"),
        ("self.one()", "self.item::[List[T]](v)"),
    ] {
        let src = format!(
            "{WALKS}    fn two(mut self) -> Result[void, K::Error]:\n        {two}\n\n    fn three[T < Show](mut self, v: List[T]) -> Result[void, K::Error]:\n        {three}\n{MAIN}"
        );
        let out = program(&src);
        assert!(codes(&out).is_empty(), "{src}\n{}", out.render());
    }
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
            sources: &dep,
            requires: Vec::new(),
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

/// `expr.try.convert`: a `?` error that is assignable or converts by
/// `From` propagates; one that does neither is `invalid-result-propagation`
/// (`expr.try.convert.none`), whether the function returns a plain error
/// type or the erased `dyn Error` (`expr.try.test.converts`).
#[test]
fn a_try_error_that_does_not_convert_is_invalid_propagation() {
    const ERRORS: &str = "\
use std.error.Error
use std.convert.From

data Parse:
    text: string

impl Display for Parse:
    fn to_string(self) -> string: self.text

impl Error for Parse

data Wrapped:
    parse: Parse

impl From[Parse] for Wrapped:
    fn from(value: Parse) -> Wrapped: Wrapped { parse: value }

fn parse() -> Result[i32, Parse]:
    .Err(Parse { text: \"x\" })

fn word() -> Result[i32, string]:
    .Err(\"x\")
";
    for ok in [
        "fn f() -> Result[i32, Wrapped]:\n    .Ok(parse()?)",
        "fn f() -> Result[i32, dyn Error]:\n    .Ok(parse()?)",
    ] {
        assert_eq!(item_codes(&format!("{ERRORS}\n{ok}\n")), [], "{ok}");
    }
    for bad in [
        "fn f() -> Result[i32, Wrapped]:\n    .Ok(word()?)",
        "fn f() -> Result[i32, dyn Error]:\n    .Ok(word()?)",
    ] {
        assert_eq!(
            item_codes(&format!("{ERRORS}\n{bad}\n")),
            [Code::InvalidResultPropagation],
            "{bad}"
        );
    }
}
