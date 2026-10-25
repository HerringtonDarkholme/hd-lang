//! Installed commands use this exact compiler through a fresh shell/PATH.
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use hd_testkit::Scratch;
use serde_json::Value;

struct Case {
    dir: Scratch,
    app: PathBuf,
    root: PathBuf,
    caller: PathBuf,
    path: std::ffi::OsString,
}

fn write(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

const PROGRAM: &str = "use std.console.{ConsoleInput, read_all}
use std.host.{Args, args}
use std.process.ExitCode

pub fn main!() -> ExitCode $ Args + ConsoleInput + Console:
    println(\"VERSION\")
    println(\"name=${$.use(Args).program()}\")
    for word in args():
        println(\"arg=$word\")
    text := read_all!().expect(\"stdin\")
    println(\"stdin=$text\")
    ExitCode(7)
";

impl Case {
    fn new(label: &str) -> Self {
        let dir = Scratch::new(env!("CARGO_TARGET_TMPDIR"), label);
        let app = dir.join("app");
        let root = dir.join("root space ' λ");
        let caller = dir.join("caller");
        let runtime = dir.join("runtime");
        std::fs::create_dir_all(&caller).unwrap();
        std::fs::create_dir_all(&runtime).unwrap();
        let executable = if cfg!(windows) { "hd.exe" } else { "hd" };
        std::fs::copy(env!("CARGO_BIN_EXE_hd"), runtime.join(executable)).unwrap();
        let mut paths = vec![root.join("bin"), runtime];
        paths.extend(std::env::split_paths(
            &std::env::var_os("PATH").unwrap_or_default(),
        ));
        let path = std::env::join_paths(paths).unwrap();
        write(
            &app.join("hd.toml"),
            "[package]\nname=\"install-control\"\n[capabilities]\nArgs=true\nConsole=true\nConsoleInput=true\n",
        );
        write(&app.join("src/main.hd"), &PROGRAM.replace("VERSION", "v1"));
        // Neither discovery nor grants may come from the caller's package.
        write(
            &caller.join("hd.toml"),
            "[package]\nname=\"caller\"\n[capabilities]\nConsole=false\nConsoleInput=false\nArgs=false\nFsRead=true\n",
        );
        Self {
            dir,
            app,
            root,
            caller,
            path,
        }
    }

    fn command(&self, program: impl AsRef<std::ffi::OsStr>) -> Command {
        let mut c = Command::new(program);
        c.current_dir(&self.caller)
            .env("PATH", &self.path)
            .env("HD_CACHE", self.dir.join("cache"))
            .env("HD_ENGINE", "node")
            .env("HD_JOBS", "2")
            .env("HD_INSTALL_ROOT", self.dir.join("other-root"));
        c
    }

    fn hd(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_hd"))
            .args(args)
            .output()
            .unwrap()
    }

    fn install(&self) -> Output {
        self.hd(&[
            "install",
            self.app.to_str().unwrap(),
            "--root",
            self.root.to_str().unwrap(),
        ])
    }

    fn registry(&self) -> PathBuf {
        self.root.join("installed/registry.json")
    }

    fn invoke(&self, args: &[&str], input: &[u8]) -> Output {
        self.invoke_on("node", "install-control", args, input)
    }

    fn invoke_on(&self, engine: &str, name: &str, args: &[&str], input: &[u8]) -> Output {
        #[cfg(unix)]
        let mut shell = {
            let mut c = self.command("sh");
            c.args(["-c", "exec \"$0\" \"$@\"", name]);
            c
        };
        #[cfg(windows)]
        let mut shell = {
            let mut c = self.command("cmd.exe");
            c.args(["/d", "/c", name]);
            c
        };
        let mut child = shell
            .env("HD_ENGINE", engine)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    }
}

fn ok(out: &Output) {
    assert!(
        out.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn rejected(out: &Output, reason: &str) {
    assert_eq!(
        out.status.code(),
        Some(101),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stderr).contains(reason),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn expected(version: &str, args: &[&str], input: &str) -> String {
    let mut text = format!("{version}\nname=install-control\n");
    for arg in args {
        writeln!(text, "arg={arg}").unwrap();
    }
    writeln!(text, "stdin={input}").unwrap();
    text
}

#[test]
fn fresh_shell_preserves_arguments_streams_status_and_needs_no_checkout() {
    let c = Case::new("install-shell");
    ok(&c.install());
    assert!(
        !c.dir.join("other-root").exists(),
        "--root precedes HD_INSTALL_ROOT"
    );
    #[cfg(windows)]
    write(
        &c.caller.join("hd.exe"),
        "a caller-local executable must not select the runtime",
    );
    let args = ["one two", "a'b", "λ", "", "--cap", "Process=true"];
    let input = "first line\nλ\n";
    let before = c.invoke(&args, input.as_bytes());
    assert_eq!(before.status.code(), Some(7));
    assert_eq!(
        String::from_utf8_lossy(&before.stdout),
        expected("v1", &args, input)
    );
    assert!(before.stderr.is_empty());
    std::fs::remove_dir_all(&c.app).unwrap();
    let after = c.invoke(&args, input.as_bytes());
    assert_eq!(after.status.code(), Some(7));
    assert_eq!(after.stdout, before.stdout);
    assert!(after.stderr.is_empty());
    #[cfg(feature = "wasmtime")]
    {
        let native = c.invoke_on("wasmtime", "install-control", &args, input.as_bytes());
        assert_eq!(native.status.code(), Some(7), "{native:?}");
        assert_eq!(native.stdout, before.stdout);
        assert!(native.stderr.is_empty(), "{native:?}");
    }
    let record: Value = serde_json::from_slice(&std::fs::read(c.registry()).unwrap()).unwrap();
    let generation = record["commands"]["install-control"]["generation"]
        .as_str()
        .unwrap();
    let payload = c.root.join("installed").join(generation);
    ok(&c.hd(&[
        "uninstall",
        "install-control",
        "--root",
        c.root.to_str().unwrap(),
    ]));
    assert!(!payload.exists());
    let list = c.hd(&["install", "--list", "--root", c.root.to_str().unwrap()]);
    assert!(list.status.success());
    assert!(list.stdout.is_empty());
}

#[test]
fn updates_preserve_old_commands_on_failure_and_deny_is_one_time() {
    let c = Case::new("install-update");
    rejected(
        &c.hd(&[
            "install",
            c.app.to_str().unwrap(),
            "--root",
            c.root.to_str().unwrap(),
            "--deny",
            "Args",
        ]),
        "Args",
    );
    assert!(!c.registry().exists());
    ok(&c.install());
    let original = std::fs::read(c.registry()).unwrap();
    let other = c.dir.join("unrelated");
    write(
        &other.join("hd.toml"),
        "[package]\nname=\"install-control\"\n[capabilities]\nConsole=true\n",
    );
    write(
        &other.join("src/main.hd"),
        "pub fn main() -> void $ Console:\n    println(\"other\")\n",
    );
    rejected(
        &c.hd(&[
            "install",
            other.to_str().unwrap(),
            "--root",
            c.root.to_str().unwrap(),
        ]),
        "another source",
    );
    assert_eq!(std::fs::read(c.registry()).unwrap(), original);
    // Intentional invalid source tests transactional build failure.
    write(
        &c.app.join("src/main.hd"),
        "pub fn main() -> void:\n    intentionally_missing_name()\n",
    );
    assert_eq!(c.install().status.code(), Some(101));
    assert_eq!(std::fs::read(c.registry()).unwrap(), original);
    let still_old = c.invoke(&[], b"");
    assert_eq!(still_old.status.code(), Some(7));
    assert!(still_old.stdout.starts_with(b"v1\n"));
    write(
        &c.app.join("src/main.hd"),
        &PROGRAM.replace("VERSION", "v2"),
    );
    ok(&c.install());
    let updated = c.invoke(&[], b"");
    assert_eq!(updated.status.code(), Some(7));
    assert!(updated.stdout.starts_with(b"v2\n"));
}

#[test]
fn abi_hash_and_registry_paths_are_checked_before_guest_effects() {
    let c = Case::new("install-guards");
    ok(&c.install());
    let saved = std::fs::read(c.registry()).unwrap();
    let mut data: Value = serde_json::from_slice(&saved).unwrap();
    data["commands"]["install-control"]["abi"] = Value::from(u32::MAX);
    std::fs::write(c.registry(), serde_json::to_vec(&data).unwrap()).unwrap();
    let failed = c.invoke(&[], b"");
    assert!(failed.stdout.is_empty());
    rejected(&failed, "incompatible installed Wasm/runtime ABI");
    std::fs::write(c.registry(), &saved).unwrap();
    let data: Value = serde_json::from_slice(&saved).unwrap();
    let generation = data["commands"]["install-control"]["generation"]
        .as_str()
        .unwrap();
    let artifact = c
        .root
        .join("installed")
        .join(generation)
        .join("install-control.wasm");
    let bytes = std::fs::read(&artifact).unwrap();
    let mut corrupt = bytes.clone();
    corrupt.extend_from_slice(b"invalid-wasm-tail");
    std::fs::write(&artifact, corrupt).unwrap();
    let failed = c.invoke(&[], b"");
    assert!(failed.stdout.is_empty());
    rejected(&failed, "hash mismatch");
    std::fs::write(artifact, bytes).unwrap();
    let mut data: Value = serde_json::from_slice(&saved).unwrap();
    data["commands"]["install-control"]["name"] = Value::from("../victim");
    std::fs::write(c.registry(), serde_json::to_vec(&data).unwrap()).unwrap();
    write(&c.root.join("victim"), "untouched");
    rejected(
        &c.hd(&["install", "--list", "--root", c.root.to_str().unwrap()]),
        "invalid installed command name",
    );
    assert_eq!(
        std::fs::read_to_string(c.root.join("victim")).unwrap(),
        "untouched"
    );
}

#[test]
fn install_time_path_grants_do_not_follow_the_callers_package() {
    let c = Case::new("install-fs-grants");
    write(
        &c.app.join("hd.toml"),
        "[package]\nname=\"install-control\"\n[capabilities]\nArgs=true\nConsole=true\nFsRead=[\"data\"]\n",
    );
    write(
        &c.app.join("src/main.hd"),
        "use std.fs.{FsRead, read_text}\nuse std.path.Path\nuse std.host.{Args, args}\n\npub fn main!() -> void $ Args + Console + FsRead:\n    match read_text!(Path(args()[0])):\n        .Ok(text) => println(text)\n        .Err(_) => println(\"denied\")\n",
    );
    let inside = c.app.join("data/value.txt");
    let outside = c.caller.join("data/value.txt");
    write(&inside, "inside");
    write(&outside, "outside");
    ok(&c.install());
    let permitted = c.invoke(&[inside.to_str().unwrap()], b"");
    assert!(
        permitted.status.success(),
        "{}",
        String::from_utf8_lossy(&permitted.stderr)
    );
    assert_eq!(permitted.stdout, b"inside\n");
    let denied = c.invoke(&[outside.to_str().unwrap()], b"");
    assert!(denied.status.success());
    assert_eq!(denied.stdout, b"denied\n");
}

#[test]
fn shared_payloads_survive_other_removals_and_launchers_are_owned() {
    let c = Case::new("install-shared");
    let manifest = "[package]\nname=\"install-control\"\n[capabilities]\nArgs=true\nConsole=true\nConsoleInput=true\n\n[[executable]]\nname=\"install-control\"\nmodule=\"main\"\n";
    let second = "\n[[executable]]\nname=\"side-tool\"\nmodule=\"main\"\n";
    write(&c.app.join("hd.toml"), &format!("{manifest}{second}"));
    ok(&c.install());
    let data: Value = serde_json::from_slice(&std::fs::read(c.registry()).unwrap()).unwrap();
    let generation = data["commands"]["side-tool"]["generation"]
        .as_str()
        .unwrap();
    let payload = c.root.join("installed").join(generation);
    ok(&c.hd(&[
        "uninstall",
        "install-control",
        "--root",
        c.root.to_str().unwrap(),
    ]));
    assert!(!payload.join("install-control.wasm").exists());
    assert!(payload.join("side-tool.wasm").exists());
    let remaining = c.invoke_on("node", "side-tool", &[], b"");
    assert_eq!(remaining.status.code(), Some(7), "{remaining:?}");
    assert!(remaining.stdout.starts_with(b"v1\nname=side-tool\n"));
    let launcher = c.root.join("bin").join(if cfg!(windows) {
        "side-tool.cmd"
    } else {
        "side-tool"
    });
    let original = std::fs::read(&launcher).unwrap();
    std::fs::write(&launcher, b"unrelated command").unwrap();
    rejected(&c.install(), "modified command");
    rejected(
        &c.hd(&["uninstall", "side-tool", "--root", c.root.to_str().unwrap()]),
        "modified command",
    );
    std::fs::write(&launcher, original).unwrap();
    std::fs::remove_file(&launcher).unwrap();
    ok(&c.install());
    assert!(
        launcher.exists(),
        "reinstall repairs a missing managed launcher"
    );
    assert!(!payload.exists(), "reinstall reclaims the old generation");
    write(&c.app.join("hd.toml"), manifest);
    ok(&c.install());
    assert!(
        !launcher.exists(),
        "update removes a no-longer-declared executable"
    );
    let list = c.hd(&["install", "--list", "--root", c.root.to_str().unwrap()]);
    ok(&list);
    assert!(!String::from_utf8_lossy(&list.stdout).contains("side-tool"));
}
