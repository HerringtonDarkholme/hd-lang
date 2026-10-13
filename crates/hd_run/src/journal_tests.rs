use super::{
    End, Event, Recorder, Request, Row, RowKind, Value, append, crc32, decode, put_float_arg,
    put_int_arg, read,
};

#[test]
fn rows_round_trip_and_a_torn_tail_is_cut() {
    let mut j = Vec::new();
    let rows = [
        Row {
            kind: RowKind::Header,
            index: 0,
            payload: b"fmt1".to_vec(),
        },
        Row {
            kind: RowKind::Call,
            index: 1,
            payload: vec![7; 300],
        },
        Row {
            kind: RowKind::End,
            index: 2,
            payload: vec![],
        },
    ];
    for r in &rows {
        append(&mut j, r);
    }
    let (back, valid) = read(&j);
    assert_eq!(back, rows);
    assert_eq!(valid, j.len());
    let torn = &j[..j.len() - 2];
    let (back, valid) = read(torn);
    assert_eq!(back.len(), 2);
    assert!(valid < torn.len());
    assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    assert_eq!(crc32(b""), 0);
}

/// Every row kind a recorder writes decodes back to what it was given; a
/// method is named once, at its first call; long arguments become a
/// digest.
#[test]
fn recorded_rows_decode_in_order() {
    let mut r = Recorder::new(1 << 20, 7);
    r.input(1, b"now()\n", b"\0asm");
    r.call(
        3,
        "hd:Clock",
        "now",
        &[],
        Value::Int(1_700_000_000_000),
        None,
    );
    let mut line = Vec::new();
    put_int_arg(&mut line, 5);
    line.extend_from_slice(b"a line longer than eight bytes");
    r.call(
        0,
        "hd:Console",
        "write_line.start",
        &line,
        Value::Pair(1, 9),
        None,
    );
    r.call(3, "hd:Clock", "now", &[], Value::Int(-1), None);
    r.block(&[9]);
    r.poll();
    r.poll();
    r.wake(&[9, 1 << 20]);
    r.finish(9, Value::Int(1), Some(&[0]));
    r.abort(4);
    r.cancel();
    r.end(End::Cancelled);
    r.input(2, b"x\n", b"\0asm");
    let mut half = Vec::new();
    put_float_arg(&mut half, 2.5f64.to_bits());
    r.call(
        1,
        "hd:prim",
        "format_f64",
        &half,
        Value::F64(0.5f64.to_bits()),
        Some(b"0.5"),
    );
    r.end(End::Exit(0));
    let (rows, valid) = read(r.journal().expect("recording"));
    assert_eq!(valid, r.journal().expect("recording").len());
    let events = decode(&rows).expect("decodes");
    let digest =
        super::request_digest(&[[5].as_slice(), b"a line longer than eight bytes"].concat());
    let source = hd_base::hash128(b"now()\n").0;
    let module = hd_base::hash128(b"\0asm").0;
    let call = |module: &str, name: &str, request, value, result| Event::Call {
        module: module.into(),
        name: name.into(),
        request,
        value,
        result,
    };
    assert_eq!(
        events,
        [
            Event::Header {
                format: super::FORMAT,
                execution: 7
            },
            Event::Input {
                number: 1,
                source,
                module
            },
            call(
                "hd:Clock",
                "now",
                Request::Args(vec![]),
                Value::Int(1_700_000_000_000),
                None
            ),
            call(
                "hd:Console",
                "write_line.start",
                Request::Digest { len: 31, digest },
                Value::Pair(1, 9),
                None
            ),
            call(
                "hd:Clock",
                "now",
                Request::Args(vec![]),
                Value::Int(-1),
                None
            ),
            Event::Block { handles: vec![9] },
            Event::Wake {
                poll: 2,
                handles: vec![9, 1 << 20]
            },
            Event::Finish {
                handle: 9,
                value: Value::Int(1),
                result: Some(vec![0])
            },
            Event::Abort { handle: 4 },
            Event::Cancel { poll: 2 },
            Event::End(End::Cancelled),
            Event::Input {
                number: 2,
                source: hd_base::hash128(b"x\n").0,
                module
            },
            call(
                "hd:prim",
                "format_f64",
                Request::Args(2.5f64.to_bits().to_le_bytes().to_vec()),
                Value::F64(0.5f64.to_bits()),
                Some(b"0.5".to_vec())
            ),
            Event::End(End::Exit(0)),
        ]
    );
}

/// A clock read through a method already named: kind, index, length, the
/// payload (method, request length, reply tag, value) and the checksum.
#[test]
fn a_clock_read_row_is_small() {
    let mut r = Recorder::new(1 << 20, 0);
    r.call(0, "hd:Clock", "monotonic", &[], Value::Int(1), None);
    let before = r.journal().expect("recording").len();
    r.call(0, "hd:Clock", "monotonic", &[], Value::Int(5_000), None);
    let row = r.journal().expect("recording").len() - before;
    assert_eq!(row, 1 + 1 + 1 + (1 + 1 + 1 + 2) + 4);
}

/// Past its cap a recorder drops its journal, records nothing more, and
/// gives its notice once.
#[test]
fn past_the_cap_recording_stops_with_one_notice() {
    let mut r = Recorder::new(80, 0);
    assert!(!r.take_notice());
    for _ in 0..3 {
        r.call(0, "hd:Clock", "now", &[], Value::Int(1), None);
    }
    assert!(r.journal().is_some());
    for _ in 0..10 {
        r.call(0, "hd:Clock", "now", &[], Value::Int(1), None);
    }
    assert_eq!(r.journal(), None);
    assert!(r.take_notice());
    assert!(!r.take_notice());
    r.end(End::Exit(0));
    assert_eq!(r.journal(), None);
    assert!(!r.take_notice());
}
