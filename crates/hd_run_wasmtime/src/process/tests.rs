use super::native::{Output, Request, Start, finish, start};
use crate::abi::Command;
use crate::codec::{Enc, Wire};
use crate::runtime::reactor::{Done, Reactor, Work};
use hd_run::{Grants, Limit};
use std::io;

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

/// A request for `name` with `argv` and `input`.
fn request(name: &str, argv: &[String], input: &[u8]) -> Request {
    Request {
        name: name.into(),
        argv: argv.to_vec(),
        input: input.to_vec(),
        env: Vec::new(),
        cwd: None,
    }
}

/// The encoded arguments of `Process.run` for `name` and `argv`, with no
/// environment pairs, directory or input.
fn call(name: &str, argv: &[&str]) -> Vec<u8> {
    let command = Command {
        program: name.into(),
        args: argv.iter().map(|arg| (*arg).to_owned()).collect(),
        env: Vec::new(),
        current_dir: None,
        stdin: String::new(),
    };
    let mut e = Enc::default();
    command.enc(&mut e);
    e.done().to_vec()
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
fn ran(done: Done) -> super::native::Ran {
    match done {
        Done::Process(ran) => ran,
        _ => panic!("the operation is not a run"),
    }
}

/// Runs `request` to its end through the reactor.
fn run(request: Request) -> io::Result<Output> {
    let mut reactor = Reactor::new(None);
    let h = offload(&mut reactor, request);
    ran(finished(&mut reactor, h)).into_result()
}

/// `run.start` then, when it runs a child, `run.finish`: the encoded result.
fn process(bytes: &[u8], grants: &Grants) -> Vec<u8> {
    match start(bytes, grants, None).expect("the arguments decode") {
        Start::Refused(e) => e.into_bytes(),
        Start::Run(request) => {
            let mut reactor = Reactor::new(None);
            let h = offload(&mut reactor, request);
            finish(ran(finished(&mut reactor, h))).into_bytes()
        }
    }
}

#[test]
fn process_grant_precedes_spawn_and_missing_program_is_not_found() {
    let args = call("hd-no-such-program-for-process-test", &[]);
    let grants = Grants {
        limits: vec![("Process".into(), Limit::Entries(vec![]))],
    };
    assert_eq!(process(&args, &grants), [1, 2]);
    let mut missing = Enc::default();
    missing
        .leb(1)
        .leb(0)
        .str("hd-no-such-program-for-process-test");
    assert_eq!(process(&args, &Grants::default()), missing.done().to_vec());
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
        let args = call(name, &argv);
        let mut error = Enc::default();
        error.leb(1).leb(3).str(expected);
        assert_eq!(process(&args, &Grants::default()), error.done().to_vec());
        let denied = Grants {
            limits: vec![("Process".into(), Limit::Deny)],
        };
        assert_eq!(process(&args, &denied), [1, 2]);
    }
    assert_eq!(process(&call("", &[]), &Grants::default()), [1, 0, 0]);
}

/// A command with an environment name or value, or a directory, that a
/// child cannot take is refused with a typed error, and a directory that
/// does not exist is not read as a missing program.
#[test]
fn process_invalid_environment_and_directory_are_typed_errors() {
    let other = |message: &str| {
        let mut error = Enc::default();
        error.leb(1).leb(3).str(message);
        error.done().to_vec()
    };
    let with = |env: &[(&str, &str)], dir: Option<&str>| {
        let command = Command {
            program: "sh".into(),
            args: Vec::new(),
            env: env
                .iter()
                .map(|(name, value)| ((*name).to_owned(), (*value).to_owned()))
                .collect(),
            current_dir: dir.map(|dir| crate::abi::Path(dir.into())),
            stdin: String::new(),
        };
        let mut e = Enc::default();
        command.enc(&mut e);
        process(e.done(), &Grants::default())
    };
    assert_eq!(
        with(&[("A", "1"), ("B=C", "2")], None),
        other("process environment name 1 is empty or holds = or a NUL byte")
    );
    assert_eq!(
        with(&[("", "1")], None),
        other("process environment name 0 is empty or holds = or a NUL byte")
    );
    assert_eq!(
        with(&[("A", "a\0b")], None),
        other("process environment value 0 contains a NUL byte")
    );
    assert_eq!(
        with(&[], Some("a\0b")),
        other("working directory contains a NUL byte")
    );
    assert_eq!(
        with(&[], Some("hd-no-such-directory-for-process-test")),
        other("working directory not found: hd-no-such-directory-for-process-test")
    );
}
