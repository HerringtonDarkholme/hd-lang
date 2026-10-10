//! Generic code instantiated at a trait-value type (trait.dyn.bound,
//! trait.dyn.bound.dispatch, codegen.md §13.2 step 4): a trait value
//! satisfies a bound on its own trait and on each supertrait, and a call
//! through the bound at `T = dyn Tr` dispatches through the value's
//! vtable, as a direct `dyn` call does. The checker half checks the
//! programs with `Goal::Analyze`; the runtime half builds them through the
//! driver and runs them on V8 (`host/run.mjs`), as the conformance runner
//! does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_diag::Code;
use hd_driver::{Executor, Goal, Host, NoClock, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn built(main: &str, goal: &Goal) -> Output {
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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("dyn-bound-{name}.wasm"));
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

/// A notification channel and a generic sender over it, shared by the
/// cases below.
const CHANNELS: &str = "\
trait Channel < Display:
    fn send(self, message: string) -> string
    fn urgent(self, message: string) -> string: self.send(message.upper())

data Email:
    address: string

impl Display for Email:
    fn to_string(self) -> string: \"email:${self.address}\"

impl Channel for Email:
    fn send(self, message: string) -> string: \"to ${self.address}: ${message}\"

data Pager:
    number: i32

impl Display for Pager:
    fn to_string(self) -> string: \"pager:${self.number}\"

impl Channel for Pager:
    fn send(self, message: string) -> string: \"page ${self.number}: ${message}\"

fn notify[C < Channel](channel: C, message: string) -> string:
    channel.send(message)

fn escalate[C < Channel](channel: C, message: string) -> string:
    channel.urgent(message)

fn label[T < Display](value: T) -> string:
    \"[${value.to_string()}]\"

fn route[C < Channel](channel: C, message: string) -> string:
    label(channel) + \" \" + notify(channel, message)

fn broadcast[C < Channel](channels: List[C], message: string) -> List[string]:
    channels.map(fn(channel: C) -> string: notify(channel, message))

fn targets[C < Channel](channels: List[C]) -> List[string]:
    channels.map(C::to_string)
";

#[test]
fn the_checker_accepts_trait_values_for_own_and_supertrait_bounds() {
    let main = format!(
        "{CHANNELS}
pub fn main() -> void:
    let channel: dyn Channel = Email {{ address: \"ops@example.com\" }}
    _ := notify(channel, \"disk full\")
    _ := label(channel)
    let shown: dyn Display = Pager {{ number: 7 }}
    _ := label(shown)
    let all: List[dyn Channel] = [Email {{ address: \"a@example.com\" }}, Pager {{ number: 1 }}]
    _ := broadcast(all, \"up\")
"
    );
    assert_eq!(codes_of(&main), Vec::<Code>::new());
}

#[test]
fn the_checker_rejects_a_trait_value_for_a_bound_its_trait_does_not_extend() {
    let main = format!(
        "{CHANNELS}
pub fn main() -> void:
    let shown: dyn Display = Pager {{ number: 7 }}
    _ := notify(shown, \"disk full\")
"
    );
    assert_eq!(codes_of(&main), vec![Code::UnsatisfiedTraitBound]);
}

#[test]
fn a_bound_call_at_a_trait_value_dispatches_through_its_vtable() {
    let main = format!(
        "{CHANNELS}
pub fn main() -> void $ Console:
    let email: dyn Channel = Email {{ address: \"ops@example.com\" }}
    let pager: dyn Channel = Pager {{ number: 7 }}
    println(notify(email, \"disk full\"))
    println(notify(pager, \"disk full\"))
    println(escalate(pager, \"down\"))
"
    );
    assert_eq!(
        output_of("own-bound", &main),
        "to ops@example.com: disk full\npage 7: disk full\npage 7: DOWN\n"
    );
}

#[test]
fn a_supertrait_bound_at_a_trait_value_reads_the_parent_vtable() {
    let main = format!(
        "{CHANNELS}
pub fn main() -> void $ Console:
    let email: dyn Channel = Email {{ address: \"ops@example.com\" }}
    println(label(email))
    println(route(email, \"hi\"))
"
    );
    assert_eq!(
        output_of("supertrait-bound", &main),
        "[email:ops@example.com]\n[email:ops@example.com] to ops@example.com: hi\n"
    );
}

#[test]
fn a_generic_over_a_list_of_trait_values_dispatches_per_element() {
    let main = format!(
        "{CHANNELS}
pub fn main() -> void $ Console:
    let all: List[dyn Channel] = [Email {{ address: \"a@example.com\" }}, Pager {{ number: 1 }}]
    for line in broadcast(all, \"up\"):
        println(line)
    for target in targets(all):
        println(target)
"
    );
    assert_eq!(
        output_of("list", &main),
        "to a@example.com: up\npage 1: up\nemail:a@example.com\npager:1\n"
    );
}

#[test]
fn a_member_reference_at_a_trait_value_calls_the_vtable_slot() {
    let main = format!(
        "{CHANNELS}
fn sender[C < Channel]() -> fn(C, string) -> string:
    C::send

pub fn main() -> void $ Console:
    let pager: dyn Channel = Pager {{ number: 7 }}
    let send = sender::[dyn Channel]()
    println(send(pager, \"ping\"))
"
    );
    assert_eq!(output_of("reference", &main), "page 7: ping\n");
}

#[test]
fn a_mut_trait_value_satisfies_a_mut_bound() {
    let main = "\
trait Counter:
    fn bump(mut self) -> void
    fn count(self) -> i32

data Visits:
    total: i32

impl Counter for Visits:
    fn bump(mut self) -> void:
        self.total = self.total + 1
    fn count(self) -> i32: self.total

fn bump_twice[C < mut Counter](counter: C) -> i32:
    counter.bump()
    counter.bump()
    counter.count()

pub fn main() -> void $ Console:
    let visits: mut Visits = Visits { total: 1 }
    let counter: mut dyn Counter = visits
    println(bump_twice(counter))
    println(visits.total)
";
    assert_eq!(output_of("mut-bound", main), "3\n3\n");
}
