//! The Node every test runs (#2059): the version the repository pins in
//! `.node-version`, found once per test process. A test that starts
//! `node` itself takes [`command`]; a test that starts `hd`, which starts
//! `node` from PATH, gives the child [`path_env`].
//!
//! Which Node: PATH's `node` when it is the pinned version, else fnm's
//! install of the pinned version, else PATH's `node` when it is newer. An
//! older Node (for example 24, without WebAssembly JSPI) is no choice:
//! every caller fails with one message naming `.node-version` and the
//! version found, never with a program's own failure.
//!
//! cost: at most two process starts (`node`, then `fnm exec`) per test
//! process, memoized; a caller pays one lock-free read.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The pinned version, as `.node-version` spells it.
const PINNED: &str = include_str!("../../../.node-version");

/// A Node version, `major.minor.patch`.
type Version = [u32; 3];

/// A Node's version and its executable.
type Found = (Version, PathBuf);

/// Where fnm's install of the pinned version was looked for.
#[derive(Debug)]
enum Fnm {
    /// No `fnm` on PATH.
    Absent,
    /// `fnm` has no install of the pinned version, or it ran some other.
    NoInstall,
    Found(Found),
}

static CHOSEN: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// The pinned version, without a leading `v`.
#[must_use]
pub fn pinned() -> &'static str {
    PINNED.trim().trim_start_matches('v')
}

/// The Node executable the tests run.
///
/// # Panics
/// No Node of the pinned version or newer is found: the message names
/// `.node-version` and the version on PATH.
#[must_use]
pub fn binary() -> &'static Path {
    found().unwrap_or_else(|why| panic!("{why}"))
}

/// [`binary`], or why no Node serves.
///
/// # Errors
/// No Node of the pinned version or newer is found.
pub fn found() -> Result<&'static Path, &'static str> {
    match CHOSEN.get_or_init(resolve) {
        Ok(path) => Ok(path),
        Err(why) => Err(why),
    }
}

/// A command that starts [`binary`].
///
/// # Panics
/// As [`binary`].
#[must_use]
pub fn command() -> Command {
    // cost: builds one command; the caller starts it once.
    Command::new(binary())
}

/// PATH with [`binary`]'s directory first, for a child that starts `node`
/// itself.
///
/// # Panics
/// As [`binary`].
#[must_use]
pub fn path_env() -> OsString {
    path_before(binary())
}

/// PATH with `node`'s directory first.
fn path_before(node: &Path) -> OsString {
    let dir = node.parent().map(Path::to_path_buf).unwrap_or_default();
    let rest = std::env::var_os("PATH").unwrap_or_default();
    std::env::join_paths(std::iter::once(dir).chain(std::env::split_paths(&rest))).unwrap_or(rest)
}

fn resolve() -> Result<PathBuf, String> {
    let want =
        parse(pinned()).ok_or_else(|| format!(".node-version holds no version: {PINNED:?}"))?;
    // cost: one `node` start, and one `fnm exec` only when PATH has no
    // pinned Node: once per test process (`CHOSEN`).
    let on_path = probe(Command::new("node"));
    choose(want, &on_path, || {
        // cost: one `fnm exec` process, once per test process.
        let mut fnm = Command::new("fnm");
        fnm.arg("exec")
            .arg(format!("--using={}", pinned()))
            .args(["--", "node"]);
        match probe(fnm) {
            Ok(found) => Fnm::Found(found),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Fnm::Absent,
            Err(_) => Fnm::NoInstall,
        }
    })
}

/// Picks the Node to run from PATH's `node` and, only when that is not the
/// pinned version, fnm's.
fn choose(
    want: Version,
    on_path: &std::io::Result<Found>,
    fnm: impl FnOnce() -> Fnm,
) -> Result<PathBuf, String> {
    if let Ok((version, path)) = on_path
        && *version == want
    {
        return Ok(path.clone());
    }
    let fnm = fnm();
    if let Fnm::Found((version, path)) = &fnm
        && *version == want
    {
        return Ok(path.clone());
    }
    if let Ok((version, path)) = on_path
        && *version > want
    {
        return Ok(path.clone());
    }
    let found = match on_path {
        Ok((version, _)) => format!("`node` on PATH is {}", show(*version)),
        Err(e) => format!("no `node` runs from PATH ({e})"),
    };
    let fnm = match fnm {
        Fnm::Absent => "fnm is not installed".to_owned(),
        Fnm::NoInstall | Fnm::Found(_) => {
            format!("fnm has no Node {} (run `fnm install`)", show(want))
        }
    };
    Err(format!(
        "the tests run on Node {}, as `.node-version` pins it, or newer; {found}, and {fnm}",
        show(want)
    ))
}

/// The version and executable of the `node` that `command` starts.
fn probe(mut command: Command) -> std::io::Result<Found> {
    let out = command
        .args(["-p", "process.version + '\\n' + process.execPath"])
        .stdin(std::process::Stdio::null())
        .output()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut lines = text.lines();
    match (
        out.status.success(),
        lines.next().and_then(parse),
        lines.next(),
    ) {
        (true, Some(version), Some(path)) => Ok((version, PathBuf::from(path))),
        _ => Err(std::io::Error::other(format!(
            "it printed no version: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))),
    }
}

fn parse(text: &str) -> Option<Version> {
    let mut parts = text.trim().trim_start_matches('v').split('.');
    let mut version = [0; 3];
    for slot in &mut version {
        *slot = parts.next()?.parse().ok()?;
    }
    parts.next().is_none().then_some(version)
}

fn show([major, minor, patch]: Version) -> String {
    format!("{major}.{minor}.{patch}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const WANT: Version = [26, 13, 1];

    #[expect(
        clippy::unnecessary_wraps,
        reason = "stands for PATH's probe, which can fail"
    )]
    fn at(version: Version, path: &str) -> std::io::Result<Found> {
        Ok((version, PathBuf::from(path)))
    }

    fn unused() -> Fnm {
        panic!("fnm is not asked when PATH has the pinned Node")
    }

    #[test]
    fn the_pinned_version_parses() {
        assert_eq!(parse(pinned()).map(show).as_deref(), Some(pinned()));
        assert_eq!(parse("v24.19.0\n"), Some([24, 19, 0]));
        for bad in ["", "v26", "26.13", "26.13.1.0", "26.x.1", "lts"] {
            assert_eq!(parse(bad), None, "{bad}");
        }
    }

    #[test]
    fn path_node_of_the_pinned_version_wins_without_fnm() {
        assert_eq!(
            choose(WANT, &at(WANT, "/path/node"), unused),
            Ok(PathBuf::from("/path/node"))
        );
    }

    #[test]
    fn fnm_pinned_node_beats_an_older_or_newer_path_node() {
        for path_version in [[24, 19, 0], [27, 0, 0]] {
            let chosen = choose(WANT, &at(path_version, "/path/node"), || {
                Fnm::Found((WANT, PathBuf::from("/fnm/node")))
            });
            assert_eq!(chosen, Ok(PathBuf::from("/fnm/node")));
        }
    }

    #[test]
    fn a_newer_path_node_serves_when_fnm_has_no_pinned_node() {
        for fnm in [Fnm::Absent, Fnm::NoInstall] {
            assert_eq!(
                choose(WANT, &at([26, 14, 0], "/path/node"), || fnm),
                Ok(PathBuf::from("/path/node"))
            );
        }
        // An fnm that ran some other version is no pinned Node.
        let chosen = choose(WANT, &at([27, 0, 0], "/path/node"), || {
            Fnm::Found(([24, 19, 0], PathBuf::from("/fnm/node")))
        });
        assert_eq!(chosen, Ok(PathBuf::from("/path/node")));
    }

    #[test]
    fn an_older_path_node_without_fnm_fails_naming_node_version_and_the_version_found() {
        let absent = choose(WANT, &at([24, 19, 0], "/path/node"), || Fnm::Absent);
        assert_eq!(
            absent,
            Err(
                "the tests run on Node 26.13.1, as `.node-version` pins it, or newer; \
                 `node` on PATH is 24.19.0, and fnm is not installed"
                    .to_owned()
            )
        );
        let other = choose(WANT, &at([26, 13, 0], "/path/node"), || {
            Fnm::Found(([26, 12, 0], PathBuf::from("/fnm/node")))
        });
        assert_eq!(
            other,
            Err(
                "the tests run on Node 26.13.1, as `.node-version` pins it, or newer; \
                 `node` on PATH is 26.13.0, and fnm has no Node 26.13.1 (run `fnm install`)"
                    .to_owned()
            )
        );
    }

    #[test]
    fn no_node_on_path_and_no_fnm_install_fails_once_with_the_reason() {
        let chosen = choose(
            WANT,
            &Err(std::io::Error::from(std::io::ErrorKind::NotFound)),
            || Fnm::NoInstall,
        );
        let why = chosen.expect_err("no Node");
        assert!(why.starts_with("the tests run on Node 26.13.1, as `.node-version` pins it"));
        assert!(why.contains("no `node` runs from PATH"), "{why}");
        assert!(
            why.ends_with("fnm has no Node 26.13.1 (run `fnm install`)"),
            "{why}"
        );
    }

    #[test]
    fn this_machine_resolves_a_node_at_least_the_pinned_one() {
        let ran = command().arg("--version").output().expect("node runs");
        let version = parse(&String::from_utf8_lossy(&ran.stdout)).expect("a version");
        assert!(version >= parse(pinned()).expect("pinned"));
        assert_eq!(found(), Ok(binary()));
        let dir = binary().parent().expect("a directory");
        let path = path_env();
        assert_eq!(std::env::split_paths(&path).next().as_deref(), Some(dir));
    }

    #[test]
    fn path_before_puts_the_node_directory_first_and_keeps_the_rest() {
        let path = path_before(Path::new("/pinned/bin/node"));
        let dirs: Vec<PathBuf> = std::env::split_paths(&path).collect();
        assert_eq!(dirs[0], Path::new("/pinned/bin"));
        let rest = std::env::var_os("PATH").unwrap_or_default();
        assert_eq!(
            dirs[1..],
            std::env::split_paths(&rest).collect::<Vec<_>>()[..]
        );
    }
}
