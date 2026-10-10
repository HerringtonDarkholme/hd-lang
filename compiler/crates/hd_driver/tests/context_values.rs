//! Context values in bodies (checking-and-tir.md §4.13.4 and codegen.md
//! §12.4, "Context values"; spec 11 "Reusable Contexts"): `$.context`
//! builds a value of `$.Context[$ Row]`, one provider per key, and
//! `$.with(ctx...)` installs its providers. Entries apply left to right
//! and a later one wins (`req.context.order`).

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn run(store: &MemoryStore, main: &str, goal: &Goal) -> Output {
    let mut src = MemorySources::default();
    src.insert("main.hd", main);
    let host = Host {
        render_tir: &[],
        sources: &src,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    build(&host, "app", goal)
}

fn codes(main: &str) -> Vec<Code> {
    run(&MemoryStore::default(), main, &Goal::Analyze)
        .diags
        .code
        .clone()
}

/// The standard output of a one-file program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    output_in(&MemoryStore::default(), name, main)
}

/// [`output_of`], building against `store`, which earlier builds filled.
fn output_in(store: &MemoryStore, name: &str, main: &str) -> String {
    let goal = Goal::Program {
        entry: "main".into(),
    };
    let out = run(store, main, &goal);
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("context-{name}.wasm"));
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

const TAGS: &str = "
trait Tag:
    fn name(self) -> string

trait Log:
    fn tag(self) -> string

data Named:
    label: string

impl Tag for Named:
    fn name(self) -> string: self.label

impl Log for Named:
    fn tag(self) -> string: \"log:\" + self.label

fn current() -> string $ Tag:
    $.use(Tag).name()

fn both() -> string $ Tag + Log:
    $.use(Tag).name() + \",\" + $.use(Log).tag()
";

/// A context built in one function and installed in another serves every
/// key of its row (`req.context.create`, `req.context.spread`).
#[test]
fn a_built_context_installs_its_providers() {
    let main = format!(
        "{TAGS}
fn make() -> $.Context[$ Tag + Log]:
    $.context(Tag=Named {{ label: \"t\" }}, Log=Named {{ label: \"l\" }})

fn install(ctx: $.Context[$ Log + Tag]) -> string:
    $.with(ctx...):
        both()

pub fn main() -> void $ Console:
    println(install(make()))
"
    );
    assert_eq!(output_of("install", &main), "t,log:l\n");
}

/// Spreads and bindings apply left to right in `$.context` and in
/// `$.with`, and a later entry wins (`req.context.order`).
#[test]
fn a_later_entry_wins_over_a_spread() {
    let main = format!(
        "{TAGS}
pub fn main() -> void $ Console:
    base := $.context(Tag=Named {{ label: \"base\" }}, Log=Named {{ label: \"base\" }})
    over := $.context(base..., Tag=Named {{ label: \"over\" }})
    $.with(over...):
        println(both())
    $.with(Tag=Named {{ label: \"explicit\" }}, base...):
        println(current())
    $.with(base..., Tag=Named {{ label: \"explicit\" }}):
        println(current())
    $.with(base..., over...):
        println(both())
"
    );
    assert_eq!(
        output_of("order", &main),
        "over,log:base\nbase\nexplicit\nover,log:base\n"
    );
}

/// A provider value of a subtrait serves its supertrait's key in a
/// context, and a trait value of the key's own trait installs as it is.
#[test]
fn trait_values_serve_as_context_providers() {
    let main = "
trait Clock:
    fn now(self) -> i32

trait Backup < Clock:
    fn label(self) -> i32

data Fixed: pass

impl Clock for Fixed:
    fn now(self) -> i32: 7

impl Backup for Fixed:
    fn label(self) -> i32: 1

fn make() -> $.Context[$ Clock + Backup] $ Backup:
    $.context(Clock=$.use(Backup), Backup=$.use(Backup))

fn read() -> i32 $ Clock + Backup:
    $.use(Clock).now() * 10 + $.use(Backup).label()

pub fn main() -> void $ Console:
    $.with(Backup=Fixed {}):
        ctx := make()
        $.with(ctx...):
            println(read())
";
    assert_eq!(output_of("subtrait", main), "71\n");
}

/// A mutable provider installed through a context keeps one value for the
/// whole block, so each call sees the last one's change
/// (`req.mut.install-mutable`, `req.mut.use-mutable`).
#[test]
fn a_mutable_provider_through_a_context_keeps_its_state() {
    let main = "
trait Counter:
    fn count(self) -> i32
    fn bump(mut self) -> void

data Memory:
    value: i32

impl Counter for Memory:
    fn count(self) -> i32: self.value

    fn bump(mut self) -> void:
        self.value = self.value + 1

fn tick() -> i32 $ Counter:
    $.use(Counter).bump()
    $.use(Counter).count()

fn counters() -> $.Context[$ Counter]:
    $.context(Counter=Memory { value: 0 })

pub fn main() -> void $ Console:
    ctx := counters()
    $.with(ctx...):
        _ := tick()
        println(tick())
    $.with(ctx...):
        println(tick())
";
    assert_eq!(output_of("mutable", main), "2\n3\n");
}

/// A context value is an ordinary value: it is stored in a field and in a
/// list, and a closure called inside the block finds the installed keys
/// (`req.context.ordinary-values`).
#[test]
fn a_context_value_flows_like_any_value() {
    let main = format!(
        "{TAGS}
data Holder:
    ctx: $.Context[$ Tag]

fn run(job: fn() -> string $ Tag) -> string $ Tag:
    job()

pub fn main() -> void $ Console:
    holder := Holder {{ ctx: $.context(Tag=Named {{ label: \"held\" }}) }}
    all := [holder.ctx, $.context(Tag=Named {{ label: \"listed\" }})]
    for ctx in all:
        $.with(ctx...):
            println(run(current))
    $.with(holder.ctx...):
        println(run(fn(): $.use(Tag).name() + \"!\"))
"
    );
    assert_eq!(output_of("flow", &main), "held\nlisted\nheld!\n");
}

/// Installed providers reach a suspending call, plain or bang, inside the
/// block, and the block resumes with them (`req.with.nearest.suspending`).
#[test]
fn a_spread_context_serves_suspending_calls() {
    let main = format!(
        "{TAGS}
fn load!() -> string $ Tag:
    $.use(Tag).name()

pub fn main!() -> void $ Console:
    ctx := $.context(Tag=Named {{ label: \"async\" }})
    $.with(ctx...):
        let pending: mut Suspend[string] = load()
        println(pending!())
        println(load!() + current())
"
    );
    assert_eq!(output_of("suspend", &main), "async\nasyncasync\n");
}

/// A generic callee's context parameter fixes its type argument from the
/// argument's keys, and the instance installs that key
/// (`req.context.row`).
#[test]
fn a_context_argument_fixes_a_generic_key() {
    let main = "
trait Repo[T]:
    fn get(self) -> T

data Ints: pass

impl Repo[i32] for Ints:
    fn get(self) -> i32: 5

data Words: pass

impl Repo[string] for Words:
    fn get(self) -> string: \"five\"

fn fetch[T](ctx: $.Context[$ Repo[T]]) -> T:
    $.with(ctx...):
        $.use(Repo[T]).get()

pub fn main() -> void $ Console:
    println(fetch($.context(Repo[i32]=Ints {})))
    println(fetch($.context(Repo[string]=Words {})))
";
    assert_eq!(output_of("generic", main), "5\nfive\n");
    let wrong = "
trait Repo[T]

trait Log

data Ints: pass

impl Repo[i32] for Ints

impl Log for Ints

fn take[T](ctx: $.Context[$ Repo[T]]) -> void:
    pass

fn run() -> void:
    take($.context(Repo[i32]=Ints {}, Log=Ints {}))
";
    assert_eq!(codes(wrong), vec![Code::TypeMismatch]);
}

/// The empty context installs nothing (`req.context.empty`).
#[test]
fn the_empty_context_installs_nothing() {
    let main = format!(
        "{TAGS}
fn none() -> $.Context[$()]:
    $.context()

pub fn main() -> void $ Console:
    $.with(Tag=Named {{ label: \"outer\" }}):
        $.with(none()...):
            println(current())
"
    );
    assert_eq!(output_of("empty", &main), "outer\n");
}

/// A context's type is its row: a context of another row where one is
/// expected is `type-mismatch`, and duplicate keys merge
/// (`req.context.row`, `req.context.one-per-key`).
#[test]
fn a_context_type_is_its_row() {
    let ok = format!(
        "{TAGS}
fn make() -> $.Context[$ Tag] $ Tag:
    base := $.context(Tag=$.use(Tag))
    $.context(base..., Tag=$.use(Tag))
"
    );
    assert_eq!(codes(&ok), vec![]);
    let wrong = format!(
        "{TAGS}
fn make() -> $.Context[$ Tag + Log] $ Tag:
    $.context(Tag=$.use(Tag))
"
    );
    assert_eq!(codes(&wrong), vec![Code::TypeMismatch]);
}

/// Only a context value spreads, and each binding still has to implement
/// its key (`req.context.spread`, `req.with.type`).
#[test]
fn spreads_take_only_contexts() {
    let spread = format!(
        "{TAGS}
fn run(tag: Named) -> void:
    $.with(tag...):
        pass
"
    );
    assert_eq!(codes(&spread), vec![Code::TypeMismatch]);
    let entry = format!(
        "{TAGS}
fn run() -> void:
    _ := $.context(Tag=42)
"
    );
    assert_eq!(codes(&entry), vec![Code::TypeMismatch]);
}

/// A key the context does not provide is still missing in the block
/// (`req.mut.row.missing-key`). `run` is public, so its row is the
/// empty one it writes, not one inferred from its body.
#[test]
fn a_key_the_context_lacks_is_missing() {
    let main = format!(
        "{TAGS}
pub fn run(ctx: $.Context[$ Tag]) -> string:
    $.with(ctx...):
        both()
"
    );
    assert_eq!(codes(&main), vec![Code::MissingRequirement]);
}

/// A mutable requirement trait needs a `mut` value in `$.context` too
/// (`req.mut.install-mutable`).
#[test]
fn a_readonly_value_for_a_mutable_key_is_an_upgrade() {
    let main = "
trait Counter:
    fn bump(mut self) -> void

data Memory:
    value: i32

impl Counter for Memory:
    fn bump(mut self) -> void:
        self.value = self.value + 1

fn counters() -> $.Context[$ Counter]:
    counter := Memory { value: 0 }
    $.context(Counter=counter)
";
    assert_eq!(codes(main), vec![Code::MutableUpgrade]);
}

/// A context entry already in error reports nothing more: a spread of a
/// parameter whose context type is in error covers its block's keys,
/// and the parameter's reads infer nothing (`req.row.param.context`).
#[test]
fn a_context_in_error_is_reported_once() {
    let main = "
fn run_job[$R](providers: $.Context[$ R], job: fn() -> void $ R) -> void:
    $.with(providers...):
        job()
    _ := $.context(providers...)
";
    assert_eq!(codes(main), vec![Code::RowParameterInContext]);
}

/// One item path with one body under two rows takes two code entries: a
/// body's code key covers its own signature, whose row keys are its
/// provider parameters (codegen.md §12.4). A program whose `read` needs
/// `Tag` must not lend its code to one whose `read` also needs `Log`.
#[test]
fn one_body_under_two_rows_shares_no_code() {
    let store = MemoryStore::default();
    let first = format!(
        "{TAGS}
fn read() -> i32 $ Tag: 42

fn call(job: fn() -> i32 $ Tag) -> i32 $ Tag:
    job()

pub fn main() -> void $ Console:
    $.with($.context(Tag=Named {{ label: \"a\" }})...):
        println(call(read))
"
    );
    assert_eq!(output_in(&store, "row-first", &first), "42\n");
    let second = format!(
        "{TAGS}
fn read() -> i32 $ Tag + Log: 42

fn call(job: fn() -> i32 $ Tag + Log) -> i32 $ Tag + Log:
    job()

pub fn main() -> void $ Console:
    $.with(Tag=Named {{ label: \"a\" }}, Log=Named {{ label: \"b\" }}):
        println(call(read))
"
    );
    assert_eq!(output_in(&store, "row-second", &second), "42\n");
}

const REPO: &str = "
trait Repo[T]

data User: pass
data Post: pass
";

/// A spread's keys collide with a later entry whose key can become the
/// same key (`req.with.collision.context`, `req.context.spread`).
#[test]
fn a_spread_key_collides_with_a_generic_entry() {
    let collide = format!(
        "{REPO}
fn choose[T, U](first: dyn Repo[T], second: dyn Repo[U]) -> void:
    base := $.context(Repo[T]=first)
    _ := $.context(base..., Repo[U]=second)
"
    );
    assert_eq!(codes(&collide), vec![Code::GenericRequirementKeyCollision]);
    let with = format!(
        "{REPO}
fn choose[T, U](first: dyn Repo[T], second: dyn Repo[U]) -> void:
    base := $.context(Repo[T]=first)
    $.with(base..., Repo[U]=second):
        pass
"
    );
    assert_eq!(codes(&with), vec![Code::GenericRequirementKeyCollision]);
}

/// A `$.with` spread collides with a key of the enclosing block, and a
/// repeated key of one expression stays the ordinary override
/// (`req.with.collision.compared`, `req.with.collision.exact`).
#[test]
fn a_spread_key_collides_with_an_enclosing_block() {
    let nested = format!(
        "{REPO}
fn choose[T, U](first: dyn Repo[T], second: dyn Repo[U]) -> void:
    $.with(Repo[T]=first):
        ctx := $.context(Repo[U]=second)
        $.with(ctx...):
            pass
"
    );
    assert_eq!(codes(&nested), vec![Code::GenericRequirementKeyCollision]);
    let exact = format!(
        "{REPO}
fn choose[T](first: dyn Repo[T], second: dyn Repo[T]) -> void:
    base := $.context(Repo[T]=first)
    $.with(base..., Repo[T]=second):
        pass
"
    );
    assert_eq!(codes(&exact), vec![]);
}

/// Distinct concrete keys stay valid (`req.with.collision.concrete`).
#[test]
fn concrete_spread_keys_do_not_collide() {
    let ok = format!(
        "{REPO}
fn choose(users: dyn Repo[User], posts: dyn Repo[Post]) -> void:
    base := $.context(Repo[User]=users)
    $.with(base..., Repo[Post]=posts):
        pass
"
    );
    assert_eq!(codes(&ok), vec![]);
}
