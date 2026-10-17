//! The wasmtime session (`f1c55020:future-work/compiler/repl-plan.md` job S1): inputs the driver builds
//! with `Goal::ReplInput` (job C4) run one after another in one store. A
//! later input sees an earlier input's declarations and top-level bindings
//! by name (job C5). An input that is one expression shows its value, and
//! an input's top level may make bang calls (job C6). A session that
//! starts with its prelude (job C8) runs it first, as input 0.

use std::collections::BTreeMap;

use hd_run::{HostSetup, Outcome};

use crate::hd_source::{hd_path, hd_string};
use crate::{EngineConfig, HostIo, Shared, WasmtimeEngine, WasmtimeSession, thread_stack};

#[path = "session_parity_tests.rs"]
mod parity;

/// A session's standard output and error, read after each input.
struct Streams {
    out: Shared,
    err: Shared,
}

/// Runs `body` with a new session, on a thread with the stack it needs.
fn with_session(body: impl FnOnce(&mut WasmtimeSession, &Streams) + Send) {
    with_session_input(None, body);
}

/// `with_session`, with `input` as the session's standard input.
fn with_session_input(
    input: Option<&'static [u8]>,
    body: impl FnOnce(&mut WasmtimeSession, &Streams) + Send,
) {
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
                    input: input.map(|b| Box::new(b) as Box<dyn std::io::BufRead + Send>),
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
    /// The text of each input that ran, for a rebuild.
    texts: BTreeMap<u32, String>,
    /// The build cache every build of the session shares, so a rebuild of
    /// unchanged inputs compiles nothing again.
    store: hd_cache::MemoryStore,
}

/// Builds `file`, the input `number`, over the session `built`.
fn build_input(
    files: &hd_project::MemorySources,
    store: &hd_cache::MemoryStore,
    built: &hd_driver::Session,
    number: u32,
) -> hd_driver::Output {
    build_inputs(files, store, built, None, number)
}

/// Builds the inputs `from` to `number` as one module over the session
/// `built`, or with `from` `None` the input `number` alone.
fn build_inputs(
    files: &hd_project::MemorySources,
    store: &hd_cache::MemoryStore,
    built: &hd_driver::Session,
    from: Option<u32>,
    number: u32,
) -> hd_driver::Output {
    let host = hd_driver::Host {
        render_tir: &[],
        sources: files,
        store,
        clock: &hd_driver::NoClock,
        executor: hd_driver::Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let packages = hd_driver::Packages {
        session: Some(built),
        inputs_from: from,
        ..hd_driver::Packages::default()
    };
    let goal = hd_driver::Goal::ReplInput { input: number };
    hd_driver::build_packages(&host, "app", &packages, &goal)
}

/// Builds the session's prelude.
fn build_prelude(store: &hd_cache::MemoryStore) -> hd_driver::Output {
    let files = hd_project::MemorySources::default();
    let host = hd_driver::Host {
        render_tir: &[],
        sources: &files,
        store,
        clock: &hd_driver::NoClock,
        executor: hd_driver::Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    hd_driver::build_repl_prelude(
        &host,
        hd_driver::BuildProfile::Checked,
        hd_driver::EngineTarget::default(),
    )
}

/// A rebuild's builder over a test session's texts: input 0 is the
/// prelude.
struct Again<'r> {
    files: &'r mut hd_project::MemorySources,
    built: &'r mut hd_driver::Session,
    store: &'r hd_cache::MemoryStore,
    texts: &'r BTreeMap<u32, String>,
    /// What each input built adds to the session once it is replayed.
    added: hd_base::HashMap<u32, hd_driver::Session>,
    /// The time spent building.
    building: std::time::Duration,
}

impl crate::Rebuilder for Again<'_> {
    fn build(&mut self, number: u32) -> Result<(String, Vec<u8>), String> {
        let started = std::time::Instant::now();
        let (text, out) = if number == 0 {
            (String::new(), build_prelude(self.store))
        } else {
            let text = self
                .texts
                .get(&number)
                .ok_or_else(|| format!("no text for input {number}"))?
                .clone();
            self.files.insert(&hd_project::repl_file(number), &text);
            (
                text,
                build_input(self.files, self.store, self.built, number),
            )
        };
        self.building += started.elapsed();
        if !out.diags.is_empty() {
            let file = hd_project::repl_file(number);
            self.files.retain(|p| p != file);
            return Err(out.render());
        }
        let (Some(wasm), Some(added)) = (out.wasm, out.session_added) else {
            return Err(format!("input {number} built no module"));
        };
        self.added.insert(number, added);
        Ok((text, wasm))
    }

    fn build_joined(&mut self, numbers: &[u32]) -> Result<(Vec<String>, Vec<u8>), String> {
        let started = std::time::Instant::now();
        let (Some(&first), Some(&last)) = (numbers.first(), numbers.last()) else {
            return Err("no inputs".into());
        };
        let mut texts = Vec::new();
        for n in numbers {
            let text = self
                .texts
                .get(n)
                .ok_or_else(|| format!("no text for input {n}"))?
                .clone();
            self.files.insert(&hd_project::repl_file(*n), &text);
            texts.push(text);
        }
        let out = build_inputs(self.files, self.store, self.built, Some(first), last);
        self.building += started.elapsed();
        if !out.diags.is_empty() {
            return Err(out.render());
        }
        let (Some(wasm), Some(added)) = (out.wasm, out.session_added) else {
            return Err(format!("inputs {first} to {last} built no module"));
        };
        self.added.insert(last, added);
        Ok((texts, wasm))
    }

    fn replayed(&mut self, number: u32, joined: bool) {
        let added = self.added.remove(&number);
        if !joined {
            let file = hd_project::repl_file(number);
            self.files.retain(|p| p != file);
        } else if let Some(added) = added {
            self.built.add(added);
        }
    }

    fn restart(&mut self) {
        *self.built = hd_driver::Session::start();
        *self.files = hd_project::MemorySources::default();
        self.added.clear();
    }
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
        let out = build_input(&self.files, &self.store, &self.built, n);
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
        self.texts.insert(built.number, built.source.clone());
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

impl Repl<'_> {
    /// Rebuilds the session from its journal over the inputs' texts, as
    /// `:reload` does; it forgets the inputs the rebuild skipped or
    /// dropped.
    fn rebuild(&mut self) -> crate::Rebuilt {
        self.rebuild_timed().0
    }

    /// `rebuild`, with the part of its time spent building the inputs.
    fn rebuild_timed(&mut self) -> (crate::Rebuilt, std::time::Duration) {
        self.built = hd_driver::Session::start();
        self.files = hd_project::MemorySources::default();
        let mut again = Again {
            files: &mut self.files,
            built: &mut self.built,
            store: &self.store,
            texts: &self.texts,
            added: hd_base::HashMap::default(),
            building: std::time::Duration::ZERO,
        };
        let rebuilt = self
            .session
            .rebuild(&mut again)
            .expect("the session rebuilds");
        let building = again.building;
        for n in rebuilt.skipped.iter().chain(&rebuilt.dropped) {
            self.texts.remove(n);
            let file = hd_project::repl_file(*n);
            self.files.retain(|p| p != file);
        }
        (rebuilt, building)
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
        texts: BTreeMap::new(),
        store: hd_cache::MemoryStore::default(),
    }
}

/// A session that starts as the REPL does: its prelude runs first and
/// owns the memory and runtime globals; inputs import what it holds.
fn repl_with_prelude(session: &mut WasmtimeSession) -> Repl<'_> {
    let store = hd_cache::MemoryStore::default();
    let out = build_prelude(&store);
    assert!(out.diags.is_empty(), "{}", out.render());
    let wasm = out.wasm.expect("the prelude");
    assert_eq!(session.run_input(0, "", &wasm), Ok(Outcome::Exit(0)));
    let mut built = hd_driver::Session::start();
    built.add(out.session_added.expect("what the prelude adds"));
    Repl {
        session,
        built,
        files: hd_project::MemorySources::default(),
        next: 0,
        texts: BTreeMap::from([(0, String::new())]),
        store,
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

/// The `prices` example of `docs/future-work/live-execution.md` §2.2, with std's `Map`
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
/// `dbg` prints it, then ` : ` and its type, on standard output
/// (`cli.repl.value.stream`).
#[test]
fn an_expression_input_shows_its_value_and_type() {
    with_session(|session, io| {
        let mut repl = repl(session);
        assert_eq!(repl.input("[3, 6]\n"), Outcome::Exit(0));
        assert_eq!(text(&io.out), "[3, 6] : List[usize]\n");
        assert_eq!(text(&io.err), "");
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
            text(&io.out),
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
        assert_eq!(text(&io.err), "repl#1:2:5: x = 21\nrepl#1:2:5: x = 1\n");
        assert_eq!(text(&io.out), "44 : i32\n");
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
        format!(
            "text := read_text!(Path({})).unwrap_or(\"\")\n",
            hd_string(manifest)
        ),
        "println(text.contains(\"hd_run_wasmtime\"))\n".to_owned(),
        format!(
            "read_text!(Path({})).is_ok()\n",
            hd_string(&format!("{manifest}/missing"))
        ),
    ];
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(text(&io.out), "true\nfalse : bool\n");
        assert_eq!(text(&io.err), "");
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
            assert_eq!(text(&io.out), shown);
            assert_eq!(text(&io.err), "");
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
                "call hd:rt panic",
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
        // The panic stub starts its report, then writes it.
        assert!(matches!(
            &events[17],
            Event::Call { request: Request::Args(args), .. } if args.is_empty()
        ));
        assert!(matches!(
            &events[18],
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

/// The `Input` rows of the session's journal, by number.
fn journal_inputs(session: &WasmtimeSession) -> Vec<u32> {
    let record = hd_run::replay::recorded(session.journal().expect("recording")).expect("reads");
    record.inputs.iter().map(|i| i.number).collect()
}

/// `cli.repl.rebuild` and `cli.repl.host.once`: a session that read the
/// clock, a file and standard input is rebuilt with every host result
/// served from its journal. No host call is made again, nothing is
/// printed again, the file's new text is not read, and the bindings are
/// the same. The session then goes on live, recording after the replayed
/// inputs.
#[test]
fn a_rebuilt_session_repeats_no_host_call() {
    let path = std::env::temp_dir().join(format!("hd-rebuild-{}.txt", std::process::id()));
    std::fs::write(&path, "first text").expect("writes");
    let inputs = [
        "use std.time.{now}\nuse std.fs.{read_text}\nuse std.path.{Path}\nuse std.console.{read_line}\n".to_owned(),
        "started := now().unix_milliseconds()\n".to_owned(),
        format!(
            "text := read_text!(Path({})).unwrap_or(\"\")\n",
            hd_path(&path)
        ),
        "answer := read_line!()\n".to_owned(),
        "println(\"read ${text}\")\n".to_owned(),
    ];
    let shown = ["started\n", "text\n", "answer\n"];
    with_session_input(Some(b"typed line\n"), |session, io| {
        let mut repl = repl_with_prelude(session);
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(text(&io.out), "read first text\n");
        let mut before = String::new();
        for input in shown {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
            before.push_str(&text(&io.out));
        }
        assert!(
            before.ends_with("\"first text\" : string\nResult.Ok(Option.Some(\"typed line\")) : Result[string?, ConsoleError]\n"),
            "{before}"
        );
        std::fs::write(&path, "second text").expect("writes");
        let calls = repl.session.host_calls();
        let rebuilt = repl.rebuild();
        assert_eq!(
            rebuilt,
            crate::Rebuilt {
                kept: (0..=8).collect(),
                ..crate::Rebuilt::default()
            }
        );
        assert_eq!(repl.session.host_calls(), calls, "a host call repeated");
        assert_eq!(text(&io.out), "");
        assert_eq!(text(&io.err), "");
        assert_eq!(journal_inputs(repl.session), (0..=8).collect::<Vec<_>>());
        let mut after = String::new();
        for input in shown {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
            after.push_str(&text(&io.out));
        }
        assert_eq!(after, before);
        assert_eq!(journal_inputs(repl.session), (0..=11).collect::<Vec<_>>());
    });
    let _ = std::fs::remove_file(&path);
}

/// `cli.repl.rebuild.stopped` and `.prefix`: a rebuild skips a stopped
/// and a cancelled input, so the stopped input's change to `kept` is lost.
/// The first input that then asks the host for something else, the one
/// that shows `kept`, is dropped with every later input. It ran before it
/// diverged, so the rebuild starts again to undo its change too.
#[test]
fn a_rebuild_skips_stopped_inputs_and_drops_from_the_first_divergence() {
    with_session(|session, io| {
        let stopper = session.stopper();
        let mut repl = repl(session);
        for input in ["use std.time.{sleep, Duration}\n", "let kept = 7\n"] {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(
            repl.input_stopped_after("kept = 8\nwhile true:\n    pass\n", &stopper, 300),
            Outcome::Exit(3)
        );
        assert_eq!(
            repl.input_stopped_waiting("sleep!(Duration.seconds(+30))\n", &stopper),
            Outcome::Exit(3)
        );
        assert_eq!(text(&io.err), "stopped\ncancelled\n");
        for input in ["1 + 1\n", "kept = kept + 1\nprintln(kept)\n", "2 + 2\n"] {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(text(&io.out), "2 : usize\n9\n4 : usize\n");
        assert_eq!(text(&io.err), "");
        let rebuilt = repl.rebuild();
        assert_eq!(rebuilt.kept, [1, 2, 5]);
        assert_eq!(rebuilt.skipped, [3, 4]);
        assert_eq!(rebuilt.dropped, [6, 7]);
        assert_eq!(
            rebuilt.reason.as_deref(),
            Some("input 6 called hd:Console write_line.start with other arguments than before")
        );
        assert_eq!(text(&io.err), "");
        assert_eq!(text(&io.out), "");
        assert_eq!(repl.input("kept\n"), Outcome::Exit(0));
        assert_eq!(text(&io.out), "7 : usize\n");
        assert_eq!(journal_inputs(repl.session), [1, 2, 5, 8]);
    });
}

/// A rebuild of a long session: 200 inputs that each add to a binding,
/// every tenth showing it, rebuilt from the journal. The build cache is
/// warm, as it is for `:reload` of unchanged inputs. Run it with
/// `cargo test -p hd_run_wasmtime long_session -- --nocapture` for the
/// times.
#[test]
fn a_long_session_rebuild_is_timed() {
    use std::time::Instant;
    let inputs: Vec<String> = (0..200)
        .map(|i| match i {
            0 => "let total = 0\n".to_owned(),
            i if i % 10 == 0 => "total\n".to_owned(),
            i => format!("total = total + {i}\n"),
        })
        .collect();
    with_session(|session, io| {
        let mut repl = repl_with_prelude(session);
        let started = Instant::now();
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let live = started.elapsed();
        let shown = text(&io.out);
        let calls = repl.session.host_calls();
        let started = Instant::now();
        let (rebuilt, building) = repl.rebuild_timed();
        let took = started.elapsed();
        assert_eq!(rebuilt.kept.len(), 201);
        assert!(rebuilt.dropped.is_empty(), "{:?}", rebuilt.reason);
        assert_eq!(repl.session.host_calls(), calls);
        // The prelude's instance, and one for the 200 inputs.
        assert_eq!(repl.session.instances(), 2);
        assert_eq!(text(&io.err), "");
        assert_eq!(repl.input("total\n"), Outcome::Exit(0));
        let total: usize = (1..200).filter(|i| i % 10 != 0).sum();
        assert_eq!(text(&io.out), format!("{total} : usize\n"));
        assert_eq!(shown.lines().count(), 19);
        println!(
            "200 inputs: built and run live in {live:?}; rebuilt from the journal in {took:?} ({:?} per input), of which building {building:?} and replaying {:?} ({:?} per input)",
            took / 201,
            took.saturating_sub(building),
            took.saturating_sub(building) / 201
        );
    });
}

/// #228: once more inputs than its bound ran, a session wants a rebuild,
/// and the rebuild holds the replayed inputs in one instance. With a
/// bound of 3 the fourth input asks for it, and only it: the session then
/// holds two instances, the prelude's and the inputs'. No host call
/// repeats, nothing prints again, and later inputs see the same bindings.
#[test]
fn a_session_past_its_bound_rebuilds_into_one_instance() {
    let inputs = [
        "use std.time.{now}\nfn twice(n: usize) -> usize: n * 2\n",
        "let total = 5\nstarted := now().unix_milliseconds()\nprintln(\"once\")\n",
        "started\n",
        "total = twice(total)\n",
        "started\n",
        "total\n",
        "1 + 1\n",
    ];
    with_session(|session, io| {
        session.set_rebuild_after(3);
        let mut repl = repl_with_prelude(session);
        let mut rebuilds = 0;
        let mut out = String::new();
        for input in inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
            out.push_str(&text(&io.out));
            if repl.session.wants_rebuild() {
                assert_eq!(repl.session.instances(), 5);
                let calls = repl.session.host_calls();
                let rebuilt = repl.rebuild();
                assert_eq!(
                    rebuilt,
                    crate::Rebuilt {
                        kept: (0..=4).collect(),
                        ..crate::Rebuilt::default()
                    }
                );
                assert_eq!(repl.session.host_calls(), calls, "a host call repeated");
                assert_eq!(repl.session.instances(), 2);
                assert_eq!(text(&io.out), "");
                rebuilds += 1;
            }
        }
        assert_eq!(rebuilds, 1);
        assert_eq!(repl.session.instances(), 5);
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 5, "{out}");
        assert_eq!(lines[0], "once");
        assert!(lines[1].ends_with(" : i64"), "{out}");
        assert_eq!(lines[2], lines[1]);
        assert_eq!(lines[3..], ["10 : usize", "2 : usize"]);
        assert_eq!(text(&io.err), "");
        assert_eq!(journal_inputs(repl.session), (0..=7).collect::<Vec<_>>());
    });
}

/// #228: an input that panicked added nothing for the inputs after it, so
/// a rebuild builds it alone, between the runs of inputs it holds in one
/// instance each. Its change to an earlier value survives.
#[test]
fn a_rebuild_builds_a_panicked_input_alone() {
    let inputs = [
        ("let log: mut List[string] = []\n", 0),
        ("log.push(\"a\")\nline := log[5]\n", 3),
        ("log.push(\"b\")\n", 0),
        ("log.len()\n", 0),
    ];
    with_session(|session, io| {
        let mut repl = repl_with_prelude(session);
        for (input, status) in inputs {
            assert_eq!(repl.input(input), Outcome::Exit(status));
        }
        assert_eq!(text(&io.out), "2 : usize\n");
        assert_eq!(
            text(&io.err),
            "panic: index-out-of-bounds: list index out of bounds\n"
        );
        assert_eq!(repl.rebuild().kept, [0, 1, 2, 3, 4]);
        // The prelude, input 1, input 2 alone, then inputs 3 and 4.
        assert_eq!(repl.session.instances(), 4);
        assert_eq!(repl.input("log.join(\", \")\n"), Outcome::Exit(0));
        assert_eq!(text(&io.out), "\"a, b\" : string\n");
        assert_eq!(text(&io.err), "");
    });
}

/// The session inputs that declare a `run!` over the `Process` provider.
#[cfg(unix)]
const PROCESS_INPUTS: [&str; 2] = [
    "use std.process.{Process, ProcessError, ProcessOutput}\n",
    "fn run!(program: string, args: List[string]) -> Result[ProcessOutput, ProcessError] $ Process:\n    $.use(Process).run!(program, args, \"\")\n",
];

/// A private temporary path for a session test's fixture.
#[cfg(unix)]
fn fixture_path(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "hd-session-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the clock is past the epoch")
            .as_nanos()
    ))
}

/// `cli.repl.cancel` and `std-process.cancel`: a Stop while an input
/// waits on `run!` cancels the input at once, and the child is killed
/// and reaped.
#[cfg(unix)]
#[test]
fn a_stop_during_run_cancels_the_input_and_kills_the_child() {
    use rustix::io::Errno;
    use rustix::process::{Pid, Signal, kill_process, test_kill_process};
    use std::time::{Duration, Instant};
    let path = fixture_path("stop-run");
    let input = format!(
        "x := run!(\"/bin/sh\", [\"-c\", \"echo $$ > {}; exec sleep 30\"])\n",
        path.display()
    );
    let mut child = None;
    with_session(|session, io| {
        let stopper = session.stopper();
        let mut repl = repl(session);
        for input in PROCESS_INPUTS {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let built = repl.build(&input);
        // Stop once the input waits and the child has started.
        let pid = {
            let path = path.clone();
            move || {
                std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|pid| pid.trim().parse::<i32>().ok())
                    .filter(|pid| *pid > 1)
                    .and_then(Pid::from_raw)
            }
        };
        let stopping = {
            let (stopper, pid) = (stopper.clone(), pid.clone());
            std::thread::spawn(move || {
                let until = Instant::now() + Duration::from_secs(10);
                while !(stopper.waiting() && pid().is_some()) && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                stopper.stop();
            })
        };
        let started = Instant::now();
        let outcome = repl.run(built);
        let took = started.elapsed();
        stopping.join().expect("the stopping thread");
        child = pid();
        assert_eq!(outcome, Outcome::Exit(3));
        assert_eq!(text(&io.err), "cancelled\n");
        assert!(
            took < Duration::from_secs(10),
            "the cancel waited: {took:?}"
        );
        assert!(!repl.session.pending(), "the run is still pending");
    });
    let _ = std::fs::remove_file(&path);
    let child = child.expect("the child started");
    let until = Instant::now() + Duration::from_secs(10);
    let mut gone = false;
    while !gone && Instant::now() < until {
        gone = test_kill_process(child) == Err(Errno::SRCH);
        std::thread::sleep(Duration::from_millis(5));
    }
    if !gone {
        let _ = kill_process(child, Signal::KILL);
    }
    assert!(gone, "the child outlived the cancel");
}

/// `cli.repl.host.once`: a rebuilt session serves `run!` from its journal
/// and spawns nothing: the child's one line in the file stays one line,
/// and the output shown later is the first run's.
#[cfg(unix)]
#[test]
fn a_rebuilt_session_runs_no_child_again() {
    let path = fixture_path("rebuild-run");
    let script = format!("printf ran >> {}; printf out", path.display());
    let mut inputs: Vec<String> = PROCESS_INPUTS.iter().map(|s| (*s).to_owned()).collect();
    inputs.push(format!(
        "out := run!(\"/bin/sh\", [\"-c\", {}])\n",
        hd_string(&script)
    ));
    inputs.push("out\n".to_owned());
    with_session(|session, io| {
        let mut repl = repl_with_prelude(session);
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        assert_eq!(
            std::fs::read_to_string(&path).expect("the child ran"),
            "ran"
        );
        let shown = text(&io.out);
        assert!(shown.contains("stdout: \"out\""), "{shown}");
        let calls = repl.session.host_calls();
        let rebuilt = repl.rebuild();
        assert_eq!(
            rebuilt,
            crate::Rebuilt {
                kept: (0..=4).collect(),
                ..crate::Rebuilt::default()
            }
        );
        assert_eq!(repl.session.host_calls(), calls, "a host call repeated");
        assert_eq!(std::fs::read_to_string(&path).expect("reads"), "ran");
        assert_eq!(text(&io.out), "", "a replayed input printed again");
        assert_eq!(repl.input("out\n"), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), shown);
    });
    let _ = std::fs::remove_file(&path);
}

/// `cli.repl.cancel` and `std-host.op.wait`: a Stop while an input waits
/// on `accept!` cancels the input at once.
#[test]
fn a_stop_during_accept_cancels_the_input() {
    use std::time::{Duration, Instant};
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port();
    let inputs = [
        "use std.net.{Net, NetError}\n".to_owned(),
        "fn accept_one!(port: u16) -> string $ Net:\n    let mut net = $.use(Net)\n    let mut listener = match net.listen!(\"127.0.0.1\", port):\n        .Ok(l) => l\n        .Err(e) => return \"${e}\"\n    match listener.accept!():\n        .Ok(_) => \"accepted\"\n        .Err(e) => \"${e}\"\n".to_owned(),
    ];
    with_session(|session, io| {
        let stopper = session.stopper();
        let mut repl = repl(session);
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let built = repl.build(&format!("x := accept_one!({port})\n"));
        let stopping = {
            let stopper = stopper.clone();
            std::thread::spawn(move || {
                let until = Instant::now() + Duration::from_secs(10);
                while !stopper.waiting() && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                stopper.stop();
            })
        };
        let started = Instant::now();
        let outcome = repl.run(built);
        let took = started.elapsed();
        stopping.join().expect("the stopping thread");
        assert_eq!(outcome, Outcome::Exit(3));
        assert_eq!(text(&io.err), "cancelled\n");
        assert!(
            took < Duration::from_secs(10),
            "the cancel waited: {took:?}"
        );
        assert!(!repl.session.pending(), "the accept is still pending");
    });
}

/// `cli.repl.host.once`: a rebuilt session serves `connect!` and `read!`
/// from its journal and makes no network call: the server sees one
/// connection, and the output shown later is the first run's.
#[test]
fn a_rebuilt_session_makes_no_network_call() {
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let server = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let port = server.local_addr().expect("an address").port();
    let connections = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&connections);
    // Detached: it waits in `accept` until the test binary ends.
    std::thread::spawn(move || {
        for mut stream in server.incoming().flatten() {
            counted.fetch_add(1, Ordering::SeqCst);
            let _ = stream.write_all(b"hi");
        }
    });
    let inputs = [
        "use std.net.{Net, NetError}\n".to_owned(),
        "fn fetch!(port: u16) -> string $ Net:\n    let mut net = $.use(Net)\n    let mut stream = match net.connect!(\"127.0.0.1\", port):\n        .Ok(s) => s\n        .Err(e) => return \"${e}\"\n    match stream.read!(100):\n        .Ok(bytes) => \"read ${bytes.len()}\"\n        .Err(e) => \"${e}\"\n".to_owned(),
        format!("out := fetch!({port})\n"),
        "out\n".to_owned(),
    ];
    with_session(|session, io| {
        let mut repl = repl_with_prelude(session);
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let shown = text(&io.out);
        assert!(shown.contains("\"read 2\""), "{shown}");
        assert_eq!(connections.load(Ordering::SeqCst), 1);
        let calls = repl.session.host_calls();
        let rebuilt = repl.rebuild();
        assert_eq!(
            rebuilt,
            crate::Rebuilt {
                kept: (0..=4).collect(),
                ..crate::Rebuilt::default()
            }
        );
        assert_eq!(repl.session.host_calls(), calls, "a host call repeated");
        assert_eq!(connections.load(Ordering::SeqCst), 1, "connected again");
        assert_eq!(text(&io.out), "", "a replayed input printed again");
        assert_eq!(repl.input("out\n"), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), shown);
    });
}

/// The session inputs that declare `fetch!` over the `Http` provider and
/// `nap!`, a 100 ms sleep.
const HTTP_INPUTS: [&str; 3] = [
    "use std.http.{Http, HttpError, get}\nuse std.task.{all, race}\nuse std.time.{Clock, ms, sleep}\n",
    "fn fetch!(url: string) -> string $ Http:\n    match get!(url):\n        .Ok(r) => \"status ${r.status}\"\n        .Err(e) => \"${e}\"\n",
    "fn nap!() -> string $ Clock:\n    sleep!(ms(100))\n    \"timeout\"\n",
];

/// A server that reads one request and then answers nothing: it reports
/// when the request arrived, then whether the client closed the
/// connection within 10 s.
fn silent_server() -> (
    String,
    std::sync::mpsc::Receiver<()>,
    std::thread::JoinHandle<bool>,
) {
    use std::io::Read;
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let url = format!("http://{}/", listener.local_addr().expect("an address"));
    let (arrived, arrival) = std::sync::mpsc::channel();
    let server = std::thread::spawn(move || {
        let (mut peer, _) = listener.accept().expect("accepts");
        peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
            .expect("sets a timeout");
        let mut request = Vec::new();
        while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
            let mut bytes = [0; 1024];
            match peer.read(&mut bytes) {
                Ok(0) | Err(_) => return false,
                Ok(count) => request.extend_from_slice(&bytes[..count]),
            }
        }
        let _ = arrived.send(());
        matches!(peer.read(&mut [0; 1]), Ok(0))
    });
    (url, arrival, server)
}

/// `std-http.send.cancel` and `std-http.send.cancel.close`: a `race!` of
/// `send!` against a 100 ms sleep ends with the sleep, and the server
/// reads the end of the connection. The request's own timeout is 30 s.
#[test]
fn a_race_ends_a_send_at_its_timeout_and_closes_the_connection() {
    use std::time::{Duration, Instant};
    let (url, _arrival, server) = silent_server();
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in HTTP_INPUTS {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let built = repl.build(&format!("race!(fetch(\"{url}\"), nap())\n"));
        let started = Instant::now();
        assert_eq!(repl.run(built), Outcome::Exit(0), "{}", text(&io.err));
        let took = started.elapsed();
        assert_eq!(text(&io.out), "\"timeout\" : string\n");
        assert!(took < Duration::from_secs(5), "the race waited: {took:?}");
    });
    assert!(
        server.join().expect("the server"),
        "the connection stayed open"
    );
}

/// `std-host.op.wait`: `all!` of two `send!` to a server that answers
/// only once both requests have arrived. One request after the other
/// would reach its 10 s timeout instead.
#[test]
fn all_sends_two_requests_at_the_same_time() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let url = format!("http://{}/", listener.local_addr().expect("an address"));
    let server = std::thread::spawn(move || {
        let mut peers = Vec::new();
        for _ in 0..2 {
            let (mut peer, _) = listener.accept().expect("accepts");
            peer.set_read_timeout(Some(std::time::Duration::from_secs(10)))
                .expect("sets a timeout");
            let mut request = Vec::new();
            while !request.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                let mut bytes = [0; 1024];
                let count = peer.read(&mut bytes).expect("reads");
                assert_ne!(count, 0, "the request arrives whole");
                request.extend_from_slice(&bytes[..count]);
            }
            peers.push(peer);
        }
        for mut peer in peers {
            peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n")
                .expect("writes");
        }
    });
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in HTTP_INPUTS {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let request = format!("Request {{ url: \"{url}\", timeout: .Some(s(10)) }}");
        assert_eq!(
            repl.input(&format!(
                "use std.http.{{Request, send}}\nuse std.time.s\nfn timed!() -> string $ Http:\n    match send!({request}):\n        .Ok(r) => \"status ${{r.status}}\"\n        .Err(e) => \"${{e}}\"\n"
            )),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(
            repl.input("all!(timed(), timed())\n"),
            Outcome::Exit(0),
            "{}",
            text(&io.err)
        );
        assert_eq!(
            text(&io.out),
            "(\"status 200\", \"status 200\") : (string, string)\n"
        );
    });
    server.join().expect("the server");
}

/// `cli.repl.cancel` and `std-http.send.cancel.close`: a Stop while an
/// input waits on `send!` cancels the input at once, and the server reads
/// the end of the connection.
#[test]
fn a_stop_during_send_cancels_the_input_and_closes_the_connection() {
    use std::time::{Duration, Instant};
    let (url, arrival, server) = silent_server();
    with_session(|session, io| {
        let stopper = session.stopper();
        let mut repl = repl(session);
        for input in HTTP_INPUTS {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let built = repl.build(&format!("x := fetch!(\"{url}\")\n"));
        let stopping = {
            let stopper = stopper.clone();
            std::thread::spawn(move || {
                let arrived = arrival.recv_timeout(Duration::from_secs(10)).is_ok();
                let until = Instant::now() + Duration::from_secs(10);
                while !stopper.waiting() && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                stopper.stop();
                arrived
            })
        };
        let started = Instant::now();
        let outcome = repl.run(built);
        let took = started.elapsed();
        assert!(stopping.join().expect("the stopping thread"), "no request");
        assert_eq!(outcome, Outcome::Exit(3));
        assert_eq!(text(&io.err), "cancelled\n");
        assert!(
            took < Duration::from_secs(10),
            "the cancel waited: {took:?}"
        );
        assert!(!repl.session.pending(), "the send is still pending");
    });
    assert!(
        server.join().expect("the server"),
        "the connection stayed open"
    );
}

/// `cli.repl.host.once`: a rebuilt session serves `send!` from its journal
/// and makes no request: the server sees one, and the output shown later
/// is the first run's.
#[test]
fn a_rebuilt_session_sends_no_request_again() {
    use std::io::{Read, Write};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
    let url = format!("http://{}/", listener.local_addr().expect("an address"));
    let requests = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&requests);
    // Detached: it waits in `accept` until the test binary ends.
    std::thread::spawn(move || {
        for mut peer in listener.incoming().flatten() {
            counted.fetch_add(1, Ordering::SeqCst);
            let _ = peer.set_read_timeout(Some(std::time::Duration::from_secs(5)));
            let _ = peer.read(&mut [0; 4096]);
            let _ = peer.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi");
        }
    });
    let mut inputs: Vec<String> = HTTP_INPUTS.iter().map(|s| (*s).to_owned()).collect();
    inputs.push(format!("out := fetch!(\"{url}\")\n"));
    inputs.push("out\n".to_owned());
    with_session(|session, io| {
        let mut repl = repl_with_prelude(session);
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let shown = text(&io.out);
        assert!(shown.contains("\"status 200\""), "{shown}");
        assert_eq!(requests.load(Ordering::SeqCst), 1);
        let calls = repl.session.host_calls();
        let rebuilt = repl.rebuild();
        assert_eq!(
            rebuilt,
            crate::Rebuilt {
                kept: (0..=5).collect(),
                ..crate::Rebuilt::default()
            }
        );
        assert_eq!(repl.session.host_calls(), calls, "a host call repeated");
        assert_eq!(requests.load(Ordering::SeqCst), 1, "sent again");
        assert_eq!(text(&io.out), "", "a replayed input printed again");
        assert_eq!(repl.input("out\n"), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), shown);
    });
}

#[cfg(unix)]
const FS_INPUTS: [&str; 3] = [
    "use std.fs.{FsRead}\nuse std.path.Path\nuse std.task.{all, race}\nuse std.time.{Clock, ms, sleep}\n",
    "fn slurp!(path: string) -> string $ FsRead:\n    match $.use(FsRead).read_bytes!(Path(path)):\n        .Ok(bytes) => \"read ${bytes.len()}\"\n        .Err(e) => \"${e}\"\n",
    "fn nap!() -> string $ Clock:\n    sleep!(ms(100))\n    \"timeout\"\n",
];

/// A FIFO in a private directory, removed with the guard.
#[cfg(unix)]
struct Fifo(std::path::PathBuf);

#[cfg(unix)]
impl Fifo {
    fn new(label: &str) -> Self {
        let path = fixture_path(label);
        crate::test_fifo::make(&path);
        Self(path)
    }

    fn write(&self, bytes: &[u8]) {
        write_fifo(&self.0, bytes);
    }
}

/// Opens the FIFO for writing, as a writer that lets a waiting read go
/// on, and writes `bytes`.
#[cfg(unix)]
fn write_fifo(path: &std::path::Path, bytes: &[u8]) {
    use std::io::Write;
    let mut writer = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("opens the writer");
    writer.write_all(bytes).expect("writes");
}

#[cfg(unix)]
impl Drop for Fifo {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

/// `std-fs.special-file` and `std-host.op.wait`: a `race!` of a read of a
/// FIFO nobody writes against a 100 ms sleep ends with the sleep. The
/// read's thread lingers (`std-fs.special-file.deadline`) until a writer
/// closes the FIFO.
#[cfg(unix)]
#[test]
fn a_race_ends_a_fifo_read_at_the_sleep() {
    use std::time::{Duration, Instant};
    let fifo = Fifo::new("race-fifo");
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in FS_INPUTS {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let built = repl.build(&format!("race!(slurp({}), nap())\n", hd_path(&fifo.0)));
        let started = Instant::now();
        assert_eq!(repl.run(built), Outcome::Exit(0), "{}", text(&io.err));
        let took = started.elapsed();
        assert_eq!(text(&io.out), "\"timeout\" : string\n");
        assert!(took < Duration::from_secs(3), "the race waited: {took:?}");
        assert!(!repl.session.pending(), "the read is still pending");
    });
    // The abandoned read opens the FIFO: a writer lets its thread end.
    fifo.write(b"");
}

/// `std-fs.special-file`: a read of a FIFO with a writer reads all that
/// the writer sends, though it arrives after the read started.
#[cfg(unix)]
#[test]
fn a_fifo_read_with_a_writer_reads_everything_in_a_session() {
    let fifo = Fifo::new("read-fifo");
    let path = fifo.0.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        write_fifo(&path, &vec![b'x'; 200_000]);
    });
    with_session(|session, io| {
        let mut repl = repl(session);
        for input in FS_INPUTS {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let shown = repl.input(&format!("slurp!({})\n", hd_path(&fifo.0)));
        assert_eq!(shown, Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), "\"read 200000\" : string\n");
    });
    writer.join().expect("the writer");
}

/// `cli.repl.cancel`: a Stop while an input waits on a FIFO read cancels
/// the input at once.
#[cfg(unix)]
#[test]
fn a_stop_during_a_fifo_read_cancels_the_input() {
    use std::time::{Duration, Instant};
    let fifo = Fifo::new("stop-fifo");
    with_session(|session, io| {
        let stopper = session.stopper();
        let mut repl = repl(session);
        for input in FS_INPUTS {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let built = repl.build(&format!("x := slurp!({})\n", hd_path(&fifo.0)));
        let stopping = {
            let stopper = stopper.clone();
            std::thread::spawn(move || {
                let until = Instant::now() + Duration::from_secs(10);
                while !stopper.waiting() && Instant::now() < until {
                    std::thread::sleep(Duration::from_millis(1));
                }
                stopper.stop();
            })
        };
        let started = Instant::now();
        let outcome = repl.run(built);
        let took = started.elapsed();
        stopping.join().expect("the stopping thread");
        assert_eq!(outcome, Outcome::Exit(3));
        assert_eq!(text(&io.err), "cancelled\n");
        assert!(
            took < Duration::from_secs(10),
            "the cancel waited: {took:?}"
        );
        assert!(!repl.session.pending(), "the read is still pending");
    });
    fifo.write(b"");
}

/// `cli.repl.host.once`: a rebuilt session serves a FIFO read from its
/// journal: the writer is gone, and the read neither waits nor opens the
/// FIFO again.
#[cfg(unix)]
#[test]
fn a_rebuilt_session_reads_no_fifo_again() {
    let fifo = Fifo::new("rebuild-fifo");
    let path = fifo.0.clone();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(100));
        write_fifo(&path, b"journaled");
    });
    let mut inputs: Vec<String> = FS_INPUTS.iter().map(|s| (*s).to_owned()).collect();
    inputs.push(format!("out := slurp!({})\n", hd_path(&fifo.0)));
    inputs.push("out\n".to_owned());
    with_session(|session, io| {
        let mut repl = repl_with_prelude(session);
        for input in &inputs {
            assert_eq!(repl.input(input), Outcome::Exit(0), "{}", text(&io.err));
        }
        let shown = text(&io.out);
        assert!(shown.contains("\"read 9\""), "{shown}");
        let calls = repl.session.host_calls();
        let rebuilt = repl.rebuild();
        assert_eq!(
            rebuilt,
            crate::Rebuilt {
                kept: (0..=5).collect(),
                ..crate::Rebuilt::default()
            }
        );
        assert_eq!(repl.session.host_calls(), calls, "a host call repeated");
        assert_eq!(text(&io.out), "", "a replayed input printed again");
        assert_eq!(repl.input("out\n"), Outcome::Exit(0), "{}", text(&io.err));
        assert_eq!(text(&io.out), shown);
    });
    writer.join().expect("the writer");
}
