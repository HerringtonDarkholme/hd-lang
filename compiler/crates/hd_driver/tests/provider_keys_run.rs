//! Provider keys carry their type arguments (`req.row.entail.generic`,
//! `req.key.binding.identity`, codegen.md §13.18): `Repo[User]` and
//! `Repo[Post]` are two keys with two providers, through a direct call, a
//! generic instance, a function value, a data field and a suspension.
//! Programs build through the driver and run on V8 (`host/run.mjs`), as
//! the conformance runner does.

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
    let path =
        PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("provider-keys-{name}.wasm"));
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

/// One generic trait with two implementations at different arguments:
/// `Repo[User]` counts 20 and `Repo[Post]` counts 1, so a function that
/// reads both prints 201 only when each key reaches its own provider.
const PRELUDE: &str = "\
use std.time.{Clock, Duration, sleep}

trait Repo[T]:
    fn count(self) -> i32

data User: pass
data Post: pass
data UserRepo: pass
data PostRepo: pass

impl Repo[User] for UserRepo:
    fn count(self) -> i32: 20

impl Repo[Post] for PostRepo:
    fn count(self) -> i32: 1

fn activity() -> i32 $ Repo[User] + Repo[Post]:
    $.use(Repo[User]).count() * 10 + $.use(Repo[Post]).count()
";

/// `main!` runs `body` with both providers bound.
fn run(name: &str, items: &str, body: &str) -> String {
    let mut main = format!(
        "{PRELUDE}\n{items}\n\
pub fn main!() -> void $ Console + Clock:
    $.with(Repo[User]=UserRepo {{}}, Repo[Post]=PostRepo {{}}):
"
    );
    for line in body.lines() {
        main.push_str("        ");
        main.push_str(line);
        main.push('\n');
    }
    output_of(name, &main)
}

#[test]
fn a_direct_call_reads_each_key_from_its_own_provider() {
    let body = "\
println(activity())
";
    assert_eq!(run("direct", "", body), "201\n");
}

#[test]
fn nested_scopes_bind_the_two_keys_separately() {
    let main = format!(
        "{PRELUDE}
pub fn main() -> void $ Console:
    $.with(Repo[User]=UserRepo {{}}):
        $.with(Repo[Post]=PostRepo {{}}):
            println(activity())
"
    );
    assert_eq!(output_of("nested", &main), "201\n");
}

#[test]
fn a_generic_function_has_one_instance_per_key() {
    let items = "\
fn count_of[T]() -> i32 $ Repo[T]:
    $.use(Repo[T]).count()

fn both[A, B]() -> i32 $ Repo[A] + Repo[B]:
    count_of::[A]() * 10 + count_of::[B]()
";
    let body = "\
println(count_of::[User]() * 10 + count_of::[Post]())
println(both::[User, Post]())
println(both::[Post, User]())
";
    assert_eq!(run("generic", items, body), "201\n201\n30\n");
}

#[test]
fn a_function_value_passes_both_providers() {
    let items = "\
fn call(callback: fn() -> i32 $ Repo[User] + Repo[Post]) -> i32 $ Repo[User] + Repo[Post]:
    callback()

fn count_of[T]() -> i32 $ Repo[T]:
    $.use(Repo[T]).count()

fn apply[T](callback: fn() -> i32 $ Repo[T]) -> i32 $ Repo[T]:
    callback()
";
    let body = "\
println(call(activity))
println(call(fn(): $.use(Repo[User]).count() * 10 + $.use(Repo[Post]).count()))
println(apply(count_of::[User]) * 10 + apply(count_of::[Post]))
";
    assert_eq!(run("value", items, body), "201\n201\n201\n");
}

#[test]
fn a_data_field_keeps_the_callable_and_its_keys() {
    let items = "\
data Job:
    callback: fn() -> i32 $ Repo[Post] + Repo[User]
";
    let body = "\
job := Job { callback: activity }
println((job.callback)())
";
    assert_eq!(run("field", items, body), "201\n");
}

#[test]
fn a_suspension_binds_both_providers_at_construction() {
    let items = "\
fn later!() -> i32 $ Repo[User] + Repo[Post] + Clock:
    first := $.use(Repo[User]).count()
    sleep!(Duration::milliseconds(1))
    first * 10 + $.use(Repo[Post]).count()
";
    let body = "\
let pending: mut Suspend[i32] = later()
println(pending!())
tally := fn!() -> i32 $ Repo[User] + Repo[Post] + Clock:
    first := $.use(Repo[Post]).count()
    sleep!(Duration::milliseconds(1))
    $.use(Repo[User]).count() * 10 + first
println(tally!())
";
    assert_eq!(run("suspension", items, body), "201\n201\n");
}

/// The rendered diagnostics of a program that must not build.
fn rejected(main: &str) -> String {
    let out = built(main);
    assert!(out.wasm.is_none(), "built: {main}");
    out.render()
}

#[test]
fn a_missing_key_is_named_with_its_arguments() {
    let call = format!(
        "{PRELUDE}
pub fn main() -> void $ Console:
    $.with(Repo[User]=UserRepo {{}}):
        println(activity())
"
    );
    let text = rejected(&call);
    assert!(
        text.contains("missing-requirement") && text.contains("`$ Repo[Post]`"),
        "{text}"
    );
    let used = format!(
        "{PRELUDE}
fn posts() -> i32 $ Repo[User]:
    $.use(Repo[Post]).count()

pub fn main() -> void $ Console:
    $.with(Repo[User]=UserRepo {{}}):
        println(posts())
"
    );
    let text = rejected(&used);
    assert!(
        text.contains("missing-requirement") && text.contains("`$ Repo[Post]`"),
        "{text}"
    );
}
