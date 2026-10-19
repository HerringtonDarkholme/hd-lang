//! Typed facts (#283; annotations, "Member-Typed Facts", "Handles"): a
//! value of a typed fact type is checked where it attaches, by matching the
//! fact type's pattern against the target's type and then checking the
//! value like `let f: D[A] = v` (annot.typed-fact.infer,
//! annot.typed-fact.check). Each error is reported once, on the decorator
//! or member line (annot.typed-fact.check.reported). A handle's
//! `h.fact::[D]()` matches the same pattern against its `F`
//! (annot.handle.fact.pattern). The run tests build through the driver and
//! run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::{Code, Severity};
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(main: &str, goal: &Goal) -> Output {
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

/// Each error of a program that only analyzes, as its code and 1-based
/// line.
fn errors(main: &str) -> Vec<(Code, usize)> {
    let out = program(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .filter(|&i| out.diags.severity[i] == Severity::Error)
        .map(|i| {
            let lo = out.diags.primary[i].lo as usize;
            let line = main[..lo.min(main.len())].matches('\n').count() + 1;
            (out.diags.code[i], line)
        })
        .collect()
}

/// The 1-based line of the first line of `main` that contains `marker`.
fn line_of(main: &str, marker: &str) -> usize {
    main.lines()
        .position(|l| l.contains(marker))
        .map_or(0, |i| i + 1)
}

/// The program must report exactly one error, `code`, on the line marked
/// `# here`.
fn one_error(main: &str, code: Code) {
    assert_eq!(
        errors(main),
        vec![(code, line_of(main, "# here"))],
        "{main}"
    );
}

/// The standard output of a `main` program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = program(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("typed-facts-{name}.wasm"),
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

const FACT: &str = "use std.annotation.annotate
use std.num.{Integer, Num}
use std.structure.{Structure, Field}

@annotate::[T](.Field)
struct Fact[T]:
    value: T

fn fact[T](value: T) -> Fact[T]:
    Fact::[T] { value: value }

@annotate::[F](.Field)
struct Positive[F < Integer]: pass

fn positive[F < Integer]() -> Positive[F]:
    Positive::[F] {}

@annotate::[i32](.Field)
struct Port:
    number: i32

fn port(number: i32) -> Port:
    Port { number: number }

@annotate::[fn(T) -> R](.Fn, .Field)
struct FnFact[T < Num, R]: pass

fn fn_fact[T < Num, R]() -> FnFact[T, R]:
    FnFact::[T, R] {}

@annotate::[F](.Fn)
struct Handler[F]: pass

fn handler[R]() -> Handler[fn(i64) -> R]:
    Handler::[fn(i64) -> R] {}

@annotate::[F](.Field)
struct Fallback[F]:
    make: fn() -> F?

fn fallback[F](make: fn() -> F?) -> Fallback[F]:
    Fallback::[F] { make: make }

fn missing[T]() -> T?:
    .None

fn missing_list[T]() -> List[T]?:
    .None

@annotate::[List[T]](.Field)
struct MaxLen[T]:
    limit: i32

fn max_len[T](limit: i32) -> MaxLen[T]:
    MaxLen::[T] { limit: limit }
";

fn with(tail: &str) -> String {
    format!("{FACT}\n{tail}\n")
}

#[test]
fn values_that_fit_their_targets_check() {
    let src = with(
        "struct Account:
    @fact(5)
    a: i32
    @positive
    id: i64
    @port(80)
    listen: i32
    @fn_fact
    convert: fn(i32) -> i64
    @fallback(missing)
    name: string
    @max_len(3)
    tags: List[string]

struct Tally[T < Integer]:
    @positive
    hits: T

enum Shape:
    Circle(@fact(1.5) radius: f64)
    Tagged(@max_len(2) tags: List[string])

impl Account by Structure:
    a = [fact(6)]

@fn_fact
fn ms(n: i32) -> i64:
    0

@fn_fact
fn half[N < Num](n: N) -> f64:
    0.5

@handler
fn count(id: i64) -> i64:
    id

@handler
fn wrap[T](id: i64) -> List[T]:
    []",
    );
    assert_eq!(errors(&src), Vec::new());
}

#[test]
fn a_value_of_another_type_is_a_mismatch_at_its_decorator() {
    one_error(
        &with(
            "struct Account:
    @fact(\"x\")  # here
    b: i32",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_value_at_other_type_arguments_is_a_mismatch() {
    one_error(
        &with(
            "struct Account:
    @positive::[i32]()  # here
    count: i64",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn an_inferred_argument_that_breaks_its_bound_is_reported() {
    one_error(
        &with(
            "struct Account:
    @positive  # here
    name: string",
        ),
        Code::UnsatisfiedTraitBound,
    );
}

#[test]
fn a_concrete_pattern_takes_only_its_type() {
    one_error(
        &with(
            "struct Server:
    @port(9090)  # here
    admin: i64",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_function_pattern_checks_arity() {
    one_error(
        &with(
            "@fn_fact  # here
fn span(low: i32, high: i32) -> i64:
    0",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_function_pattern_rejects_a_field_that_is_not_a_function() {
    one_error(
        &with(
            "struct Meter:
    @fn_fact  # here
    count: i32",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_function_pattern_checks_the_bound_of_what_it_infers() {
    one_error(
        &with(
            "@fn_fact  # here
fn named(label: string) -> i64:
    0",
        ),
        Code::UnsatisfiedTraitBound,
    );
}

#[test]
fn a_suspending_function_does_not_fit_a_function_pattern() {
    one_error(
        &with(
            "@fn_fact  # here
fn later!(n: i32) -> i64:
    0",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_suspending_function_does_not_fit_a_value_of_fn_type() {
    one_error(
        &with(
            "@handler  # here
fn later!(id: i64) -> i64:
    id",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_function_with_a_requirement_row_does_not_fit_an_empty_row() {
    one_error(
        &with(
            "@fn_fact  # here
fn loud(n: i32) -> i64 $ Console:
    println(n)
    0",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_generic_reference_that_no_argument_fits_is_one_mismatch() {
    one_error(
        &with(
            "struct Profile:
    @fallback(missing_list)
    tags: List[string]
    @fallback(missing_list)  # here
    name: string",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_member_line_element_is_checked_against_its_member() {
    one_error(
        &with(
            "struct Profile:
    name: string

impl Profile by Structure:
    name = [fallback(missing_list)]  # here",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_payload_member_is_checked_against_its_type() {
    one_error(
        &with(
            "enum Shape:
    Circle(@fact(\"r\") radius: f64)  # here",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_handle_fact_infers_its_arguments_from_the_handle() {
    let src = with(
        "fn limit_of[S](h: Field[S, List[string]]) -> i32:
    let found: MaxLen[string]? = h.fact::[MaxLen]()
    match found:
        .Some(f) => f.limit
        .None => -1",
    );
    assert_eq!(errors(&src), Vec::new());
}

#[test]
fn a_handle_whose_type_does_not_fit_the_pattern_is_a_mismatch() {
    one_error(
        &with(
            "fn limit_of[S](h: Field[S, i32]) -> i32:
    match h.fact::[MaxLen]():  # here
        .Some(f) => f.limit
        .None => -1",
        ),
        Code::TypeMismatch,
    );
}

#[test]
fn a_handle_fact_written_with_other_arguments_is_a_mismatch() {
    one_error(
        &with(
            "fn limit_of[S](h: Field[S, List[string]]) -> i32:
    match h.fact::[MaxLen[i32]]():  # here
        .Some(f) => f.limit
        .None => -1",
        ),
        Code::TypeMismatch,
    );
}

const LABELS: &str = "use std.annotation.{annotate, facts_of}
use std.structure.{Structure, Field, Variant, Describer}

@annotate::[F](.Field, .Fn)
struct Tag[F]:
    label: string

fn tag[F](label: string) -> Tag[F]:
    Tag::[F] { label: label }

trait Labels:
    fn labels() -> string

fn label_of[S, F](h: Field[S, F]) -> string:
    match h.fact::[Tag[F]]():
        .Some(found) => found.label
        .None => \"-\"

struct Collect:
    out: string

impl[S] Describer[S] for Collect:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F]) -> Result[void, never]:
        self.out = self.out + h.info.name + \":\" + label_of(h) + \";\"
        .Ok(())

impl[T] Labels for T by Structure:
    fn labels() -> string:
        let mut c = Collect { out: \"\" }
        _ := T::describe(&mut c)
        c.out
";

#[test]
fn a_typed_fact_on_a_derivation_target_is_read_through_its_handle() {
    let src = format!(
        "{LABELS}
struct Counter:
    count: i64

@derive(Labels)
struct Job:
    @tag(\"n\")
    n: i32
    @tag(\"run\")
    run: fn() -> i32
    @tag(\"hits\")
    hits: Counter
    plain: List[string]

struct Pair[A]:
    @tag(\"first\")
    first: A
    second: List[A]

impl[A] Labels for Pair[A] by Structure:
    second += [tag(\"second\")]

@derive(Labels)
enum Event:
    Click(@tag(\"x\") x: i32, y: i32)
    Key(@tag(\"code\") code: string)

@derive(Labels)
struct Lined:
    a: i64
    b: string

impl Lined by Structure:
    b += [tag(\"b-line\")]

@tag(\"f\")
fn tagged(n: i64) -> i64:
    n

pub fn main() -> void $ Console:
    println(Job::labels())
    println(Pair::[i32]::labels())
    println(Pair::[string]::labels())
    println(Event::labels())
    println(Lined::labels())
    println(facts_of(tagged).items.len())
"
    );
    assert_eq!(
        output_of("handle", &src),
        "n:n;run:run;hits:hits;plain:-;
first:first;second:second;
first:first;second:second;
x:x;y:-;code:code;
a:-;b:b-line;
1
"
    );
}
