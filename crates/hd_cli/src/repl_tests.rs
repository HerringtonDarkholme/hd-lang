//! The REPL loop driven by a script of lines in place of a terminal, one
//! test per `cli.repl.*` rule the CLI meets. Each test runs a whole
//! session: the session thread, the prelude, and each input built as
//! `repl#N` and run on wasmtime.
//!
//! A rule a transcript of standard output can judge has a session case under
//! `spec/conformance/cli/repl-*` instead (`spec/conformance/README.md`,
//! "Session Cases"). The tests here check what a transcript cannot: the
//! text of diagnostics and panic reports, the standard error stream, a
//! session that starts below the package root, interrupts, `:reload`,
//! history, completion and the line editor.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use hd_run::Outcome;
use hd_run_wasmtime::{EngineConfig, HostIo};
use hd_testkit::Scratch;

use super::{Bounds, Line, Lines, Streams, is_complete};
use crate::disk;

const SCRATCH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/tmp");

/// A stream a test reads after the session ends.
#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Write for Buffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("buffer").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl Buffer {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().expect("buffer").clone()).expect("UTF-8")
    }
}

/// A test's call for each line a script hands out.
type OnLine = Box<dyn FnMut(&str)>;

/// The lines a user types, and the prompts and history the REPL showed.
#[derive(Default)]
struct Script {
    lines: VecDeque<Line>,
    prompts: Vec<String>,
    history: Vec<String>,
    /// Called with each line before it is handed out.
    on_line: Option<OnLine>,
    /// What Tab completes, which the session updates.
    names: super::complete::Names,
}

impl Script {
    fn of(lines: &[&str]) -> Script {
        Script {
            lines: lines.iter().map(|l| Line::Text((*l).to_owned())).collect(),
            ..Script::default()
        }
    }
}

impl Lines for Script {
    fn read(&mut self, prompt: &str, initial: &str) -> Line {
        self.prompts.push(format!("{prompt}{initial}"));
        let line = self.lines.pop_front().unwrap_or(Line::Eof);
        if let (Some(hook), Line::Text(text)) = (&mut self.on_line, &line) {
            hook(text);
        }
        line
    }
    fn remember(&mut self, line: &str) {
        self.history.push(line.to_owned());
    }
    fn names(&self) -> super::complete::Names {
        self.names.clone()
    }
}

/// What a session wrote: its transcript, the guest's standard output and
/// error with the REPL's diagnostics in one stream, as a terminal shows
/// them. The value lines go to standard output (`cli.repl.value.stream`).
struct Transcript {
    out: String,
    script: Script,
}

/// Runs a session in `dir`, a package or not, over `script`. `on_start`
/// gets the session's stopper, as the Ctrl-C handler does.
fn session_in(
    dir: &Path,
    script: Script,
    on_start: impl FnOnce(crate::engine::SessionStopper),
) -> Transcript {
    session_with(dir, script, on_start, None, Bounds::default())
}

/// `session_in`, with the guest's standard output in `stdout` when given
/// and within `bounds`.
fn session_with(
    dir: &Path,
    script: Script,
    on_start: impl FnOnce(crate::engine::SessionStopper),
    stdout: Option<&Buffer>,
    bounds: Bounds,
) -> Transcript {
    session_on(
        crate::engine::EngineKind::Wasmtime,
        dir,
        script,
        on_start,
        stdout,
        bounds,
        None,
    )
}

fn session_on(
    kind: crate::engine::EngineKind,
    dir: &Path,
    mut script: Script,
    on_start: impl FnOnce(crate::engine::SessionStopper),
    stdout: Option<&Buffer>,
    bounds: Bounds,
    input: Option<&str>,
) -> Transcript {
    let err = Buffer::default();
    let out = stdout.cloned().unwrap_or_else(|| err.clone());
    let streams = Streams {
        io: HostIo {
            out: Box::new(out.clone()),
            err: Box::new(err.clone()),
            input: input.map(|text| {
                Box::new(std::io::Cursor::new(text.as_bytes().to_vec()))
                    as Box<dyn std::io::BufRead + Send>
            }),
        },
        diags: Box::new(err.clone()),
    };
    let program = disk::load_session(dir).expect("the session's scope");
    super::run_on(kind, &mut script, dir, program, streams, bounds, on_start)
        .expect("the session ends");
    Transcript {
        out: err.text(),
        script,
    }
}

/// Runs a session outside any package.
fn session(lines: &[&str]) -> Transcript {
    let dir = Scratch::new(SCRATCH, &format!("repl-outside-{}", unique()));
    session_in(&dir, Script::of(lines), |_| {})
}

fn unique() -> usize {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// A package `shop` with `files`, below the scratch directory `name`.
fn package(name: &str, files: &[(&str, &str)]) -> Scratch {
    let dir = Scratch::new(SCRATCH, name);
    let mut all = vec![("hd.toml", "[package]\nname = \"shop\"\n")];
    all.extend_from_slice(files);
    write_files(&dir, &all);
    dir
}

fn write_files(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let path = dir.join(path);
        std::fs::create_dir_all(path.parent().expect("a parent")).expect("directory");
        std::fs::write(path, text).expect("file");
    }
}

/// A session builds its program for wasmtime, in the dev pipeline
/// (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5), on the
/// linear backend.
#[test]
fn a_session_builds_for_wasmtime_in_the_dev_pipeline() {
    let pipeline = crate::Pipeline {
        target: hd_driver::EngineTarget::Wasmtime,
        level: hd_driver::OptLevel::Dev,
    };
    assert_eq!(super::PIPELINE, pipeline);
    let dir = Scratch::new(SCRATCH, &format!("repl-pipeline-{}", unique()));
    let mut program = disk::load_session(&dir).expect("the session's scope");
    let store = crate::disk_store();
    crate::take_built();
    program
        .sources
        .add_synthetic(&hd_project::repl_file(1), "x := +1\n");
    let (wasm, diags) = super::build_session(&program, &store, &[], 1, super::PIPELINE.target);
    assert!(wasm.is_some(), "{diags:?}");
    assert_eq!(crate::take_built(), [pipeline]);
}

/// `cli.dbg.repl`: an input's `dbg` lines print before its value line.
/// The `dbg` lines are on standard error; standard output holds the
/// input's own output and its value line.
#[test]
fn dbg_lines_come_before_the_value_line() {
    let dir = Scratch::new(SCRATCH, "repl-dbg");
    let stdout = Buffer::default();
    let script = Script::of(&[
        "println(\"out\")",
        "fn next(x: i32) -> i32:",
        "    dbg(x)",
        "    x + +1",
        "",
        "next(+5)",
    ]);
    let t = session_with(&dir, script, |_| {}, Some(&stdout), Bounds::default());
    assert_eq!(stdout.text(), "out\n6 : i32\n");
    assert_eq!(t.out, "repl#2:2:5: x = 5\n");
}

/// `cli.repl.value.stream`: one input writes its own output, a `dbg` line
/// and its value line. On one terminal (both streams in one buffer) they
/// come in that order.
#[test]
fn output_then_dbg_then_value_on_one_terminal() {
    let t = session(&[
        "fn show(x: i32) -> i32:",
        "    println(\"out\")",
        "    dbg(x)",
        "    x + +1",
        "",
        "show(+5)",
    ]);
    assert_eq!(t.out, "out\nrepl#1:3:5: x = 5\n6 : i32\n");
}

/// `cli.repl.redeclare.binding`: an input must not declare a function an
/// earlier input declared; the diagnostic names `repl#N:LINE:COL`. A
/// `let` binding is assigned, not declared again.
#[test]
fn a_function_is_not_declared_again() {
    let t = session(&[
        "rate := +3",
        "fn price(n: i32) -> i32: n * rate",
        "fn price(n: i32) -> i32: n * rate + 1",
        "let count = +1",
        "count = count + 1",
        "count",
    ]);
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", t.out);
    assert!(
        lines[0].starts_with("error: repl#3:1:4: duplicate-binding: "),
        "{}",
        t.out
    );
    assert_eq!(lines[1], "2 : i32");
}

/// `cli.repl.redeclare.type`: nor a type an earlier input declared.
#[test]
fn a_type_is_not_declared_again() {
    let t = session(&[
        "struct Point:",
        "    x: i32",
        "",
        "struct Point:",
        "    y: i32",
        "",
    ]);
    assert!(
        t.out.starts_with("error: repl#2:1:8: duplicate-type: "),
        "{}",
        t.out
    );
}

/// `cli.repl.redefine.impl`: an implementation that overlaps one of an
/// earlier input or of `src/lib.hd` is `overlapping-impl`, as in one
/// module, and the first one stays.
#[test]
fn an_earlier_input_s_warning_is_shown_once() {
    let t = session(&[
        "fn f() -> i32:",
        "    let unused = 1",
        "    +2",
        "",
        "f()",
        "f() + 1",
    ]);
    assert_eq!(
        t.out.matches("unused-local-binding").count(),
        1,
        "{}",
        t.out
    );
    assert!(
        t.out.contains("2 : i32") && t.out.contains("3 : i32"),
        "{}",
        t.out
    );
}

#[test]
fn a_second_impl_is_an_error() {
    let named = "trait Named:\n    fn name(self) -> string\n\nstruct Tag:\n    pub x: i32\n\nimpl Named for Tag:\n    fn name(self) -> string: \"lib\"\n";
    let dir = package("repl-impl", &[("src/lib.hd", named)]);
    let script = Script::of(&[
        "impl Named for Tag:",
        "    fn name(self) -> string: \"repl\"",
        "",
        "Tag { x: +1 }.name()",
    ]);
    let t = session_in(&dir, script, |_| {});
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", t.out);
    assert!(
        lines[0].starts_with("error: repl#1:1:1: overlapping-impl: "),
        "{}",
        t.out
    );
    assert_eq!(lines[1], "\"lib\" : string");
    let t = session(&[
        "trait Named:",
        "    fn name(self) -> string",
        "struct Tag:",
        "    pub x: i32",
        "impl Named for Tag:",
        "    fn name(self) -> string: \"first\"",
        "",
        "impl Named for Tag:",
        "    fn name(self) -> string: \"second\"",
        "",
        "Tag { x: +1 }.name()",
    ]);
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", t.out);
    assert!(
        lines[0].starts_with("error: repl#2:1:1: overlapping-impl: "),
        "{}",
        t.out
    );
    assert_eq!(lines[1], "\"first\" : string");
}

/// A session is one module for derivation blocks too
/// (`annot.block.module`): a later input derives `Debug` for an earlier
/// input's type, and the input after shows a value through it. `Eq` and
/// `Hash` are derived only with `@derive` (`annot.block.template`).
#[test]
fn a_later_input_derives_for_an_earlier_type() {
    let t = session(&[
        "struct Point:",
        "    pub x: i32",
        "    pub y: i32",
        "",
        "use std.structure.Structure",
        "",
        "impl Debug for Point by Structure",
        "",
        "debug(Point { x: +1, y: +2 })",
    ]);
    assert_eq!(t.out, "\"Point { x: 1, y: 2 }\" : string\n");
}

/// `src/lib.hd` for the member-line tests: a `Column` fact and a
/// `Columns` template whose walker writes each member's column name (its
/// `Column` fact or else its own name) and how many facts it has.
const COLUMNS_LIB: &str = r#"use std.structure.{Structure, Field, Variant, Walker}

struct Column:
    name: string

fn column(name: string) -> Column:
    Column { name: name }

trait Columns:
    fn columns(self) -> string

struct Header:
    out: string

impl[S] Walker[S] for Header:
    type Error = never

    fn variant(&mut self, v: Variant[S]) -> Result[void, never]:
        .Ok(())

    fn member[F](&mut self, h: Field[S, F], value: F) -> Result[void, never]:
        name := match h.info.facts.find::[Column]():
            .Some(c) => c.name
            .None => h.info.name
        self.out = "${self.out}${name}/${h.info.facts.items.len()};"
        .Ok(())

impl[T] Columns for T by Structure:
    fn columns(self) -> string:
        let mut w = Header { out: "" }
        _ := Structure::walk(self, &mut w)
        w.out
"#;

/// The member-line session: input 1 declares `Row`, input 3 is a block
/// with no trait for it, input 4 a derivation block with member lines.
const MEMBER_LINES: &[&str] = &[
    "struct Row:",
    "    @column(\"row_id\")",
    "    id: i32",
    "    @\"cents\"",
    "    total: i32",
    "    cache: i32 = +0",
    "",
    "use std.structure.Structure",
    "",
    "impl Row by Structure:",
    "    total += [column(\"sum_total\")]",
    "",
    "impl Columns for Row by Structure:",
    "    id = [column(\"key\")]",
    "    total += [true]",
    "    cache = pass",
    "",
    "Row { id: +1, total: +2 }.columns()",
];

/// A session is one module for member lines too (`annot.block.lines`,
/// `annot.no-trait.then-blocks`): a block with no trait in one input and a
/// later input's derivation block edit an earlier input's type, and the
/// derived columns show both. `total` keeps its declaration fact under the
/// added ones, `id` is renamed, and `cache` is left out.
#[test]
fn later_inputs_edit_an_earlier_type_s_facts() {
    let dir = package("repl-member-lines", &[("src/lib.hd", COLUMNS_LIB)]);
    let script = Script::of(MEMBER_LINES);
    let t = session_in(&dir, script, |_| {});
    assert_eq!(t.out, "\"key/1;sum_total/3;\" : string\n");
}

/// A late block with no trait changes the facts of later derivations
/// (`annot.no-trait.then-blocks`), while an already built derivation keeps
/// the facts it saw (REPL Decisions, "Late blocks with no trait").
#[test]
fn annot_no_trait_then_blocks_preserves_earlier_derivations() {
    let lib = format!(
        "{COLUMNS_LIB}{}",
        r#"
trait Columns2:
    fn columns2(self) -> string

impl[T] Columns2 for T by Structure:
    fn columns2(self) -> string:
        let mut w = Header { out: "" }
        _ := Structure::walk(self, &mut w)
        w.out
"#
    );
    let dir = package("repl-late-member-facts", &[("src/lib.hd", &lib)]);
    let script = Script::of(&[
        "use std.structure.Structure",
        "struct Row:",
        "    id: i32",
        "    total: i32",
        "",
        "impl Columns for Row by Structure",
        "Row { id: +1, total: +2 }.columns()",
        "impl Row by Structure:",
        "    total += [column(\"sum_total\")]",
        "",
        "impl Columns2 for Row by Structure",
        "Row { id: +1, total: +2 }.columns2()",
        "Row { id: +1, total: +2 }.columns()",
    ]);
    let t = session_in(&dir, script, |_| {});
    assert_eq!(
        t.out,
        "\"id/0;total/0;\" : string\n\
         \"id/0;sum_total/1;\" : string\n\
         \"id/0;total/0;\" : string\n"
    );
}

/// `cli.repl.rebuild`: after `:reload` builds every input again from its
/// text, the block with no trait and the derivation block's member lines
/// still edit the earlier input's type, and a derivation written after the
/// rebuild sees the block's facts too.
#[test]
fn member_lines_survive_a_rebuild() {
    let dir = package("repl-member-lines-reload", &[("src/lib.hd", COLUMNS_LIB)]);
    let mut lines = MEMBER_LINES.to_vec();
    lines.extend([
        ":reload",
        "Row { id: +1, total: +2 }.columns()",
        "struct Pair:",
        "    a: i32",
        "    b: i32",
        "",
        "impl Pair by Structure:",
        "    b += [column(\"second\")]",
        "",
        ":reload",
        "impl Columns for Pair by Structure",
        "Pair { a: +1, b: +2 }.columns()",
    ]);
    let t = session_in(&dir, Script::of(&lines), |_| {});
    assert_eq!(
        t.out,
        "\"key/1;sum_total/3;\" : string\n\
         reloaded: 5 inputs replayed\n\
         \"key/1;sum_total/3;\" : string\n\
         reloaded: 8 inputs replayed\n\
         \"a/0;second/1;\" : string\n"
    );
}

/// A later input implements `src/lib.hd`'s trait for an earlier input's
/// type, and a generic function of the library finds that implementation.
#[test]
fn a_library_function_finds_an_input_s_implementation() {
    let lib = "trait Named:\n    fn name(self) -> string\n\nfn greet[T < Named](x: T) -> string:\n    \"hi ${x.name()}\"\n";
    let dir = package("repl-impl-lib", &[("src/lib.hd", lib)]);
    let script = Script::of(&[
        "struct Cat:",
        "    pub age: i32",
        "",
        "impl Named for Cat:",
        "    fn name(self) -> string: \"cat ${self.age}\"",
        "",
        "greet(Cat { age: +3 })",
    ]);
    let t = session_in(&dir, script, |_| {});
    assert_eq!(t.out, "\"hi cat 3\" : string\n");
}

/// `cli.repl.panic`, `.binding`, `.mutations`: the spec's `log` example.
/// The panic is reported, the panicking input binds no `line`, its push
/// remains, and the session goes on.
#[test]
fn a_panicking_input_keeps_its_changes_and_binds_nothing() {
    let t = session(&[
        "let mut log: List[string] = []",
        "fn record_then_read(entries: &mut List[string]) -> string:",
        "    entries.push(\"started\")",
        "    entries[5]",
        "",
        "line := record_then_read(&mut log)",
        "log",
        "line",
    ]);
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines.len(), 3, "{}", t.out);
    assert_eq!(
        lines[0],
        "panic: index-out-of-bounds: list index out of bounds"
    );
    assert_eq!(lines[1], "[\"started\"] : List[string]");
    assert!(
        lines[2].starts_with("error: repl#5:1:1: unknown-name"),
        "{}",
        t.out
    );
}

/// `cli.repl.panic.declarations`: an input that panics declares nothing,
/// so its names stay free.
#[test]
fn a_panicking_input_declares_nothing() {
    let t = session(&[
        "fn lost() -> i32:",
        "    +2",
        "_ := [+1][3]",
        "fn lost() -> i32: +3",
        "lost()",
    ]);
    assert_eq!(
        t.out,
        "panic: index-out-of-bounds: list index out of bounds\n3 : i32\n"
    );
}

/// `cli.repl.stop`: Ctrl-C while an input computes stops it; it ends as a
/// panicking input does, and the next input runs.
#[test]
fn a_stopped_input_ends_and_the_session_goes_on() {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::time::Duration;
    // 1 once the loop input is sent, 2 once the next input is read.
    let phase = Arc::new(AtomicU8::new(0));
    let mut script = Script::of(&["let kept = 7", "while true:", "    pass", "", "kept + 1"]);
    let seen = Arc::clone(&phase);
    script.on_line = Some(Box::new(move |line: &str| match line {
        "" => seen.store(1, Ordering::SeqCst),
        "kept + 1" => {
            seen.store(2, Ordering::SeqCst);
            // Past the stopping thread's last stop, which the next input's
            // start clears.
            std::thread::sleep(Duration::from_millis(300));
        }
        _ => {}
    }));
    let dir = Scratch::new(SCRATCH, "repl-stop");
    let mut stopping = None;
    let t = session_in(&dir, script, |stopper| {
        // Ctrl-C while the loop runs, as the handler calls it: a stop
        // before the input starts is dropped, so it repeats.
        stopping = Some(std::thread::spawn(move || {
            while phase.load(Ordering::SeqCst) < 2 {
                if phase.load(Ordering::SeqCst) == 1 {
                    stopper.stop();
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }));
    });
    stopping
        .expect("started")
        .join()
        .expect("the stopping thread");
    assert_eq!(
        t.out,
        "stopped\nnote: repl#2 was stopped, so the next rebuild skips it; its changes are not restored\n8 : usize\n"
    );
}

/// `cli.repl.cancel`: Ctrl-C while an input waits on the host cancels it
/// at once; its `defer` runs, and it ends as a panicking input does. The
/// next input's rebuild skips it (`cli.repl.rebuild.stopped`), so what
/// the `defer` changed is not restored, which the REPL says.
#[test]
fn a_waiting_input_is_cancelled_and_the_session_goes_on() {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::time::{Duration, Instant};
    // 1 once the waiting input is sent, 2 once the next input is read.
    let phase = Arc::new(AtomicU8::new(0));
    let mut script = Script::of(&[
        "use std.time.{sleep, Duration}",
        "let mut log: List[string] = []",
        "fn nap!() -> void:",
        "    defer:",
        "        log.push(\"cleanup\")",
        "    sleep!(Duration.seconds(+30))",
        "",
        "nap!()",
        "log",
    ]);
    let seen = Arc::clone(&phase);
    script.on_line = Some(Box::new(move |line: &str| match line {
        "nap!()" => seen.store(1, Ordering::SeqCst),
        "log" => seen.store(2, Ordering::SeqCst),
        _ => {}
    }));
    let dir = Scratch::new(SCRATCH, "repl-cancel");
    let mut stopping = None;
    let started = Instant::now();
    let t = session_in(&dir, script, |stopper| {
        // Ctrl-C once the input waits on the host, as the handler calls
        // it. A stop while its Wasm still runs would stop it instead.
        stopping = Some(std::thread::spawn(move || {
            while phase.load(Ordering::SeqCst) < 2 {
                if phase.load(Ordering::SeqCst) == 1 && stopper.waiting() {
                    stopper.stop();
                    return;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }));
    });
    stopping
        .expect("started")
        .join()
        .expect("the stopping thread");
    assert!(
        started.elapsed() < Duration::from_secs(20),
        "the prompt waited for the timer: {:?}",
        started.elapsed()
    );
    assert_eq!(
        t.out,
        "cancelled\nnote: repl#4 was cancelled, so the next rebuild skips it; its changes are not restored\n[] : List[string]\n"
    );
}

/// `cli.repl.uses.other`: a use that names a module reaches later inputs.
#[test]
fn a_module_use_reaches_later_inputs() {
    let t = session(&[
        "use std.path as p",
        "p.Path(\"a\").join(\"b\").parent().is_some()",
        "p := 1",
    ]);
    assert!(t.out.starts_with("true : bool\n"), "{}", t.out);
    assert!(t.out.contains("duplicate-binding"), "{}", t.out);
}

/// `cli.repl.uses.other` and `cli.repl.rebuild`: a module use of an earlier
/// input, with or without an alias, still reaches later inputs after
/// `:reload` runs the inputs again.
#[test]
fn a_module_use_survives_a_rebuild() {
    let t = session(&[
        "use std.path",
        "use std.cmp as c",
        "path.Path(\"a\").join(\"b\").parent().is_some()",
        ":reload",
        "path.Path(\"a\").join(\"b\").parent().is_some()",
        "c.max(+1, +2)",
        "fn path() -> i32: +1",
    ]);
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines[0], "true : bool", "{}", t.out);
    assert_eq!(lines[1], "reloaded: 3 inputs replayed", "{}", t.out);
    assert_eq!(lines[2], "true : bool", "{}", t.out);
    assert_eq!(lines[3], "2 : i32", "{}", t.out);
    assert!(t.out.contains("duplicate-binding"), "{}", t.out);
}

/// `cli.repl.outside`: outside any package the session uses only `std`.
#[test]
fn outside_a_package_only_std_is_used() {
    let t = session(&["use self.util", "use std.cmp.{max}", "max(+3, +4)"]);
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", t.out);
    assert!(lines[0].starts_with("error: repl#1:1:"), "{}", t.out);
    assert_eq!(lines[1], "4 : i32");
}

/// `cli.workspace.repl`: at a workspace root the session uses only `std`.
#[test]
fn a_workspace_root_session_uses_only_std() {
    let dir = Scratch::new(SCRATCH, "repl-workspace");
    write_files(
        &dir,
        &[
            ("hd.toml", "[workspace]\nmembers = [\"shop\"]\n"),
            ("shop/hd.toml", "[package]\nname = \"shop\"\n"),
            ("shop/src/lib.hd", "fn bonus() -> i32: +7\n"),
        ],
    );
    let t = session_in(&dir, Script::of(&["bonus()", "+1"]), |_| {});
    assert!(
        t.out.starts_with("error: repl#1:1:1: unknown-name"),
        "{}",
        t.out
    );
    assert!(t.out.ends_with("\n1 : i32\n"), "{}", t.out);
}

/// `cli.repl.package.lib`: in package mode the session acts as code in
/// `src/lib.hd`, wherever it starts: it sees `lib.hd`'s private
/// declarations, and `use self.util` names `src/util.hd`.
#[test]
fn a_package_session_acts_as_lib() {
    let dir = package(
        "repl-lib",
        &[
            ("src/lib.hd", "fn bonus() -> i32: +7\n"),
            ("src/util.hd", "pub fn double(x: i32) -> i32: x * +2\n"),
            ("src/billing/invoice.hd", "pub fn fee() -> i32: +1\n"),
        ],
    );
    let t = session_in(
        &dir.join("src/billing"),
        Script::of(&["use self.util.{double}", "double(bonus())"]),
        |_| {},
    );
    assert_eq!(t.out, "14 : i32\n");
}

/// `cli.repl.uses.other`: apart from `lib.hd`'s declarations, names reach
/// the session only through its `use` declarations.
#[test]
fn other_names_need_a_use() {
    let dir = package(
        "repl-uses",
        &[
            ("src/lib.hd", "fn bonus() -> i32: +7\n"),
            ("src/util.hd", "pub fn double(x: i32) -> i32: x * +2\n"),
        ],
    );
    let t = session_in(&dir, Script::of(&["double(+1)"]), |_| {});
    assert!(
        t.out.starts_with("error: repl#1:1:1: unknown-name"),
        "{}",
        t.out
    );
}

/// `cli.repl.package.no-lib` and `.no-lib.main`: without `src/lib.hd`,
/// the session uses the public declarations of the package's modules
/// through `self`, but never `src/main.hd`.
#[test]
fn a_package_without_lib_uses_its_modules_but_not_main() {
    let dir = package(
        "repl-no-lib",
        &[
            (
                "src/main.hd",
                "pub fn main() -> void $ Console:\n    println(42)\n",
            ),
            ("src/util.hd", "pub fn double(x: i32) -> i32: x * +2\n"),
        ],
    );
    let script = Script::of(&["use self.util.{double}", "double(+4)", "use self.main"]);
    let t = session_in(&dir, script, |_| {});
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines.len(), 2, "{}", t.out);
    assert_eq!(lines[0], "8 : i32");
    assert!(lines[1].starts_with("error: repl#3:1:"), "{}", t.out);
}

/// A package file that does not check stops no input that does not reach
/// it: the session builds `src/lib.hd`, the modules it uses, and what the
/// inputs use (`cli.repl.package.lib`, `cli.repl.uses.other`), never
/// `src/main.hd` or a module nothing uses.
#[test]
fn a_broken_file_the_session_never_reaches_stops_no_input() {
    let dir = package(
        "repl-broken-main",
        &[
            (
                "src/lib.hd",
                "use self.util.{double}\n\nfn bonus() -> i32: double(+7)\n",
            ),
            ("src/util.hd", "pub fn double(x: i32) -> i32: x * +2\n"),
            (
                "src/main.hd",
                "pub fn main() -> void $ Console:\n    let n: i32 = \"no\"\n",
            ),
            ("src/junk.hd", "pub fn junk() -> i32: \"no\"\n"),
            ("src/broken/mod.hd", "fn (\n"),
        ],
    );
    let t = session_in(&dir, Script::of(&["1 + 2", "bonus()"]), |_| {});
    assert_eq!(t.out, "3 : usize\n14 : i32\n");
}

/// A session loads `src/lib.hd` when an input first names one of its
/// names: a library that does not check stops only such an input, with
/// the library's error. After the library is fixed, `:reload` rebuilds
/// the session and the input runs.
#[test]
fn a_broken_lib_stops_only_the_inputs_that_name_it() {
    let t = reload_after_editing(
        "repl-broken-lib",
        "fn bonus() -> i32: \"seven\"\n",
        "fn bonus() -> i32: +7\n",
        &["1 + 2", "x := +5", "x + bonus()", ":reload", "x + bonus()"],
    );
    let (live, rest) = t.out.split_at(t.out.find("error: ").expect("an error"));
    assert_eq!(live, "3 : usize\n");
    assert!(rest.starts_with("error: src/lib.hd:1:"), "{}", t.out);
    assert!(
        rest.ends_with("reloaded: 2 inputs replayed\n12 : i32\n"),
        "{}",
        t.out
    );
}

/// An input continues while the parser stops inside it: after a block
/// header until an empty line, and inside open brackets. Continuation
/// lines start indented; each line joins the session's history.
#[test]
fn an_input_continues_while_incomplete() {
    let t = session(&[
        "fn add(a: i32, b: i32) -> i32:",
        "    a + b",
        "",
        "add(",
        "  +1,",
        "  +2)",
    ]);
    assert_eq!(t.out, "3 : i32\n", "{:?}", t.script.prompts);
    assert_eq!(
        t.script.prompts,
        [
            "hd> ", "...     ", "...     ", "hd> ", "... ", "...   ", "hd> "
        ]
    );
    assert_eq!(
        t.script.history,
        [
            "fn add(a: i32, b: i32) -> i32:",
            "    a + b",
            "add(",
            "  +1,",
            "  +2)"
        ]
    );
}

/// Ctrl-C at a prompt drops the input so far; an empty line at the first
/// prompt is no input.
#[test]
fn an_interrupt_at_the_prompt_drops_the_input() {
    let mut script = Script::of(&["fn add(a: i32) -> i32:"]);
    script.lines.push_back(Line::Interrupted);
    script.lines.push_back(Line::Text(String::new()));
    script.lines.push_back(Line::Text("+5".into()));
    let dir = Scratch::new(SCRATCH, "repl-interrupt");
    let t = session_in(&dir, script, |_| {});
    assert_eq!(t.out, "5 : i32\n");
}

/// Diagnostics of a later line of an input name its line in `repl#N`.
#[test]
fn diagnostics_name_the_input_and_line() {
    let t = session(&["+1", "fn f() -> i32:", "    missing", ""]);
    assert!(
        t.out
            .starts_with("1 : i32\nerror: repl#2:2:5: unknown-name: "),
        "{}",
        t.out
    );
}

/// Past its journal's cap the REPL says once, after the input that passed
/// it, that the session can run no further input, since every input
/// replays the journal (`docs/future-work/live-execution.md` §4.6); each
/// later input and `:reload` is refused. The cap here is 200 bytes.
#[test]
fn a_full_journal_is_noted_once() {
    let dir = Scratch::new(SCRATCH, &format!("repl-journal-{}", unique()));
    let script = Script::of(&[
        "use std.time.{now}",
        "a := now()",
        "b := now()",
        "c := now()",
        "d := now()",
        "+7",
        ":reload",
    ]);
    let bounds = Bounds { journal_cap: 200 };
    let t = session_with(&dir, script, |_| {}, None, bounds);
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines.first(), Some(&super::JOURNAL_FULL), "{}", t.out);
    let refused = &lines[1..lines.len() - 1];
    assert!(!refused.is_empty(), "{}", t.out);
    assert!(refused.iter().all(|l| *l == super::NO_JOURNAL), "{}", t.out);
    assert_eq!(
        lines.last(),
        Some(&"error: the session's record of host calls is full, so it cannot be rebuilt")
    );
}

/// `cli.repl.rebuild` and `cli.repl.host.once`: `:reload` runs the inputs
/// again with their host results served from the journal. The clock is
/// not read again, so `t` is the same; the file is not read again; the
/// line is not printed again.
#[test]
fn reload_repeats_no_host_call() {
    let t = session(&[
        "use std.time.{now}",
        "use std.fs.{read_text}",
        "use std.path.{Path}",
        "t := now().unix_milliseconds()",
        "t",
        "size := read_text!(Path(\"Cargo.toml\")).unwrap_or(\"\").len()",
        "println(\"once\")",
        ":reload",
        "t",
        "size > 0",
    ]);
    let lines: Vec<&str> = t.out.lines().collect();
    assert_eq!(lines.len(), 5, "{}", t.out);
    assert!(lines[0].ends_with(" : i64"), "{}", t.out);
    assert_eq!(
        lines[1..4],
        ["once", "reloaded: 7 inputs replayed", lines[0]]
    );
    assert_eq!(lines[4], "true : bool");
}

/// A package `shop` whose `lib.hd` the test edits when the script hands
/// out `:reload`, as a user edits it before typing it.
fn reload_after_editing(
    name: &str,
    lib: &'static str,
    edited: &'static str,
    lines: &[&str],
) -> Transcript {
    let dir = package(name, &[("src/lib.hd", lib)]);
    let file = dir.join("src/lib.hd");
    let mut script = Script::of(lines);
    script.on_line = Some(Box::new(move |line: &str| {
        if line == ":reload" {
            std::fs::write(&file, edited).expect("edits lib.hd");
        }
    }));
    session_in(&dir, script, |_| {})
}

/// `cli.repl.rebuild.prefix`: after `lib.hd`'s `price` is renamed,
/// `:reload` keeps the input before the first that no longer checks,
/// reports that input's diagnostic, and drops it and the input after it.
#[test]
fn reload_drops_from_the_first_input_that_no_longer_checks() {
    let t = reload_after_editing(
        "repl-reload-check",
        "fn price(n: i32) -> i32: n * +2\n",
        "fn cost(n: i32) -> i32: n * +2\n",
        &["rate := +10", "price(+3)", "rate + +1", ":reload", "rate"],
    );
    let (live, reload) = t.out.split_at(t.out.find("error: ").expect("an error"));
    assert_eq!(live, "6 : i32\n11 : i32\n");
    assert!(
        reload.starts_with("error: repl#2:1:1: unknown-name"),
        "{}",
        t.out
    );
    assert!(
        reload.ends_with(
            "note: the rebuild dropped repl#2 and the 1 input after it\nreloaded: 1 input replayed\n10 : i32\n"
        ),
        "{}",
        t.out
    );
}

/// `cli.repl.rebuild.prefix`: after `price` changes what it computes, the
/// input that shows it still checks but writes another value, so it is
/// dropped with the input after it; the input before it is kept.
#[test]
fn reload_drops_from_the_first_input_that_replays_otherwise() {
    let t = reload_after_editing(
        "repl-reload-diverge",
        "fn price(n: i32) -> i32: n * +2\n",
        "fn price(n: i32) -> i32: n * +3\n",
        &[
            "rate := +10",
            "price(+3)",
            "rate + +1",
            ":reload",
            "rate",
            "price(+3)",
        ],
    );
    assert_eq!(
        t.out,
        "6 : i32\n11 : i32\n\
         error: repl#2 does not replay: input 2 called hd:rt stdout with other arguments than before\n\
         note: the rebuild dropped repl#2 and the 1 input after it\n\
         reloaded: 1 input replayed\n\
         10 : i32\n9 : i32\n"
    );
}

/// `cli.repl.rebuild.stopped`: `:reload` skips a stopped input and says
/// so; its change to `kept`, made before the stop, is not restored.
#[test]
fn reload_skips_and_reports_a_stopped_input() {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::time::Duration;
    // 1 once the loop input is sent, 2 once `:reload` is read.
    let phase = Arc::new(AtomicU8::new(0));
    let mut script = Script::of(&[
        "let kept = 7",
        "while true:",
        "    kept = 8",
        "",
        ":reload",
        "kept",
    ]);
    let seen = Arc::clone(&phase);
    script.on_line = Some(Box::new(move |line: &str| match line {
        "" => seen.store(1, Ordering::SeqCst),
        ":reload" => {
            seen.store(2, Ordering::SeqCst);
            // Past the stopping thread's last stop, which the next input's
            // start clears.
            std::thread::sleep(Duration::from_millis(300));
        }
        _ => {}
    }));
    let dir = Scratch::new(SCRATCH, "repl-reload-stop");
    let mut stopping = None;
    let t = session_in(&dir, script, |stopper| {
        stopping = Some(std::thread::spawn(move || {
            while phase.load(Ordering::SeqCst) < 2 {
                if phase.load(Ordering::SeqCst) == 1 {
                    stopper.stop();
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }));
    });
    stopping
        .expect("started")
        .join()
        .expect("the stopping thread");
    assert_eq!(
        t.out,
        "stopped\n\
         note: repl#2 was stopped, so the next rebuild skips it; its changes are not restored\n\
         reloaded: 1 input replayed\n\
         7 : usize\n"
    );
}

/// The parser decides when an input is whole.
#[test]
fn completeness_follows_the_parser() {
    for whole in [
        "1 + 2\n",
        "x := [1, 2]\n",
        "if true: println(1)\n",
        "fn f() -> i32: +1\n",
        "s := \"\"\"a\nb\"\"\"\n",
        "s := \"\"\"a\n    b\"\"\"\n",
        "f(\n    +1)\n",
    ] {
        assert!(is_complete(whole), "{whole:?}");
    }
    for part in [
        "fn f() -> i32:\n",
        "fn f() -> i32:\n    +1\n",
        "xs := [1,\n",
        "f(\n",
        "s := \"\"\"a\n",
        "1 +\n",
        "match x:\n",
        "f(\n    +1,\n",
        "if x:\n    f(\n        +1)\n",
    ] {
        assert!(!is_complete(part), "{part:?}");
    }
}

/// Tab offers the names of the inputs that joined the session, the package's
/// own names after a reload, and not those of an input that panicked.
#[test]
fn completion_follows_the_session_s_inputs() {
    let dir = package(
        "complete",
        &[("src/lib.hd", "pub fn libname() -> i32: 1\n")],
    );
    let mut script = Script::of(&[
        "fn price(n: i32) -> i32: n",
        "let dropped = panic(\"no\")",
        ":help",
    ]);
    let names = script.names.clone();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let seen_in = Arc::clone(&seen);
    script.on_line = Some(Box::new(move |line| {
        if line == ":help" {
            let mut seen = seen_in.lock().expect("lock");
            for prefix in ["pri", "dro", "lib"] {
                seen.push(super::complete::complete(&names, prefix, prefix.len()).1);
            }
        }
    }));
    session_in(&dir, script, |_| {});
    let seen = seen.lock().expect("lock").clone();
    assert_eq!(seen[0], ["price", "println"]);
    assert!(seen[1].is_empty(), "{:?}", seen[1]);
    assert_eq!(seen[2], ["libname"]);
}

/// The text of input `index` of a long session. Even inputs bind
/// `w{index} := +(1000000 + index)` and odd inputs show `w{index - 1} + 1`,
/// so every input shows the value `1000000 + index`, which marks how far
/// the session got. With `mix`, every tenth input is a function that reads
/// the binding two inputs earlier, and the odd input after a function shows
/// a call of it instead, so declarations and later reads join the session.
fn long_input(index: u32, mix: bool) -> String {
    let mark = 1_000_000 + index;
    if mix && index % 10 == 4 {
        format!("fn g{index}(n: i32) -> i32: n + w{}", index - 2)
    } else if mix && index % 10 == 5 {
        format!("g{}(+1) + w{}", index - 1, index - 3)
    } else if index.is_multiple_of(2) {
        format!("w{index} := +{mark}")
    } else {
        format!("w{} + 1", index - 1)
    }
}

/// The process's resident memory in KiB, by `ps`.
fn resident_kib() -> u64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse()
        .expect("ps prints the RSS in KiB")
}

/// Runs 1,000 inputs through the session API the REPL uses
/// (`build_session` over the kept inputs, then `run`, which replays them),
/// and prints one line every 50 inputs: the process RSS, the build and run
/// time of the input just done, the mean of the 50 before it, and the
/// journal size. `bench/metrics`'s `long-session` runs this test and reads
/// the lines.
fn long_session(mix: bool) {
    const INPUTS: u32 = 1000;
    const STEP: u32 = 50;
    let dir = Scratch::new(SCRATCH, &format!("repl-long-{}", unique()));
    let program = disk::load_session(&dir).expect("the session's scope");
    let config = EngineConfig::pipeline(super::PIPELINE.level);
    let stack = hd_run_wasmtime::thread_stack(&config);
    let thread = std::thread::Builder::new()
        .stack_size(stack)
        .spawn(move || {
            let sink = Buffer::default();
            let io = HostIo {
                out: Box::new(sink.clone()),
                err: Box::new(sink.clone()),
                input: None,
            };
            let setup = hd_run::HostSetup {
                providers: Vec::new(),
                grants: hd_run::Grants::default(),
                limits: hd_run::Limits::default(),
                args: Vec::new(),
                program: String::new(),
            };
            let session = hd_run_wasmtime::WasmtimeEngine::new(config)
                .expect("engine")
                .session(&setup, io)
                .expect("session");
            let mut repl = super::Repl::new(
                session,
                dir.to_path_buf(),
                program,
                super::complete::Names::default(),
            );
            println!("LONG-SESSION-START rss_kib={}", resident_kib());
            let (mut build_sum, mut run_sum) = (0.0f64, 0.0f64);
            for index in 0..INPUTS {
                let text = long_input(index, mix);
                repl.next += 1;
                let file = hd_project::repl_file(repl.next);
                repl.program.sources.add_synthetic(&file, &text);
                let start = Instant::now();
                let (wasm, diags) = super::build_session(
                    &repl.program,
                    &repl.store,
                    &[],
                    repl.next,
                    super::PIPELINE.target,
                );
                let build = start.elapsed().as_secs_f64();
                let wasm = wasm.unwrap_or_else(|| panic!("input {index} {text:?}: {diags:?}"));
                let mut sources = super::sources(&repl.inputs);
                sources.push((repl.next, &text));
                let start = Instant::now();
                let ran = repl.session.run(&wasm, &sources, Some(repl.next));
                let run = start.elapsed().as_secs_f64();
                assert!(
                    matches!(ran, Ok(super::Ran { live: Some((Outcome::Exit(0), _)), .. })),
                    "input {index}"
                );
                repl.inputs.insert(
                    repl.next,
                    super::Kept {
                        text: text.clone(),
                        joined: true,
                    },
                );
                build_sum += build;
                run_sum += run;
                if (index + 1) % STEP == 0 {
                    println!(
                        "LONG-SESSION inputs={} rss_kib={} build_us={:.0} run_us={:.0} mean_build_us={:.0} mean_run_us={:.0} journal={}",
                        index + 1,
                        resident_kib(),
                        build * 1e6,
                        run * 1e6,
                        build_sum / f64::from(STEP) * 1e6,
                        run_sum / f64::from(STEP) * 1e6,
                        repl.session.journal_len(),
                    );
                    (build_sum, run_sum) = (0.0, 0.0);
                }
            }
        })
        .expect("the session thread");
    thread.join().expect("the session ran");
}

/// The metric's inputs: bindings and reads, each showing `1000000 + index`.
#[test]
#[ignore = "a measurement; bench/metrics long-session runs it"]
fn long_session_plain() {
    long_session(false);
}

/// Declarations and bindings read later mixed in.
#[test]
#[ignore = "a measurement; bench/metrics long-session runs it"]
fn long_session_mixed() {
    long_session(true);
}

/// Guest input that resembles a bridge frame remains guest data.
#[test]
fn node_session_keeps_guest_stdin_separate_from_control() {
    let dir = Scratch::new(SCRATCH, "node-session-input");
    let script = Script::of(&[
        "use std.console.read_line",
        "first := read_line!().expect(\"read\").unwrap_or(\"eof\")",
        "first",
        "second := read_line!().expect(\"read\").unwrap_or(\"eof\")",
        "second",
        "+7",
    ]);
    let t = session_on(
        crate::engine::EngineKind::Node,
        &dir,
        script,
        |_| {},
        None,
        Bounds::default(),
        Some("out\t\"not a frame\"\n{\"op\":\"close\"}\n"),
    );
    assert_eq!(
        t.out,
        "\"out\\t\\\"not a frame\\\"\" : string\n\"{\\\"op\\\":\\\"close\\\"}\" : string\n7 : i32\n"
    );
}

/// Every Node input and an explicit rebuild use journaled host results.
#[test]
fn node_session_rebuilds_without_repeating_host_calls() {
    let dir = Scratch::new(SCRATCH, "node-session-replay");
    let script = Script::of(&[
        "use std.time.now",
        "t := now().unix_milliseconds()",
        "println(\"once\")",
        "t",
        ":reload",
        "t",
    ]);
    let t = session_on(
        crate::engine::EngineKind::Node,
        &dir,
        script,
        |_| {},
        None,
        Bounds::default(),
        None,
    );
    let lines: Vec<_> = t.out.lines().collect();
    assert_eq!(lines.len(), 4, "{}", t.out);
    assert_eq!(lines[0], "once");
    assert!(lines[1].ends_with(" : i64"), "{}", t.out);
    assert_eq!(lines[2], "reloaded: 4 inputs replayed");
    assert_eq!(lines[3], lines[1]);
}
