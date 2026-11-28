mod tests {
    use hd_wasm::testing::module::STATIC_BASE;
    use hd_wasm::testing::static_data::*;

    use wasm_encoder::{DataSection, Encode};

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
