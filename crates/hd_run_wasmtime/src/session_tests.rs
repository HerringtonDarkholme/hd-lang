//! The wasmtime session (repl-plan.md job S1): inputs the driver builds
//! with `Goal::ReplInput` (job C4) run one after another in one store. A
//! later input sees an earlier input's declarations and top-level bindings
//! by name (job C5). An input that is one expression shows its value, and
//! an input's top level may make bang calls (job C6). A session that
//! starts with its prelude (job C8) runs it first, as input 0.

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
        let built = self.build(text);
        self.run(built)
    }

    /// Builds `text` as the next input, then runs it while another thread
    /// stops it `ms` milliseconds after it starts.
    fn input_stopped_after(&mut self, text: &str, stopper: &crate::Stopper, ms: u64) -> Outcome {
        let built = self.build(text);
        let stopper = stopper.clone();
        let handle = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(ms));
            stopper.stop();
        });
        let outcome = self.run(built);
        handle.join().expect("the stopping thread");
        outcome
    }

    /// Builds `text` as the next input, then runs it while another thread
    /// stops it once it waits on the host. A stop that came earlier, while
    /// Wasm runs, would stop the input rather than cancel it.
    fn input_stopped_waiting(&mut self, text: &str, stopper: &crate::Stopper) -> Outcome {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        let built = self.build(text);
        let stopper = stopper.clone();
        let ended = Arc::new(AtomicBool::new(false));
        let seen = Arc::clone(&ended);
        let handle = std::thread::spawn(move || {
            while !seen.load(Ordering::SeqCst) {
                if stopper.waiting() {
                    stopper.stop();
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        });
        let outcome = self.run(built);
        ended.store(true, Ordering::SeqCst);
        handle.join().expect("the stopping thread");
        outcome
    }

    /// Builds the next input over the earlier ones: its file, its module
    /// and what it adds to the session.
    fn build(&mut self, text: &str) -> Built {
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
        let added = out.session_added.expect("what the input adds");
        Built {
            number: n,
            source: text.to_owned(),
            file,
            wasm,
            added,
        }
    }

    /// Runs a built input. One that ends with status 0 joins the session.
    fn run(&mut self, built: Built) -> Outcome {
        let outcome = self
            .session
            .run_input(built.number, &built.source, &built.wasm)
            .expect("the input runs");
        if outcome == Outcome::Exit(0) {
            self.built.add(built.added);
        } else {
            self.files.retain(|p| p != built.file);
        }
        outcome
    }
}

/// An input built for a session: its number, text, file, module and what
/// it adds to the session.
struct Built {
    number: u32,
    source: String,
    file: String,
    wasm: Vec<u8>,
    added: hd_driver::Session,
}

fn repl(session: &mut WasmtimeSession) -> Repl<'_> {
    Repl {
        session,
        built: hd_driver::Session::start(),
        files: hd_project::MemorySources::default(),
        next: 0,
    }
}

/// A session that starts as the REPL does: its prelude runs first and
/// owns the memory and runtime globals; inputs import what it holds.
fn repl_with_prelude(session: &mut WasmtimeSession) -> Repl<'_> {
    let store = hd_cache::MemoryStore::default();
    let files = hd_project::MemorySources::default();
    let host = hd_driver::Host {
        render_tir: &[],
        sources: &files,
        store: &store,
        clock: &hd_driver::NoClock,
        executor: hd_driver::Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let out = hd_driver::build_repl_prelude(&host, hd_driver::BuildProfile::Checked);
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("the prelude");
    assert_eq!(session.run_input(0, "", &wasm), Ok(Outcome::Exit(0)));
    let mut built = hd_driver::Session::start();
    built.add(out.session_added.expect("what the prelude adds"));
    Repl {
        session,
        built,
        files,
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

/// Over the prelude, inputs show values of the types it holds and of
/// others, and the session still resets the runtime globals the prelude
/// owns after a panic inside `defer` (`cli.repl.panic`).
#[test]
fn inputs_run_over_the_prelude() {
    let panics = "\
fn cleanup(xs: List[i32]) -> void:
    defer:
        _ := xs[3]
    pass

cleanup([+1])
";
    with_session(|session, io| {
        let mut repl = repl_with_prelude(session);
        assert_eq!(text(&io.out), "");
        assert_eq!(text(&io.err), "");
        for (input, shown) in [
            ("1 + 2\n", "3 : usize\n"),
            ("[3, 6]\n", "[3, 6] : List[usize]\n"),
            ("\"tea\"\n", "\"tea\" : string\n"),
            ("[+1, -2]\n", "[1, -2] : List[i32]\n"),
            ("Option.Some(1.5)\n", "Option.Some(1.5) : f64?\n"),
        ] {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
            assert_eq!(text(&io.err), shown);
        }
        assert_eq!(repl.input(panics), Outcome::Exit(3));
        assert_eq!(
            text(&io.err),
            "panic: index-out-of-bounds: list index out of bounds\n"
        );
        assert_eq!(repl.input("println(\"after\")\n"), Outcome::Exit(0));
        assert_eq!(text(&io.out), "after\n");
        assert_eq!(text(&io.err), "");
    });
}

/// `cli.repl.stop`: a loop input stopped from another thread ends as a
/// panicking input does; earlier values stay and the next input runs. It
/// computes, so it is stopped, not cancelled, though its module exports
/// `hd.cancel`.
#[test]
fn a_stopped_loop_input_ends_and_the_session_goes_on() {
    with_session(|session, io| {
        let stopper = session.stopper();
        let mut repl = repl(session);
        assert_eq!(repl.input("let kept = 7\n"), Outcome::Exit(0));
        // No input runs: harmless, and it does not stop the next input.
        stopper.stop();
        assert_eq!(repl.input("println(kept)\n"), Outcome::Exit(0));
        assert_eq!(text(&io.out), "7\n");
        assert_eq!(
            repl.input_stopped_after("while true:\n    pass\n", &stopper, 300),
            Outcome::Exit(3)
        );
        assert_eq!(text(&io.err), "stopped\n");
        assert_eq!(repl.input("println(kept + 1)\n"), Outcome::Exit(0));
        assert_eq!(text(&io.out), "8\n");
        assert_eq!(text(&io.err), "");
    });
}

/// `cli.repl.cancel`: an input that waits on the host is cancelled at
/// once by `req.cancel.steps`, so its `defer` runs; it ends as a
/// panicking input does: it adds no declaration, its `defer`'s change to
/// an earlier binding stays (`cli.repl.panic.mutations`), and the next
/// input runs.
#[test]
fn a_waiting_input_is_cancelled_and_its_defer_runs() {
    let nap = "\
fn nap!() -> void:
    defer:
        log.push(\"cleanup\")
    sleep!(Duration.seconds(+30))

nap!()
";
    with_session(|session, io| {
        let stopper = session.stopper();
        let mut repl = repl(session);
        for input in [
            "use std.time.{sleep, Duration}\n",
            "let log: mut List[string] = []\n",
        ] {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let started = std::time::Instant::now();
        assert_eq!(repl.input_stopped_waiting(nap, &stopper), Outcome::Exit(3));
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "the cancel waited for the timer: {:?}",
            started.elapsed()
        );
        assert_eq!(text(&io.err), "cancelled\n");
        // `nap` was not declared (`cli.repl.panic.declarations`).
        assert_eq!(
            repl.input("fn nap() -> string:\n    log.join(\", \")\n"),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(repl.input("println(nap())\n"), Outcome::Exit(0));
        assert_eq!(text(&io.out), "cleanup\n");
        assert_eq!(text(&io.err), "");
    });
}

/// A cancelled input's timer goes with it: it does not wake a later
/// input's own `sleep!`, which still lasts its whole duration.
#[test]
fn a_cancelled_input_leaves_no_timer_behind() {
    with_session(|session, io| {
        let stopper = session.stopper();
        let mut repl = repl(session);
        assert_eq!(
            repl.input("use std.time.{sleep, Duration}\n"),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        let cancelled = std::time::Instant::now();
        assert_eq!(
            repl.input_stopped_waiting("sleep!(Duration.milliseconds(+1500))\n", &stopper),
            Outcome::Exit(3)
        );
        assert_eq!(text(&io.err), "cancelled\n");
        assert!(!repl.session.pending(), "the cancelled timer is pending");
        // The cancelled timer would be due during this input's wait.
        let started = std::time::Instant::now();
        assert_eq!(
            repl.input("sleep!(Duration.milliseconds(+2000))\nprintln(\"woke\")\n"),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        let slept = started.elapsed();
        assert!(
            started.duration_since(cancelled) < std::time::Duration::from_millis(1500),
            "the second input started after the first's timer was due"
        );
        assert!(
            slept >= std::time::Duration::from_secs(2),
            "woken early: {slept:?}"
        );
        assert_eq!(text(&io.out), "woke\n");
        assert_eq!(text(&io.err), "");
    });
}

/// An input that panics while its timer is pending leaves no host
/// operation behind, which could wake a later input or keep its wait from
/// being a deadlock.
#[test]
fn a_panicking_input_leaves_no_operation_pending() {
    let input = "\
use std.task.{all}
use std.time.{sleep, Duration}

fn boom!() -> void:
    _ := [+1][3]

all!(sleep(Duration.seconds(+30)), boom())
";
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(repl.input(input), Outcome::Exit(3));
        assert_eq!(
            text(&io.err),
            "panic: index-out-of-bounds: list index out of bounds\n"
        );
        assert!(!repl.session.pending(), "the timer is still pending");
    });
}

/// The journal's rows, decoded, as kind and method: what a test compares.
fn journal_shape(session: &WasmtimeSession) -> Vec<String> {
    use hd_run::journal::Event;
    let (rows, valid) = hd_run::journal::read(session.journal().expect("recording"));
    assert_eq!(valid, session.journal().expect("recording").len());
    hd_run::journal::decode(&rows)
        .expect("the rows decode")
        .iter()
        .map(|event| match event {
            Event::Header { .. } => "header".to_owned(),
            Event::Input { number, .. } => format!("input {number}"),
            Event::Call { module, name, .. } => format!("call {module} {name}"),
            Event::Finish { .. } => "finish".to_owned(),
            Event::Wake { .. } => "wake".to_owned(),
            Event::Block { .. } => "block".to_owned(),
            Event::Abort { .. } => "abort".to_owned(),
            Event::Cancel { .. } => "cancel".to_owned(),
            Event::End(end) => format!("end {end:?}"),
        })
        .collect()
}

/// The session's journal holds a row per host call of each input, between
/// the input's `Input` and `End` rows, in the order they happened: its
/// clock reading, the handle a sleep waits on, woken and then finished,
/// a line written and the panic report, both by digest.
#[test]
fn a_session_journal_records_each_input_s_host_calls_in_order() {
    use hd_run::journal::{Event, Request, Value};
    let inputs = [
        "use std.time.{now, sleep, Duration}\n",
        "a := now()\n",
        "sleep!(Duration.milliseconds(+1))\n",
        "println(\"hi\")\n",
        "x := [+1][3]\n",
    ];
    with_session(|session, io| {
        let mut repl = repl(session);
        let before = std::time::SystemTime::now();
        for input in &inputs[..4] {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(repl.input(inputs[4]), Outcome::Exit(3));
        assert_eq!(text(&io.out), "hi\n");
        assert_eq!(
            journal_shape(repl.session),
            [
                "header",
                "input 1",
                "end Exit(0)",
                "input 2",
                "call hd:Clock now",
                "end Exit(0)",
                "input 3",
                "call hd:Clock sleep.start",
                "wake",
                "finish",
                "end Exit(0)",
                "input 4",
                "call hd:Console write_line.start",
                "block",
                "finish",
                "end Exit(0)",
                "input 5",
                "call hd:rt stderr",
                "end Trapped",
            ]
        );
        let (rows, _) = hd_run::journal::read(repl.session.journal().expect("recording"));
        let events = hd_run::journal::decode(&rows).expect("the rows decode");
        let Event::Input { number, source, .. } = &events[3] else {
            panic!("{:?}", events[3]);
        };
        assert_eq!(
            (*number, *source),
            (2, hd_base::hash128(inputs[1].as_bytes()).0)
        );
        let since = |t: std::time::SystemTime| {
            let ms = t
                .duration_since(std::time::UNIX_EPOCH)
                .expect("after 1970")
                .as_millis();
            i64::try_from(ms).expect("ms")
        };
        let Event::Call {
            request,
            value: Value::Int(ms),
            result: None,
            ..
        } = &events[4]
        else {
            panic!("{:?}", events[4]);
        };
        assert_eq!(request, &Request::Args(Vec::new()));
        assert!((since(before)..=since(std::time::SystemTime::now())).contains(ms));
        let Event::Call {
            value: Value::Pair(1, handle),
            ..
        } = &events[7]
        else {
            panic!("{:?}", events[7]);
        };
        let handle = u32::try_from(*handle).expect("a handle");
        assert!(matches!(&events[8], Event::Wake { handles, .. } if handles == &[handle]));
        assert!(matches!(&events[9], Event::Finish { handle: h, .. } if *h == handle));
        // `println("hi")` writes `hi`: its length then its bytes, 3 bytes.
        let Event::Call { request, .. } = &events[12] else {
            panic!("{:?}", events[12]);
        };
        assert_eq!(request, &Request::Args(vec![2, b'h', b'i']));
        assert!(matches!(
            &events[17],
            Event::Call {
                request: Request::Digest { .. },
                ..
            }
        ));
    });
}

/// A clock reading costs one `Call` row: kind, index, length, method,
/// request length, reply tag, the value, and the checksum. `now` is
/// milliseconds since 1970, six bytes of LEB128; `monotonic` milliseconds
/// since the host started, one to three.
#[test]
fn a_clock_reading_costs_one_small_row() {
    with_session(|session, io| {
        let mut repl = repl(session);
        let inputs = [
            "use std.time.{now, Clock, Instant}\n",
            "a := now()\nb := now()\n",
            "fn mono() -> Instant $ Clock:\n    $.use(Clock).monotonic()\n",
            "c := mono()\nd := mono()\n",
        ];
        for input in inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let (rows, _) = hd_run::journal::read(repl.session.journal().expect("recording"));
        let framed = |row: &hd_run::journal::Row| {
            let mut bytes = Vec::new();
            hd_run::journal::append(&mut bytes, row);
            bytes.len()
        };
        let calls: Vec<usize> = rows
            .iter()
            .filter(|r| r.kind == hd_run::journal::RowKind::Call)
            .map(framed)
            .collect();
        println!("clock rows, the first naming its method: {calls:?}");
        // The second of each pair names no method.
        assert_eq!(calls.len(), 4, "{calls:?}");
        assert_eq!(calls[1], 1 + 1 + 1 + (1 + 1 + 1 + 6) + 4);
        assert!(
            (1 + 1 + 1 + (1 + 1 + 1 + 1) + 4..=12).contains(&calls[3]),
            "{calls:?}"
        );
    });
}

/// `hd run` on wasmtime records no journal: only a session does.
#[test]
fn a_program_run_records_no_journal() {
    use hd_run::{Engine, Instance};
    let main = "use std.time.{now, Clock}\n\npub fn main() -> void $ Console + Clock:\n    println(now().unix_milliseconds() > 0)\n";
    let mut src = hd_project::MemorySources::default();
    src.insert("main.hd", main);
    let store = hd_cache::MemoryStore::default();
    let host = hd_driver::Host {
        render_tir: &[],
        sources: &src,
        store: &store,
        clock: &hd_driver::NoClock,
        executor: hd_driver::Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let goal = hd_driver::Goal::Program {
        entry: "main".into(),
    };
    let out = hd_driver::build_packages(&host, "app", &hd_driver::Packages::default(), &goal);
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("a module");
    let config = EngineConfig::default();
    std::thread::scope(|scope| {
        std::thread::Builder::new()
            .stack_size(thread_stack(&config))
            .spawn_scoped(scope, move || {
                let engine = WasmtimeEngine::new(config).expect("engine");
                let module = engine
                    .load(&wasm, &hd_cache::MemoryStore::default())
                    .expect("loads");
                let out = Shared::default();
                let io = HostIo {
                    out: Box::new(out.clone()),
                    err: Box::new(std::io::sink()),
                    input: None,
                };
                let setup = HostSetup {
                    providers: Vec::new(),
                    grants: hd_run::Grants::default(),
                    limits: hd_run::Limits::default(),
                    args: Vec::new(),
                    program: "main".into(),
                };
                let mut run = engine.instantiate_with(&module, &setup, io).expect("runs");
                assert_eq!(run.init(), Outcome::Exit(0));
                assert_eq!(hd_run::drive(&mut run, usize::MAX), Outcome::Exit(0));
                assert_eq!(text(&out), "true\n");
                assert!(run.store.data().journal.is_none());
            })
            .expect("spawn")
            .join()
            .expect("the run thread");
    });
}

/// The cost of recording on a host-call-heavy input: 10,000 clock
/// readings, with the session's journal and without it. Run it with
/// `cargo test -p hd_run_wasmtime clock_loop -- --ignored --nocapture`.
#[test]
#[ignore = "a measurement, not a check"]
fn clock_loop_recording_overhead() {
    use std::time::{Duration, Instant};
    let reads = "\
fn reads() -> i64 $ Clock:
    let sum: i64 = 0
    for _ in 0..10000:
        sum = sum + now().unix_milliseconds()
    sum
";
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(repl.input("use std.time.{now, Clock}\n"), Outcome::Exit(0));
        assert_eq!(repl.input(reads), Outcome::Exit(0), "{}", text(&io.err));
        let (mut on, mut off) = (Vec::new(), Vec::new());
        for round in 0..40 {
            let recording = round % 2 == 0;
            repl.session.record(recording);
            let built = repl.build("_ := reads()\n");
            let start = Instant::now();
            assert_eq!(repl.run(built), Outcome::Exit(0), "{}", text(&io.err));
            let took = start.elapsed();
            if recording {
                let bytes = repl.session.journal().expect("recording").len();
                assert!(bytes > 10_000 * 12, "{bytes}");
                on.push(took);
            } else {
                off.push(took);
            }
        }
        let median = |v: &mut Vec<Duration>| {
            v.sort();
            v[v.len() / 2]
        };
        let (on, off) = (median(&mut on), median(&mut off));
        println!(
            "10,000 clock reads per input, median of 20: recording {on:?}, not recording {off:?}, {:.0} ns per read",
            (on.as_secs_f64() - off.as_secs_f64()) * 1e9 / 10_000.0
        );
    });
}
