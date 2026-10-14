use wasmtime::ValType;

use super::Stop;
use crate::fs::{READ, WRITE};
use crate::net::{CLOSE, METHODS};
use crate::profiles::{self, RuntimeProfile};

/// The imports that run on wasmtime.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Kind {
    Test(TestMethod),
    Fixture {
        method: profiles::Method,
        finish: bool,
    },
    Fs {
        key: &'static str,
        method: &'static str,
        finish: bool,
    },
    ProcessStart,
    ProcessFinish,
    HttpStart,
    Net {
        method: &'static str,
        input: Option<usize>,
        close: bool,
    },
    NetFinish,
    NeverFinish,
    RandomNext,
    RandomFill,
    Sys(&'static str),
    Block,
    Abort,
    Stderr,
    Stdout,
    FormatF64,
    FormatF32,
    FormatFixed,
    Lower,
    Upper,
    ParseF64,
    WriteLineStart,
    WriteLineFinish,
    ReadLineStart,
    ReadLineFinish,
    Now,
    Monotonic,
    SleepStart,
    SleepFinish,
    Program,
    ArgsList,
    EnvGet,
    EnvNames,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum TestMethod {
    Row,
    Timeout,
    Snapshot,
    TempDir,
    Start,
    Record,
    Show,
}

/// Which journal row a call of an import writes (live-execution.md §4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Record {
    /// None: the `hd:prim` functions answer from their arguments alone, so
    /// a replay computes them again.
    Skip,
    Call,
    Finish,
    Block,
    Abort,
}

impl Kind {
    pub(crate) fn record(self) -> Record {
        match self {
            Self::FormatF64
            | Self::FormatF32
            | Self::FormatFixed
            | Self::Lower
            | Self::Upper
            | Self::ParseF64 => Record::Skip,
            Self::Block => Record::Block,
            Self::Abort => Record::Abort,
            Self::Fixture { finish: true, .. }
            | Self::Fs { finish: true, .. }
            | Self::ProcessFinish
            | Self::NetFinish
            | Self::NeverFinish
            | Self::WriteLineFinish
            | Self::ReadLineFinish
            | Self::SleepFinish => Record::Finish,
            _ => Record::Call,
        }
    }

    /// The parameter that holds the length of the exchange buffer bytes a
    /// call reads as its argument (`call.rs` reads them), if it reads any.
    /// With the scalar parameters they are the call's request, which a
    /// journal records and a replay compares.
    pub(crate) fn input(self) -> Option<usize> {
        match self {
            Self::Test(TestMethod::Snapshot | TestMethod::Show)
            | Self::Fs { finish: false, .. }
            | Self::ProcessStart
            | Self::HttpStart
            | Self::Stderr
            | Self::Stdout
            | Self::Lower
            | Self::Upper
            | Self::ParseF64
            | Self::WriteLineStart
            | Self::EnvGet => Some(0),
            Self::Net { input, .. } => input,
            _ => None,
        }
    }

    /// Reject malformed result shapes before executing any provider effects.
    /// Integer width compatibility is retained; floats/references are never
    /// silently filled with integer values.
    pub(crate) fn validate_results(self, results: &[ValType]) -> wasmtime::Result<()> {
        let integer = |ty: &ValType| matches!(ty, ValType::I32 | ValType::I64);
        let valid = match self {
            Self::Test(TestMethod::Timeout | TestMethod::Record | TestMethod::Show)
            | Self::Abort
            | Self::Stderr
            | Self::Stdout => results.is_empty(),
            Self::ParseF64 => matches!(results, [ValType::F64]),
            Self::Fs { finish: false, .. }
            | Self::ProcessStart
            | Self::HttpStart
            | Self::Net { close: false, .. }
            | Self::WriteLineStart
            | Self::ReadLineStart
            | Self::SleepStart => match results {
                [status, value] => integer(status) && integer(value),
                _ => false,
            },
            Self::Fixture { method, finish } if !finish && method.is_waiting() => match results {
                [status, value] => integer(status) && integer(value),
                _ => false,
            },
            _ => match results {
                [value] => integer(value),
                _ => false,
            },
        };
        if valid {
            Ok(())
        } else {
            Err(Stop::Contract(format!("the import {self:?} has an unexpected signature")).into())
        }
    }
    pub(crate) fn with_test(
        module: &str,
        name: &str,
        kind: crate::test_engine::TestHostKind,
    ) -> Option<Self> {
        let test = match (module, name) {
            ("hd:TestRunner", "row") => Some(TestMethod::Row),
            ("hd:TestRunner", "report_timeout") => Some(TestMethod::Timeout),
            ("hd:TestRunner", "snapshot_check") => Some(TestMethod::Snapshot),
            ("hd:TestRunner", "temp_dir") => Some(TestMethod::TempDir),
            ("hd:PropertyRunner", "start") => Some(TestMethod::Start),
            ("hd:PropertyRunner", "record") => Some(TestMethod::Record),
            ("hd:PropertyRunner", "show") => Some(TestMethod::Show),
            _ => None,
        };
        if let Some(method) = test {
            return Some(Self::Test(method));
        }
        if kind == crate::test_engine::TestHostKind::Unit && !matches!(module, "hd:rt" | "hd:prim")
        {
            return None;
        }
        Self::with_profile(module, name, kind.profile())
    }
    pub(crate) fn with_profile(module: &str, name: &str, profile: RuntimeProfile) -> Option<Self> {
        Self::of(module, name)
            .or_else(|| {
                profile
                    .waiting(module, name)
                    .map(|(method, finish)| Self::Fixture { method, finish })
            })
            .or_else(|| {
                profile.method(module, name).map(|method| Self::Fixture {
                    method,
                    finish: false,
                })
            })
    }

    fn of(module: &str, name: &str) -> Option<Kind> {
        if module == "hd:Net" {
            for &method in CLOSE {
                if name == method {
                    return Some(Kind::Net {
                        method,
                        input: None,
                        close: true,
                    });
                }
            }
            let (method, suffix) = name.rsplit_once('.')?;
            let method = *METHODS.iter().find(|m| **m == method)?;
            let input = match method {
                "lookup" => Some(0),
                "connect" | "listen" | "bind_udp" | "TcpStream.write" => Some(1),
                "UdpSocket.send_to" => Some(2),
                _ => None,
            };
            return match suffix {
                "start" => Some(Kind::Net {
                    method,
                    input,
                    close: false,
                }),
                "finish" => Some(Kind::NetFinish),
                _ => None,
            };
        }
        if let Some((key, methods)) = match module {
            "hd:FsRead" => Some(("FsRead", READ)),
            "hd:FsWrite" => Some(("FsWrite", WRITE)),
            _ => None,
        } {
            let (method, suffix) = name.rsplit_once('.')?;
            let method = *methods.iter().find(|m| **m == method)?;
            return match suffix {
                "start" => Some(Kind::Fs {
                    key,
                    method,
                    finish: false,
                }),
                "finish" => Some(Kind::Fs {
                    key,
                    method,
                    finish: true,
                }),
                _ => None,
            };
        }
        Some(match (module, name) {
            ("hd:Http", "send.start") => Kind::HttpStart,
            ("hd:Http", "send.finish") => Kind::NeverFinish,
            ("hd:Process", "run.start") => Kind::ProcessStart,
            ("hd:Process", "run.finish") => Kind::ProcessFinish,
            ("hd:Random", "next_u64") => Kind::RandomNext,
            ("hd:Random", "fill") => Kind::RandomFill,
            ("hd:Sys", "os") => Kind::Sys("os"),
            ("hd:Sys", "arch") => Kind::Sys("arch"),
            ("hd:Sys", "hostname") => Kind::Sys("hostname"),
            ("hd:Sys", "cpu_count") => Kind::Sys("cpu_count"),
            ("hd:rt", "block") => Kind::Block,
            ("hd:rt", "abort") => Kind::Abort,
            ("hd:rt", "stderr") => Kind::Stderr,
            ("hd:rt", "stdout") => Kind::Stdout,
            ("hd:prim", "format_f64") => Kind::FormatF64,
            ("hd:prim", "format_f32") => Kind::FormatF32,
            ("hd:prim", "format_f64_fixed") => Kind::FormatFixed,
            ("hd:prim", "string_lower") => Kind::Lower,
            ("hd:prim", "string_upper") => Kind::Upper,
            ("hd:prim", "parse_f64") => Kind::ParseF64,
            ("hd:Console", "write_line.start") => Kind::WriteLineStart,
            ("hd:Console", "write_line.finish") => Kind::WriteLineFinish,
            ("hd:ConsoleInput", "read_line.start") => Kind::ReadLineStart,
            ("hd:ConsoleInput", "read_line.finish") => Kind::ReadLineFinish,
            ("hd:Clock", "now") => Kind::Now,
            ("hd:Clock", "monotonic") => Kind::Monotonic,
            ("hd:Clock", "sleep.start") => Kind::SleepStart,
            ("hd:Clock", "sleep.finish") => Kind::SleepFinish,
            ("hd:Args", "program") => Kind::Program,
            ("hd:Args", "list") => Kind::ArgsList,
            ("hd:Env", "get") => Kind::EnvGet,
            ("hd:Env", "names") => Kind::EnvNames,
            _ => return None,
        })
    }
}

/// Whether an import runs on wasmtime.
#[must_use]
pub fn supported(module: &str, name: &str, profile: RuntimeProfile) -> bool {
    Kind::with_profile(module, name, profile).is_some()
}
