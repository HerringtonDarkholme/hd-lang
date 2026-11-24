mod tests {
    use crate::net::test_support::Net;
    use hd_run::{Grants, Limit};
    use hd_run_wasmtime::internals::codec::Enc;
    use hd_run_wasmtime::internals::net::validate::{covers_lookup, valid_host};

    #[test]
    fn names_literal_spelling_and_lookup_grants_match_node() {
        for valid in [
            "localhost",
            "a.",
            "a-b.example",
            "127.0.0.1",
            "::1",
            "fe80::1%lo0",
        ] {
            assert!(valid_host(valid), "{valid}");
        }
        for invalid in ["", "-a", "a-", "a..b", "a/b", "[::1]", "a\0b", "é.example"] {
            assert!(!valid_host(invalid), "{invalid}");
        }
        let grants = Grants::with_limits(vec![(
            "Net".into(),
            Limit::Entries(vec![
                "127.0.0.1:80".into(),
                "[::1]:9".into(),
                "*.example:8".into(),
            ]),
        )]);
        assert!(covers_lookup(&grants, "127.0.0.1"));
        assert!(covers_lookup(&grants, "::1"));
        assert!(covers_lookup(&grants, "one.example"));
        assert!(!covers_lookup(&grants, "example"));
        assert!(!covers_lookup(&grants, "one.EXAMPLE"));
        let mut net = Net::new();
        for host in ["0:0:0:0:0:0:0:1", "fe80::1%lo0"] {
            let actual = net
                .call(
                    "lookup",
                    &[i64::try_from(host.len()).unwrap()],
                    host.as_bytes(),
                    &Grants::default(),
                )
                .unwrap();
            let mut expected = Enc::default();
            expected.leb(0).leb(1).str(host);
            assert_eq!(actual.done(), expected.done());
        }
    }
}
