//! A closed program's immediate host drives do not root unused cold slots.

use hd_driver::{BuildProfile, EngineTarget, Executor, Goal, Host, NoClock, OptLevel, Packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

fn linked_slots(
    source: &str,
    target: EngineTarget,
    profile: BuildProfile,
    level: OptLevel,
) -> Vec<(String, bool)> {
    let mut sources = MemorySources::default();
    sources.insert("main.hd", source);
    let store = crate::warm_store();
    let out = hd_driver::build_packages(
        &Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        },
        "app",
        &Packages {
            target,
            build_profile: profile,
            level,
            ..Packages::default()
        },
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(!out.diags.has_errors(), "{}", out.render());
    let wasm = out.wasm.expect("the program links");
    wasmparser::Validator::new_with_features(wasmparser::WasmFeatures::all())
        .validate_all(&wasm)
        .expect("valid functions and element slots");
    let mut names = Vec::new();
    let mut table = std::collections::BTreeSet::new();
    for payload in wasmparser::Parser::new(0).parse_all(&wasm) {
        match payload.expect("payload") {
            wasmparser::Payload::CustomSection(section) if section.name() == "name" => {
                let map = wasmparser::NameSectionReader::new(wasmparser::BinaryReader::new(
                    section.data(),
                    section.data_offset(),
                ));
                for subsection in map {
                    if let wasmparser::Name::Function(map) = subsection.expect("name subsection") {
                        names.extend(map.into_iter().map(|entry| {
                            let entry = entry.expect("name");
                            (entry.index, entry.name.to_owned())
                        }));
                    }
                }
            }
            wasmparser::Payload::CustomSection(section) if section.name() == "hd.names" => {
                let mut reader =
                    wasmparser::BinaryReader::new(section.data(), section.data_offset());
                for _ in 0..reader.read_var_u32().expect("compact name count") {
                    let index = reader.read_var_u32().expect("function index");
                    names.push((
                        index,
                        reader.read_string().expect("function name").to_owned(),
                    ));
                }
                assert!(reader.eof());
            }
            wasmparser::Payload::ElementSection(elements) => {
                for element in elements {
                    match element.expect("element").items {
                        wasmparser::ElementItems::Functions(indices) => {
                            table.extend(
                                indices
                                    .into_iter()
                                    .map(|index| index.expect("table function")),
                            );
                        }
                        wasmparser::ElementItems::Expressions(_, expressions) => {
                            for expression in expressions {
                                let mut ops = expression
                                    .expect("element expression")
                                    .get_operators_reader();
                                if let wasmparser::Operator::RefFunc { function_index } =
                                    ops.read().expect("ref.func")
                                {
                                    table.insert(function_index);
                                }
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
    names
        .into_iter()
        .map(|(index, name)| (name, table.contains(&index)))
        .collect()
}

#[test]
fn only_immediate_blocking_host_calls_omit_the_normal_cold_slot() {
    let immediate = r"
use std.task.block_on
use std.time.{Clock, ms}
pub fn main() -> void $ Clock:
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
";
    let stored = r"
use std.task.block_on
use std.time.{Clock, ms}
fn cold() -> mut Suspend[void] $ Clock:
    let clock: *mut dyn Clock = $.use(Clock)
    clock.sleep(ms(1))
pub fn main() -> void $ Clock:
    _ := block_on(cold())
";
    // An await may need the normal slot in a shielded dispose suite. Retain
    // asynchronous methods conservatively, even beside a blocking drive.
    let asynchronous = r"
use std.task.block_on
use std.time.{Clock, ms}
pub fn main!() -> void $ Clock:
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    clock.sleep!(ms(1))
";
    let callable = r#"
use std.host.Sys
use std.task.block_on
use std.time.{Clock, ms}
fn invoke(change: fn(i32) -> i32, count: i32) -> i32:
    if count == 0: change(+0)
    else: invoke(change, count - 1)
pub fn main() -> void $ Console + Sys + Clock:
    let system: &dyn Sys = $.use(Sys)
    let change: fn(i32) -> i32 = if system.os().expect("os").len() == 0:
        fn(value: i32) -> i32: value + 1
    else:
        fn(value: i32) -> i32: value + 2
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    println("${invoke(change, +1)}")
"#;
    // No trait-method adapter occurs here: the unresolved recursive callback
    // itself must retain the conservative cold-constructor ABI guard.
    let raw_constructor = r#"
use std.host.Sys
use std.task.block_on
use std.time.{Clock, ms}
fn first!() -> i32: 1
fn second!() -> i32: 2
fn invoke(make: fn() -> mut Suspend[i32], count: i32) -> mut Suspend[i32]:
    if count == 0: make()
    else: invoke(make, count - 1)
pub fn main() -> void $ Sys + Clock:
    let system: &dyn Sys = $.use(Sys)
    let make: fn() -> mut Suspend[i32] = if system.os().expect("os").len() == 0:
        first
    else:
        second
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    _ := block_on(invoke(make, +1))
"#;
    let cold_callable = r"
use std.task.block_on
use std.time.{Clock, Duration, ms}
fn invoke(pause: fn(Duration) -> mut Suspend[void], d: Duration, count: i32) -> mut Suspend[void]:
    if count == 0: pause(d) else: invoke(pause, d, count - 1)
pub fn main!() -> void $ Clock:
    let clock: *mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    let pause: fn(Duration) -> mut Suspend[void] = clock::sleep
    let saved: mut Suspend[void] = invoke(pause, ms(1), +1)
    _ := block_on(saved)
    let bang: fn!(Duration) -> void = clock::sleep
    bang!(ms(1))
";
    let shielded = r"
use std.ops.Disposable
use std.task.block_on
use std.time.{Clock, Duration, ms}
struct Pause:
    clock: *mut dyn Clock
impl Disposable for Pause:
    fn dispose!(&mut self) -> void:
        let pause: fn!(Duration) -> void = self.clock::sleep
        pause!(ms(1))
pub fn main!() -> void $ Clock:
    let clock: &mut dyn Clock = $.use(Clock)
    _ := block_on(clock.sleep(ms(1)))
    using pause = &mut Pause { clock: $.use(Clock) }
";
    for target in [EngineTarget::V8, EngineTarget::Wasmtime] {
        for (profile, level) in [
            (BuildProfile::Checked, OptLevel::Dev),
            (BuildProfile::Checked, OptLevel::Optimized),
            (BuildProfile::Release, OptLevel::Optimized),
        ] {
            for (source, expected) in [
                (immediate, false),
                (stored, true),
                (asynchronous, true),
                (callable, false),
                (shielded, true),
                (cold_callable, true),
                (raw_constructor, true),
            ] {
                let names = linked_slots(source, target, profile, level);
                assert_eq!(
                    names
                        .iter()
                        .any(|(name, in_table)| name == "rt:hd:Clock/sleep" && *in_table),
                    expected,
                    "{target:?}/{profile:?}/{level:?}: {names:?}"
                );
                if !expected {
                    assert!(
                        names
                            .iter()
                            .any(|(name, _)| name == "rt:hd:Clock/sleep.drive"
                                || name == "rt:hd:Clock/sleep.block_drive")
                    );
                }
            }
        }
    }
}
