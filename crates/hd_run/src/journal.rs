//! The execution journal (live-execution.md §4): rows, their framing and
//! a torn-tail-tolerant reader. Replay and resume (§5, §6) are not built.
//!
//! `journal = Header Row*`; `Row = kind: u8, index: uleb, len: uleb,
//! payload, crc32: u32`.

/// Row kinds (§4.2).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RowKind {
    Header,
    Input,
    Call,
    Finish,
    Wake,
    Block,
    Abort,
    Cancel,
    End,
}

impl RowKind {
    const ALL: [RowKind; 9] = [
        RowKind::Header,
        RowKind::Input,
        RowKind::Call,
        RowKind::Finish,
        RowKind::Wake,
        RowKind::Block,
        RowKind::Abort,
        RowKind::Cancel,
        RowKind::End,
    ];
}

/// One row: its kind, event index and payload bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: RowKind,
    pub index: u64,
    pub payload: Vec<u8>,
}

/// Large payloads go to a blob store by content hash (§4.3).
pub const BLOB_THRESHOLD: usize = 4096;

/// CRC-32 (IEEE), bitwise: the journal checksum.
#[must_use]
pub fn crc32(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for &b in bytes {
        c ^= u32::from(b);
        for _ in 0..8 {
            c = if c & 1 != 0 {
                (c >> 1) ^ 0xEDB8_8320
            } else {
                c >> 1
            };
        }
    }
    !c
}

pub fn put_uleb(out: &mut Vec<u8>, mut v: u64) {
    loop {
        let byte = u8::try_from(v & 0x7f).expect("7 bits");
        v >>= 7;
        if v == 0 {
            out.push(byte);
            return;
        }
        out.push(byte | 0x80);
    }
}

fn get_uleb(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

/// Appends one framed row.
pub fn append(out: &mut Vec<u8>, row: &Row) {
    let start = out.len();
    out.push(row.kind as u8);
    put_uleb(out, row.index);
    put_uleb(out, row.payload.len() as u64);
    out.extend_from_slice(&row.payload);
    let crc = crc32(&out[start..]);
    out.extend_from_slice(&crc.to_le_bytes());
}

/// Reads rows until the end or the first torn or corrupt row; returns the
/// rows and the length of the valid prefix, where a writer resumes (§4.3).
#[must_use]
pub fn read(b: &[u8]) -> (Vec<Row>, usize) {
    let mut rows = Vec::new();
    let mut at = 0;
    loop {
        let start = at;
        let Some(&k) = b.get(at) else {
            return (rows, start);
        };
        at += 1;
        let parsed = (|| {
            let kind = *RowKind::ALL.get(k as usize)?;
            let index = get_uleb(b, &mut at)?;
            let len = usize::try_from(get_uleb(b, &mut at)?).ok()?;
            let payload = b.get(at..at + len)?.to_vec();
            at += len;
            let crc = u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?);
            (crc == crc32(&b[start..at])).then_some(Row {
                kind,
                index,
                payload,
            })
        })();
        match parsed {
            Some(r) if r.index == rows.len() as u64 => {
                at += 4;
                rows.push(r);
            }
            _ => return (rows, start),
        }
    }
}

/// A 64-bit request digest: method id and argument bytes (§4.2).
#[must_use]
pub fn request_digest(method: u32, args: &[u8]) -> u64 {
    let mut h = hd_base::StableHasher::new("req");
    h.u32(method);
    h.bytes(args);
    u64::try_from(h.finish().0 & u128::from(u64::MAX)).expect("low half")
}

#[cfg(test)]
mod tests {
    use super::{Row, RowKind, append, crc32, read};

    #[test]
    fn rows_round_trip_and_a_torn_tail_is_cut() {
        let mut j = Vec::new();
        let rows = [
            Row {
                kind: RowKind::Header,
                index: 0,
                payload: b"fmt1".to_vec(),
            },
            Row {
                kind: RowKind::Call,
                index: 1,
                payload: vec![7; 300],
            },
            Row {
                kind: RowKind::End,
                index: 2,
                payload: vec![],
            },
        ];
        for r in &rows {
            append(&mut j, r);
        }
        let (back, valid) = read(&j);
        assert_eq!(back, rows);
        assert_eq!(valid, j.len());
        let torn = &j[..j.len() - 2];
        let (back, valid) = read(torn);
        assert_eq!(back.len(), 2);
        assert!(valid < torn.len());
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
