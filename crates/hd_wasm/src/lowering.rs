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
    /// One closure base per code signature, whose code field has the
    /// concrete code type; a call reads it and calls it with `call_ref`.
    /// V8 runs this form fastest (closure-calls.md §7).
    Typed,
    /// One closure base for every signature, with an abstract `funcref`
    /// code field; a call casts the code before `call_ref`. Wasmtime reads
    /// an abstract field without a type lookup and subtype check
    /// (closure-calls.md §7).
    Abstract,
}

impl ClosureCode {
    /// The choice's name in the lowering hash.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Typed => "typed",
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
    /// The profile of `target`. `Browser` follows `V8` until another
    /// browser engine is measured (opt-pipeline.md §7 Q2).
    #[must_use]
    pub const fn of(target: EngineTarget) -> Self {
        match target {
            EngineTarget::V8 | EngineTarget::Browser => Self {
                closure_code: ClosureCode::Typed,
            },
            EngineTarget::Wasmtime => Self {
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
    use super::{ClosureCode, Lowering};
    use hd_base::EngineTarget;

    #[test]
    fn closure_code_is_abstract_on_wasmtime_only() {
        let code = |t| Lowering::of(t).closure_code;
        assert_eq!(code(EngineTarget::V8), ClosureCode::Typed);
        assert_eq!(code(EngineTarget::Browser), ClosureCode::Typed);
        assert_eq!(code(EngineTarget::Wasmtime), ClosureCode::Abstract);
    }

    #[test]
    fn the_hash_follows_content() {
        let v8 = Lowering::of(EngineTarget::V8);
        assert_eq!(v8.hash(), Lowering::of(EngineTarget::Browser).hash());
        assert_ne!(v8.hash(), Lowering::of(EngineTarget::Wasmtime).hash());
    }
}
