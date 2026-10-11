//! The copy assignment `place ...= value` (spec/lang/02-grammar.md,
//! grammar.stmt.copy-assign, grammar.stmt.assign-place; spec/lang/
//! 08-data-and-enums.md, data.part.construct, data.part.marker-required;
//! spec/lang/04-type-system.md, types.path.store-embedded): it stores the
//! copy-update `T { ...value }` into an embedded field of a `mut` value and
//! into no other place. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

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
fn output_of(name: &str, main: &str) -> String {
    let out = built(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("copy-assign-{name}.wasm"));
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
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const PARTS: &str = "\
data Timestamps:
    pub created: i32
    pub updated: i32

data Post:
    Timestamps
    id: string
";

#[test]
fn a_copy_assignment_stores_a_copy_of_the_part() {
    // data.part.construct
    let main = format!(
        "{PARTS}
pub fn main() -> void $ Console:
    let mut post = Post {{ Timestamps: ...Timestamps {{ created: 1, updated: 1 }}, id: \"p\" }}
    let mut stamps = Timestamps {{ created: 2, updated: 3 }}
    post.Timestamps ...= stamps
    println(\"${{post.created}} ${{post.updated}}\")
    stamps.updated = 9
    println(\"${{post.updated}} ${{stamps.updated}}\")
    post.Timestamps ...= Timestamps {{ created: 7, updated: 8 }}
    println(\"${{post.created}} ${{post.updated}}\")
"
    );
    assert_eq!(output_of("store", &main), "2 3\n3 9\n7 8\n");
}

#[test]
fn a_copy_assignment_copies_the_parts_of_the_part() {
    // data.part.copy-update
    let main = "\
data Clock:
    pub at: i32

data Stamps:
    Clock
    pub who: string

data Post:
    Stamps
    id: string

pub fn main() -> void $ Console:
    let mut post = Post { Stamps: ...Stamps { Clock: ...Clock { at: 1 }, who: \"a\" }, id: \"p\" }
    let mut other = Stamps { Clock: ...Clock { at: 5 }, who: \"b\" }
    post.Stamps ...= other
    other.Clock ...= Clock { at: 6 }
    println(\"${post.at} ${post.who} ${other.at}\")
";
    assert_eq!(output_of("nested", main), "5 b 6\n");
}

#[test]
fn a_copy_assignment_stores_through_a_promoted_part() {
    // names.promoted.path: the part of a part is promoted to the outer value.
    let main = "\
data Clock:
    pub at: i32

data Stamps:
    Clock
    pub who: string

data Post:
    Stamps
    id: string

pub fn main() -> void $ Console:
    let mut post = Post { Stamps: ...Stamps { Clock: ...Clock { at: 1 }, who: \"a\" }, id: \"p\" }
    post.Clock ...= Clock { at: 4 }
    println(\"${post.at} ${post.who}\")
";
    assert_eq!(output_of("promoted", main), "4 a\n");
}

#[test]
fn a_copy_assignment_to_a_readonly_value_is_a_readonly_root() {
    // types.path.store.readonly-root
    let main = format!(
        "{PARTS}
fn invalid(post: Post, stamps: Timestamps) -> void:
    post.Timestamps ...= stamps
"
    );
    assert_eq!(codes_of(&main), vec![Code::ReadonlyRoot]);
}

#[test]
fn a_readonly_copy_cannot_fill_a_mut_part() {
    // types.path.store-embedded
    let main = "\
data Inner:
    items: mut List[i32]

data Outer:
    Inner
    id: string

fn invalid(outer: mut Outer, inner: Inner) -> void:
    outer.Inner ...= inner
";
    assert_eq!(codes_of(main), vec![Code::MutableUpgrade]);
}

#[test]
fn a_copy_assignment_to_an_ordinary_field_is_an_error() {
    // data.part.marker-required
    let main = "\
data User:
    name: string

data Account:
    owner: User

fn invalid(account: mut Account, user: User) -> void:
    account.owner ...= user
";
    assert_eq!(codes_of(main), vec![Code::CopyIntoOrdinaryField]);
}

#[test]
fn a_copy_assignment_to_a_local_or_an_index_is_an_error() {
    // grammar.stmt.copy-assign
    let local = "\
data User:
    name: string

fn invalid(user: User) -> string:
    let mut mine = User { name: \"a\" }
    mine ...= user
    mine.name
";
    assert_eq!(codes_of(local), vec![Code::CopyIntoOrdinaryField]);
    let index = "\
data User:
    name: string

fn invalid(user: User) -> string:
    let mut all = [User { name: \"a\" }]
    all[0] ...= user
    all[0].name
";
    assert_eq!(codes_of(index), vec![Code::CopyIntoOrdinaryField]);
}

#[test]
fn a_plain_assignment_to_an_embedded_field_asks_for_the_copy_marker() {
    // data.part.marker-required
    let main = format!(
        "{PARTS}
fn invalid(post: mut Post, stamps: Timestamps) -> void:
    post.Timestamps = stamps
"
    );
    assert_eq!(codes_of(&main), vec![Code::EmbeddedCopyRequired]);
}

#[test]
fn an_assignment_to_an_expression_that_is_no_place_is_an_error() {
    // grammar.stmt.assign-place
    for target in ["(1 + 2) = 3", "(1 + 2) ...= 3", "(1 + 2) += 3"] {
        let main = format!("fn invalid() -> void:\n    {target}\n");
        assert_eq!(
            codes_of(&main),
            vec![Code::InvalidAssignmentTarget],
            "{target}"
        );
    }
}
