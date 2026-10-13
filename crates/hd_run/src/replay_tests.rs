use super::{Replay, recorded};
use crate::journal::{End, Recorder, Value, put_int_arg};

/// A journal of two inputs: a clock reading and a written line, then a
/// stopped input whose record ends with its `End` row.
fn journal() -> Vec<u8> {
    let mut r = Recorder::new(1 << 20, 9);
    r.input(1, b"a := now()\n", b"\0asm1");
    r.call(0, "hd:Clock", "now", &[], Value::Int(1_700), None);
    let mut line = Vec::new();
    put_int_arg(&mut line, 2);
    line.extend_from_slice(b"hi");
    r.call(
        1,
        "hd:Console",
        "write_line.start",
        &line,
        Value::Pair(1, 4),
        None,
    );
    r.poll();
    r.wake(&[4]);
    r.finish(4, Value::Int(1), Some(&[0]));
    r.end(End::Exit(0));
    r.input(2, b"while true: pass\n", b"\0asm2");
    r.end(End::Stopped);
    r.journal().expect("recording").to_vec()
}

#[test]
fn a_journal_reads_back_as_its_inputs() {
    let rec = recorded(&journal()).expect("reads");
    assert_eq!(rec.execution, 9);
    assert_eq!(rec.inputs.len(), 2);
    assert_eq!(rec.inputs[0].number, 1);
    assert_eq!(rec.inputs[0].source, hd_base::hash128(b"a := now()\n").0);
    assert_eq!(rec.inputs[0].events.len(), 4);
    assert_eq!(rec.inputs[0].end, Some(End::Exit(0)));
    assert_eq!(rec.inputs[1].events, []);
    assert_eq!(rec.inputs[1].end, Some(End::Stopped));
    // A torn tail cuts the last input's end.
    let bytes = journal();
    let cut = recorded(&bytes[..bytes.len() - 3]).expect("reads");
    assert_eq!(cut.inputs[1].end, None);
}

/// The same requests, in order, get the recorded replies; the input then
/// ends as before.
#[test]
fn a_replay_serves_the_recorded_replies() {
    let rec = recorded(&journal()).expect("reads");
    let mut replay = Replay::new(&rec.inputs[0]);
    assert_eq!(
        replay.call("hd:Clock", "now", &[]),
        Ok((Value::Int(1_700), None))
    );
    let mut line = Vec::new();
    put_int_arg(&mut line, 2);
    line.extend_from_slice(b"hi");
    assert_eq!(
        replay.call("hd:Console", "write_line.start", &line),
        Ok((Value::Pair(1, 4), None))
    );
    replay.poll();
    assert!(replay.wake_next());
    assert_eq!(replay.wake(), Ok(vec![4]));
    assert_eq!(replay.finish(4), Ok((Value::Int(1), Some(vec![0]))));
    assert_eq!(replay.end(End::Exit(0), rec.inputs[0].end), Ok(()));
    assert_eq!(replay.diverged(), None);
}

/// Another method, other arguments, an early end or another end diverge;
/// the first divergence is the one kept.
#[test]
fn a_request_that_does_not_match_diverges() {
    let rec = recorded(&journal()).expect("reads");
    let input = &rec.inputs[0];
    let mut replay = Replay::new(input);
    assert_eq!(
        replay.call("hd:Clock", "monotonic", &[]),
        Err("input 1 called hd:Clock monotonic, but the record has a call of hd:Clock now".into())
    );
    assert!(replay.call("hd:Clock", "now", &[]).is_ok());
    assert_eq!(
        replay.end(End::Exit(0), input.end),
        Err("input 1 called hd:Clock monotonic, but the record has a call of hd:Clock now".into())
    );

    let mut replay = Replay::new(input);
    assert!(replay.call("hd:Clock", "now", &[]).is_ok());
    let mut other = Vec::new();
    put_int_arg(&mut other, 2);
    other.extend_from_slice(b"ho");
    assert_eq!(
        replay.call("hd:Console", "write_line.start", &other),
        Err("input 1 called hd:Console write_line.start with other arguments than before".into())
    );

    let mut replay = Replay::new(input);
    assert!(replay.call("hd:Clock", "now", &[]).is_ok());
    assert_eq!(
        replay.end(End::Exit(0), input.end),
        Err("input 1 ended early, but the record has a call of hd:Console write_line.start".into())
    );

    let mut replay = Replay::new(&rec.inputs[1]);
    assert_eq!(
        replay.end(End::Trapped, Some(End::Exit(0))),
        Err("input 2 panicked, but before it ended".into())
    );
}
