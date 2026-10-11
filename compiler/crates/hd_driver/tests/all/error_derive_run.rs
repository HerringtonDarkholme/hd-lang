//! `@error` derivation at run time (spec 14 "Error Derivation";
//! codegen.md §13.14): the generated `Display` renders each variant's
//! message, `Error.cause` returns the `@from` or `@source` member, `?`
//! converts through the generated `From`, and transparent errors forward
//! both. Programs build through the driver and run on V8
//! (`host/run.mjs`), as the conformance runner does.
//!
//! A message read through a `dyn Error` (a supertrait call, codegen.md
//! §13.16) is not emitted yet, so causes are observed through the chain's
//! length and `cause().is_some()`.

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
    let path = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("error-derive-{name}.wasm"));
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

#[test]
fn an_enum_displays_each_variant_message() {
    let main = "\
@error
enum FsError:
    @error(\"not found: $path\")
    NotFound(path: string)
    @error(\"permission denied: $_0\")
    Denied(string)
    @error(\"${count} retries left\")
    Retry(count: i32)
    Busy

@error
enum HttpError(code: i32):
    @error(\"denied: $_0 ($code)\")
    Denied(string) -> HttpError(403)
    Missing -> HttpError(404)

pub fn main() -> void $ Console:
    println(FsError.NotFound(\"a.txt\").to_string())
    println(FsError.Denied(\"b.txt\").to_string())
    println(FsError.Retry(2).to_string())
    println(FsError.Busy.to_string())
    println(HttpError.Denied(\"f.txt\").to_string())
    println(HttpError.Missing.to_string())
";
    assert_eq!(
        output_of("enum", main),
        "not found: a.txt\npermission denied: b.txt\n2 retries left\nBusy\n\
         denied: f.txt (403)\nMissing\n"
    );
}

#[test]
fn a_data_error_interpolates_its_fields() {
    let main = "\
@error(\"bad yaml at line $line, column $column\")
data YamlError:
    line: i64
    column: i64

pub fn main() -> void $ Console:
    println(YamlError { line: 3, column: 7 }.to_string())
";
    assert_eq!(output_of("data", main), "bad yaml at line 3, column 7\n");
}

#[test]
fn question_mark_converts_through_the_generated_from() {
    let main = "\
@error(\"bad yaml at line $line\")
data YamlError:
    line: i64

@error
enum LoadError:
    @error(\"bad config\")
    Yaml(@from error: YamlError)
    @error(\"empty\")
    Empty

fn parse(text: string) -> Result[i64, YamlError]:
    if text == \"\": .Err(YamlError { line: 3 }) else: .Ok(1)

fn load(text: string) -> Result[i64, LoadError]:
    value := parse(text)?
    .Ok(value)

fn shown(result: Result[i64, LoadError]) -> string:
    match result:
        .Ok(value) => \"ok $value\"
        .Err(error) => error.to_string()

pub fn main() -> void $ Console:
    println(shown(load(\"\")))
    println(shown(load(\"x\")))
";
    assert_eq!(output_of("from", main), "bad config\nok 1\n");
}

#[test]
fn source_members_are_the_cause() {
    let main = "\
use std.error.{Error, chain}

@error(\"disk full\")
data DiskError:
    free: i64

@error
enum SaveError:
    @error(\"cannot save $path\")
    Write(path: string, @source error: DiskError)
    @error(\"cannot open $path\")
    Open(path: string, @source error: DiskError?)
    @error(\"read only\")
    ReadOnly

@error
enum SyncError:
    @error(\"sync failed\")
    Save(@from error: SaveError)

fn has_cause(error: dyn Error) -> bool:
    error.cause().is_some()

pub fn main() -> void $ Console:
    println(has_cause(SaveError.Write(\"a\", DiskError { free: 0 })))
    println(has_cause(SaveError.Open(\"a\", .None)))
    println(has_cause(SaveError.Open(\"a\", DiskError { free: 0 })))
    println(has_cause(SaveError.ReadOnly))
    println(has_cause(DiskError { free: 0 }))
    println(chain(SyncError.Save(SaveError.Write(\"a\", DiskError { free: 0 }))).len())
    println(chain(SyncError.Save(SaveError.ReadOnly)).len())
";
    assert_eq!(
        output_of("source", main),
        "true\nfalse\ntrue\nfalse\nfalse\n3\n2\n"
    );
}

#[test]
fn transparent_errors_forward_message_and_cause() {
    let main = "\
use std.error.{Error, chain}

@error(\"disk full\")
data DiskError:
    free: i64

@error
enum SaveError:
    @error(\"cannot save $path\")
    Write(path: string, @source error: DiskError)
    @error(\"not found: $_0\")
    Missing(string)

@error
enum AppError:
    @error(transparent)
    Save(@from error: SaveError)

@error(transparent)
data PublicError:
    @from
    repr: AppError

fn save(error: SaveError) -> Result[void, AppError]:
    let failed: Result[void, SaveError] = .Err(error)
    failed?
    .Ok(())

fn publish(error: SaveError) -> Result[void, PublicError]:
    save(error)?
    .Ok(())

pub fn main() -> void $ Console:
    let app = AppError.Save(SaveError.Missing(\"d.txt\"))
    println(app.to_string())
    println(chain(app).len())
    println(chain(AppError.Save(SaveError.Write(\"e\", DiskError { free: 0 }))).len())
    match publish(SaveError.Write(\"f.txt\", DiskError { free: 0 })):
        .Ok(_) => println(\"ok\")
        .Err(public) =>
            println(public.to_string())
            println(chain(public).len())
";
    assert_eq!(
        output_of("transparent", main),
        "not found: d.txt\n1\n2\ncannot save f.txt\n2\n"
    );
}

#[test]
fn a_generic_error_type_gets_derived_bounds() {
    let main = "\
use std.error.{Error, chain}

@error(\"disk full\")
data DiskError:
    free: i64

data Job:
    id: i64

@error
enum TaskError[E, T]:
    @error(\"task $name failed\")
    Failed(name: string, @source error: E, input: T)

@error
enum Wrapped[P]:
    @error(transparent)
    Inner(error: P)

@error
enum Named[T]:
    @error(\"got $value\")
    Got(value: T)

fn depth[E < Error](error: E) -> usize:
    chain(error).len()

fn show[T < Display](value: Named[T]) -> string:
    value.to_string()

pub fn main() -> void $ Console:
    let task: TaskError[DiskError, Job] = TaskError.Failed(\"build\", DiskError { free: 0 }, Job { id: 1 })
    println(task.to_string())
    println(depth(task))
    let wrapped: Wrapped[TaskError[DiskError, Job]] = Wrapped.Inner(task)
    println(wrapped.to_string())
    println(depth(wrapped))
    println(show(Named.Got(7)))
    println(show(Named.Got(\"x\")))
    let named: Named[i32] = Named.Got(1)
    println(depth(named))
";
    assert_eq!(
        output_of("generic", main),
        "task build failed\n2\ntask build failed\n2\ngot 7\ngot x\n1\n"
    );
}
