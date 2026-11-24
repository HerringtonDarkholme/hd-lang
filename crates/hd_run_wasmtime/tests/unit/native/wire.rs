mod tests {
    use hd_run_wasmtime::internals::native::handles::HandlesExt;
    use hd_run_wasmtime::internals::native::wire::{Decoder, Encoder, Serde, answer, fail, text};
    use hd_run_wasmtime::internals::native::{
        Answer, Dispatched, Failure, Handles, HostError, HostResult, NativeCall, Suspension,
        SuspensionExt, pending,
    };

    fn message<T: std::fmt::Debug>(result: Result<T, HostError>) -> String {
        result.unwrap_err().0
    }

    /// `text`'s token stream as the guest would pass it: a length-prefixed
    /// string.
    fn tokens(text: &str) -> Vec<u8> {
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.string(text);
        e.into_bytes()
    }

    fn read(text: &str) -> Result<Serde, HostError> {
        Decoder::new(&tokens(text)).serde()
    }

    fn written(value: &Serde) -> Result<String, HostError> {
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.serde(value)?;
        let bytes = e.into_bytes();
        Ok(Decoder::new(&bytes).string().unwrap())
    }

    #[test]
    fn a_token_stream_reads_into_the_typed_tree_and_back() {
        let stream = r#"[["o",3],["s","a\"\\\/\b\f\n\r\té😀"],["a",2],["i","-9223372036854775808"],["u","18446744073709551615"],["s","k"],["f","8000000000000000"],["s","m"],["o",0]]"#;
        let tree = Serde::Map(vec![
            (
                Serde::String("a\"\\/\u{8}\u{c}\n\r\té😀".into()),
                Serde::Seq(vec![Serde::I64(i64::MIN), Serde::U64(u64::MAX)]),
            ),
            (Serde::String("k".into()), Serde::F64(-0.0)),
            (Serde::String("m".into()), Serde::Map(vec![])),
        ]);
        let value = read(stream).unwrap();
        assert_eq!(value, tree);
        let Serde::Map(entries) = &value else {
            unreachable!()
        };
        assert!(matches!(entries[1].1, Serde::F64(x) if x.to_bits() == (-0.0_f64).to_bits()));
        // Whitespace between tokens is JSON's.
        assert_eq!(read(" [ [ \"n\" ] ]\n").unwrap(), Serde::None);
        assert_eq!(read(r#"[["b",false]]"#).unwrap(), Serde::Bool(false));
        assert_eq!(read(r#"[["a",0]]"#).unwrap(), Serde::Seq(vec![]));
        // What it writes reads back as the same tree.
        let text = written(&tree).unwrap();
        assert_eq!(read(&text).unwrap(), tree);
    }

    #[test]
    fn a_provider_tree_writes_as_the_js_host_writes_it() {
        let value = Serde::Seq(vec![
            Serde::Unit,
            Serde::Some(Box::new(Serde::Char('x'))),
            Serde::F32(1.5),
            Serde::F64(f64::NAN),
            Serde::F32(f32::INFINITY),
            Serde::Bytes(vec![0, 255]),
            Serde::I64(5),
            Serde::String("\u{1}".into()),
            Serde::Map(vec![(Serde::Char('k'), Serde::Bool(true))]),
        ]);
        assert_eq!(
            written(&value).unwrap(),
            r#"[["a",9],["n"],["s","x"],["f","3ff8000000000000"],["n"],["n"],["a",2],["u","0"],["u","255"],["i","5"],["s","\u0001"],["o",1],["s","k"],["b",true]]"#
        );
        for keys in [
            vec![(Serde::I64(1), Serde::Unit)],
            vec![
                (Serde::String("k".into()), Serde::Unit),
                (Serde::Char('k'), Serde::Unit),
            ],
        ] {
            assert_eq!(
                message(written(&Serde::Map(keys))),
                "package host: invalid private object key"
            );
        }
    }

    #[test]
    fn a_malformed_token_stream_fails_with_the_js_hosts_message() {
        for (stream, expected) in [
            ("", "invalid private value transport"),
            ("{}", "invalid private value transport"),
            (r#"[["n"]"#, "invalid private value transport"),
            (r#"[["n"]]x"#, "invalid private value transport"),
            (r#"[["n"] ["n"]]"#, "invalid private value transport"),
            ("[]", "invalid private value framing"),
            (r#"[["n"],["n"]]"#, "invalid private value framing"),
            (r#"[["a",2],["n"]]"#, "invalid private container length"),
            ("[1]", "invalid private value token"),
            (r#"[["n",1]]"#, "invalid private value token"),
            (r#"[["x",1]]"#, "invalid private value token"),
            (r#"[["b","true"]]"#, "invalid private value token"),
            (r#"[["s",1]]"#, "invalid private value token"),
            (r#"[["a",-1]]"#, "invalid private value token"),
            (r#"[["a",01]]"#, "invalid private value token"),
            (r#"[["a",9007199254740992]]"#, "invalid private value token"),
            (r#"[["s","\x"]]"#, "invalid private value token"),
            (r#"[["s","\ud800"]]"#, "invalid private value token"),
            (r#"[["s","\ud800A"]]"#, "invalid private value token"),
            (r#"[["s","\u00g1"]]"#, "invalid private value token"),
            ("[[\"s\",\"\u{1}\"]]", "invalid private value token"),
            (r#"[["s","open"#, "invalid private value token"),
            (r#"[["i","-0"]]"#, "invalid private numeric token"),
            (r#"[["i","01"]]"#, "invalid private numeric token"),
            (r#"[["i",""]]"#, "invalid private numeric token"),
            (r#"[["u","1x"]]"#, "invalid private numeric token"),
            (
                r#"[["f","3FF8000000000000"]]"#,
                "invalid private numeric token",
            ),
            (r#"[["f","00"]]"#, "invalid private numeric token"),
            (
                r#"[["i","9223372036854775808"]]"#,
                "private signed integer out of range",
            ),
            (r#"[["u","-1"]]"#, "private unsigned integer out of range"),
            (r#"[["f","7ff0000000000000"]]"#, "invalid private float"),
            (r#"[["o",1],["n"],["n"]]"#, "invalid private object key"),
            (
                r#"[["o",2],["s","k"],["n"],["s","k"],["n"]]"#,
                "invalid private object key",
            ),
        ] {
            assert_eq!(
                message(read(stream)),
                format!("package host: {expected}"),
                "{stream}"
            );
        }
        // A truncated string is the wire's own failure.
        assert_eq!(
            message(Decoder::new(&[5, b'[']).serde()),
            "an argument does not decode"
        );
    }

    #[test]
    fn each_scalar_is_checked_as_the_js_host_checks_it() {
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.signed(300);
        e.unsigned(300);
        e.unsigned(0xd800);
        e.unsigned(u64::from(u32::MAX) + 1);
        e.unsigned(u64::from(u32::MAX) + 1);
        e.tag(1);
        e.unsigned(u64::from(u32::MAX));
        let bytes = e.into_bytes();
        let mut d = Decoder::new(&bytes);
        assert_eq!(
            message(d.signed::<i8>()),
            "package host: integer is out of range"
        );
        assert_eq!(
            message(d.unsigned::<u8>()),
            "package host: integer is out of range"
        );
        assert_eq!(message(d.char()), "package host: invalid char");
        assert_eq!(message(d.list_len()), "package host: invalid list length");
        assert_eq!(message(d.map_len()), "package host: invalid map size");
        assert_eq!(message(d.absent::<()>()), "an argument does not decode");
        assert_eq!(d.list_len().unwrap(), u32::MAX as usize);
        assert_eq!(
            d.capacity(u32::MAX as usize),
            0,
            "a forged count reserves nothing"
        );
        assert!(d.finish().is_ok());
        for fail in [message(d.f32()), message(d.f64())] {
            assert_eq!(fail, "package host: truncated float");
        }
        for fail in [
            message(d.bool()),
            message(d.signed::<i64>()),
            message(d.unsigned::<u64>()),
            message(d.char()),
            message(d.string()),
            message(d.bytes()),
            message(d.map_len()),
        ] {
            assert_eq!(fail, "an argument does not decode");
        }
        assert_eq!(
            message(d.handle::<u8>()),
            "package host: a handle cannot be passed as a data argument"
        );
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.bool(true);
        e.char('é');
        e.f32(-0.0);
        e.f64(1.0);
        e.bytes(&[9]);
        let bytes = e.into_bytes();
        let mut d = Decoder::new(&bytes);
        assert!(d.bool().unwrap());
        assert_eq!(d.char().unwrap(), 'é');
        assert_eq!(d.f32().unwrap().to_bits(), (-0.0_f32).to_bits());
        assert_eq!(d.f64().unwrap().to_bits(), 1.0_f64.to_bits());
        assert_eq!(d.bytes().unwrap(), [9]);
        assert_eq!(text(&[b'h', 0xff]), "h\u{fffd}");
    }

    #[test]
    fn a_suspending_method_answers_ready_or_pending() {
        let encode: fn(u8, &mut Handles) -> HostResult<Vec<u8>> = |value, _| Ok(vec![value]);
        let h = &mut Handles::default();
        let Ok(Answer::Ready(bytes)) = answer(NativeCall::Ready(Ok(4)), encode, h) else {
            panic!("a ready value is encoded at once")
        };
        assert_eq!(bytes, [4]);
        // The provider's own error is its throw; the codec's is the host's.
        let failed = answer(NativeCall::Ready(Err(HostError("no".into()))), encode, h);
        assert_eq!(failed.err(), Some(Failure::Threw(HostError("no".into()))));
        let refuse: fn(u8, &mut Handles) -> HostResult<Vec<u8>> = |_, _| Err(fail("codec"));
        let failed = answer(NativeCall::Ready(Ok(4)), refuse, h);
        assert_eq!(failed.err(), Some(Failure::Host(fail("codec"))));
        let aborted = std::rc::Rc::new(std::cell::Cell::new(0));
        let hook = std::rc::Rc::clone(&aborted);
        let (call, _completion) = pending::<u8>(move || hook.set(hook.get() + 1));
        let Ok(Answer::Pending(suspension)) = answer(NativeCall::Pending(call), encode, h) else {
            panic!("a pending call stays pending")
        };
        assert_eq!(aborted.get(), 0);
        drop(suspension);
        assert_eq!(aborted.get(), 1, "dropped undelivered, it is aborted");
    }

    #[test]
    fn a_pending_calls_own_error_is_its_rejection_and_its_codecs_the_hosts() {
        let encode: fn(u8, &mut Handles) -> HostResult<Vec<u8>> = |value, _| {
            if value == 0 {
                Err(fail("codec"))
            } else {
                Ok(vec![value])
            }
        };
        let finished = |value: HostResult<u8>| {
            let (call, completion) = pending::<u8>(|| {});
            completion.complete(value);
            Suspension::new(call, encode).finish(&mut Handles::default())
        };
        assert_eq!(finished(Ok(4)), Some(Ok(vec![4])));
        assert_eq!(
            finished(Err(HostError("no".into()))),
            Some(Err(Failure::Rejected(HostError("no".into()))))
        );
        assert_eq!(finished(Ok(0)), Some(Err(Failure::Host(fail("codec")))));
        // Each kind prints the JS host's text for it.
        let label = "p/m.T.wait";
        assert_eq!(
            Failure::Threw(HostError("no".into())).message(label),
            "p/m.T.wait: provider threw no"
        );
        assert_eq!(
            Failure::Rejected(HostError("no".into())).message(label),
            "package host: provider p/m.T.wait rejected: no"
        );
        assert_eq!(
            Failure::Host(fail("codec")).message(label),
            "p/m.T.wait: package host: codec"
        );
    }

    #[allow(clippy::unnecessary_wraps)]
    fn none(_: &mut dyn std::any::Any, _: usize, _: &[u8], _: &mut Handles) -> Dispatched {
        Ok(Answer::Ready(Vec::new()))
    }

    /// Drops count their value's drops.
    struct Counted(std::rc::Rc<std::cell::Cell<u32>>);

    impl Drop for Counted {
        fn drop(&mut self) {
            self.0.set(self.0.get() + 1);
        }
    }

    #[test]
    fn a_handle_moves_into_the_registry_and_a_failed_encode_releases_it() {
        let drops = std::rc::Rc::new(std::cell::Cell::new(0));
        let mut h = Handles::default();
        let mut e = Encoder::new(&mut h);
        e.handle(Box::new(Counted(drops.clone())), 2, none).unwrap();
        let bytes = e.into_bytes();
        let number = Decoder::new(&bytes).unsigned::<u32>().unwrap();
        assert_eq!(
            (h.live(), drops.get()),
            (1, 0),
            "a written handle stays live"
        );
        // A value that fails after a handle: the guest never sees its
        // number, and the trait object drops with no provider call.
        let mut e = Encoder::new(&mut h);
        e.handle(Box::new(Counted(drops.clone())), 2, none).unwrap();
        let failed: HostResult<()> = e.serde(&Serde::Map(vec![(Serde::Bool(true), Serde::Unit)]));
        assert!(failed.is_err());
        drop(e);
        assert_eq!((h.live(), drops.get()), (1, 1));
        h.release(number, 2).unwrap();
        assert_eq!((h.live(), drops.get()), (0, 2));
    }
}
