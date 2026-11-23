//! `hd run` and `hd build` on the samples in `examples`, on V8,
//! with the disk cache in a test directory: a second run is warm.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use hd_testkit::Scratch;

fn samples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples")
}

fn cache(name: &str) -> Scratch {
    Scratch::unmade(env!("CARGO_TARGET_TMPDIR"), name)
}

/// Points `command`'s git at no global configuration and sends every
/// remote URL `hd` tries (HTTPS and SSH) to a local path that does not
/// exist, so a test that names a remote requirement fails at once and never
/// reaches the network. Git reads the rewrite from the environment, so no
/// file is left behind.
fn offline_git(command: &mut Command) {
    let nowhere = "file:///hd-no-such-remote/";
    command
        .env(
            "GIT_CONFIG_GLOBAL",
            if cfg!(windows) { "NUL" } else { "/dev/null" },
        )
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_COUNT", "3");
    for (i, from) in ["https://", "ssh://git@", "git@"].into_iter().enumerate() {
        command
            .env(
                format!("GIT_CONFIG_KEY_{i}"),
                format!("url.{nowhere}.insteadOf"),
            )
            .env(format!("GIT_CONFIG_VALUE_{i}"), from);
    }
}

fn hd(cache_dir: &Path) -> Command {
    hd_own_objects(crate::shared_objects(cache_dir))
}

fn hd_own_objects(cache_dir: &Path) -> Command {
    let mut c = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"));
    c.env("HD_CACHE", cache_dir);
    offline_git(&mut c);
    c
}

/// Runs a sample: a directory is a package, run as `hd run` from inside
/// it; a file is a single-file program, run as `hd FILE`.
fn hd_run(cache_dir: &Path, target: &Path) -> String {
    let mut command = hd(cache_dir);
    if target.is_dir() {
        command.current_dir(target).arg("run");
    } else {
        command.arg(target);
    }
    let output = command.output().expect("run hd");
    assert!(
        output.status.success(),
        "{}: {}",
        target.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("UTF-8")
}

/// The phase-1 exit program (`f1c55020:future-work/compiler/build-order.md`, M4b): `List`, `Map`,
/// `Option`, `Result`, strings with interpolation, a user trait, a
/// closure, `match`, `for` and std's own `println`.
const EXIT: &str = "total 14 of 5\n40\nrect 6\ncircle 12\nfound 4\nmissing\nok 3\n\
                    err divide by zero\nnum 7\nplus\nword hi\nann 31, cy 40\n31 years\n3\n";

/// M4d: module initialization across two modules. `prices` initializes
/// before `main`'s top level, which reads its `markup`, and both run
/// before `main`.
const INIT: &str = "hello\n15\n1\n5\n2\n";

/// M4d: `main!` awaits a host timer, then two timed operations with
/// `join!`, then a `race!` whose loser is cancelled mid-wait: its `defer`
/// runs and its end never does.
const SUSPEND: &str = "slept\nall 10 20\nrace 30\nstart 1\nstart 2\ndone 2\ncleanup 2\n\
                       done 1\ncleanup 1\nstart 3\nstart 4\ndone 3\ncleanup 3\ncleanup 4\n";

/// M4d: `defer` ordering on each exit kind: falling off the end (last
/// in, first out), `return`, `continue`, `break`, a failing `?` and a
/// nested scope's `return`.
const DEFER: &str = "5\nerr bad\nok 2\n7\nnormal: body\nnormal: second registered\n\
                     normal: first registered\nreturn: cleanup 5\nloop: cleanup 1\n\
                     loop: cleanup 2\nloop: after\ntry: cleanup x\ntry: cleanup y\n\
                     nested: inner\nnested: outer\n";

/// M4c: shared enum data (read across modules), `$x` of a top-level
/// binding, and `for` over ranges and a list.
const GAPS: &str = "200 OK false\n404 Not Found false\n503 Service Unavailable true\n\
                    level 1 top 9\nhi\n0\n1\n2\n25\ntick\ntick\na\nb\n";

/// #146: bang calls in branch and loop arms, on the ready path and then
/// the resume path, validate and give the same values.
const AWAIT_BRANCHES: &str = "2 10 30 3 41 50 60 31 3\n2 10 30 3 41 50 60 31 3\n";

const CASES: [(&str, &str); 13] = [
    ("hello/hello.hd", "42\n"),
    ("arith/main.hd", "7\n9\n3\n55\n-1\n0\n1\n16\n-10\n"),
    ("data", "3\n-4\n7\n30\n"),
    ("generic", "1\n6\n5\n"),
    ("trait", "12\n13\n101\n7\n"),
    ("fib", "6765\n"),
    ("std_types", "42\n"),
    ("exit/main.hd", EXIT),
    ("init", INIT),
    ("suspend/main.hd", SUSPEND),
    ("defer/main.hd", DEFER),
    ("gaps", GAPS),
    ("await_branches/main.hd", AWAIT_BRANCHES),
];

#[test]
fn run_samples_on_v8_cold_then_warm() {
    let dir = cache("hd-cache-samples");
    let _ = std::fs::remove_dir_all(&dir);
    for round in ["cold", "warm"] {
        for (target, expected) in CASES {
            assert_eq!(
                hd_run(&dir, &samples().join(target)),
                expected,
                "{target} ({round})"
            );
        }
    }
    assert!(
        super::cache_gc::entries(&dir.join("obj"))
            .iter()
            .any(|entry| entry.kind == hd_cache::EntryKind::Link),
        "the disk cache holds linked programs"
    );
}

/// M4c: `main` returning `.Err` prints the error's report on standard
/// error and exits with status 1 (module.entry.err-sources-to-stderr).
#[test]
fn main_returning_err_exits_with_its_report() {
    let output = hd(&cache("hd-cache-err-exit"))
        .arg(samples().join("err_exit/main.hd"))
        .output()
        .expect("run hd");
    assert_eq!(output.status.code(), Some(1));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "got 1\n");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "oops: too big\n");
}

/// `cli.stdin.program`: with no arguments and piped standard input, `hd`
/// runs the input as a single-file program.
#[test]
fn stdin_runs_as_a_single_file_program() {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = hd(&cache("hd-cache-stdin"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run hd");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(include_bytes!("programs/prints_one.hd"))
        .expect("write stdin");
    let out = child.wait_with_output().expect("hd output");
    assert_eq!(out.status.code(), Some(0), "{out:?}");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "1\n");
    assert!(out.stderr.is_empty(), "{out:?}");
}

/// `cli.stdin.program` reports a broken stdin program as `main.hd`.
#[test]
fn stdin_reports_errors_as_main_hd() {
    use std::io::Write as _;
    use std::process::Stdio;
    let mut child = hd(&cache("hd-cache-stdin-error"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("run hd");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(include_bytes!(
            "../../../../spec/conformance/cli/json-run/broken.hd"
        ))
        .expect("write stdin");
    let out = child.wait_with_output().expect("hd output");
    assert!(!out.status.success(), "{out:?}");
    let err = String::from_utf8_lossy(&out.stderr);
    assert!(
        err.contains("main.hd") && err.contains("unknown-name"),
        "{err}"
    );
}

fn scratch(name: &str) -> Scratch {
    Scratch::new(env!("CARGO_TARGET_TMPDIR"), name)
}

/// A package `shop` with one executable (`main.hd`) and one task (`seed`).
fn shop(name: &str) -> Scratch {
    let dir = scratch(name);
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("write");
    std::fs::write(
        dir.join("main.hd"),
        include_str!("../../../../spec/conformance/cli/program-leaves-test-code-out/main.hd"),
    )
    .expect("write");
    std::fs::create_dir_all(dir.join("tasks")).expect("dir");
    std::fs::write(
        dir.join("tasks/seed.hd"),
        include_str!("../../../../spec/conformance/cli/program-leaves-test-code-out/tasks/seed.hd"),
    )
    .expect("write");
    dir
}

struct Ran {
    code: Option<i32>,
    out: String,
    err: String,
}

fn ran(dir: &Path, args: &[&str]) -> Ran {
    // One cache per call, not one per process: every call clears its
    // cache first, so a shared name deletes the cache (including
    // in-progress `tmp` scratch directories) out from under another
    // test's running child, failing it with whatever syscall loses
    // the race (`ENOENT`, ...). The counter keeps calls in one test
    // binary apart; the process id keeps binaries apart, as before.
    static CACHES: AtomicU64 = AtomicU64::new(0);
    let unique = CACHES.fetch_add(1, Ordering::Relaxed);
    let output = hd(&cache(&format!("hd-cache-forms-{unique}")))
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run hd");
    Ran {
        code: output.status.code(),
        out: String::from_utf8_lossy(&output.stdout).into_owned(),
        err: String::from_utf8_lossy(&output.stderr).into_owned(),
    }
}

/// `cli.mode.package.nearest`: `hd run` runs the package's one executable
/// from a directory below the package. A CLI case runs every command in the
/// case's own directory, so this stays here.
#[test]
fn run_alone_runs_the_one_executable_from_below_the_package() {
    let dir = shop("hd-forms-default");
    let r = ran(&dir.join("tasks"), &["run"]);
    assert_eq!((r.code, r.out.as_str()), (Some(0), "main\n"), "{}", r.err);
}

/// Wasmtime gives a bad string slice the panic Node gives it. Node's four
/// are the `std-text` stage cases of `hd_driver` (`a-slice-*`, `a-reversed-slice-*`).
#[test]
#[cfg(feature = "wasmtime")]
fn string_slice_messages_distinguish_boundaries_from_bounds() {
    let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "hd-run-str-slice-cause");
    let file = dir.join("bad.hd");
    let cached = cache("hd-cache-str-slice-cause");
    for (start, end, message) in [
        (0, 2, "string slice offset is inside a character"),
        (2, 3, "string slice offset is inside a character"),
        (0, 7, "string slice out of bounds"),
        (4, 3, "string slice out of bounds"),
    ] {
        std::fs::write(
            &file,
            format!(
                "pub fn main() -> void $ Console:\n    println(\"héllo\".slice({start}, {end}))\n"
            ),
        )
        .expect("write");
        let output = hd(&cached)
            .args(["--engine", "wasmtime"])
            .arg(&file)
            .output()
            .expect("run hd");
        assert_eq!(output.status.code(), Some(3), "{start}..{end}");
        let err = String::from_utf8_lossy(&output.stderr);
        assert!(
            err.contains(&format!("panic: index-out-of-bounds: {message}")),
            "{start}..{end}: {err}"
        );
    }
}

/// Writes `files` under a fresh directory.
fn tree(name: &str, files: &[(&str, &str)]) -> Scratch {
    let dir = scratch(name);
    for (file, text) in files {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        std::fs::write(path, text).expect("write");
    }
    dir
}

const HELLO: &str =
    include_str!("../../../../spec/conformance/cli/workspace-root-members/app/src/main.hd");

/// `cli.command.help`, `cli.command.help.command`: the command list names
/// every command, and one command's help names its flags.
#[test]
fn help_lists_the_commands_and_each_commands_flags() {
    let dir = scratch("hd-forms-help");
    for args in [&["help"][..], &["--help"][..]] {
        let r = ran(&dir, args);
        assert_eq!(r.code, Some(0), "{}", r.err);
        for name in [
            "FILE.wasm",
            "build",
            "run",
            "test",
            "check",
            "lint",
            "doc",
            "new",
            "add",
            "update",
            "remove",
            "fetch",
            "clean",
            "fmt",
            "cache gc",
            "help",
        ] {
            assert!(r.out.contains(&format!("\n  {name} ")), "{name}: {}", r.out);
        }
    }
    let add = ran(&dir, &["help", "add"]);
    assert_eq!(add.code, Some(0));
    assert!(add.out.contains("--dev"), "{}", add.out);
    assert_eq!(ran(&dir, &["add", "--help"]).out, add.out);
    assert!(ran(&dir, &["clean", "--help"]).out.contains("--cache"));
    // `cli.build.output.file`, `cli.run.package-only`: what `hd build FILE`
    // writes, in both profiles, and that it needs a package.
    let build = ran(&dir, &["help", "build"]).out;
    for words in [
        "build/debug/files/STEM.wasm",
        "build/release/files/",
        "Outside a package",
    ] {
        assert!(build.contains(words), "{words}: {build}");
    }
    assert_eq!(ran(&dir, &["help", "bogus"]).code, Some(101));
}

/// `cli.new.app`, `cli.new.workspace-member`, `cli.new.existing`: a new
/// application under a workspace root runs at once and joins `members`;
/// a second `hd new` over it writes nothing.
#[test]
fn new_creates_a_package_and_joins_its_workspace() {
    let dir = tree(
        "hd-forms-new",
        &[("hd.toml", "[workspace]\nmembers = [\"old\"]\n")],
    );
    let r = ran(&dir, &["new", "--app", "--vcs", "none", "my-app"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        std::fs::read_to_string(dir.join("hd.toml")).expect("read"),
        "[workspace]\nmembers = [\"old\", \"my-app\"]\n"
    );
    assert!(dir.join("my-app/tests/my_app.hd").is_file());
    let manifest =
        hd_project::parse_manifest(&std::fs::read_to_string(dir.join("my-app/hd.toml")).unwrap())
            .unwrap();
    for key in ["Console", "Args", "Clock", "Random"] {
        assert!(
            manifest
                .capabilities
                .iter()
                .any(|(name, value, _)| name == key && *value == Some(hd_project::Grant::All))
        );
    }
    let r = ran(&dir.join("my-app"), &["run"]);
    assert_eq!(
        (r.code, r.out.as_str()),
        (Some(0), "hello, world\n"),
        "{}",
        r.err
    );
    let r = ran(&dir, &["new", "--lib", "--vcs", "none", "my-app"]);
    assert_eq!(r.code, Some(101));
    assert!(!dir.join("my-app/src/lib.hd").exists());
}

/// `cli.clean.cache.*`: `hd clean --cache` removes the read-only fetched
/// versions and the compiled entries, and keeps the cache directory; a
/// directory that holds anything else is no cache, and nothing goes.
#[test]
fn clean_cache_empties_only_an_hd_cache() {
    let dir = scratch("hd-forms-clean-cache");
    let cache = dir.join("cache");
    let version = cache.join("pkg/github.com/acme/json@2.1.0");
    std::fs::create_dir_all(version.join("src")).expect("dir");
    std::fs::write(
        version.join("hd.toml"),
        "[package]\nname = \"json\"\n\n[capabilities]\nConsole = true\n",
    )
    .expect("write");
    std::fs::create_dir_all(cache.join("obj/check")).expect("dir");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        for d in [version.join("src"), version.clone()] {
            std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o555)).expect("chmod");
        }
    }
    let clean = |cache: &Path| {
        hd_own_objects(cache)
            .current_dir(&dir)
            .args(["clean", "--cache"])
            .output()
            .expect("run hd")
    };
    let out = clean(&cache);
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        text.starts_with("removed github.com/acme/json@2.1.0\nremoved 1 version from "),
        "{text}"
    );
    assert!(cache.is_dir() && !cache.join("pkg").exists() && !cache.join("obj").exists());
    let again = clean(&cache);
    assert!(String::from_utf8_lossy(&again.stdout).contains("is empty"));
    std::fs::write(dir.join("notes.txt"), "mine").expect("write");
    let foreign = clean(&dir);
    assert_eq!(foreign.status.code(), Some(101));
    assert!(dir.join("notes.txt").is_file());
}

/// A tree hash's form, which `hd.sum` lines hold.
const H: &str = "h1:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA=";

/// `cli.dep.remove`, `cli.dep.edit`, `cli.dep.edit.empty-table`,
/// `cli.dep.tidy`: `hd remove` deletes the key's line and an emptied
/// table's header, keeps the other lines, and tidies `hd.sum` to the
/// selection; a NAME of neither table is an error.
#[test]
fn remove_deletes_a_requirement_line_by_line() {
    let dir = tree(
        "hd-forms-remove",
        &[
            (
                "hd.toml",
                "[package]\nname = \"app\"  # mine\n\n[capabilities]\nConsole = true\n\n[dependencies]\nmoney = { path = \"money\" }\n\n[dev-dependencies]\njson = \"github.com/acme/json@2.1.0\"\n",
            ),
            (
                "hd.sum",
                &format!(
                    "github.com/acme/json@2.1.0 {H}\ngithub.com/acme/json@2.1.0/hd.toml {H}\ngithub.com/acme/yaml@1.0.0 {H}\n"
                ),
            ),
            (
                "money/hd.toml",
                "[package]\nname = \"money\"\n\n[capabilities]\nConsole = true\n",
            ),
            ("money/src/lib.hd", include_str!("programs/one.hd")),
            ("src/main.hd", HELLO),
        ],
    );
    let r = ran(&dir, &["remove", "json"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert_eq!(
        std::fs::read_to_string(dir.join("hd.toml")).expect("read"),
        "[package]\nname = \"app\"  # mine\n\n[capabilities]\nConsole = true\n\n[dependencies]\nmoney = { path = \"money\" }\n"
    );
    // Tidy keeps one entry per selected version and read manifest, and no
    // other: nothing is selected now, so not even yaml's line stays.
    assert_eq!(
        std::fs::read_to_string(dir.join("hd.sum")).expect("read"),
        ""
    );
    let r = ran(&dir, &["remove", "json"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("`json` is no key"), "{}", r.err);
}

/// `cli.dep.fetch`, `cli.dep.path`: with path requirements only there is
/// nothing to fetch and no `hd.sum` line to add; a dependency requirement
/// needs a fetch, which is not built yet, and is an error.
#[test]
fn fetch_with_path_requirements_only_has_nothing_to_do() {
    let dir = tree(
        "hd-forms-fetch",
        &[
            (
                "hd.toml",
                "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n\n[dependencies]\nmoney = { path = \"money\" }\n",
            ),
            (
                "money/hd.toml",
                "[package]\nname = \"money\"\n\n[capabilities]\nConsole = true\n",
            ),
            ("money/src/lib.hd", include_str!("programs/one.hd")),
            ("src/main.hd", HELLO),
        ],
    );
    let r = ran(&dir, &["fetch", "--format", "json"]);
    assert_eq!(r.code, Some(0), "{}", r.err);
    assert!(r.out.starts_with("{\"kind\":\"summary\""), "{}", r.out);
    assert!(!dir.join("hd.sum").exists());
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"shop\"\n\n[capabilities]\nConsole = true\n\n[dependencies]\njson = \"github.com/acme/json@2.1.0\"\n",
    )
    .expect("write");
    // The remote is unreachable at once: both its URLs map to a missing path.
    let r = ran(&dir, &["fetch"]);
    assert_eq!(r.code, Some(101), "{}", r.err);
    assert!(r.err.contains("fetch-failed"), "{}", r.err);
}

/// A workspace of members `web` (executable `serve`, task `seed`) and `api`
/// (task `seed`, and task `audit`).
fn run_workspace(name: &str) -> Scratch {
    let say = |text: &str| format!("pub fn main() -> void $ Console:\n    println(\"{text}\")\n");
    tree(
        name,
        &[
            ("hd.toml", "[workspace]\nmembers = [\"web\", \"api\"]\n"),
            (
                "web/hd.toml",
                "[package]\nname = \"web\"\n\n[capabilities]\nConsole = true\n\n[[executable]]\nname = \"serve\"\nmodule = \"serve\"\n",
            ),
            ("web/src/serve.hd", &say("serving")),
            ("web/tasks/seed.hd", &say("seeded web")),
            (
                "api/hd.toml",
                "[package]\nname = \"api\"\n\n[capabilities]\nConsole = true\n",
            ),
            ("api/tasks/seed.hd", &say("seeded api")),
            ("api/tasks/audit.hd", &say("audited")),
        ],
    )
}

/// `cli.workspace.select.anywhere`: `-p NAME` picks the member from inside
/// another member, and an unknown NAME names the workspace. A CLI case runs
/// every command in its own directory, and the message holds the workspace's
/// absolute path, so these two stay here; the rest is `cli/workspace-run-name`.
#[test]
fn workspace_run_selects_members_with_p_from_inside_a_member() {
    let dir = run_workspace("hd-forms-ws-run-select");
    let r = ran(&dir.join("web"), &["run", "--package", "api", "seed"]);
    assert_eq!(
        (r.code, r.out.as_str()),
        (Some(0), "seeded api\n"),
        "{}",
        r.err
    );
    let r = ran(&dir, &["run", "-p", "nope", "seed"]);
    assert_eq!(r.code, Some(101));
    assert!(r.err.contains("`-p nope` names no member"), "{}", r.err);
}
