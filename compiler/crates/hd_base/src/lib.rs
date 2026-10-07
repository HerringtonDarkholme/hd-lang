#![forbid(unsafe_code)]

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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Span {
    pub file: FileId,
    pub lo: u32,
    pub hi: u32,
}

const _: () = assert!(core::mem::size_of::<Span>() == 12);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Hash128(pub u128);

/// Small deterministic content hash used before the cache crate exists.
#[must_use]
pub fn hash128(bytes: &[u8]) -> Hash128 {
    let mut lo = 0xcbf2_9ce4_8422_2325_u64;
    let mut hi = 0x9e37_79b9_7f4a_7c15_u64;
    for &byte in bytes {
        lo ^= u64::from(byte);
        lo = lo.wrapping_mul(0x0000_0100_0000_01b3);
        hi ^= lo.rotate_left(17).wrapping_add(u64::from(byte));
        hi = hi.wrapping_mul(0x9ddf_ea08_eb38_2d69);
    }
    Hash128(u128::from(lo) | (u128::from(hi) << 64))
}
