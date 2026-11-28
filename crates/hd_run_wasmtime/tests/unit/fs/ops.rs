mod tests {
    use crate::fs::test_support::{TempDir, error, invoke};
    use hd_run_wasmtime::internals::codec::Enc;
    use std::fs;

    #[test]
    fn reads_text_strictly_and_preserves_every_bom() {
        let dir = TempDir::new();
        let path = dir.path("text");
        fs::write(&path, "\u{feff}\u{feff}hé").unwrap();
        let actual = invoke("read_text", &path, None);
        let mut expected = Enc::default();
        expected.leb(0).str("\u{feff}\u{feff}hé");
        assert_eq!(actual.done(), expected.done());
        for bytes in [&[0xc0, 0xaf][..], &[0xed, 0xa0, 0x80], &[0xe2, 0x82]] {
            fs::write(&path, bytes).unwrap();
            assert_eq!(
                invoke("read_text", &path, None).done(),
                error(&path, 6).done()
            );
            let mut expected = Enc::default();
            expected.leb(0).bytes(bytes);
            assert_eq!(invoke("read_bytes", &path, None).done(), expected.done());
        }
    }
    #[cfg(unix)]
    #[test]
    fn rejects_oversized_regular_files_before_reading_them() {
        let dir = TempDir::new();
        let path = dir.path("sparse");
        fs::File::create(&path)
            .unwrap()
            .set_len(0x8000_0000)
            .unwrap();
        let expected = error("File size (2147483648) is greater than 2 GiB", 7);
        for method in ["read_text", "read_bytes"] {
            assert_eq!(invoke(method, &path, None).done(), expected.done());
        }
    }
    /// A directory used as a file is `IsADirectory` on every OS, whatever
    /// code the OS gives (Windows: "Incorrect function" on read, "The
    /// parameter is incorrect" on a write-open).
    #[test]
    fn a_directory_read_or_written_as_a_file_is_a_directory_error() {
        let dir = TempDir::new();
        let path = dir.0.to_string_lossy().into_owned();
        for method in ["read_text", "read_bytes"] {
            assert_eq!(
                invoke(method, &path, None).done(),
                error(&path, 5).done(),
                "{method}"
            );
        }
        for method in ["write_bytes", "write_text", "append_text"] {
            assert_eq!(
                invoke(method, &path, Some(b"no")).done(),
                error(&path, 5).done(),
                "{method}"
            );
        }
    }
    /// A path below a file is `NotADirectory` on every OS, as Node reports
    /// it (Windows says "path not found" for the open).
    #[test]
    fn a_path_below_a_file_is_a_not_a_directory_error() {
        let dir = TempDir::new();
        let file = dir.path("file");
        fs::write(&file, "hello").unwrap();
        let below = format!("{file}/child");
        for method in ["read_bytes", "read_text"] {
            assert_eq!(
                invoke(method, &below, None).done(),
                error(&below, 4).done(),
                "{method}"
            );
        }
        assert_eq!(
            invoke("write_text", &below, Some(b"no")).done(),
            error(&below, 4).done()
        );
        assert_eq!(
            invoke("list_dir", &below, None).done(),
            error(&below, 4).done()
        );
    }
    #[test]
    fn recursive_mkdir_and_stat_have_node_edge_behavior() {
        let dir = TempDir::new();
        let path = dir.path("a/b/c");
        assert_eq!(invoke("create_dir_all", &path, None).done(), &[0]);
        assert_eq!(invoke("create_dir_all", &path, None).done(), &[0]);
        assert_eq!(
            invoke("create_dir_all", "", None).done(),
            error("", 0).done()
        );
        let file = dir.path("file");
        fs::write(&file, "hello").unwrap();
        assert_eq!(
            invoke("create_dir_all", &file, None).done(),
            error(&file, 4).done()
        );
        assert_eq!(
            invoke("stat", &format!("{file}/child"), None).done(),
            &[0, 0]
        );
        assert_eq!(invoke("stat", &dir.path("absent"), None).done(), &[0, 0]);
    }
    #[cfg(unix)]
    #[test]
    fn lists_byte_order_and_reports_symlink_and_directory_sizes() {
        use hd_run_wasmtime::internals::codec::Dec;
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        fs::write(dir.path("é"), "hello").unwrap();
        fs::write(dir.path("z"), "z").unwrap();
        fs::create_dir(dir.path("a")).unwrap();
        symlink("é", dir.path("link")).unwrap();
        let path = dir.0.to_string_lossy();
        let actual = invoke("list_dir", &path, None);
        let mut d = Dec::new(actual.done());
        assert_eq!(d.leb().unwrap(), 0);
        assert_eq!(d.leb().unwrap(), 4);
        for (name, kind, size) in [("a", 1, 0), ("link", 2, 2), ("z", 0, 1), ("é", 0, 5)] {
            assert_eq!(d.str().unwrap(), format!("{path}/{name}"));
            assert_eq!(d.leb().unwrap(), kind);
            assert_eq!(d.leb().unwrap(), size);
            assert_eq!(d.leb().unwrap(), 1);
            let _: i64 = hd_run_wasmtime::internals::codec::Wire::dec(&mut d).unwrap();
        }
        let link = dir.path("link");
        let mut expected = Enc::default();
        expected.leb(0).leb(1).str(&link).leb(2).leb(2);
        crate::fs::test_support::modified(&mut expected, &link);
        assert_eq!(invoke("stat", &link, None).done(), expected.done());
        assert_eq!(invoke("remove", &link, None).done(), &[0]);
        assert!(fs::metadata(dir.path("é")).is_ok());
        let dangling = dir.path("dangling");
        symlink("missing", &dangling).unwrap();
        assert_eq!(
            invoke("create_dir_all", &dangling, None).done(),
            error(&dangling, 0).done()
        );
    }
    #[cfg(target_os = "linux")]
    #[test]
    fn write_errors_keep_the_native_message_and_path() {
        let native = std::io::Error::from_raw_os_error(rustix::io::Errno::NOSPC.raw_os_error());
        assert_eq!(
            invoke("write_text", "/dev/full", Some(b"x")).done(),
            error(&format!("write \"/dev/full\": {native}"), 7).done()
        );
    }
}
