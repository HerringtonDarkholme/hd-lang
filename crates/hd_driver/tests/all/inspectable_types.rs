//! Which types are `Inspectable` (trait.inspectable.exact), and that no
//! `Inspectable` trait is a requirement key (req.key.inspectable). Each
//! row of the spec's table is a valid case, each excluded kind an invalid
//! one with exactly one error, and each place a key can be written is
//! covered.

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn errors(main: &str) -> Vec<Code> {
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
    let out = build(&host, "app", &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .filter(|c| *c != Code::Unsupported)
        .collect()
}

const DECLS: &str = "use std.inspect.{Inspectable, TypeId}

data Plain:
    name: string

data Boxed[T]:
    value: T

enum Shade:
    Light
    Dark

type Meters(i32)

type Callback(fn() -> i32)

data Holder:
    run: fn() -> i32

trait Named < Inspectable:
    fn name(self) -> string

trait Deeper < Named:
    fn depth(self) -> i32

fn id_of[T < Inspectable]() -> TypeId:
    TypeId::of::[T]()

fn id_of_value[T < Inspectable](value: T) -> TypeId:
    TypeId::of::[T]()

fn id_of_pair[T < Inspectable](first: T, second: T) -> TypeId:
    TypeId::of::[T]()
";

/// A function `run` whose body asks for the `TypeId` of `ty`.
fn type_id_program(ty: &str) -> String {
    format!("{DECLS}\nfn run() -> TypeId:\n    TypeId::of::[{ty}]()\n")
}

/// The primitives and `string`, the declared types applied to inspectable
/// arguments, the collections, tuples, dynamic values of `Inspectable` or
/// a subtrait, and the trait values and `Any` as type arguments.
#[test]
fn every_row_of_the_table_is_inspectable() {
    let valid = [
        // trait.inspectable.primitive
        "i8",
        "i32",
        "u64",
        "usize",
        "f32",
        "f64",
        "bool",
        "char",
        "string",
        "void",
        // trait.inspectable.declared
        "Plain",
        "Boxed[i32]",
        "Boxed[Boxed[string]]",
        "Shade",
        "Meters",
        // trait.inspectable.option
        "i32?",
        "Plain?",
        "Result[void, string]",
        "Result[Plain, string]",
        // trait.inspectable.fields: identity is the declaration
        "Holder",
        "Callback",
        // trait.inspectable.collections
        "List[i32]",
        "Map[string, Plain]",
        "(i32, string)",
        "List[(i32, Map[string, bool])]",
        // trait.inspectable.dynamic
        "dyn Inspectable",
        "dyn Named",
        "dyn Deeper",
        // trait.inspectable.argument-only
        "List[dyn Display]",
        "Map[string, dyn Any]",
        "Boxed[dyn Display]",
        "Boxed[dyn Any]",
    ];
    for ty in valid {
        assert_eq!(errors(&type_id_program(ty)), vec![], "{ty}");
    }
}

/// trait.inspectable.parameter: a parameter bounded by `Inspectable` or a
/// trait that has it as a supertrait.
#[test]
fn a_bounded_type_parameter_is_inspectable() {
    let cases = [
        "fn run[T < Inspectable]() -> TypeId:\n    TypeId::of::[T]()\n",
        "fn run[T < Named]() -> TypeId:\n    TypeId::of::[T]()\n",
        "fn run[T < Deeper]() -> TypeId:\n    TypeId::of::[List[T]]()\n",
        "fn run[T < Inspectable](value: T) -> TypeId:\n    id_of_value(value)\n",
    ];
    for c in cases {
        assert_eq!(errors(&format!("{DECLS}\n{c}")), vec![], "{c}");
    }
}

/// Each excluded kind is one `unsatisfied-trait-bound`, with nothing after
/// it (trait.inspectable.not.*).
#[test]
fn excluded_types_are_one_error() {
    let invalid = [
        // trait.inspectable.not.never
        "never",
        // trait.inspectable.not.function
        "fn() -> i32",
        "fn(i32) -> string",
        // trait.inspectable.not.argument
        "Boxed[fn() -> i32]",
        "List[fn() -> i32]",
        "Map[string, fn() -> i32]",
        "(i32, fn() -> i32)",
        "(fn() -> i32)?",
        // trait.inspectable.dynamic-values: a trait value and `Any` are
        // inspectable as arguments only.
        "dyn Display",
        "dyn Any",
        // A tuple's elements are not type arguments.
        "(dyn Display, i32)",
    ];
    for ty in invalid {
        assert_eq!(
            errors(&type_id_program(ty)),
            vec![Code::UnsatisfiedTraitBound],
            "{ty}"
        );
    }
}

/// trait.inspectable.not.parameter: a parameter without the bound.
#[test]
fn an_unbounded_type_parameter_is_not_inspectable() {
    let src = format!("{DECLS}\nfn run[T]() -> TypeId:\n    TypeId::of::[T]()\n");
    assert_eq!(errors(&src), vec![Code::UnsatisfiedTraitBound]);
}

/// trait.inspectable.not.local: a declaration local to a block suite.
#[test]
fn a_local_declaration_is_not_inspectable() {
    let src = format!(
        "{DECLS}\nfn run() -> TypeId:\n    data Inner:\n        n: i32\n    TypeId::of::[Inner]()\n"
    );
    assert_eq!(errors(&src), vec![Code::UnsatisfiedTraitBound]);
}

/// trait.inspectable.not.never: a `never` argument solves the parameter
/// as `never` when nothing else does, so the bound fails there. Another
/// argument that solves it keeps the bound satisfied.
#[test]
fn a_never_argument_does_not_satisfy_the_bound() {
    let inferred = format!("{DECLS}\nfn run() -> TypeId:\n    id_of_value(panic(\"no value\"))\n");
    assert_eq!(errors(&inferred), vec![Code::UnsatisfiedTraitBound]);
    let explicit =
        format!("{DECLS}\nfn run() -> TypeId:\n    id_of_value::[never](panic(\"no value\"))\n");
    assert_eq!(errors(&explicit), vec![Code::UnsatisfiedTraitBound]);
    let solved =
        format!("{DECLS}\nfn run(n: i32) -> TypeId:\n    id_of_pair(n, panic(\"no value\"))\n");
    assert_eq!(errors(&solved), vec![]);
}

/// A function value is not inspectable as an argument either.
#[test]
fn a_function_value_is_not_inspectable() {
    let src = format!("{DECLS}\nfn run() -> TypeId:\n    id_of_value(fn() -> i32: 1)\n");
    assert_eq!(errors(&src), vec![Code::UnsatisfiedTraitBound]);
}

const KEYS: &str = "use std.inspect.Inspectable

trait Storage < Inspectable:
    fn get(self, key: string) -> string?

trait Cache < Storage:
    fn evict(mut self) -> void

trait Clock:
    fn now(self) -> i64

data MemoryStorage: pass

impl Storage for MemoryStorage:
    fn get(self, key: string) -> string?:
        .None
";

/// req.key.inspectable.error: every place a key is written is an error at
/// the key, once.
#[test]
fn an_inspectable_trait_is_no_requirement_key() {
    let cases = [
        // A requirement clause.
        (1, "fn load() -> void $ Storage:\n    pass\n"),
        // `Inspectable` itself, and a transitive subtrait.
        (1, "fn load() -> void $ Inspectable:\n    pass\n"),
        (1, "fn load() -> void $ Cache:\n    pass\n"),
        // One bad key in a row of several.
        (1, "fn load() -> void $ Clock + Storage:\n    pass\n"),
        // A function type's row, as a parameter, a result (two keys written),
        // and a field.
        (
            1,
            "fn schedule(job: fn() -> void $ Storage) -> void:\n    pass\n",
        ),
        (
            2,
            "fn make(job: fn() -> void $ Storage) -> (fn() -> void $ Storage):\n    job\n",
        ),
        (1, "data Job:\n    run: fn() -> void $ Storage\n"),
        // An alias of a function type, and a row alias.
        (1, "type Job = fn() -> void $ Storage\n"),
        (1, "type Row = $ Clock + Storage\n"),
        // A method row in an impl, and in a trait.
        (
            1,
            "data Worker: pass\n\nimpl Worker:\n    fn work(self) -> void $ Storage:\n        pass\n",
        ),
        (1, "trait Job:\n    fn run(self) -> void $ Storage\n"),
        // A provider scope.
        (
            1,
            "fn run() -> void:\n    $.with(Storage=MemoryStorage {}):\n        pass\n",
        ),
        // A context value.
        (
            1,
            "fn run() -> void:\n    _ := $.context(Storage=MemoryStorage {})\n",
        ),
        // A local function type's row.
        (
            2,
            "fn run() -> void:\n    let job: fn() -> void $ Storage = fn() -> void $ Storage: pass\n",
        ),
    ];
    for (count, c) in cases {
        let src = format!("{KEYS}\n{c}");
        let got: Vec<Code> = errors(&src)
            .into_iter()
            .filter(|c| *c != Code::UnusedLocalBinding)
            .collect();
        assert_eq!(got, vec![Code::InspectableRequirement; count], "{c}");
    }
}

/// One error per written key, none for the others around it.
#[test]
fn each_bad_key_is_one_error() {
    let src = format!(
        "{KEYS}\nfn load() -> void $ Storage + Cache + Clock:\n    pass\n\nfn run(job: fn() -> void $ Storage) -> void:\n    pass\n"
    );
    assert_eq!(
        errors(&src),
        vec![
            Code::InspectableRequirement,
            Code::InspectableRequirement,
            Code::InspectableRequirement
        ]
    );
}

/// req.key.inspectable.bound: such a trait stays valid as a bound and as a
/// value type, and an ordinary trait is still a key.
#[test]
fn an_inspectable_trait_stays_a_bound_and_a_value_type() {
    let cases = [
        "fn use_it[S < Storage](s: S) -> string?:\n    s.get(\"k\")\n",
        "fn use_dyn(s: dyn Storage) -> string?:\n    s.get(\"k\")\n",
        "fn now() -> i64 $ Clock:\n    $.use(Clock).now()\n",
        "fn schedule(job: fn() -> void $ Clock) -> void:\n    pass\n",
    ];
    for c in cases {
        assert_eq!(errors(&format!("{KEYS}\n{c}")), vec![], "{c}");
    }
}

/// A `never` that an earlier error left adds no second error.
#[test]
fn a_never_left_by_an_error_is_one_error() {
    let src = format!("{DECLS}\nfn run() -> TypeId:\n    id_of_value(missing)\n");
    assert_eq!(errors(&src), vec![Code::UnknownName]);
}
