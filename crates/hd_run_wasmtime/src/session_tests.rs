//! The wasmtime session (repl-plan.md job S1): inputs the driver builds
//! with `Goal::ReplInput` (job C4) run one after another in one store. A
//! later input sees an earlier input's declarations and top-level bindings
//! by name (job C5). An input that is one expression shows its value, and
//! an input's top level may make bang calls (job C6).

use hd_run::{HostSetup, Outcome};

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
/// the `repl#N` files of the next input's build, and the session they run
/// in.
struct Repl<'s> {
    session: &'s mut WasmtimeSession,
    built: hd_driver::Session,
    files: hd_project::MemorySources,
    next: u32,
}

impl Repl<'_> {
    /// Builds the next input over the earlier ones and runs it. An input
    /// that ends with status 0 joins the session; one that panics leaves
    /// it as it was (`cli.repl.panic.binding`).
    fn input(&mut self, text: &str) -> Outcome {
        self.next += 1;
        let n = self.next;
        let file = hd_project::repl_file(n);
        self.files.insert(&file, text);
        let store = hd_cache::MemoryStore::default();
        let host = hd_driver::Host {
            render_tir: &[],
            sources: &self.files,
            store: &store,
            clock: &hd_driver::NoClock,
            executor: hd_driver::Executor::Serial(hd_sched::SerialOrder::Priority),
        };
        let packages = hd_driver::Packages {
            session: Some(&self.built),
            ..hd_driver::Packages::default()
        };
        let goal = hd_driver::Goal::ReplInput { input: n };
        let out = hd_driver::build_packages(&host, "app", &packages, &goal);
        assert!(out.diags.is_empty(), "{file}: {}", out.render());
        let wasm = out.wasm.expect("a module");
        let outcome = self.session.run_input(&wasm).expect("the input runs");
        if outcome == Outcome::Exit(0) {
            self.built
                .add(out.session_added.expect("what the input adds"));
        } else {
            self.files.retain(|p| p != file);
        }
        outcome
    }
}

fn repl(session: &mut WasmtimeSession) -> Repl<'_> {
    Repl {
        session,
        built: hd_driver::Session::start(),
        files: hd_project::MemorySources::default(),
        next: 0,
    }
}

fn text(stream: &Shared) -> String {
    String::from_utf8(stream.take()).expect("UTF-8")
}

/// `cli.repl.panic.mutations`, the spec's `log` example: input 1 binds
/// the mutable list `log`; input 2 pushes to it and then panics; input 3
/// sees the push.
#[test]
fn a_panicking_input_keeps_its_changes_to_earlier_values() {
    let in1 = "let log: mut List[string] = []\n";
    let in2 = "log.push(\"started\")\nline := log[5]\n";
    let in3 = "println(\"${log.len()}: ${log.join(\", \")}\")\n";
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(repl.input(in1), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(repl.input(in2), Outcome::Exit(3));
        assert_eq!(
            text(&io.err),
            "panic: index-out-of-bounds: list index out of bounds\n"
        );
        assert_eq!(repl.input(in3), Outcome::Exit(0), "{}", text(&io.err));
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
        assert_eq!(repl.input(in1), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), "ready\n");
        assert_eq!(repl.input(in2), Outcome::Exit(3));
        assert_eq!(
            text(&io.err),
            "panic: index-out-of-bounds: list index out of bounds\n"
        );
        assert_eq!(repl.input(in3), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), "after\n");
        assert_eq!(text(&io.err), "");
    });
}

/// `cli.repl.host.once`: an input's host calls happen when it runs, and
/// later inputs, which call a function of an earlier one, repeat none.
#[test]
fn an_input_writes_its_output_once() {
    let in1 = "fn twice(x: i32) -> i32:\n    x * +2\n";
    let in2 = "println(\"one\")\n";
    let in3 = "println(twice(+21))\n";
    let in4 = "println(twice(+1))\n";
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(repl.input(in1), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), "");
        assert_eq!(repl.input(in2), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), "one\n");
        assert_eq!(repl.input(in3), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), "42\n");
        assert_eq!(repl.input(in4), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), "2\n");
        assert_eq!(text(&io.err), "");
    });
}

/// `cli.repl.declarations.bindings`, the spec's Redefinition example
/// without its redeclaration: input 2's function reads input 1's `rate`,
/// and `count`, bound with `let`, is assigned by the input after it and
/// read by the one after that.
#[test]
fn later_inputs_read_and_assign_earlier_bindings() {
    let inputs = [
        "rate := +3\n",
        "fn price(n: i32) -> i32:\n    n * rate\n",
        "println(price(+2))\n",
        "fn doubled() -> i32:\n    price(+2) * +2\n",
        "println(doubled())\n",
        "let count = +1\n",
        "count = count + price(+1)\n",
        "println(count)\n",
    ];
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(text(&io.out), "6\n12\n4\n");
    });
}

/// The `prices` example of live-execution.md §2.2, with std's `Map`
/// stored by index: a function of input 2 reads the map input 1 binds,
/// after input 3 stored into it.
#[test]
fn a_function_sees_a_later_change_to_an_earlier_binding() {
    let inputs = [
        "let prices: mut Map[string, u32] = {}\n",
        "fn price(item: string) -> u32:\n    prices.get(item).unwrap_or(+0)\n",
        "prices[\"tea\"] = +3\n",
        "println(price(\"tea\"))\n",
    ];
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(text(&io.out), "3\n");
    });
}

/// `cli.repl.value`: an input that is one expression shows its value as
/// `dbg` prints it, then ` : ` and its type, on the debug output.
#[test]
fn an_expression_input_shows_its_value_and_type() {
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(repl.input("[3, 6]\n"), Outcome::Exit(0));
        assert_eq!(text(&io.err), "[3, 6] : List[usize]\n");
        assert_eq!(text(&io.out), "");
    });
}

/// `cli.repl.value`: a value whose type implements neither `Display` nor
/// `Debug` shows too, field by field; a binding shows nothing, and a
/// later input shows it by name.
#[test]
fn a_value_without_debug_shows_too() {
    let inputs = [
        "data Point:\n    pub x: i32\n    pub y: i32\n",
        "Point { x: +1, y: -2 }\n",
        "origin := Point { x: +0, y: +0 }\n",
        "origin\n",
    ];
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(
            text(&io.err),
            "Point { x: 1, y: -2 } : Point\nPoint { x: 0, y: 0 } : Point\n"
        );
    });
}

/// An input of statements shows nothing of its own: its output is the
/// output its statements write. A `void` call shows no value either.
#[test]
fn a_statement_input_shows_nothing_extra() {
    let inputs = [
        "let total = +1\n",
        "total = total + +2\n",
        "println(total)\n",
        "for i in [+1, +2]:\n    println(i)\n",
    ];
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(text(&io.out), "3\n1\n2\n");
        assert_eq!(text(&io.err), "");
    });
}

/// `cli.dbg.repl`: the `dbg` lines of an input print before its value
/// line.
#[test]
fn dbg_lines_print_before_the_value_line() {
    let in1 = "fn twice(x: i32) -> i32:\n    dbg(x)\n    x * +2\n";
    let in2 = "twice(+21) + twice(+1)\n";
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(repl.input(in1), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(repl.input(in2), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(
            text(&io.err),
            "repl#1:2:5: x = 21\nrepl#1:2:5: x = 1\n44 : i32\n"
        );
    });
}

/// An input's top level is a driver context, as `main!`'s body is: its
/// bang calls are polled to completion under the default profile.
#[test]
fn an_input_makes_bang_calls_at_top_level() {
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml");
    let inputs = [
        "use std.time.{sleep, Duration}\n".to_owned(),
        "sleep!(Duration.milliseconds(+1))\n".to_owned(),
        "use std.fs.{read_text}\nuse std.path.{Path}\n".to_owned(),
        format!("text := read_text!(Path(\"{manifest}\")).unwrap_or(\"\")\n"),
        "println(text.contains(\"hd_run_wasmtime\"))\n".to_owned(),
        format!("read_text!(Path(\"{manifest}/missing\")).is_ok()\n"),
    ];
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(text(&io.out), "true\n");
        assert_eq!(text(&io.err), "false : bool\n");
    });
}
