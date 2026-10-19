//! Type-argument defaults in written types (spec/lang/04-type-system.md,
//! types.generic.default.written, types.generic.default.bare,
//! types.generic.default.written-missing; spec/lang/09-traits.md,
//! trait.decl.generic.default): a written type that omits a defaulted
//! trailing slot takes the default, with the earlier arguments and `Self`
//! substituted, in an alias, a data type, a trait reference and a method
//! call through a bound. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
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

/// The standard output of a program that must build and succeed.
pub(crate) fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("type-default-written-{name}.wasm"),
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

/// The diagnostic codes of a program checked without running.
pub(crate) fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

#[test]
fn an_alias_data_type_and_trait_fill_their_defaults() {
    let main = "\
struct AppError:
    message: string

type Outcome[T, E = AppError] = Result[T, E]

fn load(path: string) -> Outcome[string]:
    .Ok(path)

fn port(text: string) -> Outcome[i32, string]:
    .Err(text)

fn failure(outcome: Outcome[string]) -> string:
    match outcome:
        .Ok(path) => path
        .Err(error) => error.message

trait Evict:
    fn pick(self, sizes: List[i32]) -> i32

struct Lru: pass

impl Evict for Lru:
    fn pick(self, sizes: List[i32]) -> i32: i32(sizes.len())

struct Cache[K < Eq & Hash, V, P < Evict = Lru]:
    entries: Map[K, V]
    policy: P

fn size(cache: Cache[string, i32]) -> i32:
    cache.policy.pick([1, 2])

trait Same[Other = Self]:
    fn same(self, other: Other) -> bool

struct Money:
    cents: i64

impl Same for Money:
    fn same(self, other: Money) -> bool:
        self.cents == other.cents

fn check[T < Same](left: T, right: T) -> bool:
    left.same(right)

pub fn main() -> void $ Console:
    println(failure(.Err(AppError { message: \"bad\" })))
    println(failure(load(\"file\")))
    match port(\"x\"):
        .Ok(_) => println(\"ok\")
        .Err(text) => println(text)
    let entries: Map[string, i32] = {}
    let cache: Cache[string, i32] = Cache { entries: entries, policy: Lru {} }
    println(size(cache))
    println(check(Money { cents: 3 }, Money { cents: 3 }))
    println(check(Money { cents: 3 }, Money { cents: 4 }))
";
    assert_eq!(output_of("fixture", main), "bad\nfile\nx\n2\ntrue\nfalse\n");
}

#[test]
fn a_default_names_an_earlier_parameter() {
    let main = "\
type Twin[A, B = A] = (A, B)

struct Holder[T, U = List[T]]:
    first: T
    rest: U

fn twin(x: i32) -> Twin[i32]:
    (x, x + 1)

fn mixed(x: i32) -> Twin[i32, string]:
    (x, \"s\")

fn holder() -> Holder[i32]:
    Holder { first: 1, rest: [2, 3] }

pub fn main() -> void $ Console:
    t := twin(4)
    println(t._0 + t._1)
    println(mixed(5))
    h := holder()
    println(h.first + h.rest[0])
    let again: Holder[string] = Holder { first: \"a\", rest: [\"b\"] }
    println(again.rest[0])
";
    assert_eq!(output_of("earlier", main), "9\n(5, s)\n3\nb\n");
}

#[test]
fn a_default_names_self() {
    let main = "\
trait Same[Other = Self]:
    fn same(self, other: Other) -> bool

struct Money:
    cents: i64

struct Meters:
    value: i32

impl Same for Money:
    fn same(self, other: Money) -> bool:
        self.cents == other.cents

impl Same[Money] for Meters:
    fn same(self, other: Money) -> bool:
        i64(self.value) == other.cents

fn agree[T < Same](left: T, right: T) -> bool:
    left.same(right)

fn measured[T < Same[Money]](left: T, right: Money) -> bool:
    left.same(right)

pub fn main() -> void $ Console:
    println(agree(Money { cents: 2 }, Money { cents: 2 }))
    println(measured(Meters { value: 2 }, Money { cents: 2 }))
    println(measured(Money { cents: 1 }, Money { cents: 2 }))
";
    assert_eq!(output_of("self", main), "true\ntrue\nfalse\n");
}

#[test]
fn a_default_applies_whatever_the_declaration_order() {
    let main = "\
fn size(cache: Cache[string]) -> i32:
    cache.policy.pick()

fn load() -> Wrapped[i32]:
    .Ok(5)

type Wrapped[T, E = Failure] = Result[T, E]

struct Failure:
    code: i32

struct Outer[T, U = Inner]:
    item: T
    inner: U

struct Inner[V = i32]:
    value: V

struct Cache[K, P < Evict = Lru]:
    key: K
    policy: P

trait Evict:
    fn pick(self) -> i32

struct Lru: pass

impl Evict for Lru:
    fn pick(self) -> i32: 7

fn nested(outer: Outer[string]) -> i32:
    outer.inner.value

pub fn main() -> void $ Console:
    println(size(Cache { key: \"k\", policy: Lru {} }))
    match load():
        .Ok(n) => println(n)
        .Err(f) => println(f.code)
    println(nested(Outer { item: \"x\", inner: Inner { value: 8 } }))
";
    assert_eq!(output_of("order", main), "7\n5\n8\n");
}

#[test]
fn a_default_may_name_an_alias_and_an_alias_may_name_a_defaulted_type() {
    let main = "\
type Count = i32

struct Counter[T, N = Count]:
    item: T
    count: N

type Tally[T] = Counter[T]

fn tally(x: i32) -> Tally[string]:
    Counter { item: \"t\", count: x }

pub fn main() -> void $ Console:
    println(tally(6).count)
";
    assert_eq!(output_of("alias-default", main), "6\n");
}

#[test]
fn a_bare_name_omits_every_defaulted_slot() {
    let main = "\
struct Lru: pass

struct Slot[T = i32, P = Lru]:
    item: T
    policy: P

fn read(slot: Slot) -> i32:
    slot.item

pub fn main() -> void $ Console:
    println(read(Slot { item: 4, policy: Lru {} }))
";
    assert_eq!(output_of("bare", main), "4\n");
}

#[test]
fn a_method_call_through_a_bound_takes_the_method_default() {
    let main = "\
trait Blank:
    fn blank() -> Self

struct Tally:
    count: i32

impl Blank for Tally:
    fn blank() -> Tally:
        Tally { count: 5 }

trait Source:
    fn drain[C < Blank = Tally](self) -> C

struct Numbers: pass

impl Source for Numbers:
    fn drain[C < Blank = Tally](self) -> C:
        C::blank()

fn through_bound[S < Source](source: S) -> i32:
    tally := source.drain()
    tally.count

pub fn main() -> void $ Console:
    println(through_bound(Numbers {}))
";
    assert_eq!(output_of("method-bound", main), "5\n");
}

#[test]
fn too_few_arguments_without_a_default_are_partial_generic_arguments() {
    let main = "\
struct Pair[K, V]:
    key: K
    value: V

struct Mixed[A, B, C = A]:
    a: A
    b: B
    c: C

fn one(pair: Pair[string]) -> i32: 0

fn two(mixed: Mixed[string]) -> i32: 0
";
    assert_eq!(
        codes_of(main),
        vec![Code::PartialGenericArguments, Code::PartialGenericArguments]
    );
}

#[test]
fn a_filled_default_meets_its_parameters_bounds() {
    let signature = "\
struct Plain:
    x: i32

struct Tagged[T < Hash, H < Hash = T]:
    tag: T
    hash: H

fn use_it(tagged: Tagged[Plain]) -> i32: 0
";
    assert!(
        codes_of(signature)
            .iter()
            .all(|c| *c == Code::UnsatisfiedTraitBound)
            && !codes_of(signature).is_empty()
    );
    let body = "\
struct Plain:
    x: i32

struct Tagged[T < Hash, H < Hash = T]:
    tag: T
    hash: H

fn use_it() -> i32:
    let tagged: Tagged[Plain] = Tagged { tag: Plain { x: 1 }, hash: Plain { x: 1 } }
    tagged.tag.x
";
    assert!(codes_of(body).contains(&Code::UnsatisfiedTraitBound));
}
