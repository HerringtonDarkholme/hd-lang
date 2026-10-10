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
}

#[cfg(test)]
mod tests {
    use super::Enc;

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
}
