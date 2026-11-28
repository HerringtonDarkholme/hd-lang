mod tests {
    use hd_run_wasmtime::internals::http::target::{Target, remove_dot_segments, resolve};

    const BASE: &str = "http://a/b/c/d;p?q";

    fn check(cases: &[(&str, &str)]) {
        for (reference, expected) in cases {
            assert_eq!(resolve(BASE, reference), *expected, "{reference}");
        }
    }

    /// RFC 3986 section 5.4.1.
    #[test]
    fn the_rfcs_normal_examples_resolve() {
        check(&[
            ("g:h", "g:h"),
            ("g", "http://a/b/c/g"),
            ("./g", "http://a/b/c/g"),
            ("g/", "http://a/b/c/g/"),
            ("/g", "http://a/g"),
            ("//g", "http://g"),
            ("?y", "http://a/b/c/d;p?y"),
            ("g?y", "http://a/b/c/g?y"),
            ("#s", "http://a/b/c/d;p?q#s"),
            ("g#s", "http://a/b/c/g#s"),
            ("g?y#s", "http://a/b/c/g?y#s"),
            (";x", "http://a/b/c/;x"),
            ("g;x", "http://a/b/c/g;x"),
            ("g;x?y#s", "http://a/b/c/g;x?y#s"),
            ("", "http://a/b/c/d;p?q"),
            (".", "http://a/b/c/"),
            ("./", "http://a/b/c/"),
            ("..", "http://a/b/"),
            ("../", "http://a/b/"),
            ("../g", "http://a/b/g"),
            ("../..", "http://a/"),
            ("../../", "http://a/"),
            ("../../g", "http://a/g"),
        ]);
    }

    /// RFC 3986 section 5.4.2, with the strict parser.
    #[test]
    fn the_rfcs_abnormal_examples_resolve() {
        check(&[
            ("../../../g", "http://a/g"),
            ("../../../../g", "http://a/g"),
            ("/./g", "http://a/g"),
            ("/../g", "http://a/g"),
            ("g.", "http://a/b/c/g."),
            (".g", "http://a/b/c/.g"),
            ("g..", "http://a/b/c/g.."),
            ("..g", "http://a/b/c/..g"),
            ("./../g", "http://a/b/g"),
            ("./g/.", "http://a/b/c/g/"),
            ("g/./h", "http://a/b/c/g/h"),
            ("g/../h", "http://a/b/c/h"),
            ("g;x=1/./y", "http://a/b/c/g;x=1/y"),
            ("g;x=1/../y", "http://a/b/c/y"),
            ("g?y/./x", "http://a/b/c/g?y/./x"),
            ("g?y/../x", "http://a/b/c/g?y/../x"),
            ("g#s/./x", "http://a/b/c/g#s/./x"),
            ("g#s/../x", "http://a/b/c/g#s/../x"),
            ("http:g", "http:g"),
        ]);
    }

    #[test]
    fn a_base_with_an_empty_path_merges_at_the_root() {
        assert_eq!(resolve("http://a", "g"), "http://a/g");
        assert_eq!(resolve("http://a?q", "?y"), "http://a?y");
        assert_eq!(resolve("http://a/b", "http://[bad/"), "http://[bad/");
    }

    #[test]
    fn dot_segments_are_removed_from_a_path() {
        for (path, expected) in [
            ("/a/b/c/./../../g", "/a/g"),
            ("mid/content=5/../6", "mid/6"),
            ("/a/b/..", "/a/"),
            ("", ""),
        ] {
            assert_eq!(remove_dot_segments(path), expected, "{path}");
        }
    }

    #[test]
    fn a_url_is_absolute_http_or_https_with_an_ascii_host() {
        for valid in [
            "http://xn--bcher-kva.example/",
            "https://Example.COM/",
            "http://127.0.0.1:8080/p?q#f",
            "http://[::1]/",
            "http://user:pw@host/",
            "HTTP://host",
        ] {
            assert!(Target::parse(valid).is_some(), "{valid}");
        }
        for invalid in [
            "",
            "/relative",
            "relative",
            "//host/path",
            "ftp://host/",
            "file:///etc/passwd",
            "mailto:a@b.example",
            "http:///path",
            "http://",
            "http://[bad/",
            "http://host:99999/",
            "http://host:/",
            "http://bücher.example/",
            "http://user@bücher.example/",
            "http://b%C3%BCcher.example/",
            "http://host/ path",
            "http://host/é",
        ] {
            assert!(Target::parse(invalid).is_none(), "{invalid}");
        }
    }

    #[test]
    fn the_host_is_lowercase_and_the_default_port_is_left_out() {
        let t = Target::parse("HTTPS://Example.COM:443/A/./b/../c?Q#f").unwrap();
        assert_eq!(t.host, "example.com");
        assert_eq!(t.port, 443);
        assert_eq!(t.authority, "example.com");
        assert_eq!(t.path_and_query, "/A/c?Q");
        assert_eq!(t.uri(), "https://example.com/A/c?Q");
        let t = Target::parse("http://[::1]:8080").unwrap();
        assert_eq!((t.host.as_str(), t.port), ("::1", 8080));
        assert_eq!(t.authority, "[::1]:8080");
        assert_eq!(t.path_and_query, "/");
        let t = Target::parse("http://us%40er:p%3Aw@host:81/").unwrap();
        assert_eq!(t.credentials, ["us%40er", "p%3Aw"]);
        assert_eq!(t.authority, "host:81");
    }
}
