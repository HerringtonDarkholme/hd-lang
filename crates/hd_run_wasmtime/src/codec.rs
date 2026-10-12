//! The exchange-buffer encoding of runtime-and-host.md §17.4, as the
//! Node host's `Enc` writes it (`compiler/host/core.mjs`): integers as
//! LEB128 (zigzag when signed), strings and byte lists as their length
//! then their bytes, lists as their count then their elements, an
//! optional as 0, or 1 and the value, an enum as its variant index then
//! its payload.

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

    pub fn list<T>(&mut self, xs: &[T], each: impl Fn(&mut Enc, &T)) -> &mut Self {
        self.leb(xs.len() as u64);
        for x in xs {
            each(self, x);
        }
        self
    }

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

    pub fn strings(&mut self) -> wasmtime::Result<Vec<String>> {
        let count = self.leb()?;
        (0..count).map(|_| self.str()).collect()
    }
}

fn decode_error() -> wasmtime::Error {
    crate::host::Stop::Contract("an argument does not decode".into()).into()
}

#[cfg(test)]
mod tests {
    use super::{Dec, Enc};

    #[test]
    fn decodes_lengths_strings_and_signed_extremes() {
        let mut e = Enc::default();
        e.list(&["a", "hé", ""], |e, s| {
            e.str(s);
        });
        assert_eq!(
            Dec::new(e.done()).strings().expect("strings"),
            ["a", "hé", ""]
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
        e.leb(0).leb(300).str("hé").list(&["a", "bc"], |e, s| {
            e.str(s);
        });
        assert_eq!(
            e.done(),
            [
                0, 0xac, 0x02, 3, b'h', 0xc3, 0xa9, 2, 1, b'a', 2, b'b', b'c'
            ]
        );
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
