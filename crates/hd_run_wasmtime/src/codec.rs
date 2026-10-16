//! The exchange-buffer encoding of `crates/hd_run_wasmtime/DESIGN.md` §17.4, as the
//! JS host's `Enc` writes it (`host/wire.mjs`): integers as
//! LEB128 (zigzag when signed), strings and byte lists as their length
//! then their bytes, lists as their count then their elements, an
//! optional as 0, or 1 and the value, an enum as its variant index then
//! its payload, data as its fields in order.
//!
//! `Wire` is a value's codec. This file holds the codecs of the language's
//! own types (`Result`, `T?`, lists, tuples, integers, strings, handles);
//! `abi.rs`, generated from the ABI model, holds every std type a host
//! method crosses with its codec.

#[derive(Default)]
pub struct Enc {
    buf: Vec<u8>,
}

impl Enc {
    pub fn zz(&mut self, v: i64) -> &mut Self {
        self.leb((v.unsigned_abs() << 1).wrapping_sub(u64::from(v < 0)))
    }
    pub fn leb(&mut self, mut v: u64) -> &mut Self {
        loop {
            let byte = u8::try_from(v & 0x7f).unwrap_or(0);
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                return self;
            }
            self.buf.push(byte | 0x80);
        }
    }

    pub fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.leb(b.len() as u64);
        self.buf.extend_from_slice(b);
        self
    }

    pub fn str(&mut self, s: &str) -> &mut Self {
        self.bytes(s.as_bytes())
    }

    /// Raw IEEE 754 bits, little-endian, including NaN payloads and signed zero.
    pub fn f64(&mut self, value: f64) -> &mut Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    #[cfg(test)]
    pub fn done(&self) -> &[u8] {
        &self.buf
    }

    /// Transfers the encoded reply without copying its payload.
    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }
}

/// A checked decoder over copied exchange-buffer arguments. Malformed
/// input reports the same host-contract text as Node's `Dec`.
pub struct Dec<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Dec<'a> {
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn leb(&mut self) -> wasmtime::Result<u64> {
        let mut value = 0_u64;
        for shift in (0..70).step_by(7) {
            let byte = *self.bytes.get(self.at).ok_or_else(decode_error)?;
            self.at += 1;
            if shift == 63 && byte > 1 {
                return Err(decode_error());
            }
            value |= u64::from(byte & 127) << shift;
            if byte & 128 == 0 {
                return Ok(value);
            }
        }
        Err(decode_error())
    }

    pub fn bytes(&mut self) -> wasmtime::Result<&'a [u8]> {
        let len = usize::try_from(self.leb()?).map_err(|_| decode_error())?;
        let end = self.at.checked_add(len).ok_or_else(decode_error)?;
        let bytes = self.bytes.get(self.at..end).ok_or_else(decode_error)?;
        self.at = end;
        Ok(bytes)
    }

    pub fn zz(&mut self) -> wasmtime::Result<i64> {
        let value = self.leb()?;
        Ok(i64::from_le_bytes((value >> 1).to_le_bytes()) ^ -i64::try_from(value & 1).unwrap_or(0))
    }

    pub fn str(&mut self) -> wasmtime::Result<String> {
        Ok(String::from_utf8_lossy(self.bytes()?).into_owned())
    }

    /// A tag below `count`: an option's, a result's or an enum's.
    pub fn tag(&mut self, count: u64) -> wasmtime::Result<u64> {
        let tag = self.leb()?;
        if tag < count {
            Ok(tag)
        } else {
            Err(decode_error())
        }
    }
}

/// How a value crosses the exchange buffer: `enc` writes it, `dec` reads
/// it back or fails with the `host-contract` panic.
pub trait Wire: Sized {
    fn enc(&self, e: &mut Enc);
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self>;
}

/// `value`, encoded.
pub fn encode<T: Wire>(value: &T) -> Enc {
    let mut e = Enc::default();
    value.enc(&mut e);
    e
}

/// A host handle: its handle number.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Handle(pub u64);

impl Wire for Handle {
    fn enc(&self, e: &mut Enc) {
        e.leb(self.0);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self(d.leb()?))
    }
}

/// `void`, as a result's `T`: no bytes.
impl Wire for () {
    fn enc(&self, _: &mut Enc) {}
    fn dec(_: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(())
    }
}

impl Wire for String {
    fn enc(&self, e: &mut Enc) {
        e.str(self);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        d.str()
    }
}

/// An integer narrower than 64 bits: LEB128 when unsigned, zigzag LEB128
/// when signed. `u8` has no codec of its own: `List[u8]` crosses as its
/// length and its bytes.
macro_rules! narrow {
    ($put:ident, $get:ident, $wide:ty: $($t:ty),*) => {$(
        impl Wire for $t {
            fn enc(&self, e: &mut Enc) {
                e.$put(<$wide>::from(*self));
            }
            fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
                Self::try_from(d.$get()?).map_err(|_| decode_error())
            }
        }
    )*};
}
narrow!(leb, leb, u64: u16, u32);
narrow!(zz, zz, i64: i8, i16, i32);

impl Wire for u64 {
    fn enc(&self, e: &mut Enc) {
        e.leb(*self);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        d.leb()
    }
}

impl Wire for usize {
    fn enc(&self, e: &mut Enc) {
        e.leb(*self as u64);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Self::try_from(d.leb()?).map_err(|_| decode_error())
    }
}

impl Wire for i64 {
    fn enc(&self, e: &mut Enc) {
        e.zz(*self);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        d.zz()
    }
}

/// `List[u8]`: its length, then its bytes.
impl Wire for Vec<u8> {
    fn enc(&self, e: &mut Enc) {
        e.bytes(self);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(d.bytes()?.to_vec())
    }
}

/// `List[T]`: its count, then its elements.
impl<T: Wire> Wire for Vec<T> {
    fn enc(&self, e: &mut Enc) {
        e.leb(self.len() as u64);
        for x in self {
            x.enc(e);
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        let count = d.leb()?;
        (0..count).map(|_| T::dec(d)).collect()
    }
}

/// `T?`: 0, or 1 and the value.
impl<T: Wire> Wire for Option<T> {
    fn enc(&self, e: &mut Enc) {
        match self {
            None => {
                e.leb(0);
            }
            Some(v) => {
                e.leb(1);
                v.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(2)? {
            0 => None,
            _ => Some(T::dec(d)?),
        })
    }
}

/// `Result[T, E]`: 0 and `.Ok`'s value, or 1 and `.Err`'s.
impl<T: Wire, E: Wire> Wire for Result<T, E> {
    fn enc(&self, e: &mut Enc) {
        match self {
            Ok(v) => {
                e.leb(0);
                v.enc(e);
            }
            Err(error) => {
                e.leb(1);
                error.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(2)? {
            0 => Ok(T::dec(d)?),
            _ => Err(E::dec(d)?),
        })
    }
}

/// A tuple: its elements in order.
impl<A: Wire, B: Wire> Wire for (A, B) {
    fn enc(&self, e: &mut Enc) {
        self.0.enc(e);
        self.1.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok((A::dec(d)?, B::dec(d)?))
    }
}

fn decode_error() -> wasmtime::Error {
    crate::host::Stop::Contract("an argument does not decode".into()).into()
}

#[cfg(test)]
mod tests {
    use super::{Dec, Enc, Handle, Wire, encode};
    use crate::abi::{Entry, EntryKind, FsError, Path, ResourceError};

    fn round_trip<T: Wire + PartialEq + std::fmt::Debug>(value: &T, bytes: &[u8]) {
        assert_eq!(encode(value).done(), bytes, "{value:?}");
        let mut d = Dec::new(bytes);
        assert_eq!(&T::dec(&mut d).expect("decodes"), value);
    }

    #[test]
    fn decodes_lengths_strings_and_signed_extremes() {
        let strings: Vec<String> = ["a", "hé", ""].map(String::from).to_vec();
        assert_eq!(
            Vec::<String>::dec(&mut Dec::new(encode(&strings).done())).expect("strings"),
            strings
        );
        for n in [i64::MIN, -1, 0, 1, i64::MAX] {
            let mut e = Enc::default();
            e.zz(n);
            let v = Dec::new(e.done()).leb().expect("zigzag");
            let decoded =
                i64::from_le_bytes((v >> 1).to_le_bytes()) ^ -i64::try_from(v & 1).expect("bit");
            assert_eq!(decoded, n);
            assert_eq!(Dec::new(e.done()).zz().expect("signed integer"), n);
        }
        for bad in [&[128][..], &[3, b'a'][..], &[255; 11][..]] {
            let err = Dec::new(bad).bytes().expect_err("truncated arguments");
            assert_eq!(err.to_string(), "an argument does not decode");
        }
    }

    #[test]
    fn encodes_as_the_node_host_does() {
        let mut e = Enc::default();
        e.leb(0).leb(300).str("hé");
        vec!["a".to_owned(), "bc".to_owned()].enc(&mut e);
        assert_eq!(
            e.done(),
            [
                0, 0xac, 0x02, 3, b'h', 0xc3, 0xa9, 2, 1, b'a', 2, b'b', b'c'
            ]
        );
    }

    /// The language's own types, as host/wire.mjs encodes them.
    #[test]
    fn the_language_types_cross_as_the_js_host_encodes_them() {
        round_trip(&(), &[]);
        round_trip(&300_u16, &[0xac, 0x02]);
        round_trip(&-2_i32, &[3]);
        round_trip(
            &u64::MAX,
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1],
        );
        // `List[u8]` is its length and its bytes; any other list its count
        // and its elements.
        round_trip(&vec![200_u8, 1], &[2, 200, 1]);
        round_trip(&vec![200_u16, 1], &[2, 0xc8, 1, 1]);
        round_trip(&None::<String>, &[0]);
        round_trip(&Some("x".to_owned()), &[1, 1, b'x']);
        round_trip(&Ok::<u32, String>(5), &[0, 5]);
        round_trip(&Err::<u32, String>("e".into()), &[1, 1, b'e']);
        round_trip(&("a".to_owned(), "b".to_owned()), &[1, b'a', 1, b'b']);
        round_trip(&Handle(7), &[7]);
        for bad in [&[2][..], &[0x80, 1][..]] {
            let err = Option::<u32>::dec(&mut Dec::new(bad)).expect_err("a tag past the end");
            assert_eq!(err.to_string(), "an argument does not decode");
        }
        let err = u16::dec(&mut Dec::new(&[0x80, 0x80, 4])).expect_err("past u16");
        assert_eq!(err.to_string(), "an argument does not decode");
    }

    /// The generated std types (`abi.rs`): data as its fields in order, an
    /// enum as its variant index then its payload, a newtype as its field.
    #[test]
    fn the_generated_std_types_cross_as_the_js_host_encodes_them() {
        round_trip(&Path("p".into()), &[1, b'p']);
        round_trip(
            &Entry {
                path: Path("p".into()),
                kind: EntryKind::Symlink,
                size: 300,
            },
            &[1, b'p', 2, 0xac, 0x02],
        );
        round_trip(
            &FsError::InvalidUtf8 {
                path: Path("p".into()),
            },
            &[6, 1, b'p'],
        );
        round_trip(
            &FsError::Other {
                message: "m".into(),
            },
            &[7, 1, b'm'],
        );
        round_trip(&ResourceError::<FsError>::Disposed, &[1]);
        round_trip(
            &ResourceError::Operation {
                error: FsError::NotFound {
                    path: Path("p".into()),
                },
            },
            &[0, 0, 1, b'p'],
        );
        let err = EntryKind::dec(&mut Dec::new(&[3])).expect_err("no fourth kind");
        assert_eq!(err.to_string(), "an argument does not decode");
    }

    #[test]
    fn float_encoding_preserves_ieee_payload_and_negative_zero() {
        for bits in [0x7ff8_0000_0000_0001, 0x7ff0_0000_0000_0000, 1 << 63] {
            let mut encoded = Enc::default();
            encoded.f64(f64::from_bits(bits));
            assert_eq!(encoded.done(), bits.to_le_bytes());
        }
    }
}
