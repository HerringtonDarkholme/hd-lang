//! Host-only formatting of the checked collector's descriptor counters.
use std::collections::BTreeSet;

use crate::host::Host;
use wasmtime::{Instance, Store};

#[derive(Clone, Debug)]
pub(crate) struct Report(Vec<(u32, u8)>);

fn leb(bytes: &mut &[u8]) -> Option<usize> {
    let mut value = 0u32;
    for shift in (0..35).step_by(7) {
        let (&byte, rest) = bytes.split_first()?;
        *bytes = rest;
        if shift == 28 && byte > 15 {
            return None;
        }
        value |= u32::from(byte & 127) << shift;
        if byte < 128 {
            return Some(value as usize);
        }
    }
    None
}

impl Report {
    pub(crate) fn read(wasm: &[u8]) -> Option<Self> {
        let mut bytes = wasm.get(8..)?;
        let mut found = None;
        while !bytes.is_empty() {
            let (&id, rest) = bytes.split_first()?;
            bytes = rest;
            let length = leb(&mut bytes)?;
            let (mut section, rest) = bytes.split_at_checked(length)?;
            bytes = rest;
            if id != 0 {
                continue;
            }
            let length = leb(&mut section)?;
            let (name, data) = section.split_at_checked(length)?;
            if name != b"hd.cycles" {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(Self::decode(data)?);
        }
        found
    }

    fn decode(data: &[u8]) -> Option<Self> {
        let (&version, data) = data.split_first()?;
        if version != 1 || !data.len().is_multiple_of(5) {
            return None;
        }
        let mut seen = BTreeSet::new();
        let mut entries = Vec::new();
        for row in data.chunks_exact(5) {
            let id = u32::from_le_bytes(row[..4].try_into().ok()?);
            let kind = row[4];
            if id == 0 || kind > 6 || !seen.insert(id) {
                return None;
            }
            entries.push((id, kind));
        }
        Some(Self(entries))
    }

    pub(crate) fn summary(&self, instance: Instance, store: &mut Store<Host>) -> Option<String> {
        let pointer = instance
            .get_typed_func::<(), i32>(&mut *store, "hd.cycles.pointer")
            .ok()?
            .call(&mut *store, ())
            .ok()?
            .cast_unsigned() as usize;
        let length = instance
            .get_typed_func::<(), i32>(&mut *store, "hd.cycles.length")
            .ok()?
            .call(&mut *store, ())
            .ok()?
            .cast_unsigned() as usize;
        let memory = instance.get_memory(&mut *store, "hd.heap")?;
        let heap = memory.data(&*store);
        let counts = heap.get(pointer..pointer.checked_add(length.checked_mul(8)?)?)?;
        self.format(counts)
    }

    fn format(&self, counts: &[u8]) -> Option<String> {
        let mut totals = [0u64; 7];
        for &(id, kind) in &self.0 {
            let at = id as usize * 8;
            if at >= counts.len() {
                continue;
            }
            let count = u64::from_le_bytes(counts.get(at..at + 8)?.try_into().ok()?);
            totals[kind as usize] = totals[kind as usize].checked_add(count)?;
        }
        Some(format!(
            "cycles freed: {} closure envs, {} local cells, {} frames, {} waker/all nodes, {} list/map buffers, {} list/map headers, {} objects\n",
            totals[1], totals[2], totals[3], totals[4], totals[5], totals[6], totals[0]
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::Report;
    #[test]
    fn shared_glue_ids_keep_their_kinds_and_full_width_counts() {
        let report = Report::decode(&[1, 1, 0, 0, 0, 1, 2, 0, 0, 0, 2]).unwrap();
        let mut counts = vec![0; 24];
        counts[8..16].copy_from_slice(&(1u64 << 40).to_le_bytes());
        counts[16..24].copy_from_slice(&3u64.to_le_bytes());
        assert_eq!(
            report.format(&counts).unwrap(),
            "cycles freed: 1099511627776 closure envs, 3 local cells, 0 frames, 0 waker/all nodes, 0 list/map buffers, 0 list/map headers, 0 objects\n"
        );
    }
    #[test]
    fn malformed_metadata_is_ignored() {
        for data in [
            &[2][..],
            &[1, 1][..],
            &[1, 0, 0, 0, 0, 1][..],
            &[1, 1, 0, 0, 0, 7][..],
            &[1, 1, 0, 0, 0, 1, 1, 0, 0, 0, 2][..],
        ] {
            assert!(Report::decode(data).is_none());
        }
        assert!(Report::read(b"\0asm\x01\0\0\0\0\xff").is_none());
    }
}
