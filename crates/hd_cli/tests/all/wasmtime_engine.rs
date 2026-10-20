//! The wasmtime engine against the Node engine (`docs/compiler/engines-and-test-runner.md`
//! §18, "As built (W1)"): each program runs once on Node, the default,
//! and once on wasmtime (`HD_ENGINE=wasmtime` or `--engine wasmtime`), and
//! both runs print the same standard output and error and end with the
//! same status.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use hd_testkit::Scratch;

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn work(name: &str) -> Scratch {
    Scratch::new(env!("CARGO_TARGET_TMPDIR"), &format!("wasmtime-{name}"))
}

fn hd(dir: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_hd"));
    // The cache goes under the test temp dir, never next to `dir`: some
    // runs use the repository's `examples/` folders.
    let name = dir
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    c.current_dir(dir)
        .env(
            "HD_CACHE",
            Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("wasmtime-cache-{name}")),
        )
        .env_remove("HD_ENGINE");
    c
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

/// Runs `hd ARGS` in `dir` with `input` on standard input.
fn run(mut c: Command, args: &[&str], input: &str) -> Output {
    c.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = c.spawn().expect("run hd");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("hd output")
}

/// Runs `hd ARGS` in `dir` on both engines, with `env` set, and checks
/// that they agree; returns the wasmtime run.
fn both_with(dir: &Path, args: &[&str], input: &str, env: &[(&str, &str)]) -> Output {
    // Both set `HD_ENGINE`, so both see the same environment.
    let mut node = hd(dir);
    node.env("HD_ENGINE", "node");
    let mut wasmtime = hd(dir);
    wasmtime.env("HD_ENGINE", "wasmtime");
    for (k, v) in env {
        node.env(k, v);
        wasmtime.env(k, v);
    }
    let n = run(node, args, input);
    let w = run(wasmtime, args, input);
    let shown = format!(
        "hd {}\n-- node ({:?})\n{}{}\n-- wasmtime ({:?})\n{}{}",
        args.join(" "),
        n.status.code(),
        text(&n.stdout),
        text(&n.stderr),
        w.status.code(),
        text(&w.stdout),
        text(&w.stderr)
    );
    assert_eq!(text(&n.stdout), text(&w.stdout), "stdout\n{shown}");
    assert_eq!(text(&n.stderr), text(&w.stderr), "stderr\n{shown}");
    assert_eq!(n.status.code(), w.status.code(), "status\n{shown}");
    w
}

fn both(dir: &Path, args: &[&str]) -> Output {
    both_with(dir, args, "", &[])
}

/// A single-file program in a fresh directory.
fn program(name: &str, source: &str) -> Scratch {
    let dir = work(name);
    std::fs::write(dir.join("main.hd"), source).expect("write program");
    dir
}

#[test]
fn samples_agree() {
    for sample in [
        "hello/hello.hd",
        "arith/main.hd",
        "exit/main.hd",
        "suspend/main.hd",
        "defer/main.hd",
        "await_branches/main.hd",
    ] {
        let file = samples().join(sample);
        let out = both(&samples(), &[&file.to_string_lossy()]);
        assert!(out.status.success(), "{sample}: {}", text(&out.stderr));
    }
    for package in [
        "data",
        "generic",
        "trait",
        "fib",
        "std_types",
        "init",
        "gaps",
    ] {
        let out = both(&samples().join(package), &["run"]);
        assert!(out.status.success(), "{package}: {}", text(&out.stderr));
    }
}

#[test]
fn main_returning_err_reports_it_with_status_1() {
    let file = samples().join("err_exit/main.hd");
    let out = both(&samples(), &[&file.to_string_lossy()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "got 1\n");
    assert_eq!(text(&out.stderr), "oops: too big\n");
}

#[test]
fn exit_code_is_the_status() {
    let dir = program(
        "exit-code",
        "use std.process.ExitCode\n\npub fn main() -> ExitCode $ Console:\n    println(\"bye\")\n    ExitCode(7)\n",
    );
    let out = both(&dir, &["main.hd"]);
    assert_eq!(out.status.code(), Some(7));
    assert_eq!(text(&out.stdout), "bye\n");
}

#[test]
fn panics_report_and_end_with_status_3() {
    let dir = program(
        "panic",
        "pub fn main() -> void $ Console:\n    println(\"before\")\n    panic(\"boom\")\n",
    );
    let out = both(&dir, &["main.hd"]);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(text(&out.stdout), "before\n");
    assert!(
        text(&out.stderr).starts_with("panic: explicit-panic: boom"),
        "{}",
        text(&out.stderr)
    );
    let dir = program(
        "divide",
        "fn div(a: i32, b: i32) -> i32:\n    a / b\n\npub fn main() -> void $ Console:\n    println(\"start\")\n    println(div(+7, +0))\n",
    );
    let out = both(&dir, &["main.hd"]);
    assert_eq!(out.status.code(), Some(3));
    assert!(
        text(&out.stderr).starts_with("panic: integer-division-by-zero"),
        "{}",
        text(&out.stderr)
    );
}

/// `cli.test.report.frame`: both engines report a panic's frames, read
/// from their backtraces through the module's line table, the same way.
#[test]
fn test_reports_list_the_same_panic_frames() {
    let dir = program(
        "frames",
        "fn pick(xs: List[i32], i: usize) -> i32:\n    xs[i]\n\n\
         fn boom() -> void:\n    panic(\"out of stock\")\n\n\
         tests:\n    it(\"picks past the end\"):\n        pick([1, 2], 5)\n\n    \
         it(\"runs out\"):\n        boom()\n",
    );
    let out = both(&dir, &["test", "main.hd"]);
    // A single file's functions are named after its stem.
    assert_eq!(
        text(&out.stdout).lines().collect::<Vec<_>>(),
        [
            "PANIC main.hd:8: picks past the end",
            "    panic: index-out-of-bounds: list index out of bounds",
            "    main.hd:2 main.pick",
            "    main.hd:9 test \"picks past the end\"",
            "    repro: hd test main.hd --filter \"picks past the end\"",
            "PANIC main.hd:11: runs out",
            "    panic: explicit-panic: out of stock",
            "    main.hd:5 main.boom",
            "    main.hd:12 test \"runs out\"",
            "    repro: hd test main.hd --filter \"runs out\"",
            "test result: FAILED. 0 passed; 2 failed; 0 ignored",
        ]
    );
}

/// A recursion that exhausts the stack reports a few lines on both
/// engines: its repeated frames, and its cycle of two, print once with a
/// count. The counts follow each engine's stack size, so they may differ.
#[test]
fn both_engines_collapse_an_endless_recursion() {
    let dir = program(
        "recursion",
        "fn forever(n: i32) -> i32:\n    forever(n + 1) + 1\n\n\
         fn ping(n: i32) -> i32:\n    pong(n + 1) + 1\n\n\
         fn pong(n: i32) -> i32:\n    ping(n + 1) + 1\n\n\
         tests:\n    it(\"never stops\"):\n        forever(+0)\n\n    \
         it(\"plays ping pong\"):\n        ping(+0)\n",
    );
    let report = |engine: &str| {
        let mut c = hd(&dir);
        c.env("HD_ENGINE", engine);
        let out = run(c, &["test", "main.hd"], "");
        let stdout = text(&out.stdout);
        assert!(stdout.len() < 2000, "{engine}: {stdout}");
        let lines = stdout
            .lines()
            .map(|l| match l.split_once(" repeated ") {
                Some((what, _)) if what.trim_start().starts_with("...") => {
                    format!("{what} repeated N more times")
                }
                _ => l.to_owned(),
            })
            .collect::<Vec<_>>();
        assert!(
            lines
                .iter()
                .any(|l| l.contains("... last 2 frames repeated")),
            "{engine}: {stdout}"
        );
        fold_cycle(lines, &["main.ping", "main.pong"])
    };
    let node = report("node");
    assert_eq!(
        node,
        [
            "PANIC main.hd:11: never stops",
            "    panic: stack-exhausted: the call stack ran out; check for recursion that never ends",
            "    main.hd:2 main.forever",
            "    ... frame repeated N more times",
            "    main.hd:12 test \"never stops\"",
            "    repro: hd test main.hd --filter \"never stops\"",
            "PANIC main.hd:14: plays ping pong",
            "    panic: stack-exhausted: the call stack ran out; check for recursion that never ends",
            "    <cycle>",
            "    main.hd:15 test \"plays ping pong\"",
            "    repro: hd test main.hd --filter \"plays ping pong\"",
            "test result: FAILED. 0 passed; 2 failed; 0 ignored",
        ]
    );
    assert_eq!(report("wasmtime"), node);
}

/// Which frame of a cycle sits innermost when the stack runs out depends
/// on the engine's stack size, so the frames of a collapsed cycle and its
/// count line fold into one marker line before comparing.
fn fold_cycle(lines: Vec<String>, cycle: &[&str]) -> Vec<String> {
    let in_cycle = |l: &str| {
        let t = l.trim_start();
        t.starts_with("... last ") || cycle.iter().any(|f| t.ends_with(&format!(" {f}")))
    };
    let mut out: Vec<String> = Vec::new();
    for l in lines {
        if !in_cycle(&l) {
            out.push(l);
        } else if out.last().is_none_or(|p| p != "    <cycle>") {
            out.push("    <cycle>".to_owned());
        }
    }
    out
}

#[test]
fn args_and_program() {
    let dir = program(
        "args",
        "use std.host.{Args, args}\n\npub fn main() -> void $ Console + Args:\n    \
         println($.use(Args).program())\n    for word in args():\n        println(\"[${word}]\")\n",
    );
    let out = both(&dir, &["main.hd", "--", "a", "b c", ""]);
    assert_eq!(text(&out.stdout), "main.hd\n[a]\n[b c]\n[]\n");
}

#[test]
fn env_follows_the_grant() {
    let dir = program(
        "env",
        "use std.host.{Env, env}

fn show(name: string) -> string $ Env:
    match env(name):
        .Some(value) => value
        .None => \"(none)\"

pub fn main() -> void $ Console + Env:
    println(show(\"HD_W1_SEEN\"))
    println(show(\"HD_W1_HIDDEN\"))
    println(show(\"HD_W1_HIDDEN\"))
    println(show(\"HD_W1_UNSET\"))
    println(\"${$.use(Env).names().len()}\")
",
    );
    let env = [("HD_W1_SEEN", "yes"), ("HD_W1_HIDDEN", "no")];
    let out = both_with(
        &dir,
        &["--cap", "Env=HD_W1_SEEN,HD_W1_UNSET", "main.hd"],
        "",
        &env,
    );
    assert_eq!(text(&out.stdout), "yes\n(none)\n(none)\n(none)\n1\n");
    assert_eq!(
        text(&out.stderr),
        "hd: env HD_W1_HIDDEN is set but not granted; run with --cap Env=HD_W1_HIDDEN\n"
    );
    let out = both_with(&dir, &["main.hd"], "", &env);
    assert!(text(&out.stdout).starts_with("yes\nno\nno\n(none)\n"));
    // A total deny refuses the program before it starts, on either engine.
    let out = both_with(&dir, &["--cap", "Env=false", "main.hd"], "", &env);
    assert_eq!(out.status.code(), Some(101));
}

#[test]
fn float_text_and_case_mapping() {
    let dir = program(
        "floats",
        "use std.num.parse_f64

pub fn main() -> void $ Console:
    let tenth: f32 = 0.1
    let third: f32 = 0.33333334
    zero := 0.0
    println(0.1 + 0.2)
    println(tenth)
    println(third)
    println(1e21)
    println(1e20)
    println(1.5e-7)
    println(5e-324)
    println(-zero)
    println(zero / zero)
    println(-1.0 / zero)
    println(2.5.to_fixed(0))
    println(0.125.to_fixed(2))
    println(0.1.to_fixed(20))
    println(1e21.to_fixed(1))
    println(\"Stra\u{df}e \u{3a3}A\u{3a3}\".upper())
    println(\"\u{3a3}A\u{3a3} \u{130}\".lower())
    match parse_f64(\"9007199254740993\"):
        .Ok(v) => println(v)
        .Err(_) => println(\"error\")
    match parse_f64(\"2.4703282292062328e-324\"):
        .Ok(v) => println(v)
        .Err(_) => println(\"error\")
",
    );
    let out = both(&dir, &["main.hd"]);
    assert_eq!(
        text(&out.stdout),
        "0.30000000000000004\n0.1\n0.33333334\n1e+21\n100000000000000000000.0\n1.5e-7\n5e-324\n\
         -0.0\nNaN\n-inf\n2\n0.12\n0.10000000000000000555\n1000000000000000000000.0\n\
         STRASSE \u{3a3}A\u{3a3}\n\u{3c3}a\u{3c2} i\u{307}\n9007199254740992.0\n5e-324\n"
    );
}

#[test]
fn a_suspending_entry_sleeps_and_reads_the_clock() {
    let dir = program(
        "clock",
        "use std.time.{Clock, Duration, now, sleep}

pub fn main!() -> void $ Console + Clock:
    clock := $.use(Clock)
    start := clock.monotonic()
    println(\"sleeping\")
    sleep!(Duration.milliseconds(30))
    println(clock.monotonic().since(start).as_milliseconds() >= 30)
    println(now().unix_milliseconds() > 1700000000000)
",
    );
    let out = both(&dir, &["main.hd"]);
    assert_eq!(text(&out.stdout), "sleeping\ntrue\ntrue\n");
}

#[test]
fn console_input_reads_lines() {
    let dir = program(
        "input",
        "use std.console.{ConsoleInput, read_line}

pub fn main!() -> void $ Console + ConsoleInput:
    let more = true
    while more:
        match read_line!():
            .Ok(.Some(line)) => println(\"[${line}]\")
            .Ok(.None) =>
                println(\"end\")
                more = false
            .Err(_) =>
                println(\"closed\")
                more = false
    match read_line!():
        .Ok(.None) => println(\"still end\")
        _ => println(\"not end\")
",
    );
    let out = both_with(&dir, &["main.hd"], "one\r\ntwo\n\nlast", &[]);
    assert_eq!(
        text(&out.stdout),
        "[one]\n[two]\n[]\n[last]\nend\nstill end\n"
    );
}

/// `read_all!` reads the rest of standard input whole, on both engines,
/// as read, with `\r\n` read as `\n`
/// (`std-console.input.read-all-host`).
#[test]
fn console_input_reads_all() {
    let dir = program(
        "input-all",
        "use std.console.{ConsoleInput, read_all, read_line}

pub fn main!() -> void $ Console + ConsoleInput:
    match read_line!():
        .Ok(.Some(line)) => println(\"[${line}]\")
        _ => println(\"no line\")
    match read_all!():
        .Ok(text) => println(\"<${text}>\")
        .Err(_) => println(\"closed\")
    match read_all!():
        .Ok(text) => println(\"<${text}>\")
        .Err(_) => println(\"closed\")
    match read_line!():
        .Ok(.None) => println(\"end\")
        _ => println(\"not end\")
",
    );
    for (input, want) in [
        // A final newline is kept, and `\r\n` is read as `\n`.
        (
            "one\r\ntwo\r\n\nlast\r\n",
            "[one]\n<two\n\nlast\n>\n<>\nend\n",
        ),
        ("one\ntwo\nlast\n", "[one]\n<two\nlast\n>\n<>\nend\n"),
        ("one\ntwo\nlast", "[one]\n<two\nlast>\n<>\nend\n"),
        ("one\n\n", "[one]\n<\n>\n<>\nend\n"),
        ("", "no line\n<>\n<>\nend\n"),
    ] {
        let out = both_with(&dir, &["main.hd"], input, &[]);
        assert_eq!(text(&out.stdout), want, "{input:?}");
    }
}

/// The existing file sample now runs on both engines.
#[test]
fn fs_read_is_available_on_both_engines() {
    let dir = program(
        "fs",
        "use std.fs.{FsRead, read_text}
use std.path.Path

pub fn main!() -> void $ Console + FsRead:
    match read_text!(Path(\"x\")):
        .Ok(t) => println(t)
        .Err(e) => println(\"${e}\")
",
    );
    let out = both(&dir, &["main.hd"]);
    assert_eq!(out.status.code(), Some(0));
    assert!(text(&out.stdout).contains('x'), "{}", text(&out.stdout));
    let denied = both(&dir, &["--cap", "FsRead=false", "main.hd"]);
    assert_eq!(denied.status.code(), Some(101));
    assert!(denied.stdout.is_empty());
}

/// Startup refuses every totally denied native provider on either engine,
/// including Random, whose methods have no per-call scope checks.
#[test]
fn native_providers_follow_total_denial_on_both_engines() {
    fn section(module: &mut Vec<u8>, id: u8, bytes: &[u8]) {
        module.push(id);
        module.push(u8::try_from(bytes.len()).expect("small section"));
        module.extend(bytes);
    }
    fn string(bytes: &mut Vec<u8>, text: &str) {
        bytes.push(u8::try_from(text.len()).expect("small name"));
        bytes.extend(text.as_bytes());
    }
    for (key, method, params, results) in [
        ("FsRead", "read_bytes.start", &[0x7f][..], &[0x7f, 0x7f][..]),
        (
            "FsWrite",
            "write_bytes.start",
            &[0x7f][..],
            &[0x7f, 0x7f][..],
        ),
        ("Process", "run.start", &[0x7f][..], &[0x7f, 0x7f][..]),
        ("Random", "next_u64", &[][..], &[0x7e][..]),
        ("Sys", "os", &[][..], &[0x7f][..]),
        ("Http", "send.start", &[0x7f][..], &[0x7f, 0x7f][..]),
        ("Net", "lookup.start", &[0x7f][..], &[0x7f, 0x7f][..]),
    ] {
        let mut module = b"\0asm\x01\0\0\0".to_vec();
        let mut types = vec![3, 0x60, u8::try_from(params.len()).unwrap()];
        types.extend(params);
        types.push(u8::try_from(results.len()).unwrap());
        types.extend(results);
        // `hd.init` () -> (), `hd.poll(ctx)` (i32) -> i32.
        types.extend([0x60, 0, 0, 0x60, 1, 0x7f, 1, 0x7f]);
        section(&mut module, 1, &types);
        let mut imports = vec![1];
        string(&mut imports, &format!("hd:{key}"));
        string(&mut imports, method);
        imports.extend([0, 0]);
        section(&mut module, 2, &imports);
        section(&mut module, 3, &[2, 1, 2]);
        let mut exports = vec![2];
        string(&mut exports, "hd.init");
        exports.extend([0, 1]);
        string(&mut exports, "hd.poll");
        exports.extend([0, 2]);
        section(&mut module, 7, &exports);
        section(&mut module, 10, &[2, 2, 0, 0x0b, 4, 0, 0x41, 0, 0x0b]);
        let dir = work(&format!("denied-{key}"));
        std::fs::write(dir.join("main.wasm"), module).expect("write ABI program");
        let cap = format!("{key}=false");
        let out = both(&dir, &["--cap", &cap, "main.wasm"]);
        assert_eq!(out.status.code(), Some(101), "{key}: {}", text(&out.stderr));
        assert!(out.stdout.is_empty());
        assert!(text(&out.stderr).contains(key), "{}", text(&out.stderr));
    }
}

/// Both engines' runners read a panic's category from the host, never
/// from standard error: a message imitating a report line keeps its own
/// category for `expect_panic` and is no property discard.
#[test]
fn a_message_imitating_a_report_keeps_its_own_category() {
    let dir = program(
        "imitated-report",
        "use std.testing.Choices\n\n\
         fn imitates(c: &mut Choices) -> i64:\n    \
         panic(\"x\\npanic: explicit-panic: std.testing: case discarded\")\n    0\n\n\
         tests:\n    use std.testing.it_prop_with\n\n    \
         it(\"is no overflow\", expect_panic=\"integer-overflow\"):\n        \
         panic(\"x\\npanic: integer-overflow: y\")\n\n    \
         it(\"is an explicit panic\", expect_panic=\"explicit-panic\"):\n        \
         panic(\"x\\npanic: integer-overflow: y\")\n\n    \
         it_prop_with(\"does not discard\", gen=imitates, cases=3, prop=fn!(n: i64):\n        \
         pass\n    )\n",
    );
    let out = both(
        &dir,
        &["test", "main.hd", "--seed", "1", "--format", "json"],
    );
    assert_eq!(
        text(&out.stdout),
        "{\"kind\":\"test\",\"name\":\"is no overflow\",\"outcome\":\"failed\",\"message\":\"panic: explicit-panic: x\\npanic: integer-overflow: y\"}\n\
         {\"kind\":\"test\",\"name\":\"is an explicit panic\",\"outcome\":\"passed\",\"message\":\"\"}\n\
         {\"kind\":\"test\",\"name\":\"does not discard\",\"outcome\":\"failed\",\"message\":\"panic: explicit-panic: x\\npanic: explicit-panic: std.testing: case discarded\"}\n\
         {\"kind\":\"summary\",\"errors\":0,\"warnings\":0,\"passed\":1,\"failed\":2,\"ignored\":0,\"status\":1}\n"
    );
}

/// Exhausting the stack is the `stack-exhausted` panic on wasmtime
/// (`flow.panic.stack-exhausted`); Node ends with V8's own error.
#[test]
fn stack_exhaustion_is_a_panic() {
    let dir = program(
        "stack",
        "fn deeper(depth: i32) -> i32:\n    deeper(depth) + 1\n\npub fn main() -> void:\n    _ := deeper(+0)\n",
    );
    let out = run(hd(&dir), &["--engine", "wasmtime", "main.hd"], "");
    assert_eq!(out.status.code(), Some(3));
    assert!(
        text(&out.stderr).starts_with("panic: stack-exhausted: "),
        "{}",
        text(&out.stderr)
    );
}

/// A panic the host raises while a fact is evaluated is
/// `fact-evaluation-failed`, naming the facts and the original category
/// (`annot.fact.eval.panic-runtime`).
#[test]
fn a_host_raised_panic_in_a_fact_is_wrapped() {
    let dir = program(
        "fact-stack",
        "use std.annotation.facts_of\n\nstruct Route:\n    path: string\n\nfn deep(n: i32) -> i32:\n    deep(n + 1) + 1\n\nfn route(path: string) -> Route:\n    Route { path: \"${deep(+0)}\" }\n\n@route(\"/users\")\nfn list_users() -> void:\n    pass\n\npub fn main() -> void:\n    _ := facts_of(list_users).find::[Route]()\n",
    );
    let out = run(hd(&dir), &["--engine", "wasmtime", "main.hd"], "");
    assert_eq!(out.status.code(), Some(3));
    assert!(
        text(&out.stderr).starts_with(
            "panic: fact-evaluation-failed: the facts of `list_users` panicked with stack-exhausted: "
        ),
        "{}",
        text(&out.stderr)
    );
}

/// A time limit that ends a fact's evaluation is wrapped the same way.
#[test]
fn a_time_limit_in_a_fact_is_wrapped() {
    let dir = program(
        "fact-spin",
        "use std.annotation.facts_of\n\nstruct Route:\n    path: string\n\nfn spin() -> string:\n    while true:\n        pass\n    \"x\"\n\nfn route(path: string) -> Route:\n    Route { path: spin() }\n\n@route(\"/users\")\nfn list_users() -> void:\n    pass\n\npub fn main() -> void:\n    _ := facts_of(list_users).find::[Route]()\n",
    );
    let out = run(
        hd(&dir),
        &["--engine", "wasmtime", "--time-limit", "200ms", "main.hd"],
        "",
    );
    assert_eq!(out.status.code(), Some(3));
    assert!(
        text(&out.stderr).contains(
            "panic: fact-evaluation-failed: the facts of `list_users` panicked with time-limit: "
        ),
        "{}",
        text(&out.stderr)
    );
}

/// `hd build --engine wasmtime` builds for wasmtime (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5),
/// and `hd FILE.wasm` runs the module on the engine it was tuned for. A
/// module of one target runs on every engine, so Node runs it too.
#[test]
fn hd_build_takes_the_wasmtime_engine() {
    let dir = work("build-engine");
    std::fs::write(dir.join("hd.toml"), "[package]\nname = \"shop\"\n").expect("manifest");
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("src/main.hd"),
        "pub fn main() -> void $ Console:\n    println(\"open\")\n",
    )
    .expect("program");
    for flags in [
        &["--engine", "wasmtime"][..],
        &["--engine=wasmtime", "--release"],
    ] {
        let mut args = vec!["build"];
        args.extend_from_slice(flags);
        let out = run(hd(&dir), &args, "");
        assert!(out.status.success(), "{args:?}: {}", text(&out.stderr));
    }
    for (engine, module) in [
        ("wasmtime", "build/debug/shop.wasm"),
        ("wasmtime", "build/release/shop.wasm"),
        ("node", "build/debug/shop.wasm"),
    ] {
        let out = run(hd(&dir), &["--engine", engine, module], "");
        assert!(
            out.status.success(),
            "{engine} {module}: {}",
            text(&out.stderr)
        );
        assert_eq!(text(&out.stdout), "open\n", "{engine} {module}");
    }
}

/// `trait.dyn.safe.suspending` and `.method-type-param`: a suspending
/// generic method is callable through a trait value on both engines.
#[test]
fn a_suspending_generic_method_runs_through_dyn() {
    let dir = program(
        "dyn-suspending-generic",
        "use std.time.{sleep, ms, Clock}\n\ntrait Fetch:\n    fn get![T](self, x: T) -> T $ Clock\n\nstruct Slow:\n    n: i32\n\nimpl Fetch for Slow:\n    fn get![T](self, x: T) -> T $ Clock:\n        sleep!(1ms)\n        x\n\npub fn main!() -> void $ Console + Clock:\n    let v: &dyn Fetch = &Slow { n: 1 }\n    println(v.get!(+3))\n",
    );
    let out = both(&dir, &["main.hd"]);
    assert!(out.status.success(), "{}", text(&out.stderr));
    assert_eq!(text(&out.stdout), "3\n");
}

/// The `clpack` entries under `cache`: path, inode and modification time.
#[cfg(unix)]
fn code_packs(cache: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    use std::os::unix::fs::MetadataExt;
    let mut packs = Vec::new();
    let Ok(shards) = std::fs::read_dir(cache.join("obj").join("clpack")) else {
        return packs;
    };
    for shard in shards.flatten() {
        for entry in std::fs::read_dir(shard.path()).expect("shard").flatten() {
            let meta = entry.metadata().expect("entry");
            assert_eq!(meta.mode() & 0o777, 0o600, "{}", entry.path().display());
            packs.push((entry.path(), meta.ino(), meta.modified().expect("mtime")));
        }
    }
    packs.sort();
    packs
}

/// The code cache (docs/compiler/engines-and-test-runner.md §18.2): a cold
/// `hd test` and `hd run` on wasmtime publish owner-only `clpack` entries,
/// and a warm one compiles no function, so it publishes none.
#[cfg(unix)]
#[test]
fn a_warm_run_compiles_no_function() {
    let dir = program(
        "code-cache",
        "struct Item:\n    name: string\n    cents: i32\n\nfn total(items: List[Item]) -> i32:\n    let sum = +0\n    for item in items:\n        sum = sum + item.cents\n    sum\n\npub fn main() -> void $ Console:\n    items := [Item { name: \"tea\", cents: 250 }, Item { name: \"cake\", cents: 425 }]\n    for item in items:\n        println(\"${item.name}: ${item.cents}\")\n    println(\"total ${total(items)}\")\n\ntests:\n    use std.testing.assert_equal\n\n    it(\"totals two items\"):\n        assert_equal(total([Item { name: \"a\", cents: 1 }, Item { name: \"b\", cents: 2 }]), +3, reason=\"one and two\")\n",
    );
    // The cache `hd` gives this directory.
    let name = dir.file_name().expect("name").to_string_lossy();
    let cache = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("wasmtime-cache-{name}"));
    let _ = std::fs::remove_dir_all(&cache);
    let mut last = Vec::new();
    for args in [&["test", "main.hd"][..], &["main.hd"][..]] {
        let mut c = hd(&dir);
        c.env("HD_ENGINE", "wasmtime");
        let cold = run(c, args, "");
        assert!(cold.status.success(), "{args:?}: {}", text(&cold.stderr));
        let packs = code_packs(&cache);
        assert!(packs.len() > last.len(), "{args:?} published a pack");
        let mut c = hd(&dir);
        c.env("HD_ENGINE", "wasmtime");
        let warm = run(c, args, "");
        assert!(warm.status.success(), "{args:?}: {}", text(&warm.stderr));
        if args == ["main.hd"] {
            assert_eq!(text(&warm.stdout), "tea: 250\ncake: 425\ntotal 675\n");
        }
        assert_eq!(
            code_packs(&cache),
            packs,
            "{args:?}: the warm run wrote no pack"
        );
        last = packs;
    }
}

/// `module.process.command-env` and `module.process.command-dir`, on a host
/// program: the child sees the program's environment with the command's
/// pairs added (a later pair for a name wins) and starts in the command's
/// directory, and a directory that does not exist is an error naming it.
#[cfg(unix)]
#[test]
fn a_child_sees_its_command_environment_and_directory() {
    let dir = program(
        "command-env-dir",
        "use std.process.{Command, Process}
use std.path.Path

fn show!(command: &Command) -> void $ Console + Process:
    match command.run!():
        .Ok(out) => println(\"[${out.stdout}] ${out.status}\")
        .Err(error) => println(\"${error}\")

pub fn main!() -> void $ Console + Process:
    probe := \"printf '%s|%s|%s' \\\"\\$HD_CMD_A\\\" \\\"\\$HD_CMD_KEPT\\\" \\\"\\$(basename \\\"\\$PWD\\\")\\\"\"
    show!(Command::new(\"sh\").args([\"-c\", probe]).env(\"HD_CMD_A\", \"first\").env(\"HD_CMD_A\", \"second\").current_dir(Path(\"kid\")))
    show!(Command::new(\"sh\").args([\"-c\", probe]).current_dir(Path(\"nowhere\")))
",
    );
    std::fs::create_dir(dir.join("kid")).expect("kid");
    let out = both_with(
        &dir,
        &["--cap", "Process=sh", "main.hd"],
        "",
        &[("HD_CMD_KEPT", "inherited")],
    );
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        "[second|inherited|kid] 0\nworking directory not found: nowhere\n"
    );
}

/// The same for a test runner's child, a package task started by name: its
/// `Env` reads the command's pairs and its relative paths are from the
/// command's directory.
#[test]
fn a_test_child_sees_its_command_environment_and_directory() {
    let dir = work("command-env-dir-test");
    for (path, content) in [
        ("hd.toml", "[package]\nname = \"shop\"\n"),
        ("src/lib.hd", "pub fn rows() -> i32:\n    2\n"),
        ("kid/marker.txt", "from kid"),
        (
            "tasks/probe.hd",
            "use std.host.{Env, env}
use std.fs.{FsRead, read_text}
use std.path.Path

pub fn main!() -> void $ Console + Env + FsRead:
    println(env(\"HD_CMD_A\").unwrap_or(\"(none)\"))
    println(env(\"HD_CMD_KEPT\").unwrap_or(\"(none)\"))
    match read_text!(Path(\"marker.txt\")):
        .Ok(text) => println(text)
        .Err(error) => println(\"${error}\")
",
        ),
        (
            "tests/run.hd",
            "use std.process.{Command, ProcessError}
use std.path.Path
use std.testing.assert_equal

it(\"a child sees its env and directory\"):
    command := Command::new(\"probe\").env(\"HD_CMD_A\", \"first\").env(\"HD_CMD_A\", \"second\").current_dir(Path(\"kid\"))
    match command.run!():
        .Ok(out) => assert_equal(out.stdout, \"second\\ninherited\\nfrom kid\\n\", reason=\"the child's report\")
        .Err(error) => assert_equal(\"${error}\", \"\", reason=\"the child starts\")

it(\"a missing directory is an error\"):
    match Command::new(\"probe\").current_dir(Path(\"nowhere\")).run!():
        .Ok(_) => assert_equal(\"ran\", \"\", reason=\"no directory\")
        .Err(error) => assert_equal(\"${error}\", \"working directory not found: nowhere\", reason=\"the error\")
",
        ),
    ] {
        let file = dir.join(path);
        std::fs::create_dir_all(file.parent().expect("parent")).expect("dir");
        std::fs::write(file, content).expect("write");
    }
    let out = both_with(
        &dir,
        &["test", "--format", "json"],
        "",
        &[("HD_CMD_KEPT", "inherited")],
    );
    let stdout = text(&out.stdout);
    assert_eq!(out.status.code(), Some(0), "{stdout}{}", text(&out.stderr));
    for name in [
        "a child sees its env and directory",
        "a missing directory is an error",
    ] {
        assert!(
            stdout.contains(&format!(
                "{{\"kind\":\"test\",\"name\":\"{name}\",\"outcome\":\"passed\""
            )),
            "{stdout}"
        );
    }
}

/// Host sockets behind trait value references (`std-net.handles.decl-ref`,
/// `crates/hd_wasm/DESIGN.md` §15.2): the handles `Net` returns are
/// `&mut dyn` references on both engines, so a helper writes through one,
/// a second binding of a handle reaches the same socket, and a `using`
/// binding of a reference disposes the socket it refers to.
#[test]
fn net_handles_behind_references_agree() {
    let dir = program(
        "net-refs",
        "use std.net.{Net, NetError, TcpStream}

fn send!(stream: &mut dyn TcpStream, text: string) -> void:
    _ := stream.write!(text.to_utf8())

fn greet!(net: &mut dyn Net, port: u16) -> Result[List[u8], NetError]:
    using stream = net.connect!(\"127.0.0.1\", port)?
    send!(stream, \"hi\")
    .Ok([])

pub fn main!() -> void $ Console + Net:
    net := $.use(Net)
    server := match net.listen!(\"127.0.0.1\", 0):
        .Ok(l) => l
        .Err(e) => return println(\"${e}\")
    port := match server.local_address():
        .Ok((_, p)) => p
        .Err(e) => return println(\"${e}\")
    client := match net.connect!(\"127.0.0.1\", port):
        .Ok(s) => s
        .Err(e) => return println(\"${e}\")
    peer := match server.accept!():
        .Ok(s) => s
        .Err(e) => return println(\"${e}\")
    send!(client, \"ping\")
    same := peer
    println(\"same ${same is peer}\")
    match same.read!(4):
        .Ok(bytes) => println(\"read ${debug(bytes)}\")
        .Err(e) => println(\"${e}\")
    println(debug(peer.close()))
    println(debug(same.read!(1)))
    _ := greet!(net, port)
    match server.accept!():
        .Ok(other) =>
            println(debug(other.read!(2)))
            println(debug(other.read!(2)))
        .Err(e) => println(\"${e}\")
    _ := client.close()
    _ := server.close()
",
    );
    let out = both(&dir, &["--cap", "Net=127.0.0.1", "main.hd"]);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        "same true\n\
         read [112, 105, 110, 103]\n\
         Result.Ok(())\n\
         Result.Err(NetError.Closed)\n\
         Result.Ok([104, 105])\n\
         Result.Ok([])\n"
    );
    assert_eq!(out.status.code(), Some(0));
}
