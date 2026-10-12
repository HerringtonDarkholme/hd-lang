//! A derived `Default` owes `Default` only to the members that declare no
//! default (`std-ops.default.derive.member-bound.declared`, #284): its
//! `build` fills a declared member from its default as an omitted member
//! is (`annot.omit.default`), so that member's type needs no `Default`. The
//! run test builds through the driver and runs on V8 (`host/run.mjs`);
//! the others check the opt-in.

use std::path::Path;
use std::process::Command;

use hd_testkit::Scratch;

use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn program(main: &str, goal: &Goal) -> Output {
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

/// The standard output of a `main` program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
    let out = program(
        main,
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = Scratch::unmade(
        env!("CARGO_TARGET_TMPDIR"),
        &format!("derived-default-declared-{name}.wasm"),
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

/// The `member-not-derivable` messages of a program that only analyzes.
fn not_derivable(main: &str) -> Vec<String> {
    let out = program(main, &Goal::Analyze);
    let found: Vec<usize> = out
        .diags
        .content_order()
        .into_iter()
        .filter(|&i| out.diags.code[i] == Code::MemberNotDerivable)
        .collect();
    let others = out
        .diags
        .content_order()
        .into_iter()
        .filter(|&i| {
            !matches!(
                out.diags.code[i],
                Code::MemberNotDerivable | Code::Unsupported
            )
        })
        .count();
    assert_eq!(others, 0, "{}", out.render());
    found
        .into_iter()
        .map(|i| out.diags.get_text(out.diags.message[i]).to_owned())
        .collect()
}

#[test]
fn a_declared_default_needs_no_default_on_its_type() {
    let main = "\
use std.ops.Default

data Secret:
    value: i32

@derive(Default)
data Vault:
    secret: Secret = Secret { value: 7 }
    count: i32 = +5
    plain: i32
    name: string

pub fn main() -> void $ Console:
    v := Vault::default()
    println(v.secret.value)
    println(v.count)
    println(v.plain)
    println(v.name.len())
";
    assert_eq!(output_of("declared", main), "7\n5\n0\n0\n");
}

#[test]
fn the_declared_default_runs_once_per_default() {
    let main = "\
use std.ops.Default

data Secret:
    value: i32

fn seven() -> i32:
    7

@derive(Default)
data Vault:
    secret: Secret = Secret { value: seven() }

pub fn main() -> void $ Console:
    println(Vault::default().secret.value)
    println(Vault::default().secret.value + 1)
";
    assert_eq!(output_of("expression", main), "7\n8\n");
}

#[test]
fn a_member_without_a_default_still_needs_one() {
    let main = "\
use std.ops.Default

data Secret:
    value: i32

@derive(Default)
data Vault:
    secret: Secret
    other: Secret = Secret { value: 7 }
";
    let msgs = not_derivable(main);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("secret"), "{msgs:?}");
    assert!(!msgs[0].contains("other"), "{msgs:?}");
}

#[test]
fn an_enum_payload_member_still_needs_default() {
    let main = "\
use std.ops.{Default, default}

data Secret:
    value: i32

@derive(Default)
enum Choice:
    Plain
    @default
    Held(secret: Secret)
";
    let msgs = not_derivable(main);
    assert_eq!(msgs.len(), 1, "{msgs:?}");
    assert!(msgs[0].contains("secret"), "{msgs:?}");
}

#[test]
fn an_enum_default_variant_with_default_members_still_builds() {
    let main = "\
use std.ops.{Default, default}

@derive(Default)
enum Choice:
    Plain
    @default
    Held(count: i32, name: string)

pub fn main() -> void $ Console:
    match Choice::default():
        .Held(count, name) => println(\"$count ${name.len()}\")
        .Plain => println(\"plain\")
";
    assert_eq!(output_of("enum", main), "0 0\n");
}
