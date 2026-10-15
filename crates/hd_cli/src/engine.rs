//! Which engine runs a program: V8 through Node (`node::NodeEngine`, the
//! default) or wasmtime in-process (`hd_run_wasmtime`), chosen by
//! `--engine NAME` or else `HD_ENGINE`. wasmtime runs the default
//! profile's `Console`, `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`,
//! `FsWrite`, `Process`, `Random`, `Sys`, `Http`, `Net` and their handle traits; a
//! program that imports another host trait is refused with a message that
//! names it (`docs/compiler/engines-and-test-runner.md` §18, W2/W3). Tests use shared
//! prepared programs and typed named children on Wasmtime, while Node
//! keeps its existing subprocess protocol.
//!
//! The wasmtime engine is the `wasmtime` cargo feature, off by default.
//! `wasmtime.rs` has it; a build without the feature compiles
//! `no_wasmtime.rs` in its place, and naming the engine is an error.

use std::ffi::OsString;

use hd_driver::{EngineTarget, OptLevel};
use hd_run::{HostSetup, Limits, Outcome, run_program};

pub(crate) use crate::node::{CaseRun, HostCase, Snapshot};

#[cfg_attr(not(feature = "wasmtime"), path = "engine/no_wasmtime.rs")]
mod wasmtime;

/// Named test children stay typed on the native engine; only Node needs
/// its private scratch-file configuration.
pub(crate) enum TestPrograms {
    Node(crate::node::Programs),
    Wasmtime(wasmtime::Programs),
}

impl TestPrograms {
    pub(crate) fn new(
        kind: EngineKind,
        list: Vec<(String, Vec<u8>, hd_run::Grants)>,
        cwd: &std::path::Path,
    ) -> Result<Self, String> {
        match kind {
            EngineKind::Node => crate::node::Programs::new(&list, cwd).map(Self::Node),
            EngineKind::Wasmtime => wasmtime::Programs::try_from(list).map(Self::Wasmtime),
        }
    }
}

pub(crate) struct TestEnv<'a> {
    pub(crate) cwd: Option<&'a std::path::Path>,
    pub(crate) program: &'a str,
    pub(crate) programs: Option<&'a TestPrograms>,
}

pub(crate) enum PreparedCases<'a> {
    Node(&'a [u8]),
    Wasmtime(wasmtime::Prepared),
}

/// One engine and compiled child-program list for a package's test run.
/// Every test module is prepared before any guest starts.
pub(crate) enum TestSession {
    Node,
    Wasmtime(wasmtime::Session),
}

impl TestSession {
    pub(crate) fn new(
        kind: EngineKind,
        level: OptLevel,
        jobs: usize,
        limits: ProgramLimits,
        programs: Option<&TestPrograms>,
    ) -> Result<Self, String> {
        match kind {
            EngineKind::Node => Ok(Self::Node),
            EngineKind::Wasmtime => {
                wasmtime::Session::new(level, jobs, limits, programs).map(Self::Wasmtime)
            }
        }
    }

    pub(crate) fn prepare<'a>(
        &self,
        wasm: &'a [u8],
        integration: bool,
    ) -> Result<PreparedCases<'a>, String> {
        match self {
            Self::Node => Ok(PreparedCases::Node(wasm)),
            Self::Wasmtime(session) => session
                .prepare(wasm, integration)
                .map(PreparedCases::Wasmtime),
        }
    }

    pub(crate) fn run(
        &self,
        prepared: &PreparedCases<'_>,
        env: &TestEnv<'_>,
        cases: &[HostCase],
        limits: ProgramLimits,
        done: &mut dyn FnMut(CaseRun),
    ) -> Result<(), String> {
        match (self, prepared) {
            (Self::Node, PreparedCases::Node(wasm)) => {
                let programs = match env.programs {
                    Some(TestPrograms::Node(programs)) => Some(programs),
                    None => None,
                    Some(TestPrograms::Wasmtime(_)) => {
                        return Err("internal: Wasmtime child programs on Node".into());
                    }
                };
                let env = crate::node::TestEnv {
                    cwd: env.cwd,
                    program: env.program,
                    programs,
                    wall_ms: limits.wall_ms,
                };
                crate::node::run_cases(wasm, &env, cases, done)
            }
            (Self::Wasmtime(session), PreparedCases::Wasmtime(prepared)) => {
                session.run(prepared, env, cases, limits, done)
            }
            _ => Err("internal: prepared tests belong to another engine".into()),
        }
    }
}

/// Explicit program limits; neither has a default (`cli.limit.none`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct ProgramLimits {
    pub heap_bytes: Option<u64>,
    pub wall_ms: Option<u64>,
}

impl From<ProgramLimits> for Limits {
    fn from(limits: ProgramLimits) -> Self {
        Self {
            memory_bytes: limits.heap_bytes.unwrap_or(u64::MAX),
            fuel: None,
            wall_ms: limits.wall_ms,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LimitFlag {
    Heap(u64),
    Time(u64),
    CompilerMemory(u64),
}

fn scaled(value: &str, suffixes: &[(&str, u64)]) -> Option<u64> {
    let end = value
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(value.len());
    let (digits, suffix) = value.split_at(end);
    let number = digits.parse::<u64>().ok()?;
    let factor = suffixes.iter().find(|(s, _)| *s == suffix)?.1;
    number.checked_mul(factor)
}

/// Whole bytes, or a whole number with a binary K/M/G suffix.
pub(crate) fn parse_size(value: &str) -> Result<u64, String> {
    scaled(
        value,
        &[
            ("", 1),
            ("K", 1024),
            ("M", 1024 * 1024),
            ("G", 1024 * 1024 * 1024),
        ],
    )
    .ok_or_else(|| {
        format!("`{value}` is not a SIZE; use whole bytes or a whole number followed by K, M or G")
    })
}

/// Whole milliseconds, seconds, minutes or hours (`cli.limit.forms`).
pub(crate) fn parse_duration(value: &str) -> Result<u64, String> {
    scaled(
        value,
        &[("ms", 1), ("s", 1000), ("min", 60_000), ("h", 3_600_000)],
    )
    .ok_or_else(|| {
        format!("`{value}` is not a DURATION; use a whole number followed by ms, s, min or h")
    })
}

/// A resource-limit flag at `args[i]`, and the number of words it used.
pub(crate) fn limit_flag(
    args: &[OsString],
    i: usize,
) -> Option<Result<(LimitFlag, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    for flag in ["--max-heap", "--time-limit", "--max-memory"] {
        let (value, used) = if text == flag {
            let Some(value) = args.get(i + 1) else {
                return Some(Err(format!("`{flag}` needs a value")));
            };
            (value.to_string_lossy().into_owned(), 2)
        } else if let Some(value) = text.strip_prefix(&format!("{flag}=")) {
            (value.to_owned(), 1)
        } else {
            continue;
        };
        let result = match flag {
            "--max-heap" => parse_size(&value).map(LimitFlag::Heap),
            "--time-limit" => parse_duration(&value).map(LimitFlag::Time),
            _ => parse_size(&value).map(LimitFlag::CompilerMemory),
        };
        return Some(
            result
                .map(|limit| (limit, used))
                .map_err(|e| format!("{flag}: {e}")),
        );
    }
    None
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum EngineKind {
    #[default]
    Node,
    Wasmtime,
}

impl EngineKind {
    /// The stack a test worker thread needs, when the engine asks for one.
    pub(crate) fn worker_stack(self, level: OptLevel) -> Option<usize> {
        match self {
            Self::Node => None,
            Self::Wasmtime => Some(wasmtime::worker_stack(level)),
        }
    }

    /// The target a build that this engine runs is tuned for
    /// (`crates/hd_opt/DESIGN.md` (optimization pipelines) §5).
    pub(crate) const fn target(self) -> EngineTarget {
        match self {
            Self::Node => EngineTarget::V8,
            Self::Wasmtime => EngineTarget::Wasmtime,
        }
    }
}

fn parse(name: &str) -> Result<EngineKind, String> {
    match name {
        "node" => Ok(EngineKind::Node),
        "wasmtime" => match wasmtime::UNAVAILABLE {
            None => Ok(EngineKind::Wasmtime),
            Some(why) => Err(why.to_owned()),
        },
        _ => Err(format!(
            "`{name}` is no engine; the engines are `node` and `wasmtime`"
        )),
    }
}

/// `--engine NAME` or `--engine=NAME` at `args[i]`, and the words it used.
pub(crate) fn engine_flag(
    args: &[OsString],
    i: usize,
) -> Option<Result<(EngineKind, usize), String>> {
    let text = args.get(i)?.to_string_lossy();
    let (name, used) = if text == "--engine" {
        match args.get(i + 1) {
            Some(v) => (v.to_string_lossy().into_owned(), 2),
            None => return Some(Err("`--engine` needs NAME".to_owned())),
        }
    } else {
        (text.strip_prefix("--engine=")?.to_owned(), 1)
    };
    Some(parse(&name).map(|k| (k, used)))
}

/// The engine of a run with no `--engine` flag: `HD_ENGINE`, else Node.
pub(crate) fn from_env() -> Result<EngineKind, String> {
    match std::env::var("HD_ENGINE") {
        Ok(name) if !name.is_empty() => parse(&name).map_err(|e| format!("HD_ENGINE: {e}")),
        _ => Ok(EngineKind::Node),
    }
}

/// Runs a built program to its end on `kind`, with the process's standard
/// streams, with the Wasm frames of the trap it ended in, innermost first
/// (empty when it ended any other way). `level` selects wasmtime's engine
/// options.
pub(crate) fn run(
    kind: EngineKind,
    level: OptLevel,
    wasm: &[u8],
    host: &HostSetup,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    match kind {
        EngineKind::Node => {
            let store = crate::disk_store();
            let _guest = crate::memory_cap::enter_guest();
            let node = crate::node::NodeEngine::default();
            let outcome = run_program(&node, wasm, &store, host).map_err(|e| e.to_string())?;
            let frames = node.trace.lock().map(|t| t.clone()).unwrap_or_default();
            Ok((outcome, frames))
        }
        EngineKind::Wasmtime => wasmtime::run_program(level, wasm, host),
    }
}

#[cfg(test)]
mod tests {
    use super::{EngineKind, LimitFlag, ProgramLimits, limit_flag, parse_duration, parse_size};
    use hd_run::Limits;
    use std::ffi::OsString;

    #[cfg(feature = "wasmtime")]
    #[test]
    fn native_named_children_retain_modules_and_grants_without_node_configuration() {
        use super::TestPrograms;
        let grants = hd_run::Grants {
            limits: vec![("Env".into(), hd_run::Limit::Deny)],
        };
        let list = vec![("tool".to_owned(), vec![0, 97, 115, 109], grants.clone())];
        let programs = TestPrograms::new(
            EngineKind::Wasmtime,
            list,
            std::path::Path::new("unused-native-cwd"),
        )
        .unwrap();
        let TestPrograms::Wasmtime(super::wasmtime::Programs(list)) = programs else {
            panic!("native children must not use Node's scratch configuration");
        };
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "tool");
        assert_eq!(list[0].1, [0, 97, 115, 109]);
        assert_eq!(list[0].2.limits, grants.limits);
    }

    #[cfg(not(feature = "wasmtime"))]
    #[test]
    fn wasmtime_is_refused_without_the_feature() {
        use super::parse;
        let error = parse("wasmtime").unwrap_err();
        assert!(error.contains("no wasmtime engine"), "{error}");
        assert!(error.contains("--features wasmtime"), "{error}");
        assert_eq!(parse("node"), Ok(EngineKind::Node));
    }

    #[test]
    fn sizes_use_exact_binary_units_and_allow_zero() {
        for (input, expected) in [
            ("0", 0),
            ("0G", 0),
            ("0001K", 1024),
            ("65536", 65_536),
            ("256M", 256 * 1024 * 1024),
            ("2G", 2 * 1024 * 1024 * 1024),
            ("18446744073709551615", u64::MAX),
            ("18014398509481983K", u64::MAX - 1023),
        ] {
            assert_eq!(parse_size(input), Ok(expected), "{input}");
        }
    }

    #[test]
    fn sizes_reject_non_forms_and_overflow() {
        for input in [
            "",
            "K",
            "+1",
            "-1",
            " 1",
            "1 ",
            "1 K",
            "1.0",
            "1k",
            "1m",
            "1g",
            "1KB",
            "1KiB",
            "1T",
            "1ms",
            "１",
            "١",
            "18446744073709551616",
            "18014398509481984K",
            "17592186044416M",
            "17179869184G",
        ] {
            assert!(parse_size(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn durations_use_exact_units_and_allow_zero() {
        for (input, expected) in [
            ("0ms", 0),
            ("0h", 0),
            ("0001s", 1000),
            ("500ms", 500),
            ("30s", 30_000),
            ("2min", 120_000),
            ("1h", 3_600_000),
            ("18446744073709551615ms", u64::MAX),
            ("18446744073709551s", 18_446_744_073_709_551_000),
        ] {
            assert_eq!(parse_duration(input), Ok(expected), "{input}");
        }
    }

    #[test]
    fn durations_reject_non_forms_and_overflow() {
        for input in [
            "",
            "0",
            "1",
            "ms",
            "+1s",
            "-1s",
            " 1s",
            "1s ",
            "1 s",
            "1.0s",
            "1MS",
            "1S",
            "1m",
            "1d",
            "1mins",
            "1sec",
            "1K",
            "１s",
            "18446744073709551616ms",
            "18446744073709552s",
            "307445734561826min",
            "5124095576031h",
        ] {
            assert!(parse_duration(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn limit_flags_accept_both_value_forms_and_return_words_used() {
        for (flag, value, expected) in [
            ("--max-heap", "2G", LimitFlag::Heap(2 * 1024 * 1024 * 1024)),
            ("--time-limit", "2min", LimitFlag::Time(120_000)),
            (
                "--max-memory",
                "4M",
                LimitFlag::CompilerMemory(4 * 1024 * 1024),
            ),
        ] {
            let separate = [
                OsString::from("other"),
                OsString::from(flag),
                OsString::from(value),
            ];
            assert_eq!(limit_flag(&separate, 1), Some(Ok((expected, 2))));
            let joined = [OsString::from(format!("{flag}={value}"))];
            assert_eq!(limit_flag(&joined, 0), Some(Ok((expected, 1))));
        }
    }

    #[test]
    fn limit_flags_report_missing_and_invalid_values() {
        for flag in ["--max-heap", "--time-limit", "--max-memory"] {
            let args = [OsString::from(flag)];
            assert_eq!(
                limit_flag(&args, 0),
                Some(Err(format!("`{flag}` needs a value")))
            );
            for value in ["", "+1", "1.5G", "18446744073709551616"] {
                let joined = [OsString::from(format!("{flag}={value}"))];
                let error = limit_flag(&joined, 0)
                    .expect("recognized flag")
                    .expect_err("invalid value");
                assert!(error.starts_with(&format!("{flag}: ")), "{error}");
                let separate = [OsString::from(flag), OsString::from(value)];
                assert_eq!(limit_flag(&separate, 0), Some(Err(error)));
            }
        }
    }

    #[test]
    fn limit_flags_leave_other_arguments_to_the_caller() {
        for arg in [
            "--",
            "--engine=wasmtime",
            "main.hd",
            "--max-heap-extra=1K",
            "--time-limits=1s",
        ] {
            assert_eq!(limit_flag(&[OsString::from(arg)], 0), None, "{arg}");
        }
        assert_eq!(limit_flag(&[], 0), None);
        assert_eq!(limit_flag(&[OsString::from("--max-heap=0")], 1), None);
    }

    #[test]
    fn program_limits_preserve_omission_zero_and_explicit_values() {
        assert_eq!(
            Limits::from(ProgramLimits::default()),
            Limits {
                memory_bytes: u64::MAX,
                fuel: None,
                wall_ms: None,
            }
        );
        for (heap_bytes, wall_ms) in [
            (Some(0), Some(0)),
            (Some(1024), Some(500)),
            (None, Some(0)),
            (Some(0), None),
        ] {
            assert_eq!(
                Limits::from(ProgramLimits {
                    heap_bytes,
                    wall_ms
                }),
                Limits {
                    memory_bytes: heap_bytes.unwrap_or(u64::MAX),
                    fuel: None,
                    wall_ms,
                }
            );
        }
    }
}
