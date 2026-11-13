//! Address-preserving collection of unreachable immutable static bytes.
//!
//! Like Binaryen's memory packing, omit initialization ranges instead of
//! moving addresses. Mutable slots and alignment retain their original layout.
//! <https://github.com/WebAssembly/binaryen/blob/main/src/passes/MemoryPacking.cpp>

use crate::module::{HEAP, STATIC_BASE};
use std::ops::Range;
use wasm_encoder::{ConstExpr, DataSection, Encode};

pub(crate) struct Chunk {
    pub range: Range<usize>,
    pub live: bool,
}

/// Read only within one immutable allocation; mutable slots and padding have
/// no chunk. Binary search costs O(log A), and callers copy at most 31 bytes.
pub(crate) fn bytes<'a>(data: &'a [u8], chunks: &[Chunk], at: u32, len: u32) -> Option<&'a [u8]> {
    let start = at.checked_sub(STATIC_BASE)? as usize;
    let end = start.checked_add(len as usize)?;
    let next = chunks.partition_point(|chunk| chunk.range.start <= start);
    let chunk = chunks.get(next.checked_sub(1)?)?;
    (end <= chunk.range.end).then(|| &data[start..end])
}

/// A pointer into a chunk keeps the complete allocation. At an allocation
/// boundary, keep both neighbors: an empty payload can point past its header.
/// Numeric constants never act as pointers unless emission marks them.
pub(crate) fn mark(chunks: &mut [Chunk], at: u32) {
    let Some(offset) = at.checked_sub(STATIC_BASE) else {
        return;
    };
    let offset = offset as usize;
    if chunks.last().is_none_or(|chunk| offset > chunk.range.end) {
        return;
    }
    let next = chunks.partition_point(|chunk| chunk.range.start <= offset);
    if let Some(last) = next.checked_sub(1) {
        if offset <= chunks[last].range.end {
            chunks[last].live = true;
        }
        if chunks[last].range.start == offset && last > 0 && chunks[last - 1].range.end == offset {
            chunks[last - 1].live = true;
        }
    }
}

fn active(section: &mut DataSection, bytes: &[u8], range: Range<usize>) {
    if range.is_empty() {
        return;
    }
    // ruled out: Module allocated the original segment in 32-bit memory.
    let offset = crate::emit::required(u32::try_from(range.start).ok(), "static data offset fits");
    section.active(
        HEAP,
        &ConstExpr::i32_const((STATIC_BASE + offset).cast_signed()),
        bytes[range].iter().copied(),
    );
}

/// O(A + D) work/storage for A allocation records and D segment bytes.
/// Each chunk is visited once; adjacent dead chunks merge without rescanning.
/// Exact encoded pricing guarantees a strict reduction or identical output.
/// The heap-zero segment header is at most 13 bytes (flags, i32.const
/// opcode and signed offset, end, and u32 payload length); leave smaller holes to avoid many tiny segments.
pub(crate) fn section(bytes: &[u8], chunks: &[Chunk], visit: &mut impl FnMut()) -> DataSection {
    let mut original = DataSection::new();
    active(&mut original, bytes, 0..bytes.len());
    let mut cuts: Vec<Range<usize>> = Vec::new();
    for chunk in chunks {
        visit();
        if chunk.live {
            continue;
        }
        if let Some(last) = cuts.last_mut()
            && last.end == chunk.range.start
        {
            last.end = chunk.range.end;
        } else {
            cuts.push(chunk.range.clone());
        }
    }
    // No profitable range means no candidate encoding or byte comparison.
    cuts.retain(|cut| cut.len() > 13);
    if cuts.is_empty() {
        return original;
    }
    let mut candidate = DataSection::new();
    let mut start = 0;
    for cut in cuts {
        active(&mut candidate, bytes, start..cut.start);
        start = cut.end;
    }
    active(&mut candidate, bytes, start..bytes.len());
    let mut old = Vec::new();
    original.encode(&mut old);
    let mut new = Vec::new();
    candidate.encode(&mut new);
    if new.len() < old.len() {
        candidate
    } else {
        original
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encoded(section: &DataSection) -> Vec<u8> {
        let mut bytes = Vec::new();
        section.encode(&mut bytes);
        bytes
    }

    #[test]
    fn constants_do_not_read_mutable_slots_padding_or_allocation_boundaries() {
        let data: Vec<u8> = (0..32).collect();
        let chunks = [
            Chunk {
                range: 4..12,
                live: false,
            },
            Chunk {
                range: 16..24,
                live: false,
            },
        ];
        assert_eq!(bytes(&data, &chunks, STATIC_BASE + 6, 3), Some(&data[6..9]));
        for (at, len) in [(0, 1), (3, 2), (11, 2), (12, 1), (15, 2), (23, 2), (24, 1)] {
            assert!(bytes(&data, &chunks, STATIC_BASE + at, len).is_none());
        }
        assert!(bytes(&data, &chunks, STATIC_BASE - 1, 1).is_none());
        assert!(bytes(&data, &chunks, u32::MAX, 31).is_none());
    }

    #[test]
    fn keeps_live_interiors_and_both_sides_of_payload_boundaries() {
        let mut chunks = vec![
            Chunk {
                range: 0..16,
                live: false,
            },
            Chunk {
                range: 16..32,
                live: false,
            },
            Chunk {
                range: 32..64,
                live: false,
            },
        ];
        mark(&mut chunks, STATIC_BASE + 16);
        assert!(chunks[0].live && chunks[1].live && !chunks[2].live);
        mark(&mut chunks, STATIC_BASE + 40);
        assert!(chunks[2].live);
        mark(&mut chunks, STATIC_BASE - 1);
    }

    #[test]
    fn never_grows_and_keeps_live_data_at_its_original_offset() {
        let mut bytes = vec![b'x'; 128];
        bytes[64..].fill(b'y');
        let chunks = [
            Chunk {
                range: 0..64,
                live: false,
            },
            Chunk {
                range: 64..128,
                live: true,
            },
        ];
        let section = section(&bytes, &chunks, &mut || {});
        let mut expected = DataSection::new();
        active(&mut expected, &bytes, 64..128);
        assert_eq!(encoded(&section), encoded(&expected));
        for width in 0..64 {
            let bytes = vec![1; width + 2];
            let chunks = [Chunk {
                range: 1..width + 1,
                live: false,
            }];
            let candidate = section_for(&bytes, &chunks);
            let mut old = DataSection::new();
            active(&mut old, &bytes, 0..bytes.len());
            assert!(encoded(&candidate).len() <= encoded(&old).len());
        }
    }

    fn section_for(bytes: &[u8], chunks: &[Chunk]) -> DataSection {
        section(bytes, chunks, &mut || {})
    }

    #[test]
    fn adjacent_small_dead_chunks_merge_and_work_is_linear() {
        for width in [100, 1000] {
            let bytes = vec![1; width * 8];
            let chunks: Vec<_> = (0..width)
                .map(|i| Chunk {
                    range: i * 8..(i + 1) * 8,
                    live: false,
                })
                .collect();
            let mut visits = 0;
            let section = section(&bytes, &chunks, &mut || visits += 1);
            assert_eq!(visits, width);
            assert!(section.is_empty());
        }
    }
}
