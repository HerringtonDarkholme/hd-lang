//! Local functions with parameter defaults (spec 07 `fn.local.*`,
//! `fn.default.*`): a local function is a named function, so a call omits
//! parameters that have defaults, names its arguments, and evaluates each
//! default after the explicit arguments over the earlier parameters. A
//! default may read the enclosing locals the function could capture.
//! Programs build through the driver and run on V8, as the conformance
//! runner does.

use crate::generic_local_fn_run::{errors_of, output_of};

#[test]
fn the_issue_example_runs() {
    let main = "\
pub fn main() -> void $ Console:
    fn greet(name: string = \"you\") -> string: \"hi $name\"
    println(greet())
";
    assert_eq!(output_of("default-issue", main), "hi you\n");
}

#[test]
fn a_call_gives_omits_or_names_the_argument() {
    let main = "\
pub fn main() -> void $ Console:
    fn greet(name: string = \"you\", mark: string = \"!\") -> string: \"hi $name$mark\"
    println(greet())
    println(greet(\"bob\"))
    println(greet(mark=\"?\"))
    println(greet(mark=\"?\", name=\"amy\"))
";
    assert_eq!(
        output_of("default-forms", main),
        "hi you!\nhi bob!\nhi you?\nhi amy?\n"
    );
}

#[test]
fn a_default_reads_earlier_parameters_and_captured_locals() {
    // `fn.default.scope`; the capture is read at each call.
    let main = "\
pub fn main() -> void $ Console:
    let base = +10
    fn add(a: i32, b: i32 = base, c: i32 = a + b) -> i32: a + b + c
    println(add(1))
    println(add(1, 2))
    println(add(1, c=5))
    base = +20
    println(add(1))
";
    assert_eq!(output_of("default-capture", main), "22\n6\n16\n42\n");
}

#[test]
fn defaults_run_after_the_arguments_in_declaration_order() {
    // `fn.default.eval`, `fn.default.effects`.
    let main = "\
pub fn main() -> void $ Console:
    let log = \"\"
    fn note(s: string) -> string:
        log = log + s
        s
    fn f(a: string, b: string = note(\"b\"), c: string = note(\"c\")) -> string: a + b + c
    r := f(note(\"a\"))
    println(\"$r $log\")
";
    assert_eq!(output_of("default-order", main), "abc abc\n");
}

#[test]
fn a_generic_local_function_has_defaults() {
    let main = "\
pub fn main() -> void $ Console:
    fn pick[T](x: T, n: i32 = +3, y: T = x) -> T: y
    println(pick(+7))
    println(pick(\"s\", n=+1))
    println(pick(+1, +2, +9))
    println(pick::[string](\"t\"))
";
    assert_eq!(output_of("default-generic", main), "7\ns\n9\nt\n");
}

#[test]
fn a_recursive_call_uses_the_defaults() {
    let main = "\
pub fn main() -> void $ Console:
    fn total(n: i32, step: i32 = +1) -> i32:
        if n <= 0:
            return 0
        n + total(n - step)
    println(total(+4))
";
    assert_eq!(output_of("default-recursive", main), "10\n");
}

#[test]
fn a_function_value_has_no_defaults() {
    // `fn.type.no-names`, `fn.type.positional`.
    let main = "\
pub fn main() -> void $ Console:
    fn greet(name: string = \"you\") -> string: \"hi $name\"
    g := greet
    println(g(\"z\"))
";
    assert_eq!(output_of("default-value", main), "hi z\n");
    let errors = errors_of(
        "\
pub fn main() -> void:
    fn greet(name: string = \"you\") -> string: \"hi $name\"
    g := greet
    g()
",
    );
    assert!(errors.contains("argument-count"), "{errors}");
}

#[test]
fn a_default_is_requirement_free() {
    let errors = errors_of(
        "\
trait Clock:
    fn now(self) -> i32

fn current_time() -> i32 $ Clock:
    $.use(Clock).now()

fn wait!() -> i32: 1

pub fn main() -> void:
    fn stamp(time: i32 = current_time()) -> i32 $ Clock: time
    fn late(value: i32 = wait!()) -> i32: value
",
    );
    assert!(errors.contains("requirement-in-default"), "{errors}");
    assert!(errors.contains("suspension-forbidden-context"), "{errors}");
}

#[test]
fn a_parameter_after_a_default_needs_one() {
    let errors = errors_of(
        "\
pub fn main() -> void:
    fn bad(first: i32 = 1, second: i32) -> i32: second
",
    );
    assert!(errors.contains("default-order"), "{errors}");
}

#[test]
fn only_defaulted_parameters_are_omitted() {
    let errors = errors_of(
        "\
pub fn main() -> void:
    fn f(a: i32, b: i32 = 1) -> i32: a + b
    f()
",
    );
    assert!(errors.contains("argument-count"), "{errors}");
}
