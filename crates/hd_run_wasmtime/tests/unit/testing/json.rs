mod tests {
    use hd_run_wasmtime::internals::testing::CaseRun;

    #[test]
    fn result_protocol_round_trips_and_only_time_is_ignored() {
        let result = CaseRun {
            test: 7,
            status: 3,
            trapped: true,
            stdout: "hi\n\"\\\u{0}é".into(),
            stderr: "panic".into(),
            us: 123,
            row: Some((1, 2)),
            input: Some("x".into()),
            failure: Some("state left by an earlier case".into()),
            panic: Some(hd_run_wasmtime::internals::testing::PanicInfo {
                category: "explicit-panic".into(),
                message: "a\npanic: integer-overflow: \"b\"".into(),
                frames: Vec::new(),
            }),
            frames: vec![(4, 120), (9, 77)],
            snapshots: vec![hd_run_wasmtime::internals::testing::Recorded {
                text: "a\tb\n\"".into(),
            }],
        };
        assert_eq!(
            CaseRun::from_json_line(&result.to_json_line()).expect("round trip"),
            result
        );
        let mut different = result.clone();
        different.us += 1;
        different.frames.clear();
        assert!(result.equivalent(&different));
        different.input = None;
        assert!(!result.equivalent(&different));
        let mut different = result.clone();
        different.panic = None;
        assert!(!result.equivalent(&different));
        let empty = CaseRun {
            test: 2,
            row: Some((0, 0)),
            ..CaseRun::default()
        };
        assert_eq!(
            CaseRun::from_json_line(&empty.to_json_line()).expect("empty table"),
            empty
        );
        assert_eq!(empty.to_json_line(), "{\"test\":2,\"row\":0,\"rows\":0}");
    }

    #[test]
    fn malformed_result_lines_are_errors_not_fallbacks() {
        for line in [
            "not JSON",
            "[]",
            "{}",
            "{\"test\":-1}",
            "{\"test\":0,\"row\":0}",
            "{\"test\":0,\"row\":2,\"rows\":2}",
            "{\"test\":0,\"status\":0,\"trapped\":false,\"stdout\":0,\"stderr\":\"\",\"us\":0}",
        ] {
            assert!(CaseRun::from_json_line(line).is_err(), "{line}");
        }
    }
}
