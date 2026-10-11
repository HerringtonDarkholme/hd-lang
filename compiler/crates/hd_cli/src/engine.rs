//! Which engine runs a program: V8 through Node (`node::NodeEngine`, the
//! default) or wasmtime in-process (`hd_run_wasmtime`), chosen by
//! `--engine NAME` or else `HD_ENGINE`. wasmtime runs the default
//! profile's `Console`, `ConsoleInput`, `Clock`, `Args`, `Env`, `FsRead`,
//! `FsWrite`, `Process`, `Random`, `Sys`, `Http`, `Net` and their handle traits; a
//! program that imports another host trait is refused with a message that
//! names it (engines-and-test-runner.md §18, W2/W3).

use std::ffi::OsString;

use hd_cache::DiskStore;
use hd_run::{HostSetup, Limits, Outcome, run_program};

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

fn parse(name: &str) -> Result<EngineKind, String> {
    match name {
        "node" => Ok(EngineKind::Node),
        "wasmtime" => Ok(EngineKind::Wasmtime),
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
/// streams. `release` selects wasmtime's optimized pipeline.
pub(crate) fn run(
    kind: EngineKind,
    release: bool,
    wasm: &[u8],
    host: &HostSetup,
) -> Result<Outcome, String> {
    match kind {
        EngineKind::Node => {
            let store = DiskStore {
                root: crate::cache_dir(),
            };
            let _guest = crate::memory_cap::enter_guest();
            run_program(&crate::node::NodeEngine, wasm, &store, host).map_err(|e| e.to_string())
        }
        EngineKind::Wasmtime => hd_run_wasmtime::run_with_guards(
            hd_run_wasmtime::EngineConfig::pipeline(release),
            wasm,
            host,
            hd_run_wasmtime::HostIo::process(),
            || crate::memory_cap::stage("Precompile"),
            crate::memory_cap::enter_guest,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::{LimitFlag, ProgramLimits, limit_flag, parse_duration, parse_size};
    use hd_run::Limits;
    use std::ffi::OsString;

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
