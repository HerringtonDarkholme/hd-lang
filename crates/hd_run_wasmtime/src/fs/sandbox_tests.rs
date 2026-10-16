//! The path sandbox (`cli.cap.scope.path.resolved`): under a grant with
//! scope entries, no file operation leaves the granted directories, by
//! `..`, by a symbolic link planted inside, or by one swapped in between
//! the grant check and the open.

use std::fs;
use std::path::{Path, PathBuf};

use hd_run::{Grants, Limit};

use super::api::{Start, error, start};
use super::sandbox::Sandbox;
use super::test_support::TempDir;
use crate::codec::Enc;
use crate::limits::Ledger;

/// `FsRead` and `FsWrite` both limited to `entries`.
fn grants(entries: &[PathBuf]) -> Grants {
    let list: Vec<String> = entries
        .iter()
        .map(|e| hd_run::resolve_path(e).to_string_lossy().into_owned())
        .collect();
    Grants {
        limits: vec![
            ("FsRead".into(), Limit::Entries(list.clone())),
            ("FsWrite".into(), Limit::Entries(list)),
        ],
    }
}

fn key(method: &str) -> &'static str {
    if super::READ.contains(&method) {
        "FsRead"
    } else {
        "FsWrite"
    }
}

/// A call as the host makes it, with `sandbox` kept across calls.
fn call(
    sandbox: &Sandbox,
    grants: &Grants,
    cwd: &Path,
    method: &str,
    path: &str,
    payload: Option<&[u8]>,
) -> Enc {
    let mut args = Enc::default();
    args.str(path);
    if let Some(bytes) = payload {
        args.bytes(bytes);
    }
    let ledger = Ledger::new(u64::MAX);
    match start(
        key(method),
        method,
        args.done(),
        grants,
        sandbox,
        None,
        cwd,
        &ledger,
    )
    .expect("host call")
    {
        Start::Ready(encoded) => encoded,
        Start::Run(_) => panic!("{method} of {path} was offloaded"),
    }
}

/// A call whose grant check and placement see the tree before `swap`
/// changes it, and whose operation runs after: the race a symbolic link
/// swapped between the check and the open makes, without its timing.
#[cfg(unix)]
fn after_swap(
    grants: &Grants,
    cwd: &Path,
    method: &str,
    path: &str,
    payload: Option<&[u8]>,
    swap: impl FnOnce(),
) -> Enc {
    use super::api::failure;
    use super::ops::{Env, run};
    let sandbox = Sandbox::default();
    assert!(grants.covers_path(key(method), Path::new(path), cwd));
    let follow = !matches!(method, "stat" | "remove" | "rename");
    let place = sandbox
        .place(grants, key(method), &cwd.join(path), follow, false)
        .expect("granted");
    swap();
    let mut e = Enc::default();
    let env = Env {
        cwd,
        deadline: None,
        held: None,
        sandbox: Some(&sandbox),
    };
    match run(method, path, &place, None, payload, &mut e, env) {
        Ok(()) => e,
        Err(f) => failure(path, method, path, &f),
    }
}

fn ok_text(text: &str) -> Enc {
    let mut e = Enc::default();
    e.leb(0).str(text);
    e
}

/// A tree of `granted/` (with `sub/a.txt`) and `outside/a.txt`, granted
/// `granted/` alone.
fn tree() -> (TempDir, Grants) {
    let dir = TempDir::new();
    fs::create_dir_all(dir.0.join("granted/sub")).unwrap();
    fs::create_dir(dir.0.join("outside")).unwrap();
    fs::write(dir.0.join("granted/sub/a.txt"), "inside").unwrap();
    fs::write(dir.0.join("outside/a.txt"), "secret").unwrap();
    let g = grants(&[dir.0.join("granted")]);
    (dir, g)
}

#[test]
fn dot_dot_cannot_leave_a_granted_directory() {
    let (dir, g) = tree();
    let sandbox = Sandbox::default();
    let call = |method: &str, path: &str, payload: Option<&[u8]>| {
        call(&sandbox, &g, &dir.0, method, path, payload)
    };
    for path in [
        "granted/../outside/a.txt",
        "granted/sub/../../outside/a.txt",
    ] {
        assert_eq!(call("read_text", path, None).done(), error(path, 2).done());
        assert_eq!(
            call("write_text", path, Some(b"x")).done(),
            error(path, 2).done()
        );
        assert_eq!(call("remove", path, None).done(), error(path, 2).done());
    }
    assert_eq!(
        call("read_text", "granted/sub/../sub/a.txt", None).done(),
        ok_text("inside").done()
    );
    assert_eq!(fs::read(dir.0.join("outside/a.txt")).unwrap(), b"secret");
}

/// A `..` after a name that is not an existing directory fails under a
/// grant as it does by name: the OS does not skip the missing name, so
/// neither does the sandbox (`cli.engine.same-behavior`).
#[test]
fn dot_dot_after_a_missing_name_fails_as_the_os_does() {
    let (dir, g) = tree();
    let open = Grants { limits: vec![] };
    for path in ["granted/missing/../sub/a.txt", "granted/sub/a.txt/../a.txt"] {
        let by_name = call(&Sandbox::default(), &open, &dir.0, "read_text", path, None);
        let under = call(&Sandbox::default(), &g, &dir.0, "read_text", path, None);
        assert_ne!(by_name.done(), ok_text("inside").done(), "{path}");
        assert_eq!(under.done(), by_name.done(), "{path}");
    }
}

/// A granted directory removed and made again is opened afresh: the
/// handle of the removed one is forgotten.
#[test]
fn a_granted_directory_made_again_is_opened_again() {
    let dir = TempDir::new();
    let out = dir.0.join("out");
    let g = grants(std::slice::from_ref(&out));
    let sandbox = Sandbox::default();
    let call = |method: &str, path: &str, payload: Option<&[u8]>| {
        call(&sandbox, &g, &dir.0, method, path, payload)
    };
    // Not there yet: made, with its parents, by `create_dir_all`.
    assert_eq!(call("create_dir_all", "out/a/b", None).done(), &[0]);
    assert_eq!(
        call("write_text", "out/a/b/c.txt", Some(b"one")).done(),
        &[0]
    );
    assert_eq!(call("remove", "out/a/b/c.txt", None).done(), &[0]);
    assert_eq!(call("remove", "out/a/b", None).done(), &[0]);
    assert_eq!(call("remove", "out/a", None).done(), &[0]);
    assert_eq!(call("remove", "out", None).done(), &[0]);
    assert!(!out.exists());
    assert_eq!(call("create_dir_all", "out", None).done(), &[0]);
    assert_eq!(call("write_text", "out/d.txt", Some(b"two")).done(), &[0]);
    assert_eq!(fs::read(out.join("d.txt")).unwrap(), b"two");
}

/// A link planted inside a grant that points outside is refused for every
/// operation that follows it, and the target is untouched.
#[cfg(unix)]
#[test]
fn a_planted_link_to_outside_is_refused() {
    use std::os::unix::fs::symlink;
    let (dir, g) = tree();
    symlink(dir.0.join("outside/a.txt"), dir.0.join("granted/file")).unwrap();
    symlink(dir.0.join("outside"), dir.0.join("granted/dir")).unwrap();
    let sandbox = Sandbox::default();
    let call = |method: &str, path: &str, payload: Option<&[u8]>| {
        call(&sandbox, &g, &dir.0, method, path, payload)
    };
    for (method, path, payload) in [
        ("read_text", "granted/file", None),
        ("write_text", "granted/file", Some(b"x".as_slice())),
        ("append_text", "granted/file", Some(b"x".as_slice())),
        ("read_text", "granted/dir/a.txt", None),
        ("write_text", "granted/dir/new.txt", Some(b"x".as_slice())),
        ("list_dir", "granted/dir", None),
        ("create_dir_all", "granted/dir/new", None),
        ("remove", "granted/dir/a.txt", None),
    ] {
        assert_eq!(
            call(method, path, payload).done(),
            error(path, 2).done(),
            "{method} {path}"
        );
    }
    assert_eq!(fs::read(dir.0.join("outside/a.txt")).unwrap(), b"secret");
    assert_eq!(fs::read_dir(dir.0.join("outside")).unwrap().count(), 1);
    // `link/..` is resolved by its text, as the grant check compared it:
    // `granted/outside/a.txt`, which does not exist. By the link it would
    // be `outside/a.txt`.
    let through = "granted/dir/../outside/a.txt";
    assert_eq!(
        call("read_text", through, None).done(),
        error(through, 0).done()
    );
    // A link inside that points inside is followed, and `stat` and
    // `remove` act on the link itself.
    symlink("sub/a.txt", dir.0.join("granted/near")).unwrap();
    assert_eq!(
        call("read_text", "granted/near", None).done(),
        ok_text("inside").done()
    );
    let mut link = Enc::default();
    link.leb(0).leb(1).str("granted/near").leb(2).leb(9);
    assert_eq!(call("stat", "granted/near", None).done(), link.done());
    assert_eq!(call("remove", "granted/near", None).done(), &[0]);
    assert!(dir.0.join("granted/sub/a.txt").exists());
}

/// A link outside every grant that points inside one is the path a
/// `remove` or a `rename` would act on, so they refuse it.
#[cfg(unix)]
#[test]
fn a_link_outside_the_grant_is_not_removed_or_renamed() {
    use std::os::unix::fs::symlink;
    let (dir, g) = tree();
    symlink(dir.0.join("granted/sub/a.txt"), dir.0.join("outside/in")).unwrap();
    let sandbox = Sandbox::default();
    assert_eq!(
        call(&sandbox, &g, &dir.0, "remove", "outside/in", None).done(),
        error("outside/in", 2).done()
    );
    let mut args = Enc::default();
    args.str("outside/in").str("granted/moved");
    let ledger = Ledger::new(u64::MAX);
    let Start::Ready(renamed) = start(
        "FsWrite",
        "rename",
        args.done(),
        &g,
        &sandbox,
        None,
        &dir.0,
        &ledger,
    )
    .unwrap() else {
        panic!("a rename was offloaded");
    };
    assert_eq!(renamed.done(), error("outside/in", 2).done());
    assert!(fs::symlink_metadata(dir.0.join("outside/in")).is_ok());
    assert!(!dir.0.join("granted/moved").exists());
}

/// A directory inside the grant swapped for a link to outside after the
/// check: the operation refuses, and nothing outside changes.
#[cfg(unix)]
#[test]
fn a_link_swapped_in_after_the_check_is_refused() {
    use std::os::unix::fs::symlink;
    for target in ["../outside", "/"] {
        let (dir, g) = tree();
        let outside = dir.0.join("outside");
        let swap = || {
            fs::rename(dir.0.join("granted/sub"), dir.0.join("granted/old")).unwrap();
            let to = if target == "/" {
                outside.clone()
            } else {
                PathBuf::from(target)
            };
            symlink(to, dir.0.join("granted/sub")).unwrap();
        };
        let read = after_swap(&g, &dir.0, "read_text", "granted/sub/a.txt", None, swap);
        assert_eq!(
            read.done(),
            error("granted/sub/a.txt", 2).done(),
            "{target}"
        );
        fs::remove_file(dir.0.join("granted/sub")).unwrap();
        fs::rename(dir.0.join("granted/old"), dir.0.join("granted/sub")).unwrap();
        let wrote = after_swap(
            &g,
            &dir.0,
            "write_text",
            "granted/sub/a.txt",
            Some(b"x"),
            swap,
        );
        assert_eq!(
            wrote.done(),
            error("granted/sub/a.txt", 2).done(),
            "{target}"
        );
        assert_eq!(fs::read(outside.join("a.txt")).unwrap(), b"secret");
        assert_eq!(fs::read_dir(&outside).unwrap().count(), 1);
    }
}

/// A granted file swapped for a link after the check: the link at the
/// entry's own name is not followed.
#[cfg(unix)]
#[test]
fn a_granted_file_swapped_for_a_link_is_refused() {
    use std::os::unix::fs::symlink;
    let (dir, _) = tree();
    let file = dir.0.join("granted/file.txt");
    fs::write(&file, "mine").unwrap();
    let g = grants(std::slice::from_ref(&file));
    let swap = || {
        fs::remove_file(&file).unwrap();
        symlink(dir.0.join("outside/a.txt"), &file).unwrap();
    };
    let path = "granted/file.txt";
    let wrote = after_swap(&g, &dir.0, "write_text", path, Some(b"x"), swap);
    assert_eq!(wrote.done(), error(path, 2).done());
    assert_eq!(fs::read(dir.0.join("outside/a.txt")).unwrap(), b"secret");
    fs::remove_file(&file).unwrap();
    fs::write(&file, "mine").unwrap();
    let read = after_swap(&g, &dir.0, "read_text", path, None, swap);
    assert_eq!(read.done(), error(path, 2).done());
}

/// The granted directory itself replaced by a link after its handle was
/// opened: the operation stays in the directory that was opened.
#[cfg(unix)]
#[test]
fn a_granted_directory_swapped_after_opening_keeps_its_handle() {
    use std::os::unix::fs::symlink;
    let (dir, g) = tree();
    let wrote = after_swap(
        &g,
        &dir.0,
        "write_text",
        "granted/sub/b.txt",
        Some(b"kept"),
        || {
            fs::rename(dir.0.join("granted"), dir.0.join("moved")).unwrap();
            symlink(dir.0.join("outside"), dir.0.join("granted")).unwrap();
        },
    );
    assert_eq!(wrote.done(), &[0]);
    assert_eq!(fs::read(dir.0.join("moved/sub/b.txt")).unwrap(), b"kept");
    assert_eq!(fs::read_dir(dir.0.join("outside")).unwrap().count(), 1);
}
