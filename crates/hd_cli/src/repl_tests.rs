//! The REPL loop driven by a script of lines in place of a terminal, one
//! test per `cli.repl.*` rule the CLI meets. Each test runs a whole
//! session: the session thread, the prelude, and each input built as
//! `repl#N` and run on wasmtime.

use std::collections::VecDeque;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};

use hd_run_wasmtime::HostIo;
use hd_testkit::Scratch;

use super::{Line, Lines, Streams, is_complete, run};
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
}

/// What a session wrote: its transcript, the guest's standard output and
/// error with the REPL's diagnostics in one stream, as a terminal shows
/// them. The value lines go to standard error, as `dbg` lines do.
struct Transcript {
    out: String,
    script: Script,
}

/// Runs a session in `dir`, a package or not, over `script`. `on_start`
/// gets the session's stopper, as the Ctrl-C handler does.
fn session_in(
    dir: &Path,
    script: Script,
    on_start: impl FnOnce(hd_run_wasmtime::Stopper),
) -> Transcript {
    session_with(dir, script, on_start, None)
}

/// `session_in`, with the guest's standard output in `stdout` when given.
fn session_with(
    dir: &Path,
    mut script: Script,
    on_start: impl FnOnce(hd_run_wasmtime::Stopper),
    stdout: Option<&Buffer>,
) -> Transcript {
    let err = Buffer::default();
    let out = stdout.cloned().unwrap_or_else(|| err.clone());
    let streams = Streams {
        io: HostIo {
            out: Box::new(out.clone()),
            err: Box::new(err.clone()),
            input: None,
        },
        diags: Box::new(err.clone()),
    };
    let program = disk::load_session(dir).expect("the session's scope");
    run(&mut script, program, streams, on_start).expect("the session ends");
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

/// `cli.repl.value`: an expression input shows its value as `dbg` prints
/// it, then ` : ` and its type.
#[test]
fn an_expression_shows_its_value_and_type() {
    let t = session(&["[3, 6]", "1 + 2"]);
    assert_eq!(t.out, "[3, 6] : List[usize]\n3 : usize\n");
}

/// `cli.dbg.repl`: an input's `dbg` lines print before its value line.
/// The guest's standard output stays its own stream.
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
    let t = session_with(&dir, script, |_| {}, Some(&stdout));
    assert_eq!(stdout.text(), "out\n");
    assert_eq!(t.out, "repl#2:2:5: x = 5\n6 : i32\n");
}

/// `cli.repl.input-types`: each input is checked on its own; `x := 21`
/// is a `usize`, and a later input does not change it.
#[test]
fn each_input_keeps_its_own_literal_types() {
    let t = session(&["x := 21", "y := -21", "x * 2", "y"]);
    assert_eq!(t.out, "42 : usize\n-21 : i32\n");
}

/// `cli.repl.declarations.bindings`: a declaration reads an earlier
/// input's binding; the spec's `rate` example.
#[test]
fn a_declaration_reads_an_earlier_binding() {
    let t = session(&[
        "rate := +3",
        "fn price(n: i32) -> i32: n * rate",
        "fn total() -> i32: price(+2)",
        "total()",
    ]);
    assert_eq!(t.out, "6 : i32\n");
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

/// A session is one scope, as a module is: a later input constructs an
/// earlier input's type and reads its private fields.
#[test]
fn a_later_input_uses_an_earlier_type_s_private_fields() {
    let t = session(&[
        "data Point:",
        "    x: i32",
        "    y: i32",
        "",
        "p := Point { x: 12, y: 23 }",
        "p",
        "p.x + p.y",
    ]);
    assert_eq!(t.out, "Point { x: 12, y: 23 } : Point\n35 : i32\n");
}

/// `cli.repl.redeclare.type`: nor a type an earlier input declared.
#[test]
fn a_type_is_not_declared_again() {
    let t = session(&[
        "data Point:",
        "    x: i32",
        "",
        "data Point:",
        "    y: i32",
        "",
    ]);
    assert!(
        t.out.starts_with("error: repl#2:1:6: duplicate-type: "),
        "{}",
        t.out
    );
}

/// `cli.repl.redefine.impl`: an implementation that overlaps one of an
/// earlier input or of `src/lib.hd` is `overlapping-impl`, as in one
/// module, and the first one stays.
#[test]
fn a_second_impl_is_an_error() {
    let named = "trait Named:\n    fn name(self) -> string\n\ndata Tag:\n    pub x: i32\n\nimpl Named for Tag:\n    fn name(self) -> string: \"lib\"\n";
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
        "data Tag:",
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

/// A session is one module for implementation placement: a later input
/// implements a trait and adds methods for an earlier input's type, and
/// the input after calls them on a value.
#[test]
fn a_later_input_implements_for_an_earlier_type() {
    let t = session(&[
        "data Point:",
        "    pub x: i32",
        "    pub y: i32",
        "",
        "impl Display for Point:",
        "    fn to_string(self) -> string: \"(${self.x}, ${self.y})\"",
        "",
        "impl Point:",
        "    fn sum(self) -> i32: self.x + self.y",
        "",
        "p := Point { x: +12, y: +23 }",
        "\"${p}\"",
        "p.sum()",
        "p.to_string()",
    ]);
    assert_eq!(
        t.out,
        "\"(12, 23)\" : string\n35 : i32\n\"(12, 23)\" : string\n"
    );
}

/// A later input implements `src/lib.hd`'s trait for an earlier input's
/// type, and a generic function of the library finds that implementation.
#[test]
fn a_library_function_finds_an_input_s_implementation() {
    let lib = "trait Named:\n    fn name(self) -> string\n\nfn greet[T < Named](x: T) -> string:\n    \"hi ${x.name()}\"\n";
    let dir = package("repl-impl-lib", &[("src/lib.hd", lib)]);
    let script = Script::of(&[
        "data Cat:",
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
        "let log: mut List[string] = []",
        "fn record_then_read(entries: mut List[string]) -> string:",
        "    entries.push(\"started\")",
        "    entries[5]",
        "",
        "line := record_then_read(log)",
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
    assert_eq!(t.out, "stopped\n8 : usize\n");
}

/// `cli.repl.cancel`: Ctrl-C while an input waits on the host cancels it
/// at once; its `defer` runs, it ends as a panicking input does, and the
/// next input sees what the `defer` changed.
#[test]
fn a_waiting_input_is_cancelled_and_the_session_goes_on() {
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::time::{Duration, Instant};
    // 1 once the waiting input is sent, 2 once the next input is read.
    let phase = Arc::new(AtomicU8::new(0));
    let mut script = Script::of(&[
        "use std.time.{sleep, Duration}",
        "let log: mut List[string] = []",
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
    assert_eq!(t.out, "cancelled\n[\"cleanup\"] : List[string]\n");
}

/// `cli.repl.host.once`: an input's host calls happen once, when it runs;
/// a later input that calls an earlier one's function repeats none.
#[test]
fn host_calls_happen_once() {
    let t = session(&[
        "fn twice(x: i32) -> i32: x * +2",
        "println(\"one\")",
        "twice(+21)",
    ]);
    assert_eq!(t.out, "one\n42 : i32\n");
}

/// `cli.repl.host.default-profile` and `.args`: an input may call the
/// default profile's traits; `Args` gives an empty program name and no
/// arguments.
#[test]
fn args_are_empty() {
    let t = session(&[
        "use std.host.{args, Args}",
        "fn name() -> string $ Args: $.use(Args).program()",
        "name()",
        "args()",
    ]);
    assert_eq!(t.out, "\"\" : string\n[] : List[string]\n");
}

/// `cli.repl.host.cwd`: relative paths resolve from the working directory
/// of `hd`, which for the tests is this crate's directory.
#[test]
fn relative_paths_resolve_from_the_working_directory() {
    let t = session(&[
        "use std.fs.{read_text}",
        "use std.path.{Path}",
        "read_text!(Path(\"Cargo.toml\")).is_ok()",
    ]);
    assert_eq!(t.out, "true : bool\n");
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

/// `cli.repl.package.dependencies`: the session may use the package's
/// dependencies and its dev dependencies.
#[test]
fn a_package_session_uses_its_dependencies() {
    let dir = Scratch::new(SCRATCH, "repl-deps");
    write_files(
        &dir,
        &[
            (
                "shop/hd.toml",
                "[package]\nname = \"shop\"\n\n[dependencies]\nbilling = { path = \"../billing\" }\n\n[dev-dependencies]\nfixtures = { path = \"../fixtures\" }\n",
            ),
            ("shop/src/lib.hd", "fn bonus() -> i32: +7\n"),
            ("billing/hd.toml", "[package]\nname = \"billing\"\n"),
            ("billing/src/lib.hd", "pub fn tax(x: i32) -> i32: x + +1\n"),
            ("fixtures/hd.toml", "[package]\nname = \"fixtures\"\n"),
            ("fixtures/src/lib.hd", "pub fn sample() -> i32: +10\n"),
        ],
    );
    let script = Script::of(&[
        "use dep.billing.{tax}",
        "use dep.fixtures.{sample}",
        "tax(sample())",
    ]);
    let t = session_in(&dir.join("shop"), script, |_| {});
    assert_eq!(t.out, "11 : i32\n");
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

/// `:quit` ends the session; later lines are not read.
#[test]
fn quit_ends_the_session() {
    let t = session(&["+1", ":quit", "+2"]);
    assert_eq!(t.out, "1 : i32\n");
    assert_eq!(t.script.prompts, ["hd> ", "hd> "]);
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
