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
    let mut c = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"));
    c.env(
        "HD_CACHE",
        crate::shared_objects(dir.with_extension("cache")),
    )
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

/// `Args` (`std-cli.args.*`, `cli.args.pass`): `program` is the FILE the
/// command ran, and `list` the words after `--`, in order.
#[test]
fn args_hold_the_program_and_its_arguments() {
    let dir = work("host-args");
    write(
        &dir.join("main.hd"),
        include_str!("programs/host_traits/args_hold_the_program_and_its_arguments.hd"),
    );
    let out = run(&dir, &["main.hd", "--", "a", "b c", ""]);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(text(&out.stdout), "main.hd\n[a]\n[b c]\n[]\n");
    assert!(out.status.success());
}

const ENV_PROGRAM: &str = include_str!("programs/host_traits/env_program.hd");

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

/// `ConsoleInput` (`std-console.input.*`,
/// `cli.host.default-profile.input-closed`): each line of standard input
/// without its ending, then `.None` at the end, and again after it.
#[test]
fn console_input_reads_standard_input_by_lines() {
    let dir = work("host-input");
    write(
        &dir.join("main.hd"),
        include_str!("programs/host_traits/console_input_reads_standard_input_by_lines.hd"),
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
        include_str!("programs/host_traits/process_runs_a_granted_host_program.hd"),
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
        include_str!("programs/host_traits/the_default_test_runner_keeps_snapshot_files.hd"),
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

const TIMEOUT_TESTS: &str = include_str!("programs/host_traits/timeout_tests.hd");

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
    write(
        &dir.join("src/lib.hd"),
        include_str!("programs/host_traits/a_sleep_past_the_timeout_fails_the_case.hd"),
    );
    write(
        &dir.join("tests/naps.hd"),
        include_str!("programs/host_traits/a_sleep_past_the_timeout_fails_the_case_2.hd"),
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

const SYS_PROGRAM: &str = include_str!("programs/host_traits/sys_program.hd");

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

const HTTP_PROGRAM: &str = include_str!("programs/host_traits/http_program.hd");

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
        include_str!(
            "programs/host_traits/http_reaches_a_granted_host_from_an_integration_test.hd"
        ),
    );
    write(
        &dir.join("tests/fetch.hd"),
        &include_str!("programs/host_traits/http_package_fetch.hd")
            .replace("__PORT__", &port.to_string()),
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

const NET_PROGRAM: &str = include_str!("programs/host_traits/net_program.hd");

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
        include_str!("programs/host_traits/http_requests_carry_a_host_header.hd"),
    );
    let base = format!("http://127.0.0.1:{port}");
    let out = run(&dir, &["--cap", "Http=127.0.0.1", "main.hd", "--", &base]);
    assert_eq!(text(&out.stderr), "");
    assert_eq!(
        text(&out.stdout),
        format!("host 127.0.0.1:{port}\nhost example.test\n")
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
        include_str!("../../../../spec/conformance/std/time/clock/host-driver-turns.hd"),
    );
    let built = run(&dir, &["build", "--engine", "node"]);
    assert!(built.status.success(), "{}", text(&built.stderr));
    let out = hd_testkit::node::command()
        .args(["--test", "host/driver.test.ts"])
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
    let out = hd_testkit::node::command()
        .args([
            "--test",
            "--test-name-pattern=Process|Windows processLauncher",
            "host/node.test.ts",
            "host/process.test.ts",
        ])
        .env("HD_PROCESS_LAUNCHER", env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            crate::shared_objects(dir.with_extension("cache")),
        )
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

/// The synchronous stdin reader joins each result once and searches each
/// byte once, including a tenfold increase from 1 MiB to 10 MiB.
#[test]
fn node_stdin_reader_keeps_copy_and_search_work_linear() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = hd_testkit::node::command()
        .args(["--test", "host/node-input.test.ts"])
        .current_dir(root)
        .output()
        .expect("run counted stdin tests");
    assert!(
        out.status.success(),
        "{}\n{}",
        text(&out.stdout),
        text(&out.stderr)
    );
}

/// Inherited streams reach the real child on both engines, including piped stdin.
#[test]
fn process_status_inherits_streams_and_console_detects_pipes() {
    let dir = work("host-process-status");
    write(
        &dir.join("main.hd"),
        include_str!("programs/host_traits/process_status_inherits_streams.hd"),
    );
    let engines = [
        "node",
        #[cfg(feature = "wasmtime")]
        "wasmtime",
    ];
    for engine in engines {
        let mut child = hd(
            &dir,
            &[
                "main.hd",
                "--engine",
                engine,
                "--time-limit",
                "5s",
                "--",
                hd_testkit::node::binary().to_str().unwrap(),
            ],
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"parent input\n")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert_eq!(
            text(&output.stderr),
            "child error\n",
            "{engine}: {}",
            text(&output.stdout)
        );
        assert!(
            output.status.success(),
            "{engine}: {}",
            text(&output.stderr)
        );
        assert_eq!(
            text(&output.stdout),
            "false false false\nchild:parent input\nstatus=7\nrun=captured\n",
            "{engine}"
        );
    }
}

/// A terminal child can read inherited stdin and /dev/tty; the parent regains it.
#[cfg(unix)]
#[test]
fn process_status_hands_off_and_restores_the_controlling_terminal() {
    let dir = work("host-process-terminal");
    write(
        &dir.join("main.hd"),
        include_str!("programs/host_traits/process_status_terminal.hd"),
    );
    write(
        &dir.join("terminal.py"),
        include_str!("programs/host_traits/process_status_terminal.py"),
    );
    let engines = [
        "node",
        #[cfg(feature = "wasmtime")]
        "wasmtime",
    ];
    for engine in engines {
        let output = Command::new("python3")
            .arg(dir.join("terminal.py"))
            .arg(env!("CARGO_BIN_EXE_hd"))
            .arg(dir.join("main.hd"))
            .arg(engine)
            .arg(hd_testkit::node::binary())
            .env(
                "HD_CACHE",
                crate::shared_objects(dir.with_extension("cache")),
            )
            .env("PATH", hd_testkit::node::path_env())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{engine}: {}{}",
            text(&output.stdout),
            text(&output.stderr)
        );
    }
}
