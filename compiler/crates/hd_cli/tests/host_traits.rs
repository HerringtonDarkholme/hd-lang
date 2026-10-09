//! The default profile's host traits end to end (runtime-and-host.md §17,
//! spec/cli/command-line.md "Host Capabilities"): each program runs under
//! `hd` on V8, its host calls crossing the exchange buffer with the codecs
//! of §17.4, and each provider asking the grant first
//! (`cli.host.default-profile.granted`). Every program works in a scratch
//! directory below the target directory.

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

/// A fresh, empty work directory.
fn work(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(name);
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("work dir");
    std::fs::canonicalize(&dir).expect("work dir")
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
         no entry\nok\nok\naccess to denied.txt is not granted; run with --cap FsRead=denied.txt \
         or --cap FsWrite=denied.txt\nread not granted\nnot found\n"
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
        "use std.process.Process\n\nfn start!(program: string, args: List[string], input: string) -> string $ Process:\n    \
         match $.use(Process).run!(program, args, input):\n        \
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
        "3 [fed] [oops\n]\nprogram not granted; run with --cap Process=PROGRAM\nprogram not found\n"
    );
}

/// The default `TestRunner` keeps snapshot files
/// (`std-testing.snapshot-file.*`): a missing one fails, `--update`
/// records it under `__snapshots__/pkg` for `src/lib.hd`, it then passes,
/// and a differing one fails again.
#[test]
fn the_default_test_runner_keeps_snapshot_files() {
    let dir = work("host-snapshots");
    write(&dir.join("hd.toml"), "[package]\nname = \"shop\"\n");
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
