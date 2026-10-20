//! Assigning an embedded field stores a copy of the value
//! (spec/lang/08-data-and-enums.md, data.struct.embed.plain), and an
//! assignment to an expression that is no place stays an error
//! (spec/lang/02-grammar.md, grammar.stmt.assign-place).

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
fn output_of(name: &str, main: &str) -> String {
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
        &format!("copy-assign-{name}.wasm"),
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
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const PARTS: &str = "\
struct Timestamps:
    pub created: i32
    pub updated: i32

struct Post:
    Timestamps
    id: string
";

#[test]
fn a_copy_assignment_stores_a_copy_of_the_part() {
    // data.struct.embed.plain
    let main = format!(
        "{PARTS}
pub fn main() -> void $ Console:
    let mut post = Post {{ Timestamps: Timestamps {{ created: 1, updated: 1 }}, id: \"p\" }}
    let mut stamps = Timestamps {{ created: 2, updated: 3 }}
    post.Timestamps = stamps
    println(\"${{post.created}} ${{post.updated}}\")
    stamps.updated = 9
    println(\"${{post.updated}} ${{stamps.updated}}\")
    post.Timestamps = Timestamps {{ created: 7, updated: 8 }}
    println(\"${{post.created}} ${{post.updated}}\")
"
    );
    assert_eq!(output_of("store", &main), "2 3\n3 9\n7 8\n");
}

#[test]
fn a_copy_assignment_copies_the_parts_of_the_part() {
    // data.struct.embed.plain, types.value.stored
    let main = "\
struct Clock:
    pub at: i32

struct Stamps:
    Clock
    pub who: string

struct Post:
    Stamps
    id: string

pub fn main() -> void $ Console:
    let mut post = Post { Stamps: Stamps { Clock: Clock { at: 1 }, who: \"a\" }, id: \"p\" }
    let mut other = Stamps { Clock: Clock { at: 5 }, who: \"b\" }
    post.Stamps = other
    other.Clock = Clock { at: 6 }
    println(\"${post.at} ${post.who} ${other.at}\")
";
    assert_eq!(output_of("nested", main), "5 b 6\n");
}

#[test]
fn a_copy_assignment_stores_through_a_promoted_part() {
    // names.promoted.path: the part of a part is promoted to the outer value.
    let main = "\
struct Clock:
    pub at: i32

struct Stamps:
    Clock
    pub who: string

struct Post:
    Stamps
    id: string

pub fn main() -> void $ Console:
    let mut post = Post { Stamps: Stamps { Clock: Clock { at: 1 }, who: \"a\" }, id: \"p\" }
    post.Clock = Clock { at: 4 }
    println(\"${post.at} ${post.who}\")
";
    assert_eq!(output_of("promoted", main), "4 a\n");
}

#[test]
fn a_copy_assignment_to_a_readonly_value_is_a_readonly_root() {
    // types.path.store.not-writable
    let main = format!(
        "{PARTS}
fn invalid(post: &Post, stamps: Timestamps) -> void:
    post.Timestamps = stamps
"
    );
    assert_eq!(codes_of(&main), vec![Code::NotWritable]);
}

#[test]
fn an_assignment_to_an_expression_that_is_no_place_is_an_error() {
    // grammar.stmt.assign-place
    for target in ["(1 + 2) = 3", "(1 + 2) += 3"] {
        let main = format!("fn invalid() -> void:\n    {target}\n");
        assert_eq!(
            codes_of(&main),
            vec![Code::InvalidAssignmentTarget],
            "{target}"
        );
    }
}
