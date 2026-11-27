//! Builds a test's C-ABI library (a `cdylib`) from one Rust source file,
//! with the `rustc` of a toolchain already installed, and never through a
//! rustup proxy. CI's test jobs run binaries built elsewhere and install no
//! toolchain. There a proxy, seeing the repository's `rust-toolchain.toml`
//! (or the build's `RUSTUP_TOOLCHAIN`), tried to install that channel from
//! several tests at once, and the installs conflicted or hung.
//!
//! The compiler is found once per test binary ([`rustc`]): with rustup, the
//! build's own toolchain if it is installed, else the default one, read
//! from `rustup toolchain list -v` with auto-install off, outside the
//! repository; without rustup, the sysroot of the `rustc` on `PATH`. Builds
//! run one at a time, each with a bounded wait, and a failure panics with
//! the reason.
//!
//! Cost: one `rustup` process per test binary, then one compiler process
//! per library.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// How long one build, or one `rustup` query, may take.
const LIMIT: Duration = Duration::from_mins(5);

/// The compiler every build uses, or why there is none.
static RUSTC: OnceLock<Result<PathBuf, String>> = OnceLock::new();

/// Held for each build: test libraries are built one at a time.
static BUILDS: Mutex<()> = Mutex::new(());

fn exe(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    }
}

/// Runs `command` to its end within `limit`, killing it past that: its
/// output, or why it did not finish. Its standard output and error go to
/// files beside the temporary directory, so a chatty child never blocks
/// on a full pipe.
///
/// # Errors
///
/// When the command does not start, or runs over `limit`.
pub fn output_within(mut command: Command, limit: Duration) -> Result<Output, String> {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let base = std::env::temp_dir().join(format!("hd-testkit-{}-{n}", std::process::id()));
    let (out, err) = (base.with_extension("out"), base.with_extension("err"));
    let files = (std::fs::File::create(&out), std::fs::File::create(&err));
    let (Ok(stdout), Ok(stderr)) = files else {
        return Err(format!("no output files at {}", base.display()));
    };
    command.stdin(Stdio::null()).stdout(stdout).stderr(stderr);
    // cost: one child process, waited for at most `limit`.
    let mut child = command
        .spawn()
        .map_err(|e| format!("{command:?} did not start: {e}"))?;
    let deadline = Instant::now() + limit;
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) if Instant::now() < deadline => {
                // cost: a bounded poll of one child process.
                std::thread::sleep(Duration::from_millis(20));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                break Err(format!(
                    "{command:?} ran over {} s and was killed",
                    limit.as_secs()
                ));
            }
            Err(e) => break Err(format!("{command:?}: {e}")),
        }
    };
    let stdout = std::fs::read(&out).unwrap_or_default();
    let stderr = std::fs::read(&err).unwrap_or_default();
    let _ = std::fs::remove_file(&out);
    let _ = std::fs::remove_file(&err);
    finished.map(|status| Output {
        status,
        stdout,
        stderr,
    })
}

/// Runs a toolchain query or build from outside the repository, so no
/// `rust-toolchain.toml` applies, with auto-install off: its standard
/// output, or why it failed.
fn output(mut command: Command, what: &str) -> Result<String, String> {
    command
        .current_dir(std::env::temp_dir())
        .env("RUSTUP_AUTO_INSTALL", "0");
    let out = output_within(command, LIMIT)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(format!(
            "{what} failed ({}):\n{}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        ))
    }
}

/// The installed toolchain's `rustc`: the build's own when rustup has it,
/// else rustup's default, else the sysroot of the `rustc` on `PATH`.
fn resolve() -> Result<PathBuf, String> {
    let rustc = Path::new("bin").join(exe("rustc"));
    if let Ok(list) = output(
        {
            // cost: one `rustup` query per test binary.
            let mut c = Command::new("rustup");
            c.args(["toolchain", "list", "-v"]);
            c
        },
        "rustup",
    ) {
        return pick(&list, option_env!("RUSTUP_TOOLCHAIN"), |path| {
            path.is_file()
        })
        .ok_or_else(|| format!("rustup lists no installed toolchain with a rustc:\n{list}"));
    }
    let sysroot = output(
        {
            // cost: one `rustc` query per test binary, without rustup.
            let mut c = Command::new("rustc");
            c.args(["--print", "sysroot"]);
            c
        },
        "rustc",
    )?;
    let path = PathBuf::from(sysroot.trim()).join(&rustc);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!("no rustc at {}", path.display()))
    }
}

/// The `rustc` of the toolchain to use from `rustup toolchain list -v`'s
/// `list`, each line `NAME [(default)] [(active)] PATH`: the one named
/// `build`, else the default, else the first, among those whose `rustc`
/// `exists`.
fn pick(list: &str, build: Option<&str>, exists: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    let rustc = Path::new("bin").join(exe("rustc"));
    let toolchains: Vec<(&str, bool, PathBuf)> = list
        .lines()
        .filter_map(|line| {
            let (name, mut rest) = line.trim().split_once(char::is_whitespace)?;
            let mut default = false;
            // The markers before the path, which may hold spaces.
            while let Some(after) = rest.trim_start().strip_prefix('(') {
                let (marker, tail) = after.split_once(')')?;
                default |= marker.contains("default");
                rest = tail;
            }
            Some((name, default, Path::new(rest.trim()).join(&rustc)))
        })
        .filter(|(_, _, rustc)| exists(rustc))
        .collect();
    toolchains
        .iter()
        .find(|(name, _, _)| build.is_some_and(|b| name.starts_with(b) || b.starts_with(name)))
        .or_else(|| toolchains.iter().find(|(_, default, _)| *default))
        .or_else(|| toolchains.first())
        .map(|(_, _, rustc)| rustc.clone())
}

/// The `rustc` every test library is built with, found once.
///
/// # Panics
///
/// When no installed toolchain has one.
#[must_use]
pub fn rustc() -> &'static Path {
    match RUSTC.get_or_init(resolve) {
        Ok(path) => path,
        Err(why) => panic!("no installed Rust compiler builds the test library: {why}"),
    }
}

/// Builds the one-file crate `source` into the `cdylib` `out`.
///
/// # Panics
///
/// When the compiler is missing, fails, or runs over its time limit.
pub fn build(source: &Path, out: &Path) {
    let rustc = rustc();
    let _one = BUILDS.lock().unwrap_or_else(PoisonError::into_inner);
    // cost: one compiler process for the test library's one source file.
    let mut command = Command::new(rustc);
    command
        .args(["--crate-type", "cdylib", "--edition", "2024", "-O", "-o"])
        .arg(out)
        .arg(source);
    if let Err(why) = output(command, "build") {
        panic!(
            "{} could not build the test library {}: {why}",
            rustc.display(),
            source.display()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CI's test job: the build's toolchain is not installed, so the
    /// default one is used, and nothing is installed.
    #[test]
    fn a_missing_build_toolchain_falls_back_to_the_default() {
        let list = "stable-x86_64-unknown-linux-gnu (active, default) /r/toolchains/stable\n\
                    1.97.1-x86_64-unknown-linux-gnu /r/toolchains/1.97.1\n";
        let rustc = |t: &str| {
            Path::new("/r/toolchains")
                .join(t)
                .join("bin")
                .join(exe("rustc"))
        };
        let only_stable = |p: &Path| p.starts_with("/r/toolchains/stable");
        assert_eq!(
            pick(list, Some("1.97.1-x86_64-unknown-linux-gnu"), only_stable),
            Some(rustc("stable"))
        );
        assert_eq!(
            pick(list, Some("1.97.1-x86_64-unknown-linux-gnu"), |_| true),
            Some(rustc("1.97.1")),
            "the build's own, when installed"
        );
        assert_eq!(pick(list, None, |_| false), None);
    }

    /// Locally the compiler is a toolchain's own, not a proxy.
    #[test]
    fn the_compiler_is_a_toolchains_own() {
        let path = rustc();
        assert!(path.is_file(), "{}", path.display());
        assert!(path.parent().is_some_and(|bin| bin.ends_with("bin")));
    }
}
