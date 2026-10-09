//! Recovering a concrete type (trait.downcast.*, #166): `downcast_val`,
//! `downcast` and `downcast_mut` compare the value's recorded type with the
//! target's runtime identity, exactly (trait.downcast.exact). A trait
//! value's recorded type is the `TypeId` its `Inspectable` dictionary's
//! `runtime_type` slot answers, built at the concrete type the vtable was
//! built at; a concrete receiver's is its static type. A recovered
//! reference is the erased one (`is` holds), a boxed value comes back
//! unboxed. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.

use std::path::{Path, PathBuf};
use std::process::Command;

use hd_cache::MemoryStore;
use hd_driver::{Executor, Goal, Host, NoClock, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

/// The standard output of a program that must build and succeed.
fn output_of(name: &str, main: &str) -> String {
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
    let out = build(
        &host,
        "app",
        &Goal::Program {
            entry: "main".into(),
        },
    );
    assert!(out.diags.is_empty(), "{name}: {}", out.render());
    let wasm = out.wasm.expect("wasm");
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("downcast-{name}.wasm"));
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

const ERRORS: &str = "\
use std.error.Error
use std.inspect.downcast_val

data DiskError:
    device: string

impl Display for DiskError:
    fn to_string(self) -> string: \"disk ${self.device}\"

impl Error for DiskError

data Timeout:
    seconds: i32

impl Display for Timeout:
    fn to_string(self) -> string: \"timed out\"

impl Error for Timeout

enum FsError:
    NotFound(path: string)
    Denied

impl Display for FsError:
    fn to_string(self) -> string: \"fs\"

impl Error for FsError
";

/// A `dyn Error` comes back as its data error, the same reference, and
/// not as another error type (trait.downcast.some,
/// trait.downcast.same-reference).
#[test]
fn a_dyn_error_downcasts_to_its_data_error() {
    let main = format!(
        "{ERRORS}
fn disk_of(error: dyn Error) -> string:
    match error.downcast::[DiskError]():
        .Some(found) => found.device
        .None => \"none\"

pub fn main() -> void $ Console:
    disk := DiskError {{ device: \"sda\" }}
    let error: dyn Error = disk
    println(disk_of(error))
    println(disk_of(Timeout {{ seconds: 3 }}))
    match error.downcast::[DiskError]():
        .Some(found) => println(\"same: ${{found is disk}}\")
        .None => println(\"lost\")
    match error.downcast::[Timeout]():
        .Some(_) => println(\"wrong\")
        .None => println(\"not a timeout\")
"
    );
    assert_eq!(
        output_of("data-error", &main),
        "sda\nnone\nsame: true\nnot a timeout\n"
    );
}

/// An enum error comes back through `downcast_val` as an equal value
/// (trait.error.downcast, trait.downcast.unboxed).
#[test]
fn an_enum_error_comes_back_as_an_equal_value() {
    let main = format!(
        "{ERRORS}
fn fs_of(error: dyn Error) -> string:
    match downcast_val::[FsError](error):
        .Some(FsError.NotFound(path)) => \"not found ${{path}}\"
        .Some(FsError.Denied) => \"denied\"
        .None => \"none\"

pub fn main() -> void $ Console:
    println(fs_of(FsError.NotFound(\"a.txt\")))
    println(fs_of(FsError.Denied))
    println(fs_of(DiskError {{ device: \"sda\" }}))
"
    );
    assert_eq!(
        output_of("enum-error", &main),
        "not found a.txt\ndenied\nnone\n"
    );
}

/// Type arguments match exactly: an erased `Box[i64]` is not a
/// `Box[i32]` (trait.downcast.exact, trait.identity.arguments).
#[test]
fn type_arguments_match_exactly() {
    let main = "\
use std.inspect.Inspectable

data Box[T]:
    value: T

fn int_box(value: dyn Inspectable) -> string:
    match value.downcast::[Box[i32]]():
        .Some(found) => \"i32 box ${found.value}\"
        .None => \"not an i32 box\"

pub fn main() -> void $ Console:
    let narrow: i32 = 1
    let wide: i64 = 1
    println(int_box(Box { value: narrow }))
    println(int_box(Box { value: wide }))
";
    assert_eq!(output_of("exact", main), "i32 box 1\nnot an i32 box\n");
}

/// An erased `User?` comes back as `User?`, so `downcast_val` gives a
/// `User??`, and a `User` is not a `User?` (trait.downcast.optional).
#[test]
fn an_erased_optional_stays_optional() {
    let main = "\
use std.inspect.{Inspectable, downcast_val}

data User:
    name: string

fn user_of(value: dyn Inspectable) -> string:
    match downcast_val::[User?](value):
        .Some(.Some(user)) => \"user ${user.name}\"
        .Some(.None) => \"no user\"
        .None => \"not a User?\"

pub fn main() -> void $ Console:
    let present: User? = User { name: \"Ada\" }
    let absent: User? = .None
    println(user_of(present))
    println(user_of(absent))
    println(user_of(User { name: \"Bob\" }))
";
    assert_eq!(
        output_of("optional", main),
        "user Ada\nno user\nnot a User?\n"
    );
}

/// A boxed scalar is unboxed, and no numeric conversion applies
/// (trait.downcast.val, trait.downcast.no-conversion).
#[test]
fn downcast_val_unboxes_a_scalar() {
    let main = "\
use std.inspect.{Inspectable, downcast_val}

fn small(value: dyn Inspectable) -> string:
    match downcast_val::[u8](value):
        .Some(n) => \"u8 ${n}\"
        .None => \"not u8\"

fn wide(value: dyn Inspectable) -> string:
    match downcast_val::[i64](value):
        .Some(n) => \"i64 ${n}\"
        .None => \"not i64\"

pub fn main() -> void $ Console:
    let byte: u8 = 200
    let big: i64 = 5000000000
    println(small(byte))
    println(small(big))
    println(wide(big))
    println(wide(byte))
    println(wide(\"5\"))
";
    assert_eq!(
        output_of("scalar", main),
        "u8 200\nnot u8\ni64 5000000000\nnot i64\nnot i64\n"
    );
}

/// `downcast_mut` recovers mutable access to the erased object, and a
/// bound receiver recovers through its instance's type: a trait value
/// through its vtable, a concrete value by its static type.
#[test]
fn downcast_mut_and_bound_receivers() {
    let main = format!(
        "{ERRORS}
use std.inspect.Inspectable

data Counter:
    count: i32

fn bump(value: mut dyn Inspectable) -> void:
    match value.downcast_mut::[Counter]():
        .Some(counter) => counter.count = counter.count + 1
        .None => pass

fn disk_of[T < Error](value: T) -> string:
    match value.downcast::[DiskError]():
        .Some(found) => found.device
        .None => \"none\"

pub fn main() -> void $ Console:
    let mut counter = Counter {{ count: 1 }}
    bump(counter)
    println(\"${{counter.count}}\")
    let error: dyn Error = DiskError {{ device: \"sda\" }}
    println(disk_of(error))
    println(disk_of(DiskError {{ device: \"sdb\" }}))
    println(disk_of(Timeout {{ seconds: 1 }}))
"
    );
    assert_eq!(output_of("mut-and-bound", &main), "2\nsda\nsdb\nnone\n");
}

/// `runtime_type` names the recorded type (trait.inspect.dynamic,
/// trait.typeid.name.*): a trait value's is the erased concrete type, a
/// concrete value's its static type, without the outer `mut`.
#[test]
fn runtime_type_names_the_recorded_type() {
    let main = format!(
        "{ERRORS}
data User:
    name: string

pub fn main() -> void $ Console:
    let error: dyn Error = DiskError {{ device: \"sda\" }}
    println(\"${{error.runtime_type()}}\")
    let users: mut List[User?] = []
    println(\"${{users.runtime_type()}}\")
    let pair: (i32, mut List[string]) = (1, [])
    println(\"${{pair.runtime_type()}}\")
"
    );
    assert_eq!(
        output_of("runtime-type", &main),
        "app.main.DiskError\nList[app.main.User?]\n(i32, mut List[string])\n"
    );
}

/// Values of unrelated types erased to `dyn Inspectable` in one list each
/// come back at their own type through the method form with explicit type
/// arguments, and as `.None` at any other (trait.downcast.some).
#[test]
fn a_list_of_inspectable_values_downcasts_by_method() {
    let main = "\
use std.inspect.Inspectable

data User:
    name: string

data Tag:
    label: string

fn describe(value: dyn Inspectable) -> string:
    match value.downcast::[User]():
        .Some(user) => \"user ${user.name}\"
        .None => match value.downcast::[Tag]():
            .Some(tag) => \"tag ${tag.label}\"
            .None => \"other\"

pub fn main() -> void $ Console:
    let values: List[dyn Inspectable] = [User { name: \"Ada\" }, Tag { label: \"hi\" }, 3]
    for value in values:
        println(describe(value))
";
    assert_eq!(output_of("list-method", main), "user Ada\ntag hi\nother\n");
}
