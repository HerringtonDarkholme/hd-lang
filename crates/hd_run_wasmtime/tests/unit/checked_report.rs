mod tests {
    use hd_run_wasmtime::internals::checked_report::Report;
    fn sites() -> Vec<u8> {
        let mut w = hd_base::wire::Writer::default();
        w.u8(1);
        for id in [1, 2] {
            w.u32(id);
            w.str("f");
            w.str("f.hd");
            w.u32(9);
            w.str("source call");
        }
        w.bytes
    }
    #[test]
    fn pairs_are_full_width_and_each_snapshot_reports_only_its_delta() {
        let mut report = Report::decode(&[1]).unwrap();
        report.1 = hd_run_wasmtime::internals::checked_report::count_sites(&sites()).unwrap();
        let mut counts = vec![0; 24];
        counts[8..16].copy_from_slice(&(1u64 << 60).to_le_bytes());
        counts[16..24].copy_from_slice(&3u64.to_le_bytes());
        let mut previous = hd_run_wasmtime::internals::checked_report::PairTotals::new();
        let rows = report.pair_rows(&counts, &mut previous).unwrap();
        assert_eq!(
            rows,
            vec![
                serde_json::json!({"function":"f","file":"f.hd","line":9,"reason":"source call","count":"1152921504606846979"})
            ]
        );
        assert!(report.pair_rows(&counts, &mut previous).unwrap().is_empty());
        counts[16..24].copy_from_slice(&5u64.to_le_bytes());
        assert_eq!(
            report.pair_rows(&counts, &mut previous).unwrap()[0]["count"],
            "2"
        );
    }
    #[test]
    fn count_metadata_rejects_truncation_and_duplicate_ids() {
        let bytes = sites();
        for end in 2..bytes.len() {
            if end != 1 + (bytes.len() - 1) / 2 {
                assert!(
                    hd_run_wasmtime::internals::checked_report::count_sites(&bytes[..end])
                        .is_none()
                );
            }
        }
        let mut duplicate = bytes;
        // Change the second id to the first id; records have fixed fields here.
        let second = 1 + (duplicate.len() - 1) / 2;
        duplicate[second..second + 4].copy_from_slice(&1u32.to_le_bytes());
        assert!(hd_run_wasmtime::internals::checked_report::count_sites(&duplicate).is_none());
    }
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
