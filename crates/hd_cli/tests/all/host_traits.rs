//! The default profile's host traits end to end (`crates/hd_run_wasmtime/DESIGN.md` §17,
//! spec/cli/command-line.md "Host Capabilities"): each program runs under
//! `hd` on V8, its host calls crossing the exchange buffer with the codecs
//! of §17.4, and each provider asking the grant first
//! (`cli.host.default-profile.granted`). Every program works in a scratch
//! directory below the target directory.

use std::io::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use hd_testkit::Scratch;

/// A fresh, empty work directory.
fn work(name: &str) -> Scratch {
    Scratch::new(env!("CARGO_TARGET_TMPDIR"), name)
}

fn hd(dir: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_hd"));
    c.env("HD_CACHE", dir.with_extension("cache"))
        .env_remove("HD_JOBS")
        .current_dir(dir)
        .args(args);
    c
}

fn run(dir: &Path, args: &[&str]) -> Output {
    hd(dir, args).output().expect("run hd")
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn write(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, content).expect("write");
}

/// `Args` (`std-host.args.*`, `cli.args.pass`): `program` is the FILE the
/// command ran, and `list` the words after `--`, in order.
#[test]
fn args_hold_the_program_and_its_arguments() {
    let dir = work("host-args");
    write(
        &dir.join("main.hd"),
        "use std.host.{Args, args}\n\npub fn main() -> void $ Console + Args:\n    \
         println($.use(Args).program())\n    for word in args():\n        println(\"[${word}]\")\n",
    );
    let out = run(&dir, &["main.hd", "--", "a", "b c", ""]);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(text(&out.stdout), "main.hd\n[a]\n[b c]\n[]\n");
    assert!(out.status.success());
}

const ENV_PROGRAM: &str = "use std.host.{Env, env}

fn show(name: string) -> string $ Env:
    match env(name):
        .Some(value) => value
        .None => \"(none)\"

pub fn main() -> void $ Console + Env:
    println(show(\"HD_T176_SEEN\"))
    println(show(\"HD_T176_HIDDEN\"))
    println(show(\"HD_T176_HIDDEN\"))
    println(show(\"HD_T176_UNSET\"))
    println(\"${$.use(Env).names().len()}\")
";

/// `Env` (`std-host.env.*`, `cli.cap.env.*`): a granted variable reads its
/// value, an ungranted one reads as unset with one notice per name, and
/// `names` lists only granted variables. With no grant, every variable
/// reads.
#[test]
fn env_reads_granted_variables_and_names_the_rest() {
    let dir = work("host-env");
    write(&dir.join("main.hd"), ENV_PROGRAM);
    let env = |c: &mut Command| {
        c.env("HD_T176_SEEN", "yes")
            .env("HD_T176_HIDDEN", "no")
            .env_remove("HD_T176_UNSET");
    };
    let mut c = hd(
        &dir,
        &["--cap", "Env=HD_T176_SEEN,HD_T176_UNSET", "main.hd"],
    );
    env(&mut c);
    let out = c.output().expect("run hd");
    assert_eq!(text(&out.stdout), "yes\n(none)\n(none)\n(none)\n1\n");
    assert_eq!(
        text(&out.stderr),
        "hd: env HD_T176_HIDDEN is set but not granted; run with --cap Env=HD_T176_HIDDEN\n"
    );
    let mut c = hd(&dir, &["main.hd"]);
    env(&mut c);
    let out = c.output().expect("run hd");
    assert!(
        text(&out.stdout).starts_with("yes\nno\nno\n(none)\n"),
        "{}",
        text(&out.stdout)
    );
    assert_eq!(text(&out.stderr), "");
}

const FILES_PROGRAM: &str =
    "use std.fs.{EntryKind, FsError, FsRead, FsWrite, read_text, write_text}
use std.path.Path

fn kind(k: EntryKind) -> string:
    match k:
        .File => \"file\"
        .Directory => \"dir\"
        .Symlink => \"link\"

fn outcome(r: Result[void, FsError]) -> string:
    match r:
        .Ok(_) => \"ok\"
        .Err(e) => \"${e}\"

pub fn main!() -> void $ Console + FsRead + FsWrite:
    let mut files = $.use(FsWrite)
    println(outcome(files.create_dir_all!(Path(\"out/sub\"))))
    println(outcome(write_text!(Path(\"out/sub/a.txt\"), \"h\\u{E9}llo\")))
    println(outcome(files.append_text!(Path(\"out/sub/a.txt\"), \"!\")))
    println(outcome(files.write_bytes!(Path(\"out/sub/b.bin\"), [0, 200, 255])))
    match read_text!(Path(\"out/sub/a.txt\")):
        .Ok(text) => println(text)
        .Err(e) => println(\"${e}\")
    match $.use(FsRead).read_bytes!(Path(\"out/sub/b.bin\")):
        .Ok(bytes) => println(\"${bytes.len()} ${bytes[1]}\")
        .Err(e) => println(\"${e}\")
    match $.use(FsRead).list_dir!(Path(\"out/sub\")):
        .Ok(entries) =>
            for entry in entries:
                println(\"${entry.path} ${kind(entry.kind)} ${entry.size}\")
        .Err(e) => println(\"${e}\")
    match $.use(FsRead).stat!(Path(\"out/none\")):
        .Ok(.None) => println(\"no entry\")
        .Ok(.Some(_)) => println(\"an entry\")
        .Err(e) => println(\"${e}\")
    println(outcome(files.rename!(Path(\"out/sub/a.txt\"), Path(\"out/c.txt\"))))
    println(outcome(files.remove!(Path(\"out/sub/b.bin\"))))
    println(outcome(write_text!(Path(\"denied.txt\"), \"x\")))
    match read_text!(Path(\"secret.txt\")):
        .Ok(_) => println(\"read\")
        .Err(.NotGranted(_)) => println(\"read not granted\")
        .Err(e) => println(\"${e}\")
    match read_text!(Path(\"out/missing.txt\")):
        .Ok(_) => println(\"read\")
        .Err(.NotFound(_)) => println(\"not found\")
        .Err(e) => println(\"${e}\")
";

/// `FsRead` and `FsWrite` (`std-fs.read.*`, `std-fs.write.*`,
/// `cli.cap.partial.refuse`): reads and writes under the granted `out`,
/// and `NotGranted` outside it, never touching the refused path.
#[test]
fn files_are_read_and_written_inside_the_grant() {
    let dir = work("host-files");
    write(&dir.join("main.hd"), FILES_PROGRAM);
    write(&dir.join("secret.txt"), "secret");
    let out = run(
        &dir,
        &["--cap", "FsRead=out", "--cap", "FsWrite=out", "main.hd"],
    );
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        "ok\nok\nok\nok\nh\u{e9}llo!\n3 200\nout/sub/a.txt file 7\nout/sub/b.bin file 3\n\
         no entry\nok\nok\naccess to denied.txt is not granted\nread not granted\nnot found\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.join("out/c.txt")).expect("renamed"),
        "h\u{e9}llo!"
    );
    assert!(!dir.join("out/sub/b.bin").exists(), "removed");
    assert!(!dir.join("denied.txt").exists(), "never written");
}

/// `ConsoleInput` (`std-console.input.*`,
/// `cli.host.default-profile.input-closed`): each line of standard input
/// without its ending, then `.None` at the end, and again after it.
#[test]
fn console_input_reads_standard_input_by_lines() {
    let dir = work("host-input");
    write(
        &dir.join("main.hd"),
        "use std.console.{ConsoleInput, read_line}\n\npub fn main!() -> void $ Console + ConsoleInput:\n    \
         let more = true\n    while more:\n        match read_line!():\n            \
         .Ok(.Some(line)) => println(\"[${line}]\")\n            .Ok(.None) =>\n                \
         println(\"end\")\n                more = false\n            .Err(_) =>\n                \
         println(\"closed\")\n                more = false\n    match read_line!():\n        \
         .Ok(.None) => println(\"still end\")\n        _ => println(\"not end\")\n",
    );
    let mut child = hd(&dir, &["main.hd"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run hd");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(b"first\r\nsecond\n\nlast")
        .expect("feed stdin");
    let out = child.wait_with_output().expect("wait");
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        "[first]\n[second]\n[]\n[last]\nend\nstill end\n"
    );
}

/// The default profile's `Process` (`module.process.*`,
/// `cli.cap.scope.process`): a granted program runs with the given input
/// and its output and status come back; a program outside the grant is
/// `NotGranted`, and a missing one `NotFound`.
#[test]
fn process_runs_a_granted_host_program() {
    let dir = work("host-process");
    write(
        &dir.join("main.hd"),
        "use std.process.{Command, Process}\n\nfn start!(program: string, args: List[string], input: string) -> string $ Process:\n    \
         match Command::new(program).args(args).stdin(input).run!():\n        \
         .Ok(out) => \"${out.status} [${out.stdout}] [${out.stderr}]\"\n        \
         .Err(e) => \"${e}\"\n\npub fn main!() -> void $ Console + Process:\n    \
         println(start!(\"sh\", [\"-c\", \"cat; echo oops >&2; exit 3\"], \"fed\"))\n    \
         println(start!(\"ls\", [], \"\"))\n    println(start!(\"hd-no-such-program-176\", [], \"\"))\n",
    );
    let out = run(
        &dir,
        &["--cap", "Process=sh,hd-no-such-program-176", "main.hd"],
    );
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        "3 [fed] [oops\n]\nprogram not granted\nprogram not found: hd-no-such-program-176\n"
    );
}

/// The default `TestRunner` keeps snapshot files
/// (`std-testing.snapshot-file.*`): a missing one fails, `--update`
/// records it under `__snapshots__/pkg` for `src/lib.hd`, it then passes,
/// and a differing one fails again.
#[test]
fn the_default_test_runner_keeps_snapshot_files() {
    let dir = work("host-snapshots");
    write(
        &dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nArgs = true\nConsole = true\nConsoleInput = true\nFsRead = true\n",
    );
    write(
        &dir.join("src/lib.hd"),
        "pub fn greet(name: string) -> string:\n    \"hello, ${name}\"\n\ntests:\n    \
         use std.testing.snapshot_file\n\n    it(\"Greets Ada!\"):\n        snapshot_file(greet(\"Ada\"))\n",
    );
    let snap = dir.join("__snapshots__/pkg/greets-ada--1.snap");
    let out = run(&dir, &["test"]);
    assert_eq!(out.status.code(), Some(1), "{}", text(&out.stderr));
    assert!(
        text(&out.stdout).contains("__snapshots__/pkg/greets-ada--1.snap is missing"),
        "{}",
        text(&out.stdout)
    );
    assert!(!snap.exists());
    let out = run(&dir, &["test", "--update"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stdout));
    assert_eq!(
        std::fs::read_to_string(&snap).expect("recorded"),
        "hello, Ada"
    );
    let out = run(&dir, &["test"]);
    assert_eq!(out.status.code(), Some(0), "{}", text(&out.stdout));
    write(&snap, "hello, Bob");
    let out = run(&dir, &["test"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(
        text(&out.stdout).contains("differs from the snapshot file"),
        "{}",
        text(&out.stdout)
    );
}

/// The test runner's `Process` (`cli.test.process.tasks`,
/// `std-testing.hd-run.*`): an integration test runs a task by its name,
/// with arguments and standard input, in the package directory; a name
/// that names no executable or task panics.
#[test]
fn the_test_runner_runs_a_task_by_name() {
    let dir = work("host-tasks");
    write(
        &dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nArgs = true\nConsole = true\nConsoleInput = true\nFsRead = true\n\n[dev-capabilities]\nProcess = true\n",
    );
    write(&dir.join("src/lib.hd"), "pub fn rows() -> i32:\n    2\n");
    write(&dir.join("data.txt"), "from the package");
    write(
        &dir.join("tasks/seed.hd"),
        "use std.host.{Args, args}\nuse std.console.{ConsoleInput, read_line}\n\
         use std.fs.{FsRead, read_text}\nuse std.path.Path\n\n\
         pub fn main!() -> void $ Console + Args + ConsoleInput + FsRead:\n    \
         match read_line!():\n        .Ok(.Some(line)) => println(\"seeded ${args().len()} ${line}\")\n        \
         _ => println(\"no input\")\n    \
         match read_text!(Path(\"data.txt\")):\n        .Ok(text) => println(text)\n        \
         .Err(e) => println(\"${e}\")\n",
    );
    write(
        &dir.join("tests/run.hd"),
        "use std.process.Command\nuse std.testing.{assert_equal, hd_run}\n\nit(\"runs the seed task\"):\n    \
         out := hd_run!(*Command::new(\"seed\").arg(\"x\").stdin(\"rows\\n\"))\n    \
         assert_equal(out.stdout, \"seeded 1 rows\\nfrom the package\\n\", reason=\"the task's report\")\n    \
         assert_equal(out.status, 0, reason=\"a clean exit\")\n\n\
         it(\"names no program\"):\n    _ := hd_run!(*Command::new(\"nothing\"))\n",
    );
    let out = run(&dir, &["test", "--format", "json"]);
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains(r#"{"kind":"test","name":"runs the seed task","outcome":"passed""#),
        "{stdout}{}",
        text(&out.stderr)
    );
    assert!(
        stdout.contains("the package has no executable or task named 'nothing'"),
        "{stdout}"
    );
    assert_eq!(out.status.code(), Some(1), "{stdout}");
}

const TIMEOUT_TESTS: &str = "use std.time.{Duration, ms, h}

fn budget() -> Duration: 1h

fn forever(stop: i32) -> i32:
    let total: i32 = 0
    while total != stop:
        total = (total + 1) % 1000
    total

tests:
    use std.testing.{assert, it_each}

    it(\"spins\", timeout=100ms):
        _ := forever(-1)

    it_each(\"rows\", [1, -1, 2], timeout=100ms, body=fn!(value: i32):
        assert(forever(value) == value, reason=\"ends\")
    )

    it(\"within its budget\", timeout=budget()):
        assert(forever(7) == 7, reason=\"ends\")

    it(\"no limit\", timeout=.None):
        assert(true, reason=\"runs\")
";

/// `TestRunner.report_timeout` (`std-testing.option.timeout-any-duration`,
/// `.timeout-at-run`, `std-testing.runner.timeout`): a body that never
/// returns fails with `time-limit` once past its timeout, a table's other
/// rows and the later cases still run, and a timeout from a call or
/// `.None` limits nothing.
#[test]
fn the_test_runner_fails_a_body_past_its_timeout() {
    let dir = work("host-test-timeout");
    write(&dir.join("timed.hd"), TIMEOUT_TESTS);
    let out = run(&dir, &["test", "timed.hd"]);
    assert_eq!(
        text(&out.stdout),
        "PANIC timed.hd:14: spins\n    \
         panic: time-limit: the test case ran longer than its timeout of 100ms\n    \
         repro: hd test timed.hd --filter \"spins\"\n\
         PANIC timed.hd:17: rows[1]\n    \
         panic: time-limit: the test case ran longer than its timeout of 100ms\n    \
         repro: hd test timed.hd --filter \"rows[1]\"\n\
         test result: FAILED. 4 passed; 2 failed; 0 ignored\n"
    );
    assert_eq!(out.status.code(), Some(1));
}

/// A host wait is cut at the timeout (`crates/hd_run_wasmtime/DESIGN.md` §17.8): an
/// integration test that sleeps past its timeout fails with `time-limit`,
/// with the output it wrote before, while one within it passes.
#[test]
fn a_sleep_past_the_timeout_fails_the_case() {
    let dir = work("host-test-sleep");
    write(
        &dir.join("hd.toml"),
        "[package]\nname = \"naps\"\n\n[dev-capabilities]\nClock = true\nConsole = true\n",
    );
    write(&dir.join("src/lib.hd"), "pub fn naps() -> i32:\n    2\n");
    write(
        &dir.join("tests/naps.hd"),
        "use std.time.{ms, s, sleep}\n\nit(\"oversleeps\", timeout=50ms):\n    \
         println(\"falling asleep\")\n    sleep!(10s)\n\n\
         it(\"naps\", timeout=10s):\n    sleep!(1ms)\n",
    );
    let out = run(&dir, &["test", "--format", "json"]);
    let stdout = text(&out.stdout);
    assert!(
        stdout.contains(
            r#"{"kind":"test","name":"oversleeps","outcome":"failed","message":"panic: time-limit: the test case ran longer than its timeout of 50ms""#
        ),
        "{stdout}{}",
        text(&out.stderr)
    );
    assert!(
        stdout.contains(r#"{"kind":"test","name":"naps","outcome":"passed""#),
        "{stdout}"
    );
    assert_eq!(out.status.code(), Some(1), "{stdout}");
}

const RANDOM_PROGRAM: &str = "use std.random.{Random, rng}

pub fn main() -> void $ Console + Random:
    let mut source = $.use(Random)
    println(\"${source.fill(16).len()} ${source.fill(100000).len()} ${source.fill(0).len()}\")
    let distinct = 0
    let previous = source.next_u64()
    for _ in 0..8:
        next := source.next_u64()
        if next != previous:
            distinct = distinct + 1
        previous = next
    println(\"${distinct}\")
    let mut r = rng()
    roll := r.int(1..=6)
    println(\"${roll >= 1 && roll <= 6}\")
";

/// `Random` (`std-random.next`, `.fill`, cli.host.default-profile): the
/// operating system's source fills any count of bytes and draws distinct
/// values, `rng()` seeds from it, and `--cap Random=false` refuses the
/// program before it starts (`cli.cap.total.refuse`).
#[test]
fn random_draws_from_the_host_source() {
    let dir = work("host-random");
    write(&dir.join("main.hd"), RANDOM_PROGRAM);
    let out = run(&dir, &["main.hd"]);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(text(&out.stdout), "16 100000 0\n8\ntrue\n");
    assert!(out.status.success());
    let denied = run(&dir, &["--cap", "Random=false", "main.hd"]);
    assert_eq!(text(&denied.stdout), "");
    assert!(
        text(&denied.stderr).contains("needs Random, which `--cap Random=false` denies"),
        "{}",
        text(&denied.stderr)
    );
    assert_eq!(denied.status.code(), Some(101));
}

const SYS_PROGRAM: &str = "use std.host.{Sys, HostError}

fn show(r: Result[string, HostError]) -> string:
    match r:
        .Ok(text) => text
        .Err(e) => \"${e}\"

pub fn main() -> void $ Console + Sys:
    sys := $.use(Sys)
    println(show(sys.os()))
    println(show(sys.arch()))
    println(\"${sys.hostname().is_ok()}\")
    match sys.cpu_count():
        .Ok(n) => println(\"${n > 0}\")
        .Err(e) => println(\"${e}\")
    match sys.hostname():
        .Err(.NotGranted(name)) => println(\"refused ${name}\")
        _ => println(\"read\")
";

/// `Sys` (`std-host.sys.*`, `cli.cap.scope.sys`): the host's operating system
/// and architecture by std's names, its host name and processor count;
/// a grant names the methods it covers, and another method returns
/// `NotGranted` with its name.
#[test]
fn sys_reads_the_host_inside_the_grant() {
    let dir = work("host-sys");
    write(&dir.join("main.hd"), SYS_PROGRAM);
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let out = run(&dir, &["main.hd"]);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        format!("{os}\n{arch}\ntrue\ntrue\nread\n")
    );
    assert!(out.status.success());
    let partial = run(&dir, &["--cap", "Sys=os,cpu_count", "main.hd"]);
    assert_eq!(
        text(&partial.stdout),
        format!(
            "{os}\nsys access to arch is not granted\n\
             false\ntrue\nrefused hostname\n"
        )
    );
    assert!(partial.status.success());
}

/// A loopback HTTP server for the `Http` tests, on a free port of
/// 127.0.0.1: `/ok` answers with two `Set-Cookie` headers, `/redirect`
/// redirects to `/ok`, `/loop` to itself, `/slow` answers after three
/// seconds, a `POST` echoes its body, and any other path is a 404.
fn http_server() -> u16 {
    use std::io::{BufRead as _, BufReader, Read as _};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().expect("clone"));
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    return;
                }
                let mut parts = line.split_whitespace();
                let (method, path) = (
                    parts.next().unwrap_or("").to_owned(),
                    parts.next().unwrap_or("").to_owned(),
                );
                let mut length = 0;
                let mut host = String::new();
                loop {
                    let mut h = String::new();
                    if reader.read_line(&mut h).is_err() || h.trim().is_empty() {
                        break;
                    }
                    if let Some((n, v)) = h.split_once(':') {
                        if n.eq_ignore_ascii_case("content-length") {
                            length = v.trim().parse().unwrap_or(0);
                        } else if n.eq_ignore_ascii_case("host") {
                            v.trim().clone_into(&mut host);
                        }
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let (status, headers, text) = match (method.as_str(), path.as_str()) {
                    ("POST", _) => (
                        "201 Created",
                        String::new(),
                        format!("got {}", String::from_utf8_lossy(&body)),
                    ),
                    (_, "/ok") => (
                        "200 OK",
                        "Set-Cookie: a=1\r\nSet-Cookie: b=2\r\n".to_owned(),
                        "hello".to_owned(),
                    ),
                    (_, "/redirect") => {
                        ("302 Found", "Location: /ok\r\n".to_owned(), String::new())
                    }
                    (_, "/loop") => ("302 Found", "Location: /loop\r\n".to_owned(), String::new()),
                    (_, "/host") => ("200 OK", String::new(), format!("host {host}")),
                    (_, "/slow") => {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        ("200 OK", String::new(), String::new())
                    }
                    _ => ("404 Not Found", String::new(), "missing".to_owned()),
                };
                let mut stream = stream;
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\nConnection: close\r\n\r\n{text}",
                    text.len()
                );
            });
        }
    });
    port
}

const HTTP_PROGRAM: &str = "use std.http.{Http, HttpError, Method, Request, Response, get}
use std.host.{Args, args}

fn show!(r: Result[Response, HttpError]) -> string:
    match r:
        .Ok(response) => \"${response.status} ${response.text()} ${debug(response.header(\"SET-COOKIE\"))} ${response.headers.len()}\"
        .Err(.Status(response)) => \"status error ${response.status} ${response.text()}\"
        .Err(e) => \"${debug(e)}\"

pub fn main!() -> void $ Console + Http + Args:
    base := args()[0]
    println(show!(get!(\"${base}/ok\")))
    println(show!(get!(\"${base}/redirect\")))
    println(show!(Request::get(\"${base}/redirect\").redirects(0).send!()))
    println(show!(get!(\"${base}/nothing\")))
    println(show!(get!(\"${base}/loop\")))
    println(show!(Request { method: Method.Post, url: \"${base}/echo\", headers: [(\"Content-Type\", \"text/plain\")], body: \"abc\".to_utf8() }.send!()))
    println(show!(get!(\"http://example.com/\")))
    println(show!(get!(\"ftp://127.0.0.1/\")))
    println(show!(get!(\"http://bücher.example/\")))
    println(show!(get!(\"http://xn--bcher-kva.EXAMPLE/\")))
";

/// `Http` (`std-http.send.*`, `cli.cap.scope.host`, `.redirect`): a
/// request to a granted host completes with every status as a response,
/// repeated headers kept; redirects are followed up to 10; a timeout, an
/// ungranted host and a URL that is not http are errors. Every request
/// goes to a loopback server; the ungranted host is refused before any
/// connection.
#[test]
fn http_sends_requests_inside_the_grant() {
    let port = http_server();
    let dir = work("host-http");
    write(&dir.join("main.hd"), HTTP_PROGRAM);
    let base = format!("http://127.0.0.1:{port}");
    let engines = [
        "node",
        #[cfg(feature = "wasmtime")]
        "wasmtime",
    ];
    for engine in engines {
        let out = run(
            &dir,
            &[
                "--engine",
                engine,
                "--cap",
                "Http=127.0.0.1",
                "main.hd",
                "--",
                &base,
            ],
        );
        assert_eq!(text(&out.stderr), "");
        assert_eq!(
            text(&out.stdout),
            format!(
                "200 hello Option.Some(\"a=1\") 4\n\
             200 hello Option.Some(\"a=1\") 4\n\
             302  Option.None 3\n\
             status error 404 missing\n\
             HttpError.TooManyRedirects(url=\"{base}/loop\")\n\
             201 got abc Option.None 2\n\
             HttpError.NotGranted(host=\"example.com\")\n\
             HttpError.InvalidUrl(url=\"ftp://127.0.0.1/\")\n\
             HttpError.InvalidUrl(url=\"http://bücher.example/\")\n\
             HttpError.NotGranted(host=\"xn--bcher-kva.example\")\n"
            )
        );
        assert!(out.status.success(), "{engine}: {}", text(&out.stderr));
    }
    // A grant for another port of the host covers none of these requests.
    let other = run(
        &dir,
        &[
            "--cap",
            &format!("Http=127.0.0.1:{}", port ^ 1),
            "main.hd",
            "--",
            &base,
        ],
    );
    assert!(
        text(&other.stdout).starts_with("HttpError.NotGranted(host=\"127.0.0.1\")\n"),
        "{}",
        text(&other.stdout)
    );
}

/// `Http` in an integration test (`cli.test.env.integration`) under the
/// test grant of `[dev-capabilities]` (`cli.cap.dev.entries`), and
/// in a plain function through `block_on`, which the request never leaves
/// waiting on the event loop.
#[test]
fn http_reaches_a_granted_host_from_an_integration_test() {
    let port = http_server();
    let dir = work("host-http-test");
    write(
        &dir.join("hd.toml"),
        "[package]\nname = \"fetcher\"\n\n[capabilities]\nArgs = true\nConsole = true\nConsoleInput = true\nFsRead = true\n\n[dev-capabilities]\nHttp = [\"127.0.0.1\"]\n",
    );
    write(
        &dir.join("src/lib.hd"),
        "use std.http.{Http, get}\nuse std.task.block_on\n\n\
         pub fn status_of(url: string) -> u16 $ Http:\n    \
         match block_on(get(url)):\n        .Completed(.Ok(response)) => response.status\n        _ => 0\n",
    );
    write(
        &dir.join("tests/fetch.hd"),
        &format!(
            "use std.testing.assert_equal\nuse std.http.get\nuse pkg.status_of\n\n\
             it(\"reads a page\"):\n    \
             assert_equal(status_of(\"http://127.0.0.1:{port}/ok\"), 200, reason=\"granted\")\n\n\
             it(\"is refused another host\"):\n    \
             assert_equal(status_of(\"http://localhost:{port}/ok\"), 0, reason=\"not granted\")\n"
        ),
    );
    let out = run(&dir, &["test"]);
    assert_eq!(
        text(&out.stdout),
        "test result: ok. 2 passed; 0 failed; 0 ignored\n",
        "{}",
        text(&out.stderr)
    );
    assert!(out.status.success());
}

const NET_PROGRAM: &str = "use std.net.{Net, NetError}
use std.host.{Args, args}

fn port(i: usize) -> u16 $ Args:
    match (args()[i]).parse::[u32]():
        .Ok(p) => u16(p)
        .Err(_) => 0

pub fn main!() -> void $ Console + Net + Args:
    let mut net = $.use(Net)
    tcp := port(0)
    udp := port(1)
    match net.lookup!(\"127.0.0.1\"):
        .Ok(found) => println(\"lookup ${debug(found)}\")
        .Err(e) => println(\"${e}\")
    let mut server = match net.listen!(\"127.0.0.1\", tcp):
        .Ok(l) => l
        .Err(e) => return println(\"${e}\")
    let mut client = match net.connect!(\"127.0.0.1\", tcp):
        .Ok(s) => s
        .Err(e) => return println(\"${e}\")
    let mut peer = match server.accept!():
        .Ok(s) => s
        .Err(e) => return println(\"${e}\")
    _ := client.write!(\"ping\".to_utf8())
    match peer.read!(2):
        .Ok(bytes) => println(\"read ${debug(bytes)}\")
        .Err(e) => println(\"${e}\")
    _ := peer.write!(\"pong\".to_utf8())
    match client.read!(100):
        .Ok(bytes) => println(\"reply ${bytes.len()}\")
        .Err(e) => println(\"${e}\")
    _ := client.close()
    match peer.read!(100):
        .Ok(bytes) => println(\"rest ${bytes.len()}\")
        .Err(e) => println(\"${e}\")
    match peer.read!(100):
        .Ok(bytes) => println(\"at end ${bytes.len()}\")
        .Err(e) => println(\"${e}\")
    match client.read!(1):
        .Ok(_) => println(\"read after close\")
        .Err(e) => println(\"${debug(e)}\")
    match client.close():
        .Ok(_) => println(\"closed twice\")
        .Err(e) => println(\"${e}\")
    _ := server.close()
    match net.listen!(\"127.0.0.1\", tcp):
        .Ok(_) => println(\"listening again\")
        .Err(e) => println(\"${e}\")
    match net.connect!(\"example.com\", 80):
        .Ok(_) => println(\"connected\")
        .Err(e) => println(\"${debug(e)}\")
    match net.lookup!(\"example.com\"):
        .Ok(_) => println(\"looked up\")
        .Err(e) => println(\"${debug(e)}\")
    match net.connect!(\"bad host\", 80):
        .Ok(_) => println(\"connected\")
        .Err(e) => println(\"${debug(e)}\")
    let mut b = match net.bind_udp!(\"127.0.0.1\", udp):
        .Ok(s) => s
        .Err(e) => return println(\"${e}\")
    let mut a = match net.bind_udp!(\"127.0.0.1\", 0):
        .Ok(s) => s
        .Err(e) => return println(\"${e}\")
    _ := a.send_to!(\"127.0.0.1\", udp, \"hey\".to_utf8())
    match b.receive!(2):
        .Ok(d) => println(\"datagram ${debug(d.bytes)} from ${d.host}\")
        .Err(e) => println(\"${e}\")
    match a.send_to!(\"example.com\", 53, [1]):
        .Ok(_) => println(\"sent\")
        .Err(e) => println(\"${e}\")
";

/// A free loopback TCP port.
fn free_port() -> u16 {
    std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|l| l.local_addr())
        .expect("a free port")
        .port()
}

/// A free loopback UDP port. The TCP and UDP port spaces are separate, so
/// a free TCP port can be held by another process's UDP socket, and
/// `bind_udp!` reports an address in use as `Refused`.
fn free_udp_port() -> u16 {
    std::net::UdpSocket::bind("127.0.0.1:0")
        .and_then(|s| s.local_addr())
        .expect("a free port")
        .port()
}

/// `Net` and its handles (`std-net.*`, `cli.cap.scope.net`): a lookup, a
/// TCP listener, connection and stream that read at most `max` bytes and
/// an empty list at the end, every operation after `close` `Closed`, an
/// address in use refused, an address or host outside the grant and one
/// that is no host refused before any connection, and a UDP datagram.
/// Everything stays on the loopback interface.
#[test]
fn net_opens_sockets_inside_the_grant() {
    let (tcp, udp) = (free_port(), free_udp_port());
    let dir = work("host-net");
    write(&dir.join("main.hd"), NET_PROGRAM);
    let out = run(
        &dir,
        &[
            "--cap",
            "Net=127.0.0.1",
            "main.hd",
            "--",
            &tcp.to_string(),
            &udp.to_string(),
        ],
    );
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        "lookup [\"127.0.0.1\"]\n\
         read [112, 105]\n\
         reply 4\n\
         rest 2\n\
         at end 0\n\
         NetError.Closed\n\
         the socket is closed\n\
         listening again\n\
         NetError.NotGranted(address=\"example.com:80\")\n\
         NetError.NotGranted(address=\"example.com\")\n\
         NetError.InvalidAddress(address=\"bad host:80\")\n\
         datagram [104, 101] from 127.0.0.1\n\
         net access to example.com:53 is not granted\n"
    );
    assert!(out.status.success());
    // A socket's methods import from `hd:Net`, so `Net = false` refuses
    // the program before it starts (`cli.cap.total.refuse`).
    let denied = run(&dir, &["--cap", "Net=false", "main.hd", "--", "1", "2"]);
    assert_eq!(text(&denied.stdout), "");
    assert_eq!(denied.status.code(), Some(101));
}

/// An HTTP/1.1 request carries `Host` from the URL's authority, as a
/// server requires (a real server answers 400 without one); a request
/// that names its own `Host` keeps it.
#[test]
fn http_requests_carry_a_host_header() {
    let port = http_server();
    let dir = work("host-http-host");
    write(
        &dir.join("main.hd"),
        "use std.http.{Http, Request, get}
use std.host.{Args, args}

pub fn main!() -> void $ Console + Http + Args:
    base := args()[0]
    match get!(\"${base}/host\"):
        .Ok(r) => println(r.text())
        .Err(e) => println(debug(e))
    match Request { url: \"${base}/host\", headers: [(\"Host\", \"example.test\")] }.send!():
        .Ok(r) => println(r.text())
        .Err(e) => println(debug(e))
",
    );
    let base = format!("http://127.0.0.1:{port}");
    let out = run(&dir, &["--cap", "Http=127.0.0.1", "main.hd", "--", &base]);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        format!("host 127.0.0.1:{port}\nhost example.test\n")
    );
}

/// The SDK-owned package trait serves real requests through an ordinary
/// retained main. Windows graceful listener shutdown belongs to #266's
/// supervision slice; the HD provider fixtures do not use OS signals.
#[cfg(unix)]
#[test]
fn server_sdk_main_keeps_state_and_serves_concurrent_requests() {
    server_sdk_node_tests(&[
        "packages/server-sdk/test/node.test.mjs",
        "packages/server-sdk/test/provider.test.mjs",
        "packages/server-sdk/test/dispatcher.test.mjs",
        "packages/server-sdk/test/application.test.mjs",
        "packages/server-sdk/test/application-client.test.mjs",
        "packages/server-sdk/test/application-logs.test.mjs",
        "packages/server-sdk/test/console.test.mjs",
        "packages/server-sdk/test/node-client.test.mjs",
        "packages/server-sdk/test/http.test.mjs",
    ]);
}

/// Fetch mapping and worker termination do not depend on Unix signals.
#[test]
fn server_sdk_fetch_and_supervised_application_cleanup() {
    server_sdk_node_tests(&[
        "packages/server-sdk/test/fetch.test.mjs",
        "packages/server-sdk/test/supervisor.test.mjs",
        "packages/server-sdk/test/supervised-application.test.mjs",
    ]);
}

/// Deployment policy is read by SDK tasks from the root production table.
#[test]
fn server_sdk_deployment_grants_are_production_only() {
    server_sdk_node_tests(&["packages/server-sdk/test/grants.test.mjs"]);
}

fn server_sdk_node_tests(files: &[&str]) {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    // Each file already exercises real overlapping requests and workers.
    // Avoid multiplying CPU-bound fixtures across files inside an already
    // parallel cargo test run: that can starve a healthy cleanup ACK past
    // its unchanged 200 ms host backstop.
    let out = Command::new("node")
        .args(["--test", "--test-concurrency=1"])
        .args(files)
        .env("HD_SDK_TEST_HD", env!("CARGO_BIN_EXE_hd"))
        .current_dir(root)
        .output()
        .expect("run SDK HTTP test");
    assert!(
        out.status.success(),
        "{}\n{}",
        text(&out.stdout),
        text(&out.stderr)
    );
}

/// The synchronous embedding driver runs the same compiled fixture as
/// ordinary conformance, using explicit turns instead of `host.run`.
#[test]
fn a_compiled_root_runs_through_synchronous_host_turns() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = work("host-driver-turns");
    write(
        &dir.join("hd.toml"),
        "[package]\nname=\"driver\"\n[capabilities]\nClock=true\nConsole=true\n",
    );
    write(
        &dir.join("src/main.hd"),
        include_str!("../../../../spec/conformance/runtime/valid/host-driver-turns.hd"),
    );
    let built = run(&dir, &["build", "--engine", "node"]);
    assert!(built.status.success(), "{}", text(&built.stderr));
    let out = Command::new("node")
        .args(["--test", "host/driver.test.mjs"])
        .env("HD_DRIVER_GUEST", dir.join("build/debug/driver.wasm"))
        .current_dir(root)
        .output()
        .expect("run synchronous driver tests");
    assert!(
        out.status.success(),
        "{}\n{}",
        text(&out.stdout),
        text(&out.stderr)
    );
}

/// Runs the real Node provider and, on Windows, this build's Job launcher.
/// This wrapper makes the Windows regression part of automatic host CI.
#[test]
fn node_process_job_owns_descendants_and_preserves_output() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let dir = work("node-process-job");
    let out = Command::new("node")
        .args([
            "--test",
            "--test-name-pattern=Process|Windows processLauncher",
            "host/node.test.mjs",
            "host/process.test.mjs",
        ])
        .env("HD_PROCESS_LAUNCHER", env!("CARGO_BIN_EXE_hd"))
        .env("HD_CACHE", dir.with_extension("cache"))
        .current_dir(root)
        .output()
        .expect("run Node Process tests");
    assert!(
        out.status.success(),
        "{}\n{}",
        text(&out.stdout),
        text(&out.stderr)
    );
}
