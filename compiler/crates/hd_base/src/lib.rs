#![deny(unsafe_code)]
//! `hd_base`: the ID newtype macro, `Hash128` and `StableHasher`, `Span`,
//! `Fuel`, `AppendVec`, the little-endian entry codec and the structured
//! "not implemented" error that every skeleton stage returns
//! (design-overview.md §2.1, data-structures.md §3.1, §3.2, §3.9.4).
//! `append` is the workspace's one audited `unsafe` module.

pub mod append;
pub mod stable;
pub mod unsupported;
pub mod wire;

pub use append::{AppendVec, Col, Range32};
pub use stable::{StableHash, StableHasher};
pub use unsupported::{NotImplemented, Stage, StageResult};

macro_rules! id {
    ($name:ident) => {
        #[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
        #[repr(transparent)]
        pub struct $name(u32);

        impl $name {
            pub const NONE: Self = Self(u32::MAX);

            #[must_use]
            pub const fn from_raw(raw: u32) -> Self {
                Self(raw)
            }

            #[must_use]
            pub const fn raw(self) -> u32 {
                self.0
            }

            #[must_use]
            pub const fn get(self) -> Option<Self> {
                if self.0 == u32::MAX { None } else { Some(self) }
            }

            #[must_use]
            pub fn idx(self) -> usize {
                debug_assert!(self.0 != u32::MAX);
                self.0 as usize
            }
        }

        const _: () = assert!(core::mem::size_of::<$name>() == 4);
    };
}

id!(Symbol);
id!(FileId);
id!(TokenIdx);
id!(NodeIdx);
id!(ItemIdx);
id!(PackageId);
id!(ModuleId);
id!(FolderId);
id!(PathId);
id!(DefId);
id!(InstId);
id!(ProgramId);
id!(InferVar);
id!(LocalId);
id!(SubId);
id!(CaptureId);
id!(LabelId);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub file: FileId,
    pub lo: u32,
    pub hi: u32,
}

const _: () = assert!(core::mem::size_of::<Span>() == 12);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Hash128(pub u128);

/// The content hash of a byte string: xxh3-128 (data-structures.md §3.2).
#[must_use]
pub fn hash128(bytes: &[u8]) -> Hash128 {
    Hash128(xxhash_rust::xxh3::xxh3_128(bytes))
}

/// Per-body (or per-goal) work budget (checking-and-tir.md §4.15,
/// trait-solver.md §7.4). Fuel counts steps, never time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fuel {
    left: u64,
    spent: u64,
}

impl Fuel {
    /// Default per-body budget (§4.15).
    pub const BODY_DEFAULT: u64 = 1 << 24;

    #[must_use]
    pub const fn new(steps: u64) -> Self {
        Self {
            left: steps,
            spent: 0,
        }
    }
    /// Charges `steps`; false once the budget is gone.
    pub fn charge(&mut self, steps: u64) -> bool {
        self.spent = self.spent.saturating_add(steps);
        if steps > self.left {
            self.left = 0;
            false
        } else {
            self.left -= steps;
            true
        }
    }
    #[must_use]
    pub const fn left(self) -> u64 {
        self.left
    }
    #[must_use]
    pub const fn spent(self) -> u64 {
        self.spent
    }
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.left == 0
    }
}

#[cfg(test)]
mod tests {
    use super::{DefId, Fuel};

    #[test]
    fn sentinel_is_absent() {
        assert_eq!(DefId::NONE.get(), None);
        assert_eq!(DefId::from_raw(3).get(), Some(DefId::from_raw(3)));
    }

    #[test]
    fn fuel_runs_out() {
        let mut f = Fuel::new(5);
        assert!(f.charge(3));
        assert!(!f.charge(3));
        assert!(f.is_empty());
        assert_eq!(f.spent(), 6);
    }
}
