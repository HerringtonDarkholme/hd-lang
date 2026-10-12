use super::{process, random, sys_read};
use crate::codec::Enc;
use hd_run::{Grants, Limit};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

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
    let denied = sys_read("hostname", &grants, |_| panic!("denied read ran"));
    let mut expected = Enc::default();
    expected.leb(1).leb(0).str("hostname");
    assert_eq!(denied.done(), expected.done());

    let unsupported = sys_read("hostname", &Grants::default(), |e| {
        e.str("discard this partial answer");
        false
    });
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
    assert_eq!(process(args.done(), &grants).unwrap().done(), [1, 2]);
    assert_eq!(
        process(args.done(), &Grants::default()).unwrap().done(),
        [1, 0]
    );
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
        args.str(name)
            .list(&argv, |e, arg| {
                e.str(arg);
            })
            .bytes(&[]);
        let mut error = Enc::default();
        error.leb(1).leb(3).str(expected);
        assert_eq!(
            process(args.done(), &Grants::default()).unwrap().done(),
            error.done()
        );
        let denied = Grants {
            limits: vec![("Process".into(), Limit::Deny)],
        };
        assert_eq!(process(args.done(), &denied).unwrap().done(), [1, 2]);
    }
    let mut args = Enc::default();
    args.str("").leb(0).bytes(&[]);
    assert_eq!(
        process(args.done(), &Grants::default()).unwrap().done(),
        [1, 0]
    );
}
