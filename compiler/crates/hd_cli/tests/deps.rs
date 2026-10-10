//! Remote dependencies end to end (`cli.dep.*`, `cli.sum.*`,
//! `cli.cache.*`), with no network: each "remote" is a local repository,
//! and git's own `url.<base>.insteadOf`, in a test-only global git
//! configuration, maps `https://github.com/` to it. The environment is
//! cleared, so `HOME`, `HD_CACHE` and git's configuration are the test's
//! own: no test reads the user's git configuration, credentials or cache.
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use hd_deps::sum::summary_hash;

/// A test's directory: its remotes, home, cache and packages.
struct World {
    root: PathBuf,
}

impl World {
    fn new(name: &str) -> World {
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("hd-deps-{name}"));
        let _ = hd_deps::cache::remove_tree(&root);
        std::fs::create_dir_all(root.join("home")).expect("mkdir");
        std::fs::create_dir_all(root.join("remotes")).expect("mkdir");
        let config = format!(
            "[user]\n\tname = hd test\n\temail = test@example.invalid\n[init]\n\tdefaultBranch = main\n\
             [tag]\n\tgpgSign = false\n[commit]\n\tgpgSign = false\n\
             [url \"file://{}/\"]\n\tinsteadOf = https://github.com/\n",
            root.join("remotes").display()
        );
        std::fs::write(root.join("gitconfig"), config).expect("write");
        let world = World { root };
        world.install_ssh();
        world
    }

    /// A fake `ssh` first on `PATH`, so no test reaches a real host, and
    /// a test sees what `hd` asked ssh for. It logs its arguments to
    /// `ssh.log`, then serves `example.com` from the test's remotes by
    /// running the command it is given on the local path; any other host
    /// is refused, as `Permission denied (publickey)`.
    fn install_ssh(&self) {
        let bin = self.root.join("bin");
        std::fs::create_dir_all(&bin).expect("mkdir");
        let script = "#!/bin/sh\n\
            here=$(dirname \"$0\")\n\
            echo \"$*\" >> \"$here/../ssh.log\"\n\
            while [ \"$1\" = -o ]; do shift 2; done\n\
            case \"$1\" in\n\
            git@example.com) ;;\n\
            *) echo \"$1: Permission denied (publickey).\" >&2; exit 255 ;;\n\
            esac\n\
            command=$(printf '%s' \"$2\" | sed \"s|'/|'$here/../remotes/|; s|^git-upload-pack|git upload-pack|\")\n\
            exec sh -c \"$command\"\n";
        write(&bin, &[("ssh", script)]);
        // The user's own ssh command of a test, which logs and delegates.
        let user = "#!/bin/sh\n\
            here=$(dirname \"$0\")\n\
            echo user-ssh >> \"$here/../ssh.log\"\n\
            exec \"$here/ssh\" \"$@\"\n";
        write(&bin, &[("user-ssh", user)]);
        for name in ["ssh", "user-ssh"] {
            let path = bin.join(name);
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
        }
    }

    /// The arguments of each call of the fake ssh, one string each, and
    /// of the user's command, as `user-ssh`.
    fn ssh_calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.root.join("ssh.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    /// Adds `text` to the test's global git configuration.
    fn configure(&self, text: &str) {
        let path = self.root.join("gitconfig");
        let old = std::fs::read_to_string(&path).expect("read");
        std::fs::write(path, format!("{old}{text}")).expect("write");
    }

    /// Maps the URL prefix `from` to the local path `to`, as git's
    /// `insteadOf`.
    fn map(&self, from: &str, to: &Path) {
        self.configure(&format!(
            "[url \"file://{}\"]\n\tinsteadOf = {from}\n",
            to.display()
        ));
    }

    /// A command with only the test's environment.
    fn command(&self, program: &str, cwd: &Path) -> Command {
        let path = std::env::join_paths(std::iter::once(self.root.join("bin")).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .expect("PATH");
        let mut c = Command::new(program);
        c.env_clear()
            .current_dir(cwd)
            .env("PATH", path)
            .env("HOME", self.root.join("home"))
            .env("HD_CACHE", self.cache())
            .env("GIT_CONFIG_GLOBAL", self.root.join("gitconfig"))
            .env("GIT_CONFIG_NOSYSTEM", "1");
        c
    }

    fn cache(&self) -> PathBuf {
        self.root.join("cache")
    }

    fn git(&self, cwd: &Path, args: &[&str]) -> String {
        let out = self.command("git", cwd).args(args).output().expect("git");
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).into_owned()
    }

    /// A remote repository `github.com/acme/NAME`, one commit and tag per
    /// version, each commit holding exactly `files`.
    fn remote(&self, name: &str, versions: &[(&str, &[(&str, &str)])]) -> PathBuf {
        let dir = self.root.join("remotes/acme").join(name);
        std::fs::create_dir_all(&dir).expect("mkdir");
        self.git(&dir, &["init", "--quiet"]);
        for (tag, files) in versions {
            self.git(&dir, &["rm", "-r", "--quiet", "--ignore-unmatch", "."]);
            write(&dir, files);
            self.git(&dir, &["add", "--all"]);
            let message = if tag.is_empty() { "untagged" } else { tag };
            self.git(&dir, &["commit", "--quiet", "--allow-empty", "-m", message]);
            if !tag.is_empty() {
                self.git(&dir, &["tag", tag]);
            }
        }
        dir
    }

    fn package(&self, name: &str, files: &[(&str, &str)]) -> PathBuf {
        let dir = self.root.join(name);
        write(&dir, files);
        dir
    }

    fn hd(&self, cwd: &Path, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_hd"), cwd)
            .args(args)
            .output()
            .expect("hd")
    }
}

fn write(dir: &Path, files: &[(&str, &str)]) {
    for (path, text) in files {
        let p = dir.join(path);
        std::fs::create_dir_all(p.parent().expect("parent")).expect("mkdir");
        std::fs::write(p, text).expect("write");
    }
}

fn text(out: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[track_caller]
fn ok(out: &Output) -> (String, String) {
    let (stdout, stderr) = text(out);
    assert!(out.status.success(), "stdout: {stdout}\nstderr: {stderr}");
    (stdout, stderr)
}

#[track_caller]
fn fails(out: &Output) -> (String, String) {
    let (stdout, stderr) = text(out);
    assert_eq!(
        out.status.code(),
        Some(101),
        "stdout: {stdout}\nstderr: {stderr}"
    );
    (stdout, stderr)
}

/// A version's two `hd.sum` lines (`cli.sum.line`,
/// `cli.sum.manifest-line`): its tree line, then its manifest line.
fn sum_lines(key: &str, files: &[(&str, &str)]) -> String {
    let owned: Vec<(String, Vec<u8>)> = files
        .iter()
        .map(|(p, t)| ((*p).to_owned(), t.as_bytes().to_vec()))
        .collect();
    let manifest: Vec<(String, Vec<u8>)> = owned
        .iter()
        .filter(|(p, _)| p == "hd.toml")
        .cloned()
        .collect();
    format!(
        "{key} {}\n{key}/hd.toml {}\n",
        summary_hash(&owned),
        summary_hash(&manifest)
    )
}

fn manifest_line(key: &str, files: &[(&str, &str)]) -> String {
    sum_lines(key, files)
        .lines()
        .nth(1)
        .map(|l| format!("{l}\n"))
        .unwrap_or_default()
}

const TEXT_V1: &[(&str, &str)] = &[
    ("hd.toml", "[package]\nname = \"text\"\n"),
    (
        "src/lib.hd",
        "pub fn greeting() -> string: \"hello from 1.0\"\n",
    ),
];
const TEXT_V12: &[(&str, &str)] = &[
    ("hd.toml", "[package]\nname = \"text\"\n"),
    (
        "src/lib.hd",
        "pub fn greeting() -> string: \"hello from 1.2\"\n\npub fn shout(s: string) -> string: s + \"!\"\n",
    ),
];

const APP_MAIN: &str =
    "use dep.text.{greeting}\n\npub fn main() -> void $ Console:\n    println(greeting())\n";

fn app(world: &World, name: &str, deps: &str, main: &str) -> PathBuf {
    world.package(
        name,
        &[
            (
                "hd.toml",
                &format!("[package]\nname = \"{name}\"\n\n[dependencies]\n{deps}"),
            ),
            ("src/main.hd", main),
        ],
    )
}

/// `hd fetch` records the hashes, `hd run` builds against the fetched
/// library, and once cached it needs neither the repository nor git.
#[test]
fn fetch_records_hashes_and_run_works_offline() {
    let world = World::new("offline");
    let remote = world.remote("text", &[("v1.0.0", TEXT_V1), ("v1.2.0", TEXT_V12)]);
    let dir = app(
        &world,
        "app",
        "text = \"github.com/acme/text@1.0.0\"\n",
        APP_MAIN,
    );
    // Without hd.sum lines, a build fails before it fetches anything
    // (`cli.dep.missing-sum.manifest`).
    let (stdout, _) = fails(&world.hd(&dir, &["check", "--format", "json"]));
    assert!(
        stdout.contains("\"code\":\"missing-sum-entry\""),
        "{stdout}"
    );
    assert!(
        stdout.contains("\"file\":\"hd.toml\",\"line\":5"),
        "{stdout}"
    );
    assert!(!world.cache().join("pkg").exists());

    let (_, stderr) = ok(&world.hd(&dir, &["fetch"]));
    assert!(
        stderr.contains("hd: fetching github.com/acme/text@1.0.0"),
        "{stderr}"
    );
    let sum = std::fs::read_to_string(dir.join("hd.sum")).expect("hd.sum");
    assert_eq!(sum, sum_lines("github.com/acme/text@1.0.0", TEXT_V1));
    // The entry holds the tree, read-only, with its hash recorded beside it.
    let entry = world.cache().join("pkg/github.com/acme/text@1.0.0");
    let lib = entry.join("src/lib.hd");
    assert!(
        std::fs::metadata(&lib)
            .expect("lib")
            .permissions()
            .readonly()
    );
    assert!(!entry.join(".git").exists());
    let record = world.cache().join("hash/github.com/acme/text@1.0.0");
    assert_eq!(
        std::fs::read_to_string(&record).expect("record"),
        format!(
            "{}\n",
            sum.split_once(' ')
                .expect("hash")
                .1
                .lines()
                .next()
                .expect("line")
        )
    );

    let (stdout, _) = ok(&world.hd(&dir, &["run"]));
    assert_eq!(stdout, "hello from 1.0\n");
    // A second fetch finds everything: no fetch, no change.
    let (_, stderr) = ok(&world.hd(&dir, &["fetch"]));
    assert!(!stderr.contains("fetching"), "{stderr}");
    assert_eq!(
        std::fs::read_to_string(dir.join("hd.sum")).expect("hd.sum"),
        sum
    );

    // The repository is gone and git is not on PATH: the cache serves
    // every version (`cli.dep.offline`).
    hd_deps::cache::remove_tree(&remote).expect("remove remote");
    let (stdout, _) = ok(&world.hd(&dir, &["run"]));
    assert_eq!(stdout, "hello from 1.0\n");
    let empty = world.root.join("empty-path");
    std::fs::create_dir_all(&empty).expect("mkdir");
    let check = world
        .command(env!("CARGO_BIN_EXE_hd"), &dir)
        .env("PATH", &empty)
        .arg("check")
        .output()
        .expect("hd");
    ok(&check);
    // A build never writes hd.sum (`cli.dep.no-sum-write`).
    assert_eq!(
        std::fs::read_to_string(dir.join("hd.sum")).expect("hd.sum"),
        sum
    );
}

/// A tampered `hd.sum` line or cache entry is `sum-mismatch`, never a
/// warning, and `hd fetch` never replaces an entry (`cli.dep.verify`,
/// `cli.dep.verify.manifest`, `cli.sum.keep`).
#[test]
fn tampering_is_a_mismatch() {
    let world = World::new("tamper");
    world.remote("text", &[("v1.0.0", TEXT_V1)]);
    let dir = app(
        &world,
        "app",
        "text = \"github.com/acme/text@1.0.0\"\n",
        APP_MAIN,
    );
    ok(&world.hd(&dir, &["fetch"]));
    let good = std::fs::read_to_string(dir.join("hd.sum")).expect("hd.sum");
    let forged = format!("h1:{}=", "A".repeat(43));
    let (tree_line, manifest) = good.split_once('\n').expect("two lines");
    for bad in [
        format!(
            "{} {forged}\n{manifest}",
            tree_line.split_once(' ').expect("key").0
        ),
        format!(
            "{tree_line}\n{} {forged}\n",
            manifest.split_once(' ').expect("key").0
        ),
    ] {
        std::fs::write(dir.join("hd.sum"), &bad).expect("write");
        let (stdout, _) = fails(&world.hd(&dir, &["check", "--format", "json"]));
        assert!(
            stdout.contains("\"code\":\"sum-mismatch\",\"severity\":\"error\""),
            "{stdout}"
        );
        let (_, stderr) = fails(&world.hd(&dir, &["run"]));
        assert!(stderr.contains("hd.toml:5:1: sum-mismatch"), "{stderr}");
        fails(&world.hd(&dir, &["fetch"]));
        assert_eq!(
            std::fs::read_to_string(dir.join("hd.sum")).expect("sum"),
            bad
        );
    }
    std::fs::write(dir.join("hd.sum"), &good).expect("write");
    ok(&world.hd(&dir, &["check"]));
    // The cache's record of the tree hash, or the entry's manifest, changed.
    let record = world.cache().join("hash/github.com/acme/text@1.0.0");
    std::fs::write(&record, format!("{forged}\n")).expect("write");
    let (_, stderr) = fails(&world.hd(&dir, &["check"]));
    assert!(stderr.contains("sum-mismatch"), "{stderr}");
    hd_deps::cache::remove_tree(&world.cache()).expect("clean");
    ok(&world.hd(&dir, &["check"]));
    let entry = world.cache().join("pkg/github.com/acme/text@1.0.0");
    for (path, mode) in [(&entry, 0o755), (&entry.join("hd.toml"), 0o644)] {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).expect("chmod");
    }
    write(&entry, &[("hd.toml", "[package]\nname = \"evil\"\n")]);
    let (_, stderr) = fails(&world.hd(&dir, &["check"]));
    assert!(stderr.contains("sum-mismatch"), "{stderr}");
    assert!(stderr.contains("manifest's hash"), "{stderr}");
}

/// Minimal version selection reads a subdirectory package's manifest and
/// selects the largest minimum; an unknown tag or an unreachable
/// repository is an error with no fallback.
#[test]
fn selection_takes_the_largest_minimum() {
    let world = World::new("select");
    world.remote("text", &[("v1.0.0", TEXT_V1), ("v1.2.0", TEXT_V12)]);
    let lint: &[(&str, &str)] = &[
        (
            "hd.toml",
            "[package]\nname = \"lint\"\n\n[dependencies]\ntext = \"github.com/acme/text@1.2.0\"\n",
        ),
        (
            "src/lib.hd",
            "use dep.text.{shout}\n\npub fn hint(word: string) -> string: shout(word)\n",
        ),
    ];
    world.remote(
        "tools",
        &[(
            "lint/v0.3.0",
            &[
                ("README.md", "tools\n"),
                ("lint/hd.toml", lint[0].1),
                ("lint/src/lib.hd", lint[1].1),
                ("lint/nested/hd.toml", "[package]\nname = \"nested\"\n"),
                ("lint/nested/src/lib.hd", "pub fn n() -> i32: 1\n"),
            ],
        )],
    );
    let dir = app(
        &world,
        "app",
        "text = \"github.com/acme/text@1.0.0\"\nlint = \"github.com/acme/tools/lint@0.3.0\"\n",
        "use dep.lint.{hint}\nuse dep.text.{greeting}\n\npub fn main() -> void $ Console:\n    println(hint(greeting()))\n",
    );
    ok(&world.hd(&dir, &["fetch"]));
    // text 1.2.0 is selected; selection still read 1.0.0's manifest
    // (`cli.sum.manifest-line`), and the nested package is no part of
    // lint's tree (`cli.sum.tree`).
    assert_eq!(
        std::fs::read_to_string(dir.join("hd.sum")).expect("hd.sum"),
        manifest_line("github.com/acme/text@1.0.0", TEXT_V1)
            + &sum_lines("github.com/acme/text@1.2.0", TEXT_V12)
            + &sum_lines("github.com/acme/tools/lint@0.3.0", lint)
    );
    let (stdout, _) = ok(&world.hd(&dir, &["run"]));
    assert_eq!(stdout, "hello from 1.2!\n");

    let unknown = app(
        &world,
        "unknown",
        "text = \"github.com/acme/text@1.1.0\"\n",
        APP_MAIN,
    );
    let (stdout, _) = fails(&world.hd(&unknown, &["fetch", "--format", "json"]));
    assert!(stdout.contains("\"code\":\"unknown-version\""), "{stdout}");
    assert!(stdout.contains("1.0.0, 1.2.0"), "{stdout}");
    assert!(!unknown.join("hd.sum").exists());

    let unreachable = app(
        &world,
        "unreachable",
        "secret = \"github.com/acme/private@1.0.0\"\n",
        APP_MAIN,
    );
    let (_, stderr) = fails(&world.hd(&unreachable, &["fetch"]));
    assert!(
        stderr.contains("fetch-failed: cannot fetch github.com/acme/private@1.0.0"),
        "{stderr}"
    );
    assert!(
        stderr.contains("ssh://git@github.com/acme/private failed"),
        "{stderr}"
    );
    assert!(
        stderr.contains("configure a git credential helper"),
        "{stderr}"
    );
}

/// At a workspace root, `hd fetch` selects for every member at once and
/// writes the one `hd.sum` beside the workspace manifest
/// (`cli.dep.workspace-fetch`, `module.workspace.sum`); a member's build
/// verifies against it.
#[test]
fn a_workspace_fetches_once() {
    let world = World::new("workspace");
    world.remote("text", &[("v1.0.0", TEXT_V1), ("v1.2.0", TEXT_V12)]);
    let ws = world.package(
        "ws",
        &[("hd.toml", "[workspace]\nmembers = [\"app\", \"tool\"]\n")],
    );
    app(
        &world,
        "ws/app",
        "text = \"github.com/acme/text@1.0.0\"\n",
        APP_MAIN,
    );
    world.package(
        "ws/tool",
        &[
            (
                "hd.toml",
                "[package]\nname = \"tool\"\n\n[dependencies]\ntext = \"github.com/acme/text@1.2.0\"\n",
            ),
            ("src/lib.hd", "pub fn t() -> i32: 1\n"),
        ],
    );
    ok(&world.hd(&ws, &["fetch"]));
    assert!(!ws.join("app/hd.sum").exists());
    assert_eq!(
        std::fs::read_to_string(ws.join("hd.sum")).expect("hd.sum"),
        manifest_line("github.com/acme/text@1.0.0", TEXT_V1)
            + &sum_lines("github.com/acme/text@1.2.0", TEXT_V12)
    );
    // One selection for the workspace: app gets tool's minimum.
    let (stdout, _) = ok(&world.hd(&ws.join("app"), &["run"]));
    assert_eq!(stdout, "hello from 1.2\n");
}

/// A pseudo-version names one untagged commit by its time and hash
/// (`cli.dep.pseudo`); a wrong time is `unknown-version`.
#[test]
fn a_pseudo_version_fetches_its_commit() {
    let world = World::new("pseudo");
    let remote = world.remote("text", &[("v1.0.0", TEXT_V1), ("", TEXT_V12)]);
    // A commit's hash prefix and UTC time, as a pseudo-version names it.
    let commit = |rev: &str| {
        let hash = world.git(&remote, &["rev-parse", rev]);
        let stamp = world
            .command("git", &remote)
            .env("TZ", "UTC")
            .args([
                "log",
                "-1",
                "--date=format-local:%Y%m%d%H%M%S",
                "--format=%cd",
                rev,
            ])
            .output()
            .expect("git");
        let stamp = String::from_utf8_lossy(&stamp.stdout).trim().to_owned();
        assert_eq!(stamp.len(), 14, "{stamp}");
        (hash[..12].to_owned(), stamp)
    };
    let (hash, stamp) = commit("HEAD");
    let version = format!("1.0.1-0.{stamp}-{hash}");
    let dir = app(
        &world,
        "app",
        &format!("text = \"github.com/acme/text@{version}\"\n"),
        APP_MAIN,
    );
    ok(&world.hd(&dir, &["fetch"]));
    let (stdout, _) = ok(&world.hd(&dir, &["run"]));
    assert_eq!(stdout, "hello from 1.2\n");
    let (tagged, tagged_stamp) = commit("v1.0.0");
    for (name, wrong, why) in [
        (
            "time",
            format!("1.0.1-0.20000101000000-{hash}"),
            format!("did you mean {version}?"),
        ),
        (
            "base",
            format!("1.0.5-0.{stamp}-{hash}"),
            "has no tag v1.0.4".to_owned(),
        ),
        (
            "tagged",
            format!("1.0.1-0.{tagged_stamp}-{tagged}"),
            "is the tag v1.0.0 itself, so require 1.0.0".to_owned(),
        ),
        (
            "missing",
            format!("1.0.1-0.{stamp}-0123456789ab"),
            "has no commit 0123456789ab".to_owned(),
        ),
    ] {
        let bad = app(
            &world,
            name,
            &format!("text = \"github.com/acme/text@{wrong}\"\n"),
            APP_MAIN,
        );
        let (_, stderr) = fails(&world.hd(&bad, &["fetch"]));
        assert!(stderr.contains("unknown-version"), "{stderr}");
        assert!(stderr.contains(&why), "{stderr}");
        assert!(!bad.join("hd.sum").exists());
    }
}

const TEXT_V13: &[(&str, &str)] = &[
    ("hd.toml", "[package]\nname = \"text\"\n"),
    (
        "src/lib.hd",
        "pub fn greeting() -> string: \"hello from 1.3\"\n",
    ),
];
const TEXT_V2: &[(&str, &str)] = &[
    ("hd.toml", "[package]\nname = \"text\"\n"),
    (
        "src/lib.hd",
        "pub fn greeting() -> string: \"hello from 2.0\"\n",
    ),
];

/// A package with `hd.toml` text as given and a main that uses `text`.
fn bare(world: &World, name: &str, manifest: &str) -> PathBuf {
    world.package(name, &[("hd.toml", manifest), ("src/main.hd", APP_MAIN)])
}

fn read(dir: &Path, file: &str) -> String {
    std::fs::read_to_string(dir.join(file)).expect(file)
}

/// `hd add` sets the requirement, fetches, and records the hashes; a lower
/// version of the same host path lowers it, says so, and tidies the
/// older version's lines away (`cli.dep.add`, `cli.dep.add.lower`,
/// `cli.dep.select`, `cli.dep.tidy`, `cli.dep.edit`).
#[test]
fn add_records_then_lowers() {
    let world = World::new("add");
    world.remote("text", &[("v1.0.0", TEXT_V1), ("v1.2.0", TEXT_V12)]);
    let dir = bare(&world, "app", "[package]\nname = \"app\"  # mine\n");
    let (stdout, stderr) = ok(&world.hd(&dir, &["add", "text", "github.com/acme/text@1.2.0"]));
    assert_eq!(stdout, "");
    assert!(
        stderr.contains("hd: fetching github.com/acme/text@1.2.0"),
        "{stderr}"
    );
    assert_eq!(
        read(&dir, "hd.toml"),
        "[package]\nname = \"app\"  # mine\n\n[dependencies]\ntext = \"github.com/acme/text@1.2.0\"\n"
    );
    assert_eq!(
        read(&dir, "hd.sum"),
        sum_lines("github.com/acme/text@1.2.0", TEXT_V12)
    );
    let (stdout, _) = ok(&world.hd(&dir, &["run"]));
    assert_eq!(stdout, "hello from 1.2\n");

    let (stdout, _) = ok(&world.hd(&dir, &["add", "text", "github.com/acme/text@1.0.0"]));
    assert_eq!(stdout, "lowered text 1.2.0 -> 1.0.0\n");
    assert_eq!(
        read(&dir, "hd.toml"),
        "[package]\nname = \"app\"  # mine\n\n[dependencies]\ntext = \"github.com/acme/text@1.0.0\"\n"
    );
    assert_eq!(
        read(&dir, "hd.sum"),
        sum_lines("github.com/acme/text@1.0.0", TEXT_V1)
    );
    let (stdout, _) = ok(&world.hd(&dir, &["run"]));
    assert_eq!(stdout, "hello from 1.0\n");

    // Raising says nothing.
    let (stdout, _) = ok(&world.hd(&dir, &["add", "text", "github.com/acme/text@1.2.0"]));
    assert_eq!(stdout, "");
}

/// A lowered requirement is still a minimum: another member's higher one
/// stays selected (`cli.dep.add.lower.selection`), and the one
/// `hd.sum` of the workspace holds both reached manifests.
#[test]
fn a_lowered_requirement_stays_a_minimum() {
    let world = World::new("lower-selection");
    world.remote("text", &[("v1.0.0", TEXT_V1), ("v1.2.0", TEXT_V12)]);
    let ws = world.package(
        "ws",
        &[("hd.toml", "[workspace]\nmembers = [\"app\", \"tool\"]\n")],
    );
    app(
        &world,
        "ws/app",
        "text = \"github.com/acme/text@1.2.0\"\n",
        APP_MAIN,
    );
    world.package(
        "ws/tool",
        &[
            (
                "hd.toml",
                "[package]\nname = \"tool\"\n\n[dependencies]\ntext = \"github.com/acme/text@1.2.0\"\n",
            ),
            ("src/lib.hd", "pub fn t() -> i32: 1\n"),
        ],
    );
    ok(&world.hd(&ws, &["fetch"]));
    let (stdout, _) = ok(&world.hd(
        &ws.join("app"),
        &["add", "text", "github.com/acme/text@1.0.0"],
    ));
    assert_eq!(stdout, "lowered text 1.2.0 -> 1.0.0\n");
    assert!(read(&ws.join("app"), "hd.toml").contains("text@1.0.0"));
    assert_eq!(
        read(&ws, "hd.sum"),
        manifest_line("github.com/acme/text@1.0.0", TEXT_V1)
            + &sum_lines("github.com/acme/text@1.2.0", TEXT_V12)
    );
    assert!(!ws.join("app/hd.sum").exists());
    let (stdout, _) = ok(&world.hd(&ws.join("app"), &["run"]));
    assert_eq!(stdout, "hello from 1.2\n");
}

/// `hd add --dev` writes `[dev-dependencies]`; a key of the other table is
/// an error that writes nothing (`cli.dep.add.dev`,
/// `cli.dep.add.other-table`, `cli.dep.unchanged-on-error`).
#[test]
fn add_dev_and_the_other_table() {
    let world = World::new("add-dev");
    world.remote("text", &[("v1.0.0", TEXT_V1)]);
    let dir = bare(&world, "app", "[package]\nname = \"app\"\n");
    ok(&world.hd(
        &dir,
        &["add", "--dev", "text", "github.com/acme/text@1.0.0"],
    ));
    let manifest =
        "[package]\nname = \"app\"\n\n[dev-dependencies]\ntext = \"github.com/acme/text@1.0.0\"\n";
    assert_eq!(read(&dir, "hd.toml"), manifest);
    let sum = read(&dir, "hd.sum");
    assert_eq!(sum, sum_lines("github.com/acme/text@1.0.0", TEXT_V1));

    // The key is in `[dev-dependencies]`, so a plain add is an error.
    let (_, stderr) = fails(&world.hd(&dir, &["add", "text", "github.com/acme/text@1.0.0"]));
    assert_eq!(
        stderr,
        "hd add: text is in [dev-dependencies]; run `hd remove text` first\n"
    );
    assert_eq!(read(&dir, "hd.toml"), manifest);
    assert_eq!(read(&dir, "hd.sum"), sum);

    // And the reverse names `[dependencies]`.
    let other = bare(
        &world,
        "other",
        "[package]\nname = \"other\"\n\n[dependencies]\ntext = \"github.com/acme/text@1.0.0\"\n",
    );
    let (_, stderr) = fails(&world.hd(
        &other,
        &["add", "--dev", "text", "github.com/acme/text@1.0.0"],
    ));
    assert_eq!(
        stderr,
        "hd add: text is in [dependencies]; run `hd remove text` first\n"
    );
    assert!(!other.join("hd.sum").exists());
}

/// A requirement that fails its check, names no tag, or has no reachable
/// repository changes neither file (`cli.dep.add.check`,
/// `cli.dep.unchanged-on-error`).
#[test]
fn a_failed_add_writes_nothing() {
    let world = World::new("add-fails");
    world.remote("text", &[("v1.0.0", TEXT_V1)]);
    world.remote("tail", &[("v0.1.0", TEXT_V12)]);
    let manifest =
        "[package]\nname = \"app\"\n\n[dependencies]\ntail = \"github.com/acme/tail@0.1.0\"\n";
    let dir = bare(&world, "app", manifest);
    ok(&world.hd(&dir, &["fetch"]));
    let sum = read(&dir, "hd.sum");
    for (args, code) in [
        (
            ["add", "text", "github.com/acme/text@v1.0.0"],
            "invalid-requirement",
        ),
        (
            ["add", "text", "github.com/acme@1.0.0"],
            "invalid-requirement",
        ),
        (
            ["add", "9text", "github.com/acme/text@1.0.0"],
            "invalid-requirement",
        ),
        // `tail` already names this package and compatibility line.
        (
            ["add", "again", "github.com/acme/tail@0.1.0"],
            "invalid-requirement",
        ),
        (
            ["add", "text", "github.com/acme/text@1.9.0"],
            "unknown-version",
        ),
        (
            ["add", "text", "github.com/acme/missing@1.0.0"],
            "fetch-failed",
        ),
    ] {
        let (_, stderr) = fails(&world.hd(&dir, &args));
        assert!(stderr.contains(code), "{args:?}: {stderr}");
        assert_eq!(read(&dir, "hd.toml"), manifest, "{args:?}");
        assert_eq!(read(&dir, "hd.sum"), sum, "{args:?}");
    }
    // The message of a requirement that is not in `hd.toml` has no line.
    let (_, stderr) = fails(&world.hd(&dir, &["add", "text", "github.com/acme/text@1.9.0"]));
    assert!(!stderr.contains("hd.toml:"), "{stderr}");
    let (_, stderr) = fails(&world.hd(&dir, &["add", "text"]));
    assert!(stderr.contains("takes NAME and PATH@VERSION"), "{stderr}");
}

/// `hd update` moves each requirement, dev ones too, to the newest release
/// on its compatibility line, skipping pre-releases, and `hd update NAME`
/// only NAME's (`cli.dep.update`, `cli.dep.update.release`,
/// `cli.dep.update.dev`).
#[test]
fn update_moves_to_the_newest_release() {
    let world = World::new("update");
    world.remote(
        "text",
        &[
            ("v1.0.0", TEXT_V1),
            ("v1.2.0", TEXT_V12),
            ("v1.3.0-rc.1", TEXT_V13),
            ("v2.0.0", TEXT_V2),
        ],
    );
    world.remote("tail", &[("v0.1.0", TEXT_V1), ("v0.1.4", TEXT_V12)]);
    let dir = bare(
        &world,
        "app",
        "[package]\nname = \"app\"\n\n[dependencies]\n# the text\ntext = \"github.com/acme/text@1.0.0\"  # pinned\n\n[dev-dependencies]\ntail = \"github.com/acme/tail@0.1.0\"\n",
    );
    ok(&world.hd(&dir, &["fetch"]));

    // Only NAME moves.
    let (stdout, _) = ok(&world.hd(&dir, &["update", "tail"]));
    assert_eq!(stdout, "tail: 0.1.0 -> 0.1.4\n");
    assert!(read(&dir, "hd.toml").contains("tail = \"github.com/acme/tail@0.1.4\"\n"));
    assert!(read(&dir, "hd.toml").contains("text@1.0.0\"  # pinned"));

    let (stdout, _) = ok(&world.hd(&dir, &["update"]));
    assert_eq!(stdout, "text: 1.0.0 -> 1.2.0\n");
    assert_eq!(
        read(&dir, "hd.toml"),
        "[package]\nname = \"app\"\n\n[dependencies]\n# the text\ntext = \"github.com/acme/text@1.2.0\"  # pinned\n\n[dev-dependencies]\ntail = \"github.com/acme/tail@0.1.4\"\n"
    );
    // Tidy: the new versions' lines are there, and the old ones' are gone.
    assert_eq!(
        read(&dir, "hd.sum"),
        sum_lines("github.com/acme/tail@0.1.4", TEXT_V12)
            + &sum_lines("github.com/acme/text@1.2.0", TEXT_V12)
    );
    // Already the newest release on its line: nothing moves.
    let before = read(&dir, "hd.toml");
    let (stdout, _) = ok(&world.hd(&dir, &["update"]));
    assert_eq!(stdout, "");
    assert_eq!(read(&dir, "hd.toml"), before);

    let (_, stderr) = fails(&world.hd(&dir, &["update", "nothing"]));
    assert!(
        stderr.starts_with("hd update: `nothing` is no key"),
        "{stderr}"
    );
}

/// `hd update` lists the tags even when the cache holds every version, so
/// an unreachable repository is an error that writes nothing
/// (`cli.dep.update.network`, `cli.dep.unchanged-on-error`).
#[test]
fn update_needs_the_network() {
    let world = World::new("update-network");
    let remote = world.remote("text", &[("v1.0.0", TEXT_V1), ("v1.2.0", TEXT_V12)]);
    let manifest =
        "[package]\nname = \"app\"\n\n[dependencies]\ntext = \"github.com/acme/text@1.0.0\"\n";
    let dir = bare(&world, "app", manifest);
    ok(&world.hd(&dir, &["fetch"]));
    let sum = read(&dir, "hd.sum");
    hd_deps::cache::remove_tree(&remote).expect("remove remote");
    let (_, stderr) = fails(&world.hd(&dir, &["update"]));
    assert!(stderr.contains("fetch-failed"), "{stderr}");
    assert_eq!(read(&dir, "hd.toml"), manifest);
    assert_eq!(read(&dir, "hd.sum"), sum);
    // `hd check` still works offline.
    ok(&world.hd(&dir, &["check"]));
}

/// `hd remove` tidies `hd.sum`: the removed requirement's lines go, the
/// others stay; at a member it tidies the workspace's `hd.sum`, where
/// another member's requirement keeps a version
/// (`cli.dep.remove`, `cli.dep.tidy`, `module.workspace.sum`).
#[test]
fn remove_tidies_hd_sum() {
    let world = World::new("remove");
    world.remote("text", &[("v1.0.0", TEXT_V1)]);
    world.remote("tail", &[("v0.1.0", TEXT_V12)]);
    let dir = bare(
        &world,
        "app",
        "[package]\nname = \"app\"\n\n[dependencies]\ntext = \"github.com/acme/text@1.0.0\"\n\n[dev-dependencies]\ntail = \"github.com/acme/tail@0.1.0\"\n",
    );
    ok(&world.hd(&dir, &["fetch"]));
    ok(&world.hd(&dir, &["remove", "tail"]));
    assert_eq!(
        read(&dir, "hd.toml"),
        "[package]\nname = \"app\"\n\n[dependencies]\ntext = \"github.com/acme/text@1.0.0\"\n"
    );
    assert_eq!(
        read(&dir, "hd.sum"),
        sum_lines("github.com/acme/text@1.0.0", TEXT_V1)
    );

    let ws = world.package(
        "ws",
        &[("hd.toml", "[workspace]\nmembers = [\"one\", \"two\"]\n")],
    );
    for member in ["one", "two"] {
        world.package(
            &format!("ws/{member}"),
            &[
                (
                    "hd.toml",
                    &format!(
                        "[package]\nname = \"{member}\"\n\n[dependencies]\ntext = \"github.com/acme/text@1.0.0\"\n"
                    ),
                ),
                ("src/lib.hd", "pub fn t() -> i32: 1\n"),
            ],
        );
    }
    ok(&world.hd(&ws, &["fetch"]));
    let sum = read(&ws, "hd.sum");
    ok(&world.hd(&ws.join("one"), &["remove", "text"]));
    assert_eq!(read(&ws, "hd.sum"), sum, "two still requires text");
    assert!(!read(&ws.join("one"), "hd.toml").contains("text"));
    ok(&world.hd(&ws.join("two"), &["remove", "text"]));
    assert_eq!(read(&ws, "hd.sum"), "");
    assert_eq!(
        read(&ws.join("two"), "hd.toml"),
        "[package]\nname = \"two\"\n"
    );
}

/// At a workspace root the commands that edit a requirement are errors
/// that point at a member (`cli.dep.workspace-member-only`), and outside a
/// package they point at `hd new` (`cli.dep.package-only`).
#[test]
fn edits_belong_to_a_member() {
    let world = World::new("member-only");
    let manifest = "[workspace]\nmembers = [\"app\"]\n";
    let ws = world.package("ws", &[("hd.toml", manifest)]);
    app(&world, "ws/app", "", APP_MAIN);
    for args in [
        &["add", "text", "github.com/acme/text@1.0.0"][..],
        &["update"],
        &["remove", "text"],
    ] {
        let (_, stderr) = fails(&world.hd(&ws, args));
        assert!(
            stderr.starts_with(&format!("hd {}:", args[0])) && stderr.contains("member"),
            "{args:?}: {stderr}"
        );
        assert_eq!(read(&ws, "hd.toml"), manifest);
        assert!(!ws.join("hd.sum").exists());
    }
    let empty = world.root.join("nowhere");
    std::fs::create_dir_all(&empty).expect("mkdir");
    for args in [
        &["add", "text", "github.com/acme/text@1.0.0"][..],
        &["update"],
        &["remove", "text"],
    ] {
        let (_, stderr) = fails(&world.hd(&empty, args));
        assert!(stderr.contains("hd new"), "{args:?}: {stderr}");
    }
}

/// A directory no repository is in, which a host's HTTPS URLs map to in a
/// test where HTTPS fails.
fn no_remotes(world: &World) -> PathBuf {
    world.root.join("no-such-remotes")
}

/// HTTPS answers: `hd` never starts ssh (`cli.dep.repo-url`).
#[test]
fn https_answers_without_ssh() {
    let world = World::new("https-first");
    world.remote("text", &[("v1.0.0", TEXT_V1)]);
    let dir = app(
        &world,
        "app",
        "text = \"github.com/acme/text@1.0.0\"\n",
        APP_MAIN,
    );
    ok(&world.hd(&dir, &["fetch"]));
    assert!(world.ssh_calls().is_empty(), "{:?}", world.ssh_calls());
}

/// HTTPS fails, and the SSH URL `ssh://git@HOST/REPO` answers; the
/// `insteadOf` of the user's configuration applies to it as to the HTTPS
/// one, so ssh itself never starts here.
#[test]
fn ssh_answers_when_https_fails() {
    let world = World::new("ssh-fallback");
    world.remote("text.git", &[("v1.0.0", TEXT_V1)]);
    world.map("https://example.com/", &no_remotes(&world));
    world.map("ssh://git@example.com/", &world.root.join("remotes/"));
    let dir = app(
        &world,
        "app",
        "text = \"example.com/acme/text.git@1.0.0\"\n",
        APP_MAIN,
    );
    ok(&world.hd(&dir, &["fetch"]));
    assert!(read(&dir, "hd.sum").contains("example.com/acme/text.git@1.0.0"));
    assert!(world.ssh_calls().is_empty(), "{:?}", world.ssh_calls());
}

/// Once a host answers over SSH, its later repositories try SSH first:
/// the HTTPS URL of `b.git` would answer, and is not used. ssh runs in
/// batch mode with a connect timeout, and the ls-remote and the fetch of
/// one repository use the same scheme.
#[test]
fn a_host_that_answered_over_ssh_is_tried_over_ssh_first() {
    let world = World::new("ssh-memory");
    world.remote("a.git", &[("v1.0.0", TEXT_V1)]);
    world.remote("b.git", &[("v1.0.0", TEXT_V1)]);
    world.map("https://example.com/acme/a.git", &no_remotes(&world));
    world.map(
        "https://example.com/acme/b.git",
        &world.root.join("remotes/acme/b.git"),
    );
    let dir = app(
        &world,
        "app",
        "first = \"example.com/acme/a.git@1.0.0\"\nsecond = \"example.com/acme/b.git@1.0.0\"\n",
        APP_MAIN,
    );
    ok(&world.hd(&dir, &["fetch"]));
    let calls = world.ssh_calls();
    let of = |repo: &str| calls.iter().filter(|c| c.contains(repo)).count();
    assert_eq!(of("acme/a.git"), 2, "{calls:?}");
    assert_eq!(of("acme/b.git"), 2, "{calls:?}");
    for call in &calls {
        assert!(
            call.starts_with("-o BatchMode=yes -o ConnectTimeout=10 "),
            "{call}"
        );
    }
}

/// Both URLs fail: one `fetch-failed` names the version, both URLs and
/// git's last line of each, and says how to settle on one scheme.
#[test]
fn both_urls_failing_is_one_error() {
    let world = World::new("ssh-both-fail");
    world.map("https://example.org/", &no_remotes(&world));
    let dir = app(
        &world,
        "app",
        "text = \"example.org/acme/gone.git@1.0.0\"\n",
        APP_MAIN,
    );
    let (_, stderr) = fails(&world.hd(&dir, &["fetch"]));
    for part in [
        "fetch-failed: cannot fetch example.org/acme/gone.git@1.0.0: ",
        "https://example.org/acme/gone.git failed: ",
        "does not appear to be a git repository; ssh://git@example.org/acme/gone.git failed: ",
        "failed: git@example.org: Permission denied (publickey); to use SSH only, run `git config --global url.\"git@example.org:\".insteadOf \"https://example.org/\"`; to use HTTPS, configure a git credential helper",
    ] {
        assert!(stderr.contains(part), "{part}: {stderr}");
    }
    assert_eq!(stderr.matches("fetch-failed").count(), 1, "{stderr}");
    // git's generic trailer does not stand for the cause.
    assert!(!stderr.contains("Could not read from remote"), "{stderr}");
}

/// An ssh command the user set stays as it is.
#[test]
fn a_user_ssh_command_is_kept() {
    let world = World::new("ssh-user-command");
    world.remote("text.git", &[("v1.0.0", TEXT_V1)]);
    world.map("https://example.com/", &no_remotes(&world));
    let dir = app(
        &world,
        "app",
        "text = \"example.com/acme/text.git@1.0.0\"\n",
        APP_MAIN,
    );
    let out = world
        .command(env!("CARGO_BIN_EXE_hd"), &dir)
        .env("GIT_SSH_COMMAND", world.root.join("bin/user-ssh"))
        .arg("fetch")
        .output()
        .expect("hd");
    ok(&out);
    let calls = world.ssh_calls();
    assert!(calls.iter().any(|c| c == "user-ssh"), "{calls:?}");
    assert!(
        calls
            .iter()
            .all(|c| !c.contains("BatchMode") && !c.contains("ConnectTimeout")),
        "{calls:?}"
    );
}
