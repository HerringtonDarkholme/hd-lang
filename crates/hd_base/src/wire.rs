//! The little-endian codec of cache-entry sections (data-structures.md
//! §3.20): a writer over `Vec<u8>` and a reader that never panics. A read
//! past the end yields zeros and marks the reader bad; the caller checks
//! `ok()` once and treats a bad section as a cache miss (cache.md §5.4).

use crate::Hash128;

/// Appends little-endian fields.
#[derive(Default, Debug, Clone)]
pub struct Writer {
    pub bytes: Vec<u8>,
}

impl Writer {
    pub fn u8(&mut self, v: u8) {
        self.bytes.push(v);
    }
    pub fn u32(&mut self, v: u32) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn u64(&mut self, v: u64) {
        self.bytes.extend_from_slice(&v.to_le_bytes());
    }
    pub fn hash(&mut self, h: Hash128) {
        self.bytes.extend_from_slice(&h.0.to_le_bytes());
    }
    /// Length then bytes.
    pub fn blob(&mut self, b: &[u8]) {
        self.u32(u32::try_from(b.len()).expect("section over 4 GiB"));
        self.bytes.extend_from_slice(b);
    }
    pub fn str(&mut self, s: &str) {
        self.blob(s.as_bytes());
    }
    pub fn len_of<T>(&mut self, v: &[T]) {
        self.u32(u32::try_from(v.len()).expect("list over u32"));
    }
}

/// Reads little-endian fields; never panics.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    bad: bool,
}

impl<'a> Reader<'a> {
    #[must_use]
    pub fn new(bytes: &'a [u8]) -> Self {
        Self {
            bytes,
            pos: 0,
            bad: false,
        }
    }
    fn take(&mut self, n: usize) -> &'a [u8] {
        if let Some(s) = self.bytes.get(self.pos..self.pos.saturating_add(n)) {
            self.pos += n;
            s
        } else {
            self.bad = true;
            self.pos = self.bytes.len();
            &[]
        }
    }
    fn arr<const N: usize>(&mut self) -> [u8; N] {
        self.take(N).try_into().unwrap_or([0; N])
    }
    pub fn u8(&mut self) -> u8 {
        self.arr::<1>()[0]
    }
    pub fn u32(&mut self) -> u32 {
        u32::from_le_bytes(self.arr())
    }
    pub fn u64(&mut self) -> u64 {
        u64::from_le_bytes(self.arr())
    }
    pub fn hash(&mut self) -> Hash128 {
        Hash128(u128::from_le_bytes(self.arr()))
    }
    pub fn blob(&mut self) -> &'a [u8] {
        let n = self.u32() as usize;
        self.take(n)
    }
    pub fn str(&mut self) -> &'a str {
        let b = self.blob();
        core::str::from_utf8(b).unwrap_or_else(|_| {
            self.bad = true;
            ""
        })
    }
    /// A list length, capped by the bytes left so a corrupt length cannot
    /// make the caller allocate without bound.
    pub fn count(&mut self) -> usize {
        let n = self.u32() as usize;
        if n > self.bytes.len() - self.pos {
            self.bad = true;
            return 0;
        }
        n
    }
    #[must_use]
    pub fn ok(&self) -> bool {
        !self.bad
    }
    #[must_use]
    pub fn at_end(&self) -> bool {
        self.pos == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::{Reader, Writer};
    use crate::Hash128;

    #[test]
    fn round_trip_and_short_reads_are_flagged() {
        let mut w = Writer::default();
        w.u32(7);
        w.str("ab");
        w.hash(Hash128(9));
        let mut r = Reader::new(&w.bytes);
        assert_eq!((r.u32(), r.str(), r.hash()), (7, "ab", Hash128(9)));
        assert!(r.ok() && r.at_end());
        let mut r = Reader::new(&w.bytes[..5]);
        let _ = r.u32();
        let _ = r.str();
        assert!(!r.ok());
    }
}
