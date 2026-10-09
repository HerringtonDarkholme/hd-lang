//! Block-local declarations (spec 03 `names.local-fn.*`,
//! `names.local-type.*`, `names.local-impl.*`; spec 07 `fn.local.*`;
//! checking-and-tir.md "Local items lift to hidden module items"): local
//! types and impls are hidden module items, a local function is a named
//! closure. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = built(main);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("local-items-{name}.wasm"));
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

/// The rendered diagnostics of a program that must not build.
fn errors_of(main: &str) -> String {
    let out = built(main);
    assert!(out.wasm.is_none(), "the program must not build");
    out.render()
}

#[test]
fn a_local_data_value_goes_through_a_generic_function() {
    let main = "\
fn keep[T](value: T) -> T:
    value

fn route() -> string:
    data Stop:
        name: string
        minutes: i32
    stop := keep(Stop { name: \"Utrecht\", minutes: 27 })
    \"${stop.name} in ${stop.minutes}\"

pub fn main() -> void $ Console:
    println(route())
";
    assert_eq!(output_of("generic", main), "Utrecht in 27\n");
}

#[test]
fn two_functions_declare_local_data_of_one_name() {
    let main = "\
fn invoice_total() -> i32:
    data Line:
        quantity: i32
        unit: i32
    line := Line { quantity: 3, unit: 20 }
    line.quantity * line.unit

fn invoice_note() -> string:
    data Line:
        text: string
    Line { text: \"paid\" }.text

pub fn main() -> void $ Console:
    println(\"${invoice_total()} ${invoice_note()}\")
";
    assert_eq!(output_of("same-name", main), "60 paid\n");
}

#[test]
fn a_local_type_implements_display_locally() {
    let main = "\
fn receipt(cents: i32) -> string:
    data Money:
        cents: i32
    impl Display for Money:
        fn to_string(self) -> string:
            \"EUR ${self.cents / 100}.${self.cents % 100}\"
    total := Money { cents: cents }
    \"total ${total}\"

pub fn main() -> void $ Console:
    println(receipt(1250))
";
    assert_eq!(output_of("display", main), "total EUR 12.50\n");
}

#[test]
fn a_local_function_captures_a_local_and_recurses() {
    let main = "\
fn countdown(start: i32) -> string:
    separator := \", \"
    fn from(n: i32) -> string:
        if n == 0:
            \"liftoff\"
        else:
            \"${n}${separator}${from(n - 1)}\"
    from(start)

pub fn main() -> void $ Console:
    println(countdown(3))
";
    assert_eq!(output_of("recursion", main), "3, 2, 1, liftoff\n");
}

#[test]
fn a_local_function_is_not_visible_before_its_declaration() {
    let main = "\
fn shipping(weight: i32) -> i32:
    early := surcharge(weight)
    fn surcharge(w: i32) -> i32: w * 2
    early

pub fn main() -> void $ Console:
    println(shipping(4))
";
    let errors = errors_of(main);
    assert!(errors.contains("unknown-name"), "{errors}");
    assert!(errors.contains("surcharge"), "{errors}");
}

#[test]
fn a_local_enum_is_matched() {
    let main = "\
fn signal(speed: i32) -> string:
    enum Light:
        Red
        Green(limit: i32)
    light := if speed > 0: Light.Green(limit = speed) else: Light.Red
    match light:
        .Red => \"stop\"
        .Green(limit) => \"go up to ${limit}\"

pub fn main() -> void $ Console:
    println(signal(50))
    println(signal(0))
";
    assert_eq!(output_of("enum", main), "go up to 50\nstop\n");
}
