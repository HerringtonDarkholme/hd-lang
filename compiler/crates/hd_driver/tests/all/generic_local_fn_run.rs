//! Generic local functions (spec 07 `fn.local.*`, `fn.type.generic.*`;
//! spec 03 `names.type-param.no-redeclare.body`): a local function's own
//! type parameters, bounds and inference follow the ordinary generic
//! function rules. Its body is checked once over them; each use
//! instantiates it with the captures of its declaration. Programs build
//! through the driver and run on V8 (`host/run.mjs`, `host/test.mjs`), as
//! the conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = crate::warm_store();
    let host = Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn program(main: &str) -> Output {
    built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// Runs `wasm` on a Node host script with `args`.
fn node(name: &str, wasm: &[u8], script: &str, args: &[String]) -> std::process::Output {
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("generic-local-{name}.wasm"),
    );
    std::fs::write(&path, wasm).expect("write wasm");
    let ran = Command::new("node")
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../host")
                .join(script),
        )
        .arg(&path)
        .args(args)
        .output()
        .expect("node");
    let _ = std::fs::remove_file(&path);
    ran
}

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = program(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let ran = node(name, &out.wasm.expect("wasm"), "run.mjs", &[]);
    assert!(
        ran.status.success(),
        "{name}: {}",
        String::from_utf8_lossy(&ran.stderr)
    );
    String::from_utf8(ran.stdout).expect("UTF-8")
}

/// The rendered diagnostics of a program that must not build.
fn errors_of(main: &str) -> String {
    let out = program(main);
    assert!(out.wasm.is_none(), "the program must not build");
    out.render()
}

#[test]
fn the_issue_example_runs() {
    let main = "\
pub fn main() -> void $ Console:
    fn id[U](u: U) -> U: u
    println(id(+4))
";
    assert_eq!(output_of("issue", main), "4\n");
}

#[test]
fn one_body_is_called_at_two_types() {
    let main = "\
fn badges() -> string:
    fn badge[U < Display](label: U) -> string: \"[${label}]\"
    \"${badge(+3)} ${badge(\"new\")} ${badge(+3)}\"

pub fn main() -> void $ Console:
    println(badges())
";
    assert_eq!(output_of("two-types", main), "[3] [new] [3]\n");
}

#[test]
fn it_captures_an_enclosing_local_and_type_parameter() {
    // `count` is assigned through the capture; `T` is the enclosing
    // function's type parameter, `U` the local function's own.
    let main = "\
fn number_rows[T < Display](rows: List[T], prefix: string) -> string:
    let count = +0
    fn row[U < Display](item: T, note: U) -> string:
        count = count + 1
        \"${prefix}${count} ${item} ${note}\"
    each := [for item in rows => row(item, true)].join(\"; \")
    \"${each}; ${row(rows[0], \"again\")}\"

pub fn main() -> void $ Console:
    println(number_rows([\"ada\", \"bob\"], \"#\"))
";
    assert_eq!(
        output_of("captures", main),
        "#1 ada true; #2 bob true; #3 ada again\n"
    );
}

#[test]
fn a_bound_supplies_a_method() {
    let main = "\
trait Shape:
    fn area(self) -> i32

data Square:
    side: i32

impl Shape for Square:
    fn area(self) -> i32: self.side * self.side

data Rect:
    width: i32
    height: i32

impl Shape for Rect:
    fn area(self) -> i32: self.width * self.height

fn floor_area() -> i32:
    fn area_of[S < Shape](shape: S) -> i32: shape.area()
    area_of(Square { side: 3 }) + area_of(Rect { width: 2, height: 5 })

pub fn main() -> void $ Console:
    println(floor_area())
";
    assert_eq!(output_of("bound", main), "19\n");
}

#[test]
fn it_recurses_directly_and_from_a_closure_in_its_body() {
    let main = "\
fn depth() -> i32:
    floor := +100
    fn down[U](tag: U, n: i32) -> i32:
        if n == 0: 0 else: 1 + down(tag, n - 1)
    fn walk[U](tag: U, n: i32) -> i32:
        if n == 0:
            floor
        else:
            [n].map(fn(x: i32) -> i32: walk(tag, x - 1))[0]
    down(\"x\", 4) + down(true, 2) + walk(\"go\", 3)

pub fn main() -> void $ Console:
    println(depth())
";
    assert_eq!(output_of("recursion", main), "106\n");
}

#[test]
fn a_use_as_a_value_is_instantiated() {
    let main = "\
fn apply[A, B](value: A, f: fn(A) -> B) -> B:
    f(value)

fn sizes() -> string:
    fn wrap[U](u: U) -> List[U]: [u, u]
    let typed: fn(i32) -> List[i32] = wrap
    explicit := wrap::[string]
    passed := apply(true, wrap)
    \"${typed(+1).len()} ${explicit(\"a\").len()} ${passed.len()}\"

pub fn main() -> void $ Console:
    println(sizes())
";
    assert_eq!(output_of("value", main), "2 2 2\n");
}

#[test]
fn it_runs_inside_a_test_case() {
    let src = "\
pub fn main() -> void:
    pass

tests:
    use std.testing.assert_equal

    it(\"shouts at two types\"):
        suffix := \"!\"
        fn shout[U < Display](u: U) -> string: \"${u}${suffix}\"
        assert_equal(shout(+3), \"3!\", reason=\"number\")
        assert_equal(shout(\"hi\"), \"hi!\", reason=\"text\")
";
    let out = built(
        src,
        &Goal::Tests {
            module: None,
            filter: None,
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let runs: Vec<String> = out
        .tests
        .iter()
        .map(|t| {
            let (test, init) = t.run.expect("a runnable test");
            format!("{test}:{init}")
        })
        .collect();
    assert_eq!(runs.len(), 1);
    let ran = node(
        "test-case",
        &out.wasm.expect("wasm"),
        "test.mjs",
        &[runs.join(",")],
    );
    let stdout = String::from_utf8_lossy(&ran.stdout);
    assert!(stdout.contains("\"status\":0,"), "{stdout}");
    assert!(!stdout.contains("\"trapped\":true"), "{stdout}");
}

#[test]
fn a_type_parameter_of_the_enclosing_function_is_not_redeclared() {
    let errors = errors_of(
        "\
fn work[T](value: T) -> i32:
    fn convert[T](other: T) -> i32: 0
    0

pub fn main() -> void:
    pass
",
    );
    assert!(errors.contains("duplicate-binding"), "{errors}");
    assert!(
        errors.contains("`T` is a type parameter in scope"),
        "{errors}"
    );
}

#[test]
fn its_rules_are_a_generic_function_s() {
    let errors = errors_of(
        "\
trait Shape:
    fn area(self) -> i32

fn unsolved() -> i32:
    fn id[U](u: U) -> U: u
    f := id
    0

fn unbounded() -> i32:
    fn area_of[S < Shape](shape: S) -> i32: shape.area()
    area_of(+3)

fn checked_once() -> i32:
    fn size[U](u: U) -> i32: u.area()
    0

pub fn main() -> void:
    pass
",
    );
    // `fn.type.generic.unsolved`, a bound at the use, and the body checked
    // over its parameters, not over a use's types.
    assert!(errors.contains("cannot-infer-type"), "{errors}");
    assert!(
        errors.contains("unsatisfied-trait-bound: i32 does not implement Shape"),
        "{errors}"
    );
    assert!(
        errors.contains("unknown-method: no method `area`"),
        "{errors}"
    );
}
