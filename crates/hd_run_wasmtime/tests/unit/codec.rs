mod tests {
    use hd_run_wasmtime::internals::abi::{
        ConsoleError, ConsoleStream, Entry, EntryKind, FsError, HostError, NetError, Path,
    };
    use hd_run_wasmtime::internals::codec::{Dec, Enc, Handle, Wire, encode};

    fn round_trip<T: Wire + PartialEq + std::fmt::Debug>(value: &T, bytes: &[u8]) {
        assert_eq!(encode(value).done(), bytes, "{value:?}");
        let mut d = Dec::new(bytes);
        assert_eq!(&T::dec(&mut d).expect("decodes"), value);
    }

    #[test]
    fn decodes_lengths_strings_and_signed_extremes() {
        let strings: Vec<String> = ["a", "hé", ""].map(String::from).to_vec();
        assert_eq!(
            Vec::<String>::dec(&mut Dec::new(encode(&strings).done())).expect("strings"),
            strings
        );
        for n in [i64::MIN, -1, 0, 1, i64::MAX] {
            let mut e = Enc::default();
            e.zz(n);
            let v = Dec::new(e.done()).leb().expect("zigzag");
            let decoded =
                i64::from_le_bytes((v >> 1).to_le_bytes()) ^ -i64::try_from(v & 1).expect("bit");
            assert_eq!(decoded, n);
            assert_eq!(Dec::new(e.done()).zz().expect("signed integer"), n);
        }
        for bad in [&[128][..], &[3, b'a'][..], &[255; 11][..]] {
            let err = Dec::new(bad).bytes().expect_err("truncated arguments");
            assert_eq!(err.to_string(), "an argument does not decode");
        }
    }

    #[test]
    fn encodes_as_the_node_host_does() {
        let mut e = Enc::default();
        e.leb(0).leb(300).str("hé");
        vec!["a".to_owned(), "bc".to_owned()].enc(&mut e);
        assert_eq!(
            e.done(),
            [
                0, 0xac, 0x02, 3, b'h', 0xc3, 0xa9, 2, 1, b'a', 2, b'b', b'c'
            ]
        );
    }

    /// The language's own types, as host/wire.ts encodes them.
    #[test]
    fn the_language_types_cross_as_the_js_host_encodes_them() {
        round_trip(&(), &[]);
        round_trip(&300_u16, &[0xac, 0x02]);
        round_trip(&-2_i32, &[3]);
        round_trip(
            &u64::MAX,
            &[0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 1],
        );
        // `List[u8]` is its length and its bytes; any other list its count
        // and its elements.
        round_trip(&vec![200_u8, 1], &[2, 200, 1]);
        round_trip(&vec![200_u16, 1], &[2, 0xc8, 1, 1]);
        round_trip(&None::<String>, &[0]);
        round_trip(&Some("x".to_owned()), &[1, 1, b'x']);
        round_trip(&Ok::<u32, String>(5), &[0, 5]);
        round_trip(&Err::<u32, String>("e".into()), &[1, 1, b'e']);
        round_trip(&("a".to_owned(), "b".to_owned()), &[1, b'a', 1, b'b']);
        round_trip(&Handle(7), &[7]);
        for bad in [&[2][..], &[0x80, 1][..]] {
            let err = Option::<u32>::dec(&mut Dec::new(bad)).expect_err("a tag past the end");
            assert_eq!(err.to_string(), "an argument does not decode");
        }
        let err = u16::dec(&mut Dec::new(&[0x80, 0x80, 4])).expect_err("past u16");
        assert_eq!(err.to_string(), "an argument does not decode");
    }

    /// The generated std types (`abi.rs`): data as its fields in order, an
    /// enum as its variant index then its payload, a newtype as its field.
    #[test]
    fn the_generated_std_types_cross_as_the_js_host_encodes_them() {
        round_trip(&Path("p".into()), &[1, b'p']);
        round_trip(
            &Entry {
                path: Path("p".into()),
                kind: EntryKind::Symlink,
                size: 300,
                modified: None,
            },
            &[1, b'p', 2, 0xac, 0x02, 0],
        );
        round_trip(
            &FsError::InvalidUtf8 {
                path: Path("p".into()),
            },
            &[6, 1, b'p'],
        );
        round_trip(
            &FsError::Other {
                message: "m".into(),
            },
            &[7, 1, b'm'],
        );
        round_trip(
            &Entry {
                path: Path("t".into()),
                kind: EntryKind::File,
                size: 3,
                modified: Some(hd_run_wasmtime::internals::abi::Timestamp { millis: -1 }),
            },
            &[1, b't', 0, 3, 1, 1],
        );
        round_trip(
            &hd_run_wasmtime::internals::abi::ProcessError::Exited {
                output: hd_run_wasmtime::internals::abi::ProcessOutput {
                    stdout: "o".into(),
                    stderr: "e".into(),
                    status: 7,
                    program: "tool".into(),
                },
            },
            &[4, 1, b'o', 1, b'e', 14, 4, b't', b'o', b'o', b'l'],
        );
        round_trip(
            &HostError::InvalidValue {
                name: "PORT".into(),
                reason: "bad".into(),
            },
            &[3, 4, b'P', b'O', b'R', b'T', 3, b'b', b'a', b'd'],
        );
        round_trip(
            &HostError::Missing {
                name: "PORT".into(),
            },
            &[4, 4, b'P', b'O', b'R', b'T'],
        );
        round_trip(&NetError::Closed, &[5]);
        round_trip(
            &ConsoleError::NotGranted {
                stream: ConsoleStream::Stdout,
            },
            &[1, 0],
        );
        round_trip(&ConsoleError::Other { code: -5 }, &[2, 9]);
        let err = EntryKind::dec(&mut Dec::new(&[3])).expect_err("no fourth kind");
        assert_eq!(err.to_string(), "an argument does not decode");
        let err = ConsoleError::dec(&mut Dec::new(&[1, 3]))
            .expect_err("NotGranted needs a valid stream tag");
        assert_eq!(err.to_string(), "an argument does not decode");
        let err =
            ConsoleError::dec(&mut Dec::new(&[2])).expect_err("Other needs a signed error code");
        assert_eq!(err.to_string(), "an argument does not decode");
    }

    #[test]
    fn float_encoding_preserves_ieee_payload_and_negative_zero() {
        for bits in [0x7ff8_0000_0000_0001, 0x7ff0_0000_0000_0000, 1 << 63] {
            let mut encoded = Enc::default();
            encoded.f64(f64::from_bits(bits));
            assert_eq!(encoded.done(), bits.to_le_bytes());
        }
    }
}
