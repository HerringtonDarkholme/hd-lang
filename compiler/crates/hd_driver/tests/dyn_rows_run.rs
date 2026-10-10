//! Trait methods with a requirement row called through a trait value
//! (trait.dyn.safe.suspending, codegen.md §12.4, §13.5.1): a vtable slot
//! takes the providers of its method's row after the arguments, as a
//! direct call of the method does, and the slot's adapter passes them on
//! to the impl's method. Programs build through the driver and run on V8
//! (`host/run.mjs`), whose default profile has a host timer, so the
//! suspending methods really wait.

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("dyn-rows-{name}.wasm"));
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

/// A sensor trait whose suspending method waits on the clock and logs,
/// with two implementations.
const SENSORS: &str = "\
use std.time.{Clock, Duration, sleep}

trait Sensor:
    fn sample!(self, tick: i32) -> i32 $ Console + Clock
    fn name(self) -> string

data Thermometer:
    offset: i32

impl Sensor for Thermometer:
    fn sample!(self, tick: i32) -> i32 $ Console + Clock:
        sleep!(Duration::milliseconds(2))
        println(\"thermometer ${tick}\")
        self.offset + tick

    fn name(self) -> string: \"thermometer\"

data Barometer:
    scale: i32

impl Sensor for Barometer:
    fn sample!(self, tick: i32) -> i32 $ Console + Clock:
        println(\"barometer ${tick}\")
        sleep!(Duration::milliseconds(1))
        self.scale * tick

    fn name(self) -> string: \"barometer\"
";

#[test]
fn a_suspending_dyn_method_with_a_row_waits_and_uses_its_providers() {
    let main = format!(
        "{SENSORS}
fn read_twice!(sensor: dyn Sensor) -> i32 $ Console + Clock:
    sensor.sample!(1) + sensor.sample!(2)

pub fn main!() -> void $ Console + Clock:
    println(read_twice!(Thermometer {{ offset: 10 }}))
    let sensors: List[dyn Sensor] = [Barometer {{ scale: 3 }}, Thermometer {{ offset: 0 }}]
    for s in sensors:
        println(\"${{s.name()}} ${{s.sample!(5)}}\")
"
    );
    assert_eq!(
        output_of("suspending", &main),
        "thermometer 1\nthermometer 2\n23\nbarometer 5\nbarometer 15\nthermometer 5\nthermometer 5\n"
    );
}

#[test]
fn a_dyn_method_with_a_row_through_a_bound_and_a_function_reference() {
    let main = format!(
        "{SENSORS}
fn via_bound![S < Sensor](sensor: S, tick: i32) -> i32 $ Console + Clock:
    sensor.sample!(tick)

fn via_reference![S < Sensor](sensor: S, tick: i32) -> i32 $ Console + Clock:
    sample := S::sample
    sample!(sensor, tick)

pub fn main!() -> void $ Console + Clock:
    let sensor: dyn Sensor = Barometer {{ scale: 2 }}
    println(via_bound!(sensor, 4))
    println(via_reference!(sensor, 6))
"
    );
    assert_eq!(
        output_of("reference", &main),
        "barometer 4\n8\nbarometer 6\n12\n"
    );
}

#[test]
fn a_plain_dyn_method_with_a_row_gets_its_provider() {
    let main = "\
trait Greeter:
    fn greet(self, who: string) -> void $ Console

data Polite:
    title: string

impl Greeter for Polite:
    fn greet(self, who: string) -> void $ Console:
        println(\"Good day, ${self.title} ${who}\")

fn greet_all(greeter: dyn Greeter, names: List[string]) -> void $ Console:
    for n in names:
        greeter.greet(n)

pub fn main() -> void $ Console:
    greet_all(Polite { title: \"Dr.\" }, [\"Ada\", \"Alan\"])
";
    assert_eq!(
        output_of("plain", main),
        "Good day, Dr. Ada\nGood day, Dr. Alan\n"
    );
}
