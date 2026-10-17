use super::platform::sys_read;
use super::random;
use crate::codec::Enc;
use hd_run::{Grants, Limit};

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

/// `Sys.hostname` reads the same name `hostname(1)` prints.
#[cfg(unix)]
#[test]
fn sys_hostname_matches_hostname_1() {
    let printed = std::process::Command::new("hostname")
        .output()
        .expect("runs hostname(1)");
    let name = String::from_utf8(printed.stdout).unwrap();
    let mut expected = Enc::default();
    expected.leb(0).str(name.trim_end());
    let grants = Grants::default();
    assert_eq!(
        super::platform::sys("hostname", &grants).done(),
        expected.done()
    );
}
