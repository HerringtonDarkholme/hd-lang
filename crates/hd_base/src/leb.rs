//! Unsigned LEB128, the integer form of the Wasm binary format and of the
//! frames hd writes itself (run journals, the web wire, frame metadata):
//! the one encoder, decoder and length every crate uses.

/// Appends `v` as unsigned LEB128: seven bits a byte, low bits first.
pub fn put(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = v.to_le_bytes()[0] & 0x7f;
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

/// How many bytes [`put`] writes for `v`.
#[must_use]
pub fn len(v: u64) -> usize {
    (64 - v.max(1).leading_zeros() as usize).div_ceil(7)
}

/// The unsigned LEB128 number of at most `bits` bits (32 or 64) at `*at`,
/// which moves past it. `None` when the bytes end first, or when the last
/// byte the width allows holds more bits than are left or goes on, as
/// the Wasm binary format rejects.
#[must_use]
pub fn get(bytes: &[u8], at: &mut usize, bits: u32) -> Option<u64> {
    let mut v = 0u64;
    let mut shift = 0;
    loop {
        let byte = *bytes.get(*at)?;
        *at += 1;
        if shift + 7 > bits && u32::from(byte) >> (bits - shift) != 0 {
            return None;
        }
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
        shift += 7;
    }
}

/// [`get`] from the front of `bytes`, which then starts after it.
#[must_use]
pub fn take(bytes: &mut &[u8], bits: u32) -> Option<u64> {
    let mut at = 0;
    let v = get(bytes, &mut at, bits)?;
    *bytes = &bytes[at..];
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::{get, len, put, take};

    #[test]
    fn every_width_round_trips_and_its_length_is_the_bytes_written() {
        for v in [
            0,
            1,
            127,
            128,
            300,
            16_383,
            16_384,
            u64::from(u32::MAX),
            u64::MAX,
        ] {
            let mut out = Vec::new();
            put(&mut out, v);
            assert_eq!(out.len(), len(v), "{v}");
            let mut at = 0;
            assert_eq!(get(&out, &mut at, 64), Some(v));
            assert_eq!(at, out.len());
            let mut at = 0;
            let narrow = get(&out, &mut at, 32);
            assert_eq!(narrow, u32::try_from(v).ok().map(u64::from), "{v}");
            out.push(9);
            let mut rest = out.as_slice();
            assert_eq!(take(&mut rest, 64), Some(v));
            assert_eq!(rest, [9]);
        }
    }

    #[test]
    fn a_short_or_too_wide_number_reads_as_none() {
        assert_eq!(get(&[0x80], &mut 0, 64), None);
        // The fifth byte of a 32-bit number holds four bits and ends it.
        assert_eq!(get(&[0xff, 0xff, 0xff, 0xff, 0x10], &mut 0, 32), None);
        assert_eq!(get(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x00], &mut 0, 32), None);
        assert_eq!(
            get(&[0xff, 0xff, 0xff, 0xff, 0x0f], &mut 0, 32),
            Some(u64::from(u32::MAX))
        );
        // The tenth byte of a 64-bit number holds one bit.
        let mut wide = vec![0xff; 9];
        wide.push(0x02);
        assert_eq!(get(&wide, &mut 0, 64), None);
    }
}
