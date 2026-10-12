//! The wasmtime session (repl-plan.md job S1): inputs the driver builds
//! as session modules run one after another in one store. The driver's
//! input goal is job C4, so each input here is a package file that later
//! inputs reach through `use pkg.<input>`. A package module's init must be
//! requirement-free, so an input whose top level calls the host is not
//! kept as a file of the later inputs' builds.

use hd_run::{HostSetup, Outcome};

use crate::session_link_tests::session_module;
use crate::{EngineConfig, HostIo, Shared, WasmtimeEngine, WasmtimeSession, thread_stack};

/// A session's standard output and error, read after each input.
struct Streams {
    out: Shared,
    err: Shared,
}

/// Runs `body` with a new session, on a thread with the stack it needs.
fn with_session(body: impl FnOnce(&mut WasmtimeSession, &Streams) + Send) {
    let config = EngineConfig::default();
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(thread_stack(&config))
            .spawn_scoped(scope, move || {
                let engine = WasmtimeEngine::new(config).expect("engine");
                let streams = Streams {
                    out: Shared::default(),
                    err: Shared::default(),
                };
                let io = HostIo {
                    out: Box::new(streams.out.clone()),
                    err: Box::new(streams.err.clone()),
                    input: None,
                };
                let setup = HostSetup {
                    providers: Vec::new(),
                    grants: hd_run::Grants::default(),
                    limits: hd_run::Limits::default(),
                    args: Vec::new(),
                    program: String::new(),
                };
                let mut session = engine.session(&setup, io).expect("session");
                body(&mut session, &streams);
            })
            .expect("spawn")
            .join()
            .expect("the session thread");
    });
}

/// The inputs that ran without a panic, as the driver's session and as
/// package files, and the session they run in.
struct Repl<'s> {
    session: &'s mut WasmtimeSession,
    built: hd_driver::Session,
    files: Vec<(String, String)>,
}

impl Repl<'_> {
    /// Builds input `name` over the earlier inputs and runs it. An input
    /// that ends with status 0 joins the session; one that panics leaves
    /// it as it was (`cli.repl.panic.binding`).
    fn input(&mut self, name: &str, text: &str) -> Outcome {
        self.run(name, text, true)
    }

    /// Like `input`, for an input whose top level calls the host: later
    /// inputs cannot use it.
    fn statement(&mut self, name: &str, text: &str) -> Outcome {
        self.run(name, text, false)
    }

    fn run(&mut self, name: &str, text: &str, keep: bool) -> Outcome {
        let mut files: Vec<(&str, &str)> = self
            .files
            .iter()
            .map(|(p, t)| (p.as_str(), t.as_str()))
            .collect();
        let path = format!("{name}.hd");
        files.push((&path, text));
        let (wasm, added) = session_module(&files, name, &self.built);
        let outcome = self.session.run_input(&wasm).expect("the input runs");
        if outcome == Outcome::Exit(0) {
            self.built.add(added);
            if keep {
                self.files.push((path, text.to_owned()));
            }
        }
        outcome
    }
}

fn repl(session: &mut WasmtimeSession) -> Repl<'_> {
    Repl {
        session,
        built: hd_driver::Session::start(),
        files: Vec::new(),
    }
}

fn text(stream: &Shared) -> String {
    String::from_utf8(stream.take()).expect("UTF-8")
}

/// `cli.repl.panic.mutations`, the spec's `log` example: input 1 binds
/// the mutable list `log`; input 2 pushes to it and then panics; input 3
/// sees the push. Reading `log` by name from a later input is job C5, so
/// input 1 also declares the functions that reach it.
#[test]
fn a_panicking_input_keeps_its_changes_to_earlier_values() {
    let in1 = "\
let log: mut List[string] = []

fn record_then_read(entries: mut List[string]) -> string:
    entries.push(\"started\")
    entries[5]

pub fn record() -> string:
    record_then_read(log)

pub fn entries() -> string:
    \"${log.len()}: ${log.join(\", \")}\"
";
    let in2 = "use pkg.in1.{record}\n\nline := record()\n";
    let in3 = "use pkg.in1.{entries}\n\nprintln(entries())\n";
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(
            repl.input("in1", in1),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(repl.input("in2", in2), Outcome::Exit(3));
        assert_eq!(
            text(&io.err),
            "panic: index-out-of-bounds: list index out of bounds\n"
        );
        assert_eq!(
            repl.statement("in3", in3),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(text(&io.out), "1: started\n");
    });
}

/// `cli.repl.panic`: a panic inside `defer` leaves the forbidden-context
/// counter set; the session resets it, so the next input's `println`
/// works.
#[test]
fn an_input_after_a_panic_inside_defer_prints() {
    let in1 = "println(\"ready\")\n";
    let in2 = "\
fn cleanup(xs: List[i32]) -> void:
    defer:
        _ := xs[3]
    pass

cleanup([+1])
";
    let in3 = "println(\"after\")\n";
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(
            repl.statement("in1", in1),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(text(&io.out), "ready\n");
        assert_eq!(repl.input("in2", in2), Outcome::Exit(3));
        assert_eq!(
            text(&io.err),
            "panic: index-out-of-bounds: list index out of bounds\n"
        );
        assert_eq!(
            repl.statement("in3", in3),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(text(&io.out), "after\n");
        assert_eq!(text(&io.err), "");
    });
}

/// `cli.repl.host.once`: an input's host calls happen when it runs, and
/// later inputs, which call a function of an earlier one, repeat none.
#[test]
fn an_input_writes_its_output_once() {
    let in1 = "pub fn twice(x: i32) -> i32:\n    x * +2\n";
    let in2 = "println(\"one\")\n";
    let in3 = "use pkg.in1.{twice}\n\nprintln(twice(+21))\n";
    let in4 = "use pkg.in1.{twice}\n\nprintln(twice(+1))\n";
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(
            repl.input("in1", in1),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(text(&io.out), "");
        assert_eq!(
            repl.statement("in2", in2),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(text(&io.out), "one\n");
        assert_eq!(
            repl.statement("in3", in3),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(text(&io.out), "42\n");
        assert_eq!(
            repl.statement("in4", in4),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(text(&io.out), "2\n");
        assert_eq!(text(&io.err), "");
    });
}
