mod tests {
    use crate::fs::test_support::{TempDir, assert_time_limit, error, limited};
    use hd_run::{Grants, Limit};
    use hd_run_wasmtime::internals::codec::{Dec, Enc};
    use hd_run_wasmtime::internals::fs::api::{Failure, call, call_until_at};
    use std::fs;
    use std::time::Instant;

    #[test]
    fn temporary_directory_creation_is_atomic_and_unpublished_owners_clean_up() {
        use hd_run_wasmtime::internals::fs::Sandbox;
        use hd_run_wasmtime::internals::fs::api::{Start, start};
        let cwd = std::env::current_dir().unwrap();
        let sandbox = Sandbox::default();
        let directories = std::thread::scope(|threads| {
            let started: Vec<_> = (0..16)
                .map(|_| {
                    let cwd = &cwd;
                    let sandbox = &sandbox;
                    threads.spawn(move || {
                        match start(
                            "FsWrite",
                            "create_temp_dir",
                            b"hd-owned-",
                            &Grants::default(),
                            sandbox,
                            None,
                            cwd,
                        )
                        .unwrap()
                        {
                            Start::Temporary(directory) => directory,
                            _ => panic!("temporary creation did not return its owner"),
                        }
                    })
                })
                .collect();
            started
                .into_iter()
                .map(|thread| thread.join().unwrap())
                .collect::<Vec<_>>()
        });
        let paths: hd_base::HashSet<_> = directories
            .iter()
            .map(|directory| directory.path.clone())
            .collect();
        assert_eq!(paths.len(), 16);
        for path in &paths {
            fs::create_dir(path.join("nested")).unwrap();
            fs::write(path.join("nested/file"), b"contents").unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(path).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
        }
        let outside = TempDir::new();
        fs::write(outside.0.join("preserve"), b"outside").unwrap();
        #[cfg(unix)]
        for path in &paths {
            std::os::unix::fs::symlink(&outside.0, path.join("link")).unwrap();
        }
        drop(directories);
        assert!(paths.iter().all(|path| !path.exists()));
        assert_eq!(fs::read(outside.0.join("preserve")).unwrap(), b"outside");
    }

    #[test]
    fn copy_uses_independent_endpoint_grants_and_preserves_opened_aliases() {
        let dir = TempDir::new();
        let source = dir.0.join("source");
        let target = dir.0.join("target");
        fs::write(&source, [0, 255, 10]).unwrap();
        fs::write(&target, b"an old longer destination").unwrap();
        let entries = |path: &std::path::Path| {
            Limit::Entries(vec![
                hd_run::resolve_path(path).to_string_lossy().into_owned(),
            ])
        };
        let grants = |read: Limit, write: Limit| {
            Grants::with_limits([("FsRead".into(), read), ("FsWrite".into(), write)])
        };
        let invoke = |grants: &Grants| {
            let mut args = Enc::default();
            args.str("source").str("target");
            call_until_at("FsWrite", "copy", args.done(), grants, None, &dir.0)
                .unwrap()
                .done()
                .to_vec()
        };
        let both = grants(entries(&source), entries(&target));
        assert_eq!(invoke(&both), [0]);
        assert_eq!(fs::read(&target).unwrap(), [0, 255, 10]);
        let no_read = grants(Limit::Deny, entries(&target));
        assert_eq!(invoke(&no_read), error("source", 2).done());
        let no_write = grants(entries(&source), Limit::Deny);
        assert_eq!(invoke(&no_write), error("target", 2).done());
        assert_eq!(fs::read(&target).unwrap(), [0, 255, 10]);
        fs::remove_file(&target).unwrap();
        fs::hard_link(&source, &target).unwrap();
        assert_eq!(invoke(&grants(entries(&source), entries(&target))), [0]);
        assert_eq!(fs::read(&source).unwrap(), [0, 255, 10]);
        assert_eq!(fs::read(&target).unwrap(), [0, 255, 10]);
    }

    #[cfg(unix)]
    #[test]
    fn copy_refuses_final_links_replaced_after_scoped_resolution() {
        use hd_run_wasmtime::internals::fs::api::{Job, Sandbox};
        use std::os::unix::fs::symlink;
        let dir = TempDir::new();
        for name in ["source", "target", "outside"] {
            fs::write(dir.0.join(name), name).unwrap();
        }
        let source = dir.0.join("source");
        let target = dir.0.join("target");
        let outside = dir.0.join("outside");
        let grants = Grants::with_limits(vec![
            (
                "FsRead".into(),
                Limit::Entries(vec![
                    hd_run::resolve_path(&source).to_string_lossy().into_owned(),
                ]),
            ),
            (
                "FsWrite".into(),
                Limit::Entries(vec![
                    hd_run::resolve_path(&target).to_string_lossy().into_owned(),
                ]),
            ),
        ]);
        let sandbox = Sandbox::default();
        let job = || {
            Box::new(Job {
                method: "copy".into(),
                path: "source".into(),
                to: Some("target".into()),
                place: sandbox
                    .place(&grants, "FsRead", &source, true, false)
                    .unwrap(),
                to_place: Some(
                    sandbox
                        .place(&grants, "FsWrite", &target, true, false)
                        .unwrap(),
                ),
                payload: None,
                cwd: dir.0.clone(),
            })
        };
        let pending = job();
        fs::remove_file(&source).unwrap();
        symlink(&outside, &source).unwrap();
        let encoded = pending.run_blocking();
        let mut decoded = Dec::new(encoded.done());
        assert_eq!(decoded.leb().unwrap(), 1);
        assert_eq!(decoded.leb().unwrap(), 2); // NotGranted
        assert_eq!(fs::read(&target).unwrap(), b"target");
        fs::remove_file(&source).unwrap();
        fs::write(&source, b"source").unwrap();
        let pending = job();
        fs::remove_file(&target).unwrap();
        symlink(&outside, &target).unwrap();
        let encoded = pending.run_blocking();
        let mut decoded = Dec::new(encoded.done());
        assert_eq!(decoded.leb().unwrap(), 1);
        assert_eq!(decoded.leb().unwrap(), 2);
        assert_eq!(fs::read(&outside).unwrap(), b"outside");
    }

    #[test]
    fn concurrent_case_directories_do_not_change_process_cwd() {
        let before = std::env::current_dir().unwrap();
        let left = TempDir::new();
        let right = TempDir::new();
        std::thread::scope(|scope| {
            for (dir, payload) in [(&left, b"left".as_slice()), (&right, b"right".as_slice())] {
                scope.spawn(move || {
                    let mut args = Enc::default();
                    args.str("value").bytes(payload);
                    let result = call_until_at(
                        "FsWrite",
                        "write_bytes",
                        args.done(),
                        &Grants::default(),
                        None,
                        &dir.0,
                    )
                    .unwrap();
                    assert_eq!(result.done(), &[0]);
                    let mut args = Enc::default();
                    args.str("value");
                    let result = call_until_at(
                        "FsRead",
                        "read_bytes",
                        args.done(),
                        &Grants::default(),
                        None,
                        &dir.0,
                    )
                    .unwrap();
                    let mut dec = Dec::new(result.done());
                    assert_eq!(dec.leb().unwrap(), 0);
                    assert_eq!(dec.bytes().unwrap(), payload);
                });
            }
        });
        assert_eq!(std::env::current_dir().unwrap(), before);
        assert_eq!(fs::read(left.0.join("value")).unwrap(), b"left");
        assert_eq!(fs::read(right.0.join("value")).unwrap(), b"right");
    }
    #[test]
    fn case_directory_results_keep_guest_paths_and_grant_resolution() {
        let dir = TempDir::new();
        fs::write(dir.0.join("item"), b"x").unwrap();
        let invoke = |key: &str, method: &str, path: &str, grants: &Grants| {
            let mut args = Enc::default();
            args.str(path);
            call_until_at(key, method, args.done(), grants, None, &dir.0).unwrap()
        };
        let result = invoke("FsRead", "list_dir", ".", &Grants::default());
        let mut dec = Dec::new(result.done());
        assert_eq!(dec.leb().unwrap(), 0);
        assert_eq!(dec.leb().unwrap(), 1);
        assert_eq!(dec.str().unwrap(), "./item");
        assert_eq!(dec.leb().unwrap(), 0);
        assert_eq!(dec.leb().unwrap(), 1);
        assert_eq!(
            invoke("FsRead", "read_text", "absent", &Grants::default()).done(),
            error("absent", 0).done()
        );
        let grants = Grants::with_limits(vec![(
            "FsRead".into(),
            Limit::Entries(vec![
                hd_run::resolve_path(&dir.0.join("item"))
                    .to_string_lossy()
                    .into_owned(),
            ]),
        )]);
        assert_eq!(
            invoke("FsRead", "read_text", "item", &grants).done(),
            Enc::default().leb(0).str("x").done()
        );
        assert_eq!(
            invoke("FsRead", "read_text", "../outside", &grants).done(),
            error("../outside", 2).done()
        );
        assert_eq!(
            invoke("FsWrite", "create_dir_all", "", &Grants::default()).done(),
            error("", 0).done()
        );
    }
    #[test]
    fn expired_deadline_prevents_filesystem_mutation_and_is_a_host_stop() {
        let dir = TempDir::new();
        let path = dir.path("keep");
        fs::write(&path, b"original").unwrap();
        let expired = Instant::now();
        assert_time_limit(limited("write_bytes", &path, Some(b"replacement"), expired));
        assert_eq!(fs::read(&path).unwrap(), b"original");
        assert_time_limit(limited(
            "create_dir_all",
            &dir.path("new/a/b"),
            None,
            expired,
        ));
        assert!(!std::path::Path::new(&dir.path("new")).exists());
        assert_time_limit(limited("remove", &path, None, expired));
        assert_eq!(fs::read(&path).unwrap(), b"original");
    }
    #[test]
    fn grants_precede_validation_and_cover_both_rename_paths() {
        let denied = Grants::with_limits(vec![("FsWrite".into(), Limit::Deny)]);
        // Every argument decodes before the grant is checked: a missing
        // payload breaks the ABI even outside the grant.
        let mut args = Enc::default();
        args.str("bad\0path");
        let Err(err) = call("FsWrite", "write_text", args.done(), &denied) else {
            panic!("a call with no payload decodes");
        };
        assert_eq!(err.to_string(), "an argument does not decode");
        args.str("text");
        assert_eq!(
            call("FsWrite", "write_text", args.done(), &denied)
                .unwrap()
                .done(),
            error("bad\0path", 2).done()
        );
        let dir = TempDir::new();
        // This is a plain absolute path, not an alias through the host's
        // temporary-directory symlink (such as /var -> /private/var).
        let allowed = hd_run::canonicalize(&dir.0)
            .unwrap()
            .join("from")
            .to_string_lossy()
            .into_owned();
        let refused = format!("{allowed}-outside");
        fs::write(&allowed, "keep").unwrap();
        let grants = Grants::with_limits(vec![(
            "FsWrite".into(),
            Limit::Entries(vec![
                hd_run::canonicalize(&allowed)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
            ]),
        )]);
        for (from, to, rejected) in [
            (&allowed, &refused, &refused),
            (&refused, &allowed, &refused),
        ] {
            let mut args = Enc::default();
            args.str(from).str(to);
            assert_eq!(
                call("FsWrite", "rename", args.done(), &grants)
                    .unwrap()
                    .done(),
                error(rejected, 2).done()
            );
        }
        assert_eq!(fs::read(&allowed).unwrap(), b"keep");
    }
    #[test]
    fn nul_paths_have_readable_validation_errors() {
        for (from, to, argument) in [
            ("a\0b", "to", "source path"),
            ("from", "a\0b", "destination path"),
        ] {
            let mut args = Enc::default();
            args.str(from).str(to);
            assert_eq!(
                call("FsWrite", "rename", args.done(), &Grants::default())
                    .unwrap()
                    .done(),
                error(&format!("{argument} contains a NUL byte"), 7).done()
            );
        }
    }
    #[test]
    fn other_errors_keep_native_messages_and_operation_paths() {
        for (method, syscall) in [
            ("read_bytes", "read"),
            ("write_text", "write"),
            ("rename", "rename"),
        ] {
            let native = std::io::Error::other("native failure");
            let failure = Failure::new(syscall, native);
            let paths = if method == "rename" {
                "\"from\" -> \"to\""
            } else {
                "\"to\""
            };
            assert_eq!(
                hd_run_wasmtime::internals::fs::api::failure("to", method, "from", &failure).done(),
                error(&format!("{syscall} {paths}: native failure"), 7).done()
            );
        }
    }
}
