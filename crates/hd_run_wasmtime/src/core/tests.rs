use super::process::{Output, Request, Start, finish, start};
use super::{random, sys_read};
use crate::codec::{Enc, Wire};
use crate::limits::{Ledger, Reservation};
use crate::reactor::{Done, Reactor, Work};
use hd_run::{Grants, Limit};
use std::io;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// A request for `name` with `argv` and `input`, under a heap budget of
/// `limit` bytes for its output.
fn request(name: &str, argv: &[String], input: &[u8], limit: u64) -> Request {
    let ledger = Ledger::new(limit);
    Request {
        name: name.into(),
        argv: argv.to_vec(),
        input: input.to_vec(),
        cwd: None,
        held: Reservation::empty(&ledger),
        ledger,
    }
}

/// Offloads the run of `request` onto the `hd-io` runtime, as `.start`
/// does, and returns the operation's handle.
fn offload(reactor: &mut Reactor, request: Request) -> i32 {
    reactor.offload(request.run()).expect("offloads")
}

/// Waits for the operation `h` to complete and takes its result, as
/// `block` and `.finish` do.
fn finished(reactor: &mut Reactor, h: i32) -> Done {
    let mut out = Vec::new();
    // An earlier `due` may already have completed `h` with another handle.
    while !reactor.is_completed(h) && !reactor.due(&mut out).contains(&h) {
        assert!(reactor.wait(), "the run is pending");
    }
    match reactor.remove(h).map(|op| op.work) {
        Some(Work::Offloaded(Some(done))) => done,
        _ => panic!("the run has no result"),
    }
}

/// The run of a finished operation.
fn ran(done: Done) -> super::process::Ran {
    match done {
        Done::Process(ran) => ran,
        Done::Exhausted => panic!("the output passed the heap budget"),
        _ => panic!("the operation is not a run"),
    }
}

/// Runs `request` to its end through the reactor.
fn run(request: Request) -> io::Result<Output> {
    let mut reactor = Reactor::new(None);
    let h = offload(&mut reactor, request);
    ran(finished(&mut reactor, h)).into_result()
}

/// Whether `request`'s output passes its heap budget.
#[cfg(unix)]
fn exhausts(request: Request) -> bool {
    let mut reactor = Reactor::new(None);
    let h = offload(&mut reactor, request);
    matches!(finished(&mut reactor, h), Done::Exhausted)
}

/// `run.start` then, when it runs a child, `run.finish`: the encoded result.
fn process(bytes: &[u8], grants: &Grants) -> Vec<u8> {
    match start(bytes, grants, None, &Ledger::new(u64::MAX)).expect("the arguments decode") {
        Start::Refused(e) => e.into_bytes(),
        Start::Run(request) => {
            let mut reactor = Reactor::new(None);
            let h = offload(&mut reactor, request);
            finish(ran(finished(&mut reactor, h))).into_bytes()
        }
    }
}

#[test]
fn random_returns_the_requested_number_of_bytes() {
    assert!(random(0).unwrap().is_empty());
    assert_eq!(random(8).unwrap().len(), 8);
    assert_eq!(random(1024).unwrap().len(), 1024);
}

#[test]
fn sys_grant_is_checked_before_reading_and_failure_is_unsupported() {
    let grants = Grants {
        limits: vec![("Sys".into(), Limit::Entries(vec!["os".into()]))],
    };
    let denied = sys_read::<String>("hostname", &grants, || panic!("denied read ran"));
    let mut expected = Enc::default();
    expected.leb(1).leb(0).str("hostname");
    assert_eq!(denied.done(), expected.done());

    let unsupported = sys_read::<String>("hostname", &Grants::default(), || None);
    let mut expected = Enc::default();
    expected.leb(1).leb(1).str("hostname");
    assert_eq!(unsupported.done(), expected.done());
}

#[test]
fn process_grant_precedes_spawn_and_missing_program_is_not_found() {
    let mut args = Enc::default();
    args.str("hd-no-such-program-for-process-test")
        .leb(0)
        .bytes(&[]);
    let grants = Grants {
        limits: vec![("Process".into(), Limit::Entries(vec![]))],
    };
    assert_eq!(process(args.done(), &grants), [1, 2]);
    assert_eq!(process(args.done(), &Grants::default()), [1, 0]);
}

#[test]
fn process_invalid_names_and_arguments_are_typed_errors() {
    for (name, argv, expected) in [
        ("a\0b", vec![], "program name contains a NUL byte"),
        (
            "sh",
            vec!["first", "a\0b"],
            "process argument 1 contains a NUL byte",
        ),
    ] {
        let mut args = Enc::default();
        args.str(name);
        argv.iter()
            .map(|arg| (*arg).to_owned())
            .collect::<Vec<String>>()
            .enc(&mut args);
        args.bytes(&[]);
        let mut error = Enc::default();
        error.leb(1).leb(3).str(expected);
        assert_eq!(
            process(args.done(), &Grants::default()),
            error.done().to_vec()
        );
        let denied = Grants {
            limits: vec![("Process".into(), Limit::Deny)],
        };
        assert_eq!(process(args.done(), &denied), [1, 2]);
    }
    let mut args = Enc::default();
    args.str("").leb(0).bytes(&[]);
    assert_eq!(process(args.done(), &Grants::default()), [1, 0]);
}
