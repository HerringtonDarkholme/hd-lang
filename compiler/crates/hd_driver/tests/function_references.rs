//! Function references (spec/lang/07-functions.md, Generic Function
//! Values and Method References) through the checker: each kind's
//! `ItemRef` and type, a bound reference's closure, and the errors. Then
//! called through their adapters (codegen.md §13.11), built by the driver
//! and run on V8 (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn analyze(main: &str, render: &[&str]) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let store = MemoryStore::default();
    let host = Host {
        render_tir: render,
        sources: &src,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", &Goal::Analyze)
}

fn errors(src: &str) -> Vec<Code> {
    let out = analyze(src, &[]);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const COUNTER: &str = "data Counter:
    value: i32

impl Counter:
    fn bump(mut self, by: i32) -> void:
        self.value = self.value + by

    fn read(self) -> i32:
        self.value

    fn zero() -> Counter:
        Counter { value: 0 }

    fn pick[V](self, value: V) -> V:
        value

trait Named:
    fn label(self) -> string

trait Factory:
    fn create() -> Self

impl Factory for Counter:
    fn create() -> Counter:
        Counter { value: 1 }

data Store:
    name: string

impl Store:
    fn load!(self, key: i32) -> i32:
        key

fn identity[T](value: T) -> T:
    value

fn apply[A, B](value: A, f: fn(A) -> B) -> B:
    f(value)

fn empty[C = i32]() -> Option[C]:
    .None
";

/// The rendered TIR line of the body's first instruction with `tag`.
fn line_of(out: &Output, body: &str, tag: &str) -> String {
    let text = out
        .tir_text
        .get(body)
        .unwrap_or_else(|| panic!("no body {body}; have {:?}", out.tir_text.keys()));
    text.lines()
        .find(|l| l.contains(&format!("= {tag} ")))
        .unwrap_or_else(|| panic!("no {tag} in {body}:\n{text}"))
        .to_owned()
}

/// Each kind of reference is an `ItemRef` of the member's function
/// type: receiver first for a method, the member's own parameters for
/// an associated function, `Self` from the expected type for a trait
/// member, type arguments from the expected type, an explicit list or
/// the call, and a default when nothing else solves one.
#[test]
fn unbound_references_have_their_function_types() {
    let src = format!(
        "{COUNTER}
fn reader() -> fn(Counter) -> i32:
    Counter::read

fn stepper() -> fn(mut Counter, i32) -> void:
    Counter::bump

fn maker() -> fn() -> Counter:
    Counter::zero

fn factory() -> fn() -> Counter:
    Factory::create

fn labels[T < Named](items: List[T]) -> fn(T) -> string:
    T::label

fn picker() -> fn(Counter, i32) -> i32:
    Counter::pick::[i32]

fn loader() -> fn!(Store, i32) -> i32:
    Store::load

fn counted() -> i32:
    apply(3, identity)

fn explicit() -> fn(string) -> string:
    identity::[string]

fn made() -> void:
    make := empty
"
    );
    let bodies = [
        "app/main/reader",
        "app/main/stepper",
        "app/main/maker",
        "app/main/factory",
        "app/main/labels",
        "app/main/picker",
        "app/main/loader",
        "app/main/counted",
        "app/main/explicit",
        "app/main/made",
    ];
    let out = analyze(&src, &bodies);
    assert!(!out.diags.has_errors(), "{}", out.render());
    let want = [
        ("app/main/reader", ": fn(Counter) -> i32"),
        ("app/main/stepper", ": fn(mut Counter, i32) -> void"),
        ("app/main/maker", ": fn() -> Counter"),
        ("app/main/factory", ": fn() -> Counter"),
        ("app/main/labels", ": fn(labels#0) -> string"),
        ("app/main/picker", ": fn(Counter, i32) -> i32"),
        ("app/main/loader", ": fn!(Store, i32) -> i32"),
        ("app/main/counted", ": fn(i32) -> i32"),
        ("app/main/explicit", ": fn(string) -> string"),
        ("app/main/made", ": fn() -> i32?"),
    ];
    for (body, ty) in want {
        let line = line_of(&out, body, "ItemRef");
        assert!(line.ends_with(ty), "{body}: {line}");
    }
}

/// `counter::bump` is a closure over the method call that captures the
/// receiver, evaluated once; its type drops the receiver.
#[test]
fn a_bound_reference_is_a_closure_over_the_receiver() {
    let src = format!(
        "{COUNTER}
fn bumper(counter: mut Counter) -> fn(i32) -> void:
    counter::bump
"
    );
    let out = analyze(&src, &["app/main/bumper"]);
    assert!(!out.diags.has_errors(), "{}", out.render());
    let closure = line_of(&out, "app/main/bumper", "Closure");
    assert!(closure.ends_with(": fn(i32) -> void"), "{closure}");
    let call = line_of(&out, "app/main/bumper", "Call");
    assert!(call.contains("bump"), "{call}");
    let text = &out.tir_text["app/main/bumper"];
    assert!(text.contains("$receiver"), "{text}");
    assert!(!text.contains("ItemRef"), "{text}");
}

/// The errors of spec 07's Method References and Generic Function
/// Values.
#[test]
fn reference_errors_are_reported() {
    let cases = [
        // `fn.ref.no-fields`.
        (
            "data User:\n    email: string\n\nfn emails() -> fn(User) -> string:\n    User::email\n",
            Code::UnknownMethod,
        ),
        // `fn.ref.bound.associated`.
        (
            "data Counter:\n    value: i32\n\nimpl Counter:\n    fn zero() -> Counter:\n        Counter { value: 0 }\n\nfn restart(counter: Counter) -> fn() -> Counter:\n    counter::zero\n",
            Code::UnknownMethod,
        ),
        // `fn.ref.bound.mut`.
        (
            "data Counter:\n    value: i32\n\nimpl Counter:\n    fn bump(mut self, by: i32) -> void:\n        self.value = self.value + by\n\nfn frozen(counter: Counter) -> fn(i32) -> void:\n    counter::bump\n",
            Code::MutableReceiverRequired,
        ),
        // `fn.ref.trait-self.unsolved`.
        (
            "fn unsolved() -> void:\n    _ := Display::to_string\n",
            Code::CannotInferType,
        ),
        // `fn.type.generic.unsolved`, through a call.
        (
            "fn identity[T](value: T) -> T:\n    value\n\nfn call_with_nothing[A, B](f: fn(A) -> B) -> void:\n    pass\n\nfn run() -> void:\n    call_with_nothing(identity)\n",
            Code::CannotInferType,
        ),
        // `fn.type.generic.instantiate-from`: a later statement does not
        // instantiate a reference.
        (
            "fn identity[T](value: T) -> T:\n    value\n\nfn run() -> i32:\n    f := identity\n    f(3)\n",
            Code::CannotInferType,
        ),
        // `fn.ref.lookup.ambiguous`.
        (
            "trait Left:\n    fn name(self) -> string\n\ntrait Right:\n    fn name(self) -> string\n\ndata Both: pass\n\nimpl Left for Both:\n    fn name(self) -> string: \"l\"\n\nimpl Right for Both:\n    fn name(self) -> string: \"r\"\n\nfn named() -> fn(Both) -> string:\n    Both::name\n",
            Code::AmbiguousMethod,
        ),
        // `fn.ref.unbound.mut-self` with `fn.type.declared-variance`.
        (
            "data Counter:\n    value: i32\n\nimpl Counter:\n    fn reset(mut self) -> void:\n        self.value = 0\n\nfn readonly() -> fn(Counter) -> void:\n    Counter::reset\n",
            Code::TypeMismatch,
        ),
    ];
    for (src, code) in cases {
        assert_eq!(errors(src), vec![code], "{src}");
    }
}

fn build_program(store: &MemoryStore, main: &str) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
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
    assert!(out.diags.is_empty(), "{}", out.render());
    out
}

/// Runs a built program on V8 and returns its standard output.
fn run(out: &Output, name: &str) -> String {
    let wasm = out.wasm.as_ref().expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("fnref-run-{name}.wasm"));
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

/// Each kind of unbound reference is called through its adapter: a plain
/// function, a generic function instantiated from the expected type
/// (`fn.type.generic.instantiate-from`), an unbound method taking the
/// receiver first (`fn.ref.unbound.receiver`), an associated function
/// (`fn.ref.associated`), a trait member whose `Self` comes from the
/// expected type (`fn.ref.trait-self`), a suspending method that keeps
/// its suspension (`fn.ref.suspending`), and a function whose row the
/// adapter forwards from the caller's context (`fn.ref.value`). A second
/// build re-emits nothing.
#[test]
fn references_call_through_their_adapters() {
    let main = format!(
        "{COUNTER}
fn double(x: i32) -> i32:
    x * 2

fn shout(x: i32) -> void $ Console:
    println(x + 1)

fn twice(f: fn(i32) -> i32, x: i32) -> i32:
    f(f(x))

fn fetch!(f: fn!(Store, i32) -> i32, store: Store) -> i32:
    f!(store, 5)

pub fn main!() -> void $ Console:
    println(twice(double, 5))
    let echo: fn(string) -> string = identity
    println(echo(\"generic\"))
    bump := Counter::bump
    let mut counter = Counter {{ value: 1 }}
    bump(counter, 4)
    println(counter.value)
    zero := Counter::zero
    println(zero().value)
    let make: fn() -> Counter = Factory::create
    println(make().value)
    println(fetch!(Store::load, Store {{ name: \"cache\" }}))
    let say: fn(i32) -> void $ Console = shout
    say(9)
"
    );
    let store = MemoryStore::default();
    let out = build_program(&store, &main);
    assert_eq!(run(&out, "kinds"), "20\ngeneric\n5\n0\n1\n5\n10\n");
    let again = build_program(&store, &main);
    assert_eq!(again.counters.emitted, 0, "re-emitted");
    assert_eq!(again.wasm, out.wasm);
}
