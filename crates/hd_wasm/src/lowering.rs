//! The lowering profile of each engine target (opt-pipeline.md §4): the
//! lowering choices that depend on the engine. Every profile emits valid
//! Wasm GC for every engine, so a choice only tunes speed. The driver hashes
//! the profile into the pipeline key, by content and never by target name,
//! so targets with equal profiles share cache entries.

use hd_base::{EngineTarget, Hash128, StableHasher};

/// How a closure's code is stored and called (option E1 of
/// closure-calls.md).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClosureCode {
    /// One closure base for every signature, with an abstract `funcref`
    /// code field; a call casts the code before `call_ref`
    /// (`layout::closure_base`).
    Abstract,
}

impl ClosureCode {
    /// The choice's name in the lowering hash.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Abstract => "abstract",
        }
    }
}

/// The lowering choices of one engine target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Lowering {
    pub closure_code: ClosureCode,
}

impl Lowering {
    /// The profile of `target`. Every target lowers alike today; job (d)
    /// of opt-pipeline.md §6 gives `V8` and `Browser` typed closure code.
    #[must_use]
    pub const fn of(target: EngineTarget) -> Self {
        match target {
            EngineTarget::V8 | EngineTarget::Wasmtime | EngineTarget::Browser => Self {
                closure_code: ClosureCode::Abstract,
            },
        }
    }

    /// The hash of each field, by name and value (opt-pipeline.md §3.3).
    #[must_use]
    pub fn hash(&self) -> Hash128 {
        let mut h = StableHasher::new("lowering");
        h.str("closure_code");
        h.str(self.closure_code.name());
        h.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::Lowering;
    use hd_base::EngineTarget;

    #[test]
    fn every_target_lowers_alike_today() {
        let v8 = Lowering::of(EngineTarget::V8);
        for target in EngineTarget::ALL {
            assert_eq!(Lowering::of(target), v8, "{target:?}");
            assert_eq!(Lowering::of(target).hash(), v8.hash(), "{target:?}");
        }
    }
}
