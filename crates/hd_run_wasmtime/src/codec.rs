//! The exchange-buffer encoding of `crates/hd_run_wasmtime/DESIGN.md` §17.4, as the
//! JS host's `Enc` writes it (`host/wire.ts`): integers as
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
    pub fn leb(&mut self, v: u64) -> &mut Self {
        hd_base::leb::put(&mut self.buf, v);
        self
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
    pub fn f32(&mut self, value: f32) -> &mut Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    /// Raw IEEE 754 bits, little-endian, including NaN payloads and signed zero.
    pub fn f64(&mut self, value: f64) -> &mut Self {
        self.buf.extend_from_slice(&value.to_le_bytes());
        self
    }

    #[cfg(feature = "testing")]
    #[must_use]
    pub fn done(&self) -> &[u8] {
        &self.buf
    }

    /// Transfers the encoded reply without copying its payload.
    #[must_use]
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
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    pub fn leb(&mut self) -> wasmtime::Result<u64> {
        hd_base::leb::get(self.bytes, &mut self.at, 64).ok_or_else(decode_error)
    }

    /// The bytes not read yet.
    #[must_use]
    pub fn rest(&self) -> usize {
        self.bytes.len() - self.at
    }

    /// The next `N` bytes, as a float's raw little-endian bits.
    pub fn raw<const N: usize>(&mut self) -> wasmtime::Result<[u8; N]> {
        let end = self.at.checked_add(N).ok_or_else(decode_error)?;
        let bytes = self.bytes.get(self.at..end).ok_or_else(decode_error)?;
        self.at = end;
        bytes.try_into().map_err(|_| decode_error())
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

impl Wire for bool {
    fn enc(&self, e: &mut Enc) {
        e.leb(u64::from(*self));
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(d.tag(2)? == 1)
    }
}
