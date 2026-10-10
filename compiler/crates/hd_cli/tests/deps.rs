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
        World { root }
    }

    /// A command with only the test's environment.
    fn command(&self, program: &str, cwd: &Path) -> Command {
        let mut c = Command::new(program);
        c.env_clear()
            .current_dir(cwd)
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
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
    assert!(stderr.contains("git's own credentials"), "{stderr}");
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
