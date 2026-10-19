//! A `mut` name in a `let` data pattern (spec/lang/02-grammar.md,
//! grammar.stmt.let-pattern.mut; spec/lang/04-type-system.md,
//! types.bind.let-pattern-mut.data-field): `mut` precedes the name of a field,
//! and the field is bound as a writable copy, in plain `let`, in
//! `let ... else`, and under a variant pattern. `match` takes no `mut`
//! (grammar.stmt.let-mut.only-let). Programs build through the driver and
//! run on V8 (`host/run.mjs`), as the conformance runner does.

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
        &format!("let-pattern-mut-{name}.wasm"),
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

/// The diagnostic codes of a program that must not build.
fn codes_of(main: &str) -> Vec<Code> {
    let out = built(main, &Goal::Analyze);
    out.diags
        .content_order()
        .into_iter()
        .map(|i| out.diags.code[i])
        .collect()
}

const TALLY: &str = "\
struct Tally:
    tags: List[string]
    name: string
    count: i32
";

fn with_tally(rest: &str) -> String {
    format!("{TALLY}\n{rest}")
}

#[test]
fn a_mut_field_binds_with_mutable_access() {
    let main = with_tally(
        "\
pub fn main() -> void $ Console:
    t := Tally { tags: [\"a\"], name: \"box\", count: +2 }
    let Tally { mut tags, name } = t
    tags.push(name)
    tags = tags
    println(tags.len())
    println(t.tags.len())
",
    );
    // The binding is a copy of the list (`types.coll.value`): the push
    // changes `tags` and leaves `t.tags` as it was.
    assert_eq!(output_of("mut-field", &main), "2\n1\n");
}

#[test]
fn a_pattern_mixes_mut_and_plain_fields() {
    let main = with_tally(
        "\
pub fn main() -> void $ Console:
    t := Tally { tags: [\"a\"], name: \"box\", count: +2 }
    let Tally { name, mut tags, count: n } = t
    tags.push(name)
    println(\"${name} ${tags.len()} ${n}\")
",
    );
    assert_eq!(output_of("mixed", &main), "box 2 2\n");
}

#[test]
fn a_plain_field_in_the_same_pattern_stays_readonly() {
    let main = with_tally(
        "\
fn edit(t: Tally) -> void:
    let Tally { tags, name } = t
    tags.push(name)
",
    );
    assert_eq!(codes_of(&main), vec![Code::MutableReceiverRequired]);
}

#[test]
fn let_else_binds_a_mut_field_under_a_variant() {
    let main = with_tally(
        "\
fn pick(t: Tally, ok: bool) -> Tally?:
    if ok:
        .Some(t)
    else:
        .None

fn total(t: Tally, ok: bool) -> usize:
    let .Some(Tally { mut tags, name }) = pick(t, ok) else: return 0
    tags.push(name)
    tags.len()

pub fn main() -> void $ Console:
    t := Tally { tags: [\"a\"], name: \"box\", count: +2 }
    println(total(t, false))
    println(total(t, true))
    println(t.tags.len())
",
    );
    // `mut tags` binds a copy of the list (`types.coll.value`), so `t.tags`
    // keeps its one element.
    assert_eq!(output_of("let-else", &main), "0\n2\n1\n");
}

#[test]
fn a_data_pattern_in_a_match_arm_binds_plain_fields() {
    let main = with_tally(
        "\
fn size(t: Tally) -> usize:
    match t:
        Tally { name, tags } => name.len() + tags.len()

pub fn main() -> void $ Console:
    println(size(Tally { tags: [\"a\"], name: \"box\", count: +2 }))
",
    );
    assert_eq!(output_of("match", &main), "4\n");
}

#[test]
fn a_match_arm_takes_no_mut_field() {
    let main = with_tally(
        "\
fn size(t: Tally) -> usize:
    match t:
        Tally { mut tags, name } => tags.len()
",
    );
    assert_eq!(codes_of(&main), vec![Code::SyntaxError]);
}

#[test]
fn a_mut_list_field_of_a_readonly_subject_binds_a_copy() {
    // A list is a value: `mut tags` binds a writable copy, which grants no
    // write to the subject (`types.coll.let-mut`).
    let main = with_tally(
        "\
fn edit(t: Tally) -> void $ Console:
    let Tally { mut tags } = t
    tags.push(\"x\")
    println(tags.len())
",
    );
    assert_eq!(codes_of(&main), Vec::<Code>::new());
}

#[test]
fn an_unknown_field_still_reports_after_mut() {
    let main = with_tally(
        "\
fn edit(t: Tally) -> void:
    let Tally { mut missing } = t
    pass
",
    );
    assert_eq!(codes_of(&main), vec![Code::UnknownField]);
}

#[test]
fn a_field_listed_twice_reports_a_duplicate_through_mut() {
    let main = with_tally(
        "\
fn edit(t: Tally) -> void:
    let Tally { mut tags, tags: _ } = t
    println(tags.len())
",
    );
    assert_eq!(codes_of(&main), vec![Code::DuplicateDataPatternField]);
}
