//! Provider keys carry their type arguments (`req.row.entail.generic`,
//! `req.key.binding.identity`, `crates/hd_mono/DESIGN.md` §13.18): `Repo[User]` and
//! `Repo[Post]` are two keys with two providers, through a direct call, a
//! generic instance, a function value, a data field and a suspension.
//! Programs build through the driver and run on V8 (`host/run.ts`), as
//! the conformance runner does.

use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str) -> Output {
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
    build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    )
}

/// One generic trait with two implementations at different arguments:
/// `Repo[User]` counts 20 and `Repo[Post]` counts 1, so a function that
/// reads both prints 201 only when each key reaches its own provider.
const PRELUDE: &str = "\
use std.time.{Clock, Duration, sleep}

trait Repo[T]:
    fn count(self) -> i32

struct User: pass
struct Post: pass
struct UserRepo: pass
struct PostRepo: pass

impl Repo[User] for UserRepo:
    fn count(self) -> i32: 20

impl Repo[Post] for PostRepo:
    fn count(self) -> i32: 1

fn activity() -> i32 $ Repo[User] + Repo[Post]:
    $.use(Repo[User]).count() * 10 + $.use(Repo[Post]).count()
";

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

/// Each key of a multi-key `$.use` must be in the row, and the tuple has
/// one element per key.
#[test]
fn a_multi_key_use_checks_each_key_and_its_arity() {
    let missing = format!(
        "{PRELUDE}
fn posts() -> i32 $ Repo[User]:
    let (users, posts) = $.use(Repo[User], Repo[Post])
    users.count() + posts.count()

pub fn main() -> void $ Console:
    $.with(Repo[User]=UserRepo {{}}, Repo[Post]=PostRepo {{}}):
        println(posts())
"
    );
    let text = rejected(&missing);
    assert!(
        text.contains("missing-requirement") && text.contains("`$ Repo[Post]`"),
        "{text}"
    );
    let arity = format!(
        "{PRELUDE}
fn three() -> i32 $ Repo[User] + Repo[Post]:
    let (users, posts, _extra) = $.use(Repo[User], Repo[Post])
    users.count() + posts.count()

pub fn main() -> void $ Console:
    $.with(Repo[User]=UserRepo {{}}, Repo[Post]=PostRepo {{}}):
        println(three())
"
    );
    let text = rejected(&arity);
    // A `let` tuple pattern against a tuple of another size is a type
    // mismatch (`flow.let.tuple-arity`), not a pattern arity.
    assert!(text.contains("type-mismatch"), "{text}");
}
