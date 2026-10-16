use hd_host_abi::{Codec, Scalar, Wait};
use wasmtime::ValType;

use crate::abi::{HostMethod, METHODS};
use crate::profiles::{self, RuntimeProfile};

/// The imports that run on wasmtime.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Kind {
    /// A host method of the ABI model (`abi.rs`): its one import, or its
    /// `.start`, or its `.finish` when `finish`.
    Host {
        method: &'static HostMethod,
        finish: bool,
    },
    Fixture {
        method: profiles::Method,
        finish: bool,
    },
    Block,
    Abort,
    Stderr,
    Stdout,
    Panic,
    FormatF64,
    FormatF32,
    FormatFixed,
    Lower,
    Upper,
    ParseF64,
}

/// Which journal row a call of an import writes (`docs/future-work/live-execution.md` §4.2).
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
            Self::Fixture { finish: true, .. } | Self::Host { finish: true, .. } => Record::Finish,
            _ => Record::Call,
        }
    }

    /// The parameter that holds the length of the exchange buffer bytes a
    /// call reads as its argument (`call.rs` reads them), if it reads any.
    /// With the scalar parameters they are the call's request, which a
    /// journal records and a replay compares.
    pub(crate) fn input(self) -> Option<usize> {
        match self {
            // After its scalar parameters, when it takes a structured one.
            Self::Host {
                method,
                finish: false,
            } => {
                let scalars = method
                    .params
                    .iter()
                    .filter(|p| matches!(p, Codec::Scalar(_)))
                    .count();
                (scalars < method.params.len()).then_some(scalars)
            }
            Self::Stderr | Self::Stdout | Self::Lower | Self::Upper | Self::ParseF64 => Some(0),
            _ => None,
        }
    }

    /// Whether `results` are this import's result shape, checked before
    /// any provider effect. Integer width compatibility is retained;
    /// floats/references are never silently filled with integer values.
    pub(crate) fn takes_results(self, results: &[ValType]) -> bool {
        let integer = |ty: &ValType| matches!(ty, ValType::I32 | ValType::I64);
        let pair = || match results {
            [status, value] => integer(status) && integer(value),
            _ => false,
        };
        match self {
            Self::Host {
                method,
                finish: false,
            } => match (method.wait, method.result) {
                (Wait::May, _) => pair(),
                (Wait::Never, Codec::Void) => results.is_empty(),
                (Wait::Never, Codec::Scalar(Scalar::F64)) => matches!(results, [ValType::F64]),
                (Wait::Never, _) => matches!(results, [value] if integer(value)),
            },
            Self::Abort | Self::Stderr | Self::Stdout | Self::Panic => results.is_empty(),
            Self::ParseF64 => matches!(results, [ValType::F64]),
            Self::Fixture { method, finish } if !finish && method.is_waiting() => pair(),
            _ => matches!(results, [value] if integer(value)),
        }
    }

    /// A host method of the ABI model by its import's module and name: one
    /// that never waits by its own name, one that may by `.start` and
    /// `.finish`.
    fn host(module: &str, name: &str) -> Option<Kind> {
        METHODS.iter().find_map(|method| {
            if method.module != module {
                return None;
            }
            let finish = match method.wait {
                Wait::Never => (name == method.name).then_some(false)?,
                Wait::May => match name.rsplit_once('.')? {
                    (base, "start") if base == method.name => false,
                    (base, "finish") if base == method.name => true,
                    _ => return None,
                },
            };
            Some(Kind::Host { method, finish })
        })
    }

    /// Whether `module` is a test runner's (`hd_host_abi::RUNNERS`), which
    /// only a test binds.
    fn runner(module: &str) -> bool {
        hd_host_abi::RUNNERS
            .iter()
            .any(|t| module.strip_prefix("hd:") == Some(t.key))
    }

    pub(crate) fn with_test(
        module: &str,
        name: &str,
        kind: crate::test_engine::TestHostKind,
    ) -> Option<Self> {
        if Self::runner(module) {
            return Self::host(module, name);
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
        if Self::runner(module) {
            return None;
        }
        if let Some(kind) = Self::host(module, name) {
            return Some(kind);
        }
        Some(match (module, name) {
            ("hd:rt", "block") => Kind::Block,
            ("hd:rt", "abort") => Kind::Abort,
            ("hd:rt", "stderr") => Kind::Stderr,
            ("hd:rt", "stdout") => Kind::Stdout,
            ("hd:rt", "panic") => Kind::Panic,
            ("hd:prim", "format_f64") => Kind::FormatF64,
            ("hd:prim", "format_f32") => Kind::FormatF32,
            ("hd:prim", "format_f64_fixed") => Kind::FormatFixed,
            ("hd:prim", "string_lower") => Kind::Lower,
            ("hd:prim", "string_upper") => Kind::Upper,
            ("hd:prim", "parse_f64") => Kind::ParseF64,
            _ => return None,
        })
    }
}

/// Whether an import runs on wasmtime.
#[must_use]
pub fn supported(module: &str, name: &str, profile: RuntimeProfile) -> bool {
    Kind::with_profile(module, name, profile).is_some()
}
