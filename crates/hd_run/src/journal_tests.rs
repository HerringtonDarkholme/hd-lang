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

/// A recorder that wrote every row kind: the journal both this crate's
/// tests and the JS host's (`website/playground/test/journal.test.ts`)
/// check.
fn every_row_kind() -> Recorder {
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
    r
}

/// Every row kind a recorder writes decodes back to what it was given; a
/// method is named once, at its first call; long arguments become a
/// digest.
#[test]
fn recorded_rows_decode_in_order() {
    let r = every_row_kind();
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

/// The JS host's journal (`host/journal.mjs`) is checked against the same
/// bytes in `website/playground/test/journal.test.ts`: the journal of
/// `every_row_kind`, and xxh3-128 (`hd_base::hash128`) of a prefix of each
/// length class of a fixed byte string.
#[test]
fn the_js_host_checks_the_same_bytes() {
    let hex = |b: &[u8]| {
        b.iter().fold(String::new(), |mut s, x| {
            use std::fmt::Write as _;
            let _ = write!(s, "{x:02x}");
            s
        })
    };
    let journal = every_row_kind();
    assert_eq!(hex(journal.journal().expect("recording")), SHARED_JOURNAL);
    let bytes: Vec<u8> = (0..2100u32)
        .map(|i| u8::try_from(i * 31 % 251).expect("a byte"))
        .collect();
    for (len, hash) in SHARED_XXH3 {
        assert_eq!(
            format!("{:032x}", hd_base::hash128(&bytes[..len]).0),
            hash,
            "{len}"
        );
    }
}

const SHARED_JOURNAL: &str = concat!(
    "000011010700000000000000000000000000000042427bb4010121016ff49b92",
    "aa48a5e2dd754a768d92cd38655451d8e889b2b73d5204d3aa0ec02d870f2d11",
    "020216000868643a436c6f636b036e6f77000180d095ffbc31ea9a847a020329",
    "010a68643a436f6e736f6c651077726974655f6c696e652e73746172741ff980",
    "8ebc2a9dd86d0301095567bdba02040d000001ffffffffffffffffff01ffa078",
    "210505020109d4d55f5a040606020209808040a50630f8030705090501010082",
    "bed05c0608010420244a1407090102478c5744080a0103de127569010b2102db",
    "1edfbd0872f61b8e9b61920d252825655451d8e889b2b73d5204d3aa0ec02dd6",
    "a29313020c2a020768643a7072696d0a666f726d61745f663634080000000000",
    "00044006000000000000e03f03302e35e9c75caf080d0200006fb0bc07",
);

const SHARED_XXH3: [(usize, &str); 25] = [
    (0, "99aa06d3014798d86001c324468d497f"),
    (1, "a6cd5e9392000f6ac44bdff4074eecdb"),
    (2, "e3e3af28e278815150a0dc43c774eb84"),
    (3, "4af3603b5bd30dfe3698b80191e625f9"),
    (4, "11527735d62fce43eecdfa5a3b083ba8"),
    (7, "969f5114845f170c0624d295fe95cc20"),
    (8, "791e22cdc30880ba073a25812fe4f600"),
    (9, "22775b5e55c2ace94557529981ebbef0"),
    (16, "4ad789329416e4387df51ed58f26bee2"),
    (17, "950dda39877c26d1ff1aad989e0dd9f2"),
    (32, "ca5f929f445a49b8b88ffeebd235e209"),
    (33, "a9f9da0aaab1a6d38b0cd55037ee0dea"),
    (64, "3a648b5f5e597066a09efb35b93b4dd4"),
    (65, "10723f3d271e6ae65aa37a76af0a6bc0"),
    (96, "ecaf417d05d3e773d6e1529784e4567e"),
    (97, "64f1cf1beb24c0d4ea2789b946d00e9d"),
    (128, "2420677499e7498929b103324f4df782"),
    (129, "385edf5918e02b86a1a1b3d61e7a4029"),
    (200, "cab4b6c1d7f66c432a1a7281f2883dff"),
    (240, "7cf77784002208a5423167d7599e74a5"),
    (241, "095d0994f2a5191d82d3ebf50a3e9c42"),
    (1024, "5f6ea7ec3ae785ebcbdff56b050f2c1c"),
    (1025, "4e0bf1adc121b7df9babf50235cea0a0"),
    (2048, "3af1daea3bdb2821eef46c3d948c544e"),
    (2100, "99f2fb8295d819cdc04609bcfd9a9cd2"),
];
