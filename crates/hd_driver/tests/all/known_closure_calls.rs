//! Known closure calls (`crates/hd_opt/DESIGN.md` (known closure calls), options A1 and A2, #53): in the
//! optimized pipeline, a call of a closure value made in the same body,
//! directly or through a local bound once, is a direct `call` of the
//! closure's code (A1). A small callee that calls a parameter gets one copy
//! per closure literal passed for it, and inside the copy that call is
//! direct too, as it is in the closures the copy creates (A2). The dev
//! pipeline keeps `call_ref`. Both run the same on V8 (`host/run.mjs`).

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{
    EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Output, Packages, build_packages,
};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The Wasm of the program `main`, through the optimized pipeline or the
/// dev one.
fn wasm_of(main: &str, optimized: bool) -> Vec<u8> {
    build_of(main, optimized).wasm.expect("wasm")
}

/// The build of the program `main`, through the optimized pipeline or the
/// dev one, for the default engine's target.
fn build_of(main: &str, optimized: bool) -> Output {
    let level = if optimized {
        OptLevel::Optimized
    } else {
        OptLevel::Dev
    };
    build_for(main, EngineTarget::default(), level, &crate::warm_store())
}

/// The build of the program `main` for `target` at `level`, over `store`.
fn build_for(
    main: &str,
    target: EngineTarget,
    level: OptLevel,
    store: &dyn hd_cache::CacheStore,
) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let packages = Packages {
        backend: hd_driver::Backend::WasmGc,
        target,
        level,
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
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

/// The debug names of the functions a module defines.
fn names_in(wasm: &[u8]) -> Vec<String> {
    let mut names = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let wasmparser::Payload::CustomSection(c) = payload.expect("payload")
            && let wasmparser::KnownCustom::Name(r) = c.as_known()
        {
            for n in r {
                if let wasmparser::Name::Function(m) = n.expect("name") {
                    names.extend(m.into_iter().map(|f| f.expect("name").name.to_owned()));
                }
            }
        }
    }
    names
}

/// The names of the copies (A2) of the instance named `plain`: `plain`
/// then `+` and a key prefix.
fn copies_of(wasm: &[u8], plain: &str) -> Vec<String> {
    let prefix = format!("{plain}+");
    names_in(wasm)
        .into_iter()
        .filter(|n| n.starts_with(&prefix))
        .collect()
}

/// Whether the module holds the plain instance named `name`.
fn has(wasm: &[u8], name: &str) -> bool {
    names_in(wasm).iter().any(|n| n == name)
}

/// The `call` and `call_ref` counts in the body of the function whose
/// debug name is `name`, in a module that validates.
fn calls_in(wasm: &[u8], name: &str) -> (usize, usize) {
    wasmparser::Validator::new()
        .validate_all(wasm)
        .expect("the module validates");
    let mut imports = 0u32;
    let mut index = None;
    let mut names = Vec::new();
    let mut bodies = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::ImportSection(r) => {
                for i in r.into_imports() {
                    if matches!(i.expect("import").ty, wasmparser::TypeRef::Func(_)) {
                        imports += 1;
                    }
                }
            }
            wasmparser::Payload::CodeSectionEntry(body) => bodies.push(body),
            wasmparser::Payload::CustomSection(c) => {
                if let wasmparser::KnownCustom::Name(r) = c.as_known() {
                    for n in r {
                        if let wasmparser::Name::Function(m) = n.expect("name") {
                            for f in m {
                                let f = f.expect("function name");
                                if f.name == name {
                                    index = Some(f.index);
                                }
                                names.push(f.name.to_owned());
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    let Some(index) = index else {
        panic!("no function named {name}: {names:?}");
    };
    let body = &bodies[(index - imports) as usize];
    let (mut call, mut call_ref) = (0, 0);
    let mut ops = body.get_operators_reader().expect("operators");
    while !ops.eof() {
        match ops.read().expect("operator") {
            wasmparser::Operator::Call { .. } => call += 1,
            wasmparser::Operator::CallRef { .. } => call_ref += 1,
            _ => {}
        }
    }
    (call, call_ref)
}

/// The standard output of `wasm` on V8.
fn run(name: &str, wasm: &[u8]) -> String {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("known-closure-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../host/run.mjs"))
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

/// `add_one` holds only the closure call, so its `call` count is the
/// direct calls A1 made.
const BOUND_ONCE: &str = "\
fn add_one() -> i32:
    let f = fn(x: i32) -> i32: x + 1
    f(2)

pub fn main() -> void $ Console:
    println(add_one())
";

#[test]
fn a_closure_bound_once_is_called_directly_in_the_optimized_pipeline() {
    let dev = wasm_of(BOUND_ONCE, false);
    let opt = wasm_of(BOUND_ONCE, true);
    assert_eq!(calls_in(&dev, "app/main/add_one#0"), (0, 1), "dev");
    assert_eq!(calls_in(&opt, "app/main/add_one#0"), (1, 0), "optimized");
    assert_eq!(run("bound-dev", &dev), "3\n");
    assert_eq!(run("bound-opt", &opt), "3\n");
}

/// A capturing closure, a function reference, a literal called in place,
/// and a reassigned local, which stays indirect.
const FORMS: &str = "\
fn double(x: i32) -> i32:
    x * 2

fn forms(k: i32) -> i32:
    let add = fn(x: i32) -> i32: x + k
    let twice = double
    let total = add(1) + twice(2) + (fn(x: i32) -> i32: x * 7)(3)
    let swap = fn(x: i32) -> i32: x
    swap = fn(x: i32) -> i32: x - 1
    total + swap(1)

pub fn main() -> void $ Console:
    println(forms(+10))
";

#[test]
fn captures_function_references_and_literals_are_direct_but_reassigned_locals_are_not() {
    let dev = wasm_of(FORMS, false);
    let opt = wasm_of(FORMS, true);
    assert_eq!(calls_in(&dev, "app/main/forms#0").1, 4, "dev");
    assert_eq!(calls_in(&opt, "app/main/forms#0").1, 1, "optimized");
    assert_eq!(run("forms-dev", &dev), "36\n");
    assert_eq!(run("forms-opt", &opt), "36\n");
}

/// A suspending closure keeps its call protocol: the cold call is direct,
/// its suspension is polled as before.
const SUSPENDING: &str = "\
fn delayed!(value: i32) -> i32:
    value

fn drive!(base: i32) -> i32:
    loader := fn!(offset: i32) -> i32:
        delayed!(base + offset)
    loader!(2)

pub fn main!() -> void $ Console:
    println(drive!(+40))
";

#[test]
fn a_suspending_closure_is_constructed_directly_and_polled_as_before() {
    let dev = wasm_of(SUSPENDING, false);
    let opt = wasm_of(SUSPENDING, true);
    let (dev_calls, dev_refs) = calls_in(&dev, "app/main/drive#0$Body");
    let (opt_calls, opt_refs) = calls_in(&opt, "app/main/drive#0$Body");
    assert_eq!(opt_refs + 1, dev_refs, "one call_ref fewer");
    assert_eq!(opt_calls, dev_calls + 1, "one direct call more");
    assert_eq!(run("susp-dev", &dev), "42\n");
    assert_eq!(run("susp-opt", &opt), "42\n");
}

/// A program the other tests do not build, so collection runs for it in
/// both pipelines even with a warm store.
const PLANNED: &str = "\
fn apply(f: fn(i32) -> i32, x: i32) -> i32:
    f(x)

fn planned() -> i32:
    let g = fn(x: i32) -> i32: x * 3
    g(1) + apply(fn(x: i32) -> i32: x + 4, +2)

pub fn main() -> void $ Console:
    println(planned())
";

/// Dev collection does no plan work: the dev pipeline has no body-unit
/// pass, so its planner returns at once for every body and no pass runs.
/// The optimized pipeline runs its body passes on the same program.
#[test]
fn dev_collection_runs_no_body_pass() {
    let dev = build_of(PLANNED, false);
    let opt = build_of(PLANNED, true);
    assert_eq!(dev.counters.body_passes, 0, "dev");
    assert!(opt.counters.body_passes > 0, "optimized");
    let (dev, opt) = (dev.wasm.expect("wasm"), opt.wasm.expect("wasm"));
    assert_eq!(run("planned-dev", &dev), "9\n");
    assert_eq!(run("planned-opt", &opt), "9\n");
}

/// `apply_twice` calls its parameter in its own body: the optimized
/// pipeline copies it for the literal, and the copy calls the literal's
/// code directly. The plain instance is not built, as no call needs it.
const SPECIALIZED: &str = "\
fn apply_twice(f: fn(i32) -> i32, x: i32) -> i32:
    f(f(x))

fn run() -> i32:
    apply_twice(fn(x: i32) -> i32: x + 1, +5)

pub fn main() -> void $ Console:
    println(run())
";

/// `V8` and `Browser` have the same pipeline content at each level, so the
/// same bytes (`crates/hd_opt/DESIGN.md` (optimization pipelines) §3.3); `Wasmtime` stores closure code
/// differently (`closure_code_targets.rs`). Each build has its own store,
/// so none is a cache hit.
#[test]
fn targets_with_equal_content_build_the_same_module_at_each_level() {
    for level in OptLevel::ALL {
        let fresh = hd_cache::MemoryStore::default;
        let v8 = build_for(SPECIALIZED, EngineTarget::V8, level, &fresh()).wasm;
        let browser = build_for(SPECIALIZED, EngineTarget::Browser, level, &fresh()).wasm;
        assert_eq!(browser, v8, "{level:?}");
    }
}

#[test]
fn a_callee_is_copied_per_literal_and_calls_it_directly_in_the_copy() {
    let dev = wasm_of(SPECIALIZED, false);
    let opt = wasm_of(SPECIALIZED, true);
    let plain = "app/main/apply_twice#0";
    assert_eq!(calls_in(&dev, plain), (0, 2), "dev");
    assert!(copies_of(&dev, plain).is_empty(), "dev makes no copy");
    let copies = copies_of(&opt, plain);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert_eq!(calls_in(&opt, &copies[0]), (2, 0), "optimized");
    assert!(!has(&opt, plain), "no caller needs the plain instance");
    assert_eq!(run("copy-dev", &dev), "7\n");
    assert_eq!(run("copy-opt", &opt), "7\n");
}

/// A callee over 64 TIR instructions is never copied: its call of the
/// parameter stays a `call_ref` in the plain instance. `small`, of the same
/// shape, is.
const OVER_BOUND: &str = "\
fn small(f: fn(i32) -> i32, x: i32) -> i32:
    let a1 = x * 3 + 1
    f(a1 % 1000)

fn big(f: fn(i32) -> i32, x: i32) -> i32:
    let a1 = x * 3 + 1
    let a2 = a1 * 3 + 2
    let a3 = a2 * 3 + 3
    let a4 = a3 * 3 + 4
    let a5 = a4 * 3 + 5
    let a6 = a5 % 1000 + 6
    let a7 = a6 * 3 + 7
    let a8 = a7 * 3 + 8
    let a9 = a8 * 3 + 9
    let a10 = a9 % 1000 + 10
    let a11 = a10 * 3 + 11
    let a12 = a11 * 3 + 12
    let a13 = a12 * 3 + 13
    let a14 = a13 % 1000 + 14
    let a15 = a14 * 3 + 15
    let a16 = a15 * 3 + 16
    let a17 = a16 * 3 + 17
    let a18 = a17 % 1000 + 18
    let a19 = a18 * 3 + 19
    let a20 = a19 * 3 + 20
    f(a20 % 1000)

pub fn main() -> void $ Console:
    println(big(fn(x: i32) -> i32: x + 1, +5))
    println(small(fn(x: i32) -> i32: x + 1, +5))
";

#[test]
fn a_callee_over_the_size_bound_keeps_its_call_ref() {
    let dev = wasm_of(OVER_BOUND, false);
    let opt = wasm_of(OVER_BOUND, true);
    let plain = "app/main/big#0";
    assert!(copies_of(&opt, plain).is_empty(), "no copy");
    assert_eq!(calls_in(&opt, plain).1, 1, "optimized");
    assert_eq!(
        copies_of(&opt, "app/main/small#0").len(),
        1,
        "small is copied"
    );
    assert_eq!(run("bound-over-dev", &dev), run("bound-over-opt", &opt));
}

/// A copy that passes its literal on to itself calls itself: one copy, and
/// no plain instance.
const PASS_THROUGH: &str = "\
fn repeat(f: fn(i32) -> i32, n: i32, x: i32) -> i32:
    if n == 0:
        x
    else:
        repeat(f, n - 1, f(x))

pub fn main() -> void $ Console:
    println(repeat(fn(x: i32) -> i32: x * 2, +3, +1))
";

#[test]
fn a_copy_passing_its_literal_on_to_itself_is_one_copy() {
    let dev = wasm_of(PASS_THROUGH, false);
    let opt = wasm_of(PASS_THROUGH, true);
    let plain = "app/main/repeat#0";
    let copies = copies_of(&opt, plain);
    assert_eq!(copies.len(), 1, "{copies:?}");
    assert!(!has(&opt, plain), "no plain instance");
    assert_eq!(calls_in(&opt, &copies[0]).1, 0, "every call is direct");
    assert_eq!(run("pass-dev", &dev), "8\n");
    assert_eq!(run("pass-opt", &opt), "8\n");
}

/// Two literals make two copies; one literal passed twice, through a local
/// bound once, makes one.
const LITERALS: &str = "\
fn apply(f: fn(i32) -> i32, x: i32) -> i32:
    f(x)

fn two() -> i32:
    apply(fn(x: i32) -> i32: x + 1, +1) + apply(fn(x: i32) -> i32: x * 10, +2)

fn same() -> i32:
    let inc = fn(x: i32) -> i32: x + 100
    apply(inc, +1) + apply(inc, +2)

pub fn main() -> void $ Console:
    println(two())
    println(same())
";

#[test]
fn two_literals_make_two_copies_and_one_literal_twice_makes_one() {
    let dev = wasm_of(LITERALS, false);
    let opt = wasm_of(LITERALS, true);
    let copies = copies_of(&opt, "app/main/apply#0");
    assert_eq!(copies.len(), 3, "{copies:?}");
    for c in &copies {
        assert_eq!(calls_in(&opt, c), (1, 0), "{c}");
    }
    assert_eq!(run("lits-dev", &dev), "22\n203\n");
    assert_eq!(run("lits-opt", &opt), "22\n203\n");
}

/// A copy may request one more copy for a literal it passes on; past that
/// depth the callee is the plain instance, with its `call_ref`.
const DEPTH: &str = "\
fn third(f: fn(i32) -> i32, x: i32) -> i32:
    f(x) + 3

fn second(f: fn(i32) -> i32, x: i32) -> i32:
    f(x) + third(f, x)

fn first(f: fn(i32) -> i32, x: i32) -> i32:
    f(x) + second(f, x)

pub fn main() -> void $ Console:
    println(first(fn(x: i32) -> i32: x + 1, +1))
";

#[test]
fn copies_stop_at_depth_two() {
    let dev = wasm_of(DEPTH, false);
    let opt = wasm_of(DEPTH, true);
    assert_eq!(copies_of(&opt, "app/main/first#0").len(), 1);
    assert_eq!(copies_of(&opt, "app/main/second#0").len(), 1);
    assert!(copies_of(&opt, "app/main/third#0").is_empty());
    assert_eq!(
        calls_in(&opt, "app/main/third#0").1,
        1,
        "plain past the depth"
    );
    assert_eq!(run("depth-dev", &dev), "9\n");
    assert_eq!(run("depth-opt", &opt), "9\n");
}

/// The std adapters call the user's closure inside the step closure they
/// pass to `from_fn`: the copy's step closure carries the literal, so that
/// call is direct. A function reference counts as a literal. Only `next`,
/// which reads the `step` field, keeps a `call_ref`.
const ADAPTERS: &str = "\
fn double(x: i32) -> i32:
    x * 2

fn total(items: List[i32]) -> i32:
    items.iter().map(fn(x): x * 3 + 1).filter(fn(x): x % 2 == 0).fold(0, fn(a, x): a + x)

fn refs(items: List[i32]) -> i32:
    items.iter().map(double).fold(0, fn(a, x): a + x)

pub fn main() -> void $ Console:
    println(total([+1, +2, +3, +4, +5]))
    println(refs([+1, +2, +3, +4]))
";

#[test]
fn closures_a_copy_creates_call_its_known_literals_directly() {
    let dev = wasm_of(ADAPTERS, false);
    let opt = wasm_of(ADAPTERS, true);
    let iter = "std/iter/impl [ T ] Iterator [ T ]";
    assert_eq!(calls_in(&dev, &format!("{iter}.map#1")).1, 1, "dev");
    for step in ["map#1", "filter#1"] {
        let copies = copies_of(&opt, &format!("{iter}.{step}"));
        assert!(!copies.is_empty(), "{step} is carried");
        for c in &copies {
            assert_eq!(calls_in(&opt, c).1, 0, "{c}");
        }
    }
    assert_eq!(
        copies_of(&opt, &format!("{iter}.map#1")).len(),
        2,
        "per literal"
    );
    assert!(
        !has(&opt, &format!("{iter}.map#1")),
        "no plain step closure"
    );
    assert_eq!(run("adapters-dev", &dev), "30\n20\n");
    assert_eq!(run("adapters-opt", &opt), "30\n20\n");
}

/// A function that wraps its parameter in a new closure and recurses makes
/// a new literal at each level: the depth bound ends the copies, and the
/// plain instance takes over past it.
const WRAPPING: &str = "\
fn wrap(g: fn(i32) -> i32, n: i32, x: i32) -> i32:
    if n == 0:
        g(x)
    else:
        wrap(fn(y: i32) -> i32: g(y) + 1, n - 1, x)

pub fn main() -> void $ Console:
    println(wrap(fn(x: i32) -> i32: x * 10, +5, +1))
";

#[test]
fn wrapping_recursion_stops_at_the_depth_bound() {
    let dev = wasm_of(WRAPPING, false);
    let opt = wasm_of(WRAPPING, true);
    let copies = copies_of(&opt, "app/main/wrap#0");
    assert!(
        (1..=4).contains(&copies.len()),
        "bounded copies: {copies:?}"
    );
    assert!(
        has(&opt, "app/main/wrap#0"),
        "the plain instance past the bound"
    );
    assert_eq!(run("wrap-dev", &dev), "15\n");
    assert_eq!(run("wrap-opt", &opt), "15\n");
}

/// Closures a copy creates may escape it, returned or stored in data, and
/// still call the literal directly; a closure nested in one carries it too.
const ESCAPING: &str = "\
fn double(x: i32) -> i32:
    x * 2

fn doubled(f: fn(i32) -> i32) -> fn(i32) -> i32:
    fn(x: i32) -> i32: f(x) * 2

struct Holder:
    run: fn(i32) -> i32

fn hold(f: fn(i32) -> i32) -> Holder:
    Holder { run: fn(x: i32) -> i32: f(x) - 1 }

fn nested(f: fn(i32) -> i32) -> fn(i32) -> i32:
    let outer = fn(x: i32) -> i32:
        let inner = fn(y: i32) -> i32: f(y) + f(x)
        inner(x + 1)
    outer

pub fn main() -> void $ Console:
    let d = doubled(fn(x: i32) -> i32: x + 1)
    println(d(+20))
    let h = hold(fn(x: i32) -> i32: x * 3)
    println((h.run)(+7))
    let fs = [doubled(double), doubled(fn(x: i32) -> i32: x - 4)]
    println(fs[0](+3) + fs[1](+3))
    println(nested(fn(x: i32) -> i32: x * x)(+2))
";

#[test]
fn closures_escaping_a_copy_keep_their_direct_calls() {
    let dev = wasm_of(ESCAPING, false);
    let opt = wasm_of(ESCAPING, true);
    let carried = copies_of(&opt, "app/main/doubled#1");
    assert_eq!(carried.len(), 3, "one per literal: {carried:?}");
    for c in carried
        .iter()
        .chain(&copies_of(&opt, "app/main/hold#1"))
        .chain(&copies_of(&opt, "app/main/nested#2"))
    {
        assert_eq!(calls_in(&opt, c).1, 0, "{c}");
    }
    assert_eq!(run("escape-dev", &dev), "42\n20\n10\n13\n");
    assert_eq!(run("escape-opt", &opt), "42\n20\n10\n13\n");
}
