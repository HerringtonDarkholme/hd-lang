//! Spec-defined core provider shape parity and native-host regressions.
//! Native OS messages and UTF-8 BOM preservation are not Node contracts.

use std::fs;
#[cfg(unix)]
use std::path::PathBuf;

use hd_run::Limit;
use wasm_encoder::{BlockType, Instruction as I, ValType};

use crate::support::*;

#[test]
fn fs_read_all_methods_binary_text_entries_and_errors() {
    let scratch = Scratch::new();
    let dir = scratch.at("entries");
    fs::create_dir(&dir).unwrap();
    let file = dir.join("a");
    fs::write(&file, b"hello\0world").unwrap();
    let sub = dir.join("z");
    fs::create_dir(&sub).unwrap();
    let text = scratch.at("text");
    fs::write(&text, "héllo").unwrap();
    let bad = scratch.at("bad");
    fs::write(&bad, [0xff, 0xfe]).unwrap();
    let missing = scratch.at("absent");
    let notdir = file.join("child");
    let deep_missing = missing.join("a/b");
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&file),
        Enc::default().leb(0).bytes(b"hello\0world").0,
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&text),
        Enc::default().leb(0).str("héllo").0,
    );
    p.call("hd:FsRead", "read_text", one(&bad), fs_error(&bad, 6));
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&missing),
        fs_error(&missing, 0),
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&missing),
        fs_error(&missing, 0),
    );
    p.call(
        "hd:FsRead",
        "stat",
        one(&file),
        Enc::default()
            .leb(0)
            .leb(1)
            .str(path(&file))
            .leb(0)
            .leb(11)
            .modified(&file)
            .0,
    );
    p.call(
        "hd:FsRead",
        "stat",
        one(&sub),
        Enc::default()
            .leb(0)
            .leb(1)
            .str(path(&sub))
            .leb(1)
            .leb(0)
            .modified(&sub)
            .0,
    );
    p.call("hd:FsRead", "stat", one(&missing), vec![0, 0]);
    p.call("hd:FsRead", "stat", one(&notdir), vec![0, 0]);
    p.call(
        "hd:FsRead",
        "list_dir",
        one(&dir),
        Enc::default()
            .leb(0)
            .leb(2)
            .str(&listed_path(&dir, "a"))
            .leb(0)
            .leb(11)
            .modified(&file)
            .str(&listed_path(&dir, "z"))
            .leb(1)
            .leb(0)
            .modified(&sub)
            .0,
    );
    p.call(
        "hd:FsRead",
        "list_dir",
        one(&missing),
        fs_error(&missing, 0),
    );
    p.call("hd:FsRead", "list_dir", one(&file), fs_error(&file, 4));
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&notdir),
        fs_error(&notdir, 4),
    );
    p.call("hd:FsRead", "read_bytes", one(&dir), fs_error(&dir, 5));
    // Below a file is NotADirectory; below a missing chain is NotFound.
    p.call("hd:FsRead", "read_text", one(&notdir), fs_error(&notdir, 4));
    p.call("hd:FsRead", "list_dir", one(&notdir), fs_error(&notdir, 4));
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&deep_missing),
        fs_error(&deep_missing, 0),
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn fs_write_all_methods_success_and_os_errors() {
    let scratch = Scratch::new();
    let area = scratch.at("area");
    let binary = area.join("binary");
    let text = area.join("text");
    let nested = area.join("nested/child");
    let moved = area.join("moved");
    let missing = area.join("missing");
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "write_bytes",
        Enc::default().str(path(&binary)).bytes(&[0, 255, 42]).0,
        vec![0],
    );
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&binary),
        Enc::default().leb(0).bytes(&[0, 255, 42]).0,
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(path(&text)).str("hé").0,
        vec![0],
    );
    p.call(
        "hd:FsWrite",
        "append_text",
        Enc::default().str(path(&text)).str("llo").0,
        vec![0],
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&text),
        Enc::default().leb(0).str("héllo").0,
    );
    p.call("hd:FsWrite", "create_dir_all", one(&nested), vec![0]);
    p.call("hd:FsWrite", "create_dir_all", one(&nested), vec![0]);
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(path(&text)).str(path(&moved)).0,
        vec![0],
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&moved),
        Enc::default().leb(0).str("héllo").0,
    );
    p.call("hd:FsWrite", "remove", one(&moved), vec![0]);
    p.call("hd:FsWrite", "remove", one(&nested), vec![0]);
    p.call("hd:FsWrite", "remove", one(&missing), fs_error(&missing, 0));
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        one(&binary),
        fs_error(&binary, 4),
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(path(&area)).str("no").0,
        fs_error(&area, 5),
    );
    let below = binary.join("child");
    for method in ["write_bytes", "write_text", "append_text"] {
        p.call(
            "hd:FsWrite",
            method,
            Enc::default().str(path(&below)).str("no").0,
            fs_error(&below, 4),
        );
    }
    let deep = missing.join("a/b");
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(path(&deep)).str("no").0,
        fs_error(&deep, 0),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(path(&missing)).str(path(&moved)).0,
        fs_error(&moved, 0),
    );
    assert_program(&scratch, p, vec![], || {
        if area.exists() {
            fs::remove_dir_all(&area).unwrap();
        }
        fs::create_dir(&area).unwrap();
    });
}

#[test]
fn filesystem_grants_check_every_method_and_both_rename_paths() {
    let scratch = Scratch::new();
    let allowed = scratch.at("allowed");
    let outside = scratch.at("allowed-sibling");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&outside).unwrap();
    let inside_file = allowed.join("file");
    let outside_file = outside.join("file");
    fs::write(&inside_file, "inside").unwrap();
    fs::write(&outside_file, "outside").unwrap();
    let traversal = allowed.join("../allowed-sibling/file");
    let mut p = Program::new();
    for method in ["read_bytes", "read_text", "list_dir", "stat"] {
        p.call(
            "hd:FsRead",
            method,
            one(&outside_file),
            fs_error(&outside_file, 2),
        );
    }
    for method in [
        "write_bytes",
        "write_text",
        "append_text",
        "create_dir_all",
        "remove",
    ] {
        let args = if method.contains("text") || method == "write_bytes" {
            Enc::default().str(path(&outside_file)).str("changed").0
        } else {
            one(&outside_file)
        };
        p.call("hd:FsWrite", method, args, fs_error(&outside_file, 2));
    }
    p.call(
        "hd:FsRead",
        "read_text",
        one(&traversal),
        fs_error(&traversal, 2),
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(path(&traversal)).str("changed").0,
        fs_error(&traversal, 2),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default()
            .str(path(&outside_file))
            .str(path(&inside_file))
            .0,
        fs_error(&outside_file, 2),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default()
            .str(path(&inside_file))
            .str(path(&outside_file))
            .0,
        fs_error(&outside_file, 2),
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&inside_file),
        Enc::default().leb(0).str("inside").0,
    );
    assert_program(
        &scratch,
        p,
        vec![
            ("FsRead", Limit::Entries(vec![path(&allowed).into()])),
            ("FsWrite", Limit::Entries(vec![path(&allowed).into()])),
        ],
        || {
            assert_eq!(fs::read_to_string(&inside_file).unwrap(), "inside");
            assert_eq!(fs::read_to_string(&outside_file).unwrap(), "outside");
        },
    );
    assert_eq!(fs::read_to_string(outside_file).unwrap(), "outside");
}

#[cfg(unix)]
#[test]
fn filesystem_grants_resolve_symlinks_and_report_symlink_entries() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let allowed = scratch.at("allowed");
    let outside = scratch.at("outside");
    fs::create_dir(&allowed).unwrap();
    fs::create_dir(&outside).unwrap();
    let file = allowed.join("file");
    let secret = outside.join("secret");
    fs::write(&file, "hello").unwrap();
    fs::write(&secret, "secret").unwrap();
    let good = allowed.join("good");
    let bad = allowed.join("bad");
    symlink(&file, &good).unwrap();
    symlink(&outside, &bad).unwrap();
    let escaped = bad.join("secret");
    let nonexistent = bad.join("new/child");
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "read_text",
        one(&good),
        Enc::default().leb(0).str("hello").0,
    );
    p.call(
        "hd:FsRead",
        "stat",
        one(&good),
        Enc::default()
            .leb(0)
            .leb(1)
            .str(path(&good))
            .leb(2)
            .leb(path(&file).len() as u64)
            .modified(&good)
            .0,
    );
    p.call(
        "hd:FsRead",
        "list_dir",
        one(&allowed),
        Enc::default()
            .leb(0)
            .leb(3)
            .str(&listed_path(&allowed, "bad"))
            .leb(2)
            .leb(path(&outside).len() as u64)
            .modified(&bad)
            .str(&listed_path(&allowed, "file"))
            .leb(0)
            .leb(5)
            .modified(&file)
            .str(&listed_path(&allowed, "good"))
            .leb(2)
            .leb(path(&file).len() as u64)
            .modified(&good)
            .0,
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&escaped),
        fs_error(&secret, 2),
    );
    p.call("hd:FsRead", "stat", one(&bad), fs_error(&outside, 2));
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        one(&nonexistent),
        fs_error(&outside.join("new/child"), 2),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(path(&file)).str(path(&escaped)).0,
        fs_error(&secret, 2),
    );
    assert_program(
        &scratch,
        p,
        vec![
            ("FsRead", Limit::Entries(vec![path(&allowed).into()])),
            ("FsWrite", Limit::Entries(vec![path(&allowed).into()])),
        ],
        || {},
    );
    assert_eq!(fs::read_to_string(secret).unwrap(), "secret");
    assert!(!outside.join("new").exists());
}

/// `cli.cap.scope.path.resolved` and `.access`: a `..` after a symbolic
/// link goes up from where the link leads, as the OS resolves it, so
/// `granted/link/../x` names `outside/x` when `link` leads to
/// `outside/deep`. Every method refuses it, and a link that stays in the
/// grant still works, with its `..` resolved the same way (#399).
#[cfg(unix)]
#[test]
fn filesystem_grants_resolve_a_link_before_the_dot_dot_after_it() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let granted = scratch.at("granted");
    let outside = scratch.at("outside");
    fs::create_dir_all(granted.join("sub/deep")).unwrap();
    fs::create_dir_all(outside.join("deep")).unwrap();
    fs::write(outside.join("x"), "secret").unwrap();
    fs::write(granted.join("y"), "granted y").unwrap();
    fs::write(granted.join("sub/y"), "sub y").unwrap();
    symlink("../outside/deep", granted.join("link")).unwrap();
    symlink("sub/deep", granted.join("inlink")).unwrap();
    symlink("../outside/made", granted.join("broken")).unwrap();
    let file = granted.join("file");
    let escaped = granted.join("link").join("..").join("x");
    let through = granted.join("link").join("new.txt");
    let up = granted.join("link").join("..");
    let created = granted.join("missing").join("..").join("link").join("made");
    let broken = granted.join("broken");
    let mut p = Program::new();
    for method in ["read_bytes", "read_text", "stat"] {
        p.call(
            "hd:FsRead",
            method,
            one(&escaped),
            fs_error(&outside.join("x"), 2),
        );
    }
    p.call("hd:FsRead", "list_dir", one(&up), fs_error(&outside, 2));
    for method in ["write_bytes", "write_text", "append_text"] {
        for (target, resolved) in [
            (&escaped, outside.join("x")),
            (&through, outside.join("deep/new.txt")),
            (&broken, outside.join("made")),
        ] {
            p.call(
                "hd:FsWrite",
                method,
                Enc::default().str(path(target)).str("changed").0,
                fs_error(&resolved, 2),
            );
        }
    }
    p.call(
        "hd:FsWrite",
        "remove",
        one(&escaped),
        fs_error(&outside.join("x"), 2),
    );
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        one(&created),
        fs_error(&outside.join("deep/made"), 2),
    );
    for (to, resolved) in [
        (&escaped, outside.join("x")),
        (&through, outside.join("deep/new.txt")),
    ] {
        p.call(
            "hd:FsWrite",
            "rename",
            Enc::default().str(path(&file)).str(path(to)).0,
            fs_error(&resolved, 2),
        );
    }
    p.call(
        "hd:FsRead",
        "read_text",
        one(&granted.join("inlink").join("..").join("y")),
        Enc::default().leb(0).str("sub y").0,
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&granted.join("sub").join("..").join("y")),
        Enc::default().leb(0).str("granted y").0,
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default()
            .str(path(&granted.join("inlink").join("..").join("new.txt")))
            .str("written")
            .0,
        Enc::default().leb(0).0,
    );
    let untouched = || {
        assert_eq!(fs::read_to_string(outside.join("x")).unwrap(), "secret");
        assert_eq!(fs::read_dir(outside.join("deep")).unwrap().count(), 0);
        assert!(!outside.join("made").exists());
    };
    assert_program(
        &scratch,
        p,
        vec![
            ("FsRead", Limit::Entries(vec![path(&granted).into()])),
            ("FsWrite", Limit::Entries(vec![path(&granted).into()])),
        ],
        || {
            untouched();
            fs::write(&file, "file").unwrap();
            let _ = fs::remove_file(granted.join("sub/new.txt"));
        },
    );
    untouched();
    assert_eq!(
        fs::read_to_string(granted.join("sub/new.txt")).unwrap(),
        "written"
    );
}

#[cfg(unix)]
#[test]
fn fs_create_dir_broken_symlink_reports_not_found() {
    use std::os::unix::fs::symlink;
    let scratch = Scratch::new();
    let broken = scratch.at("broken");
    symlink(scratch.at("missing-target"), &broken).unwrap();
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        one(&broken),
        fs_error(&broken, 0),
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn filesystem_invalid_paths_have_readable_native_validation() {
    let mut p = Program::new();
    let nul = "a\0b";
    let other = |argument: &str| {
        Enc::default()
            .leb(1)
            .leb(7)
            .str(&format!("{argument} contains a NUL byte"))
            .0
    };
    p.call(
        "hd:FsRead",
        "read_text",
        Enc::default().str(nul).0,
        other("path"),
    );
    p.call(
        "hd:FsWrite",
        "write_text",
        Enc::default().str(nul).str("content").0,
        other("path"),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(nul).str("to").0,
        other("source path"),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str("from").str(nul).0,
        other("destination path"),
    );
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        Enc::default().str("").0,
        Enc::default().leb(1).leb(0).str("").0,
    );
    assert_native_program(p);
}

#[cfg(unix)]
#[test]
fn filesystem_long_paths_keep_native_errors_and_operation_context() {
    let scratch = Scratch::new();
    let too_long = scratch.at(&"x".repeat(300));
    let mut p = Program::new();
    for (module, method, syscall) in [
        ("hd:FsRead", "read_bytes", "open"),
        ("hd:FsRead", "stat", "lstat"),
        ("hd:FsRead", "list_dir", "scandir"),
        ("hd:FsWrite", "create_dir_all", "mkdir"),
    ] {
        let native =
            std::io::Error::from_raw_os_error(rustix::io::Errno::NAMETOOLONG.raw_os_error());
        let error = format!("{syscall} {:?}: {native}", path(&too_long));
        p.call(
            module,
            method,
            one(&too_long),
            Enc::default().leb(1).leb(7).str(&error).0,
        );
    }
    assert_native_program(p);
}

#[cfg(target_os = "linux")]
#[test]
fn filesystem_write_failure_keeps_the_native_message_and_path() {
    // `/dev/full` is a device, so the write waits on the blocking pool
    // (`std-fs.special-file`) and completes at `.finish`.
    let mut p = Program::new();
    p.call_waiting(
        "hd:FsWrite",
        "write_bytes",
        Enc::default().str("/dev/full").bytes(b"full").0,
        Enc::default()
            .leb(1)
            .leb(7)
            .str(&format!(
                "write \"/dev/full\": {}",
                std::io::Error::from_raw_os_error(rustix::io::Errno::NOSPC.raw_os_error())
            ))
            .0,
    );
    assert_native_program(p);
}

#[cfg(unix)]
#[test]
fn filesystem_permission_denied_matches_node() {
    use std::os::unix::fs::PermissionsExt;
    struct RestorePermissions(PathBuf, fs::Permissions);
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            let _ = fs::set_permissions(&self.0, self.1.clone());
        }
    }
    let scratch = Scratch::new();
    let locked = scratch.at("locked");
    fs::write(&locked, "locked").unwrap();
    let original = fs::metadata(&locked).unwrap().permissions();
    let _restore = RestorePermissions(locked.clone(), original);
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o0)).unwrap();
    // A privileged test runner may legitimately bypass Unix mode bits.
    if fs::read(&locked).is_ok() {
        return;
    }
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "read_bytes",
        one(&locked),
        fs_error(&locked, 1),
    );
    p.call("hd:FsRead", "read_text", one(&locked), fs_error(&locked, 1));
    for method in ["write_bytes", "write_text", "append_text"] {
        p.call(
            "hd:FsWrite",
            method,
            Enc::default().str(path(&locked)).str("changed").0,
            fs_error(&locked, 1),
        );
    }
    assert_program(&scratch, p, vec![], || {});
}

fn process_args(name: &str, argv: &[&str], input: &[u8]) -> Vec<u8> {
    process_command(name, argv, &[], None, input)
}

/// The arguments of `Process.run` for a `Command`: its program, arguments,
/// environment pairs, directory and standard input.
fn process_command(
    name: &str,
    argv: &[&str],
    env: &[(&str, &str)],
    dir: Option<&str>,
    input: &[u8],
) -> Vec<u8> {
    let mut e = Enc::default().str(name).leb(argv.len() as u64);
    for arg in argv {
        e = e.str(arg);
    }
    e = e.leb(env.len() as u64);
    for (key, value) in env {
        e = e.str(key).str(value);
    }
    e = match dir {
        Some(dir) => e.leb(1).str(dir),
        None => e.leb(0),
    };
    e.bytes(input).0
}

#[test]
fn process_binary_input_lossy_outputs_status_and_missing_program() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    let script = "const fs=require('node:fs');process.stdout.write(fs.readFileSync(0));process.stderr.write(Buffer.from([255,0,195,169]));process.exitCode=23";
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("node", &["-e", script], &[0, 255, 195, 169]),
        Enc::default()
            .leb(0)
            .str("\0�é")
            .str("�\0é")
            .leb(46)
            .str("node")
            .0,
    );
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("hd-missing-program-parity-778811", &[], b""),
        Enc::default()
            .leb(1)
            .leb(0)
            .str("hd-missing-program-parity-778811")
            .0,
    );
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn process_drains_both_pipes_while_child_waits_for_large_input() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    let script = "const fs=require('node:fs');fs.writeSync(1,Buffer.alloc(70000,65));fs.writeSync(2,Buffer.alloc(70000,66));const b=fs.readFileSync(0);fs.writeSync(1,String(b.length))";
    let input = vec![42; 100_000];
    let out = format!("{}100000", "A".repeat(70_000));
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("node", &["-e", script], &input),
        Enc::default()
            .leb(0)
            .str(&out)
            .str(&"B".repeat(70_000))
            .leb(0)
            .str("node")
            .0,
    );
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(unix)]
#[test]
fn process_child_closing_stdin_retains_output_and_status() {
    let mut p = Program::new();
    p.call_waiting(
        "hd:Process",
        "run",
        process_args(
            "/bin/sh",
            &["-c", "exec 0<&-; printf out; printf err >&2; exit 7"],
            &vec![42; 262_144],
        ),
        Enc::default()
            .leb(0)
            .str("out")
            .str("err")
            .leb(14)
            .str("/bin/sh")
            .0,
    );
    assert_native_program(p);
}

// std-process.stdin.unread: a child that stops reading its stdin drops the
// rest, and the call still returns `.Ok` on both hosts.
#[cfg(unix)]
#[test]
fn process_unread_stdin_is_dropped_and_returns_ok() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    // The child reads one line and exits while most of the input is unsent.
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("head", &["-n", "1"], "line\n".repeat(200_000).as_bytes()),
        Enc::default()
            .leb(0)
            .str("line\n")
            .str("")
            .leb(0)
            .str("head")
            .0,
    );
    // The child closes its stdin, keeps running, and prints afterwards.
    p.call_waiting(
        "hd:Process",
        "run",
        process_args(
            "/bin/sh",
            &["-c", "exec 0<&-; sleep 0.2; printf late; exit 3"],
            &vec![42; 1_000_000],
        ),
        Enc::default()
            .leb(0)
            .str("late")
            .str("")
            .leb(6)
            .str("/bin/sh")
            .0,
    );
    // A missing program is still NotFound, with input given.
    p.call_waiting(
        "hd:Process",
        "run",
        process_args(
            "hd-missing-program-parity-778811",
            &[],
            &vec![42; 1_000_000],
        ),
        Enc::default()
            .leb(1)
            .leb(0)
            .str("hd-missing-program-parity-778811")
            .0,
    );
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(unix)]
#[test]
fn filesystem_sparse_file_size_bound_matches_node() {
    let scratch = Scratch::new();
    let file = scratch.at("too-large");
    // This allocates no file contents: both hosts must reject from metadata
    // before reading the sparse file or reserving a multi-GiB result.
    fs::File::create(&file)
        .unwrap()
        .set_len(0x8000_0000)
        .unwrap();
    let mut p = Program::new();
    for method in ["read_bytes", "read_text"] {
        p.call(
            "hd:FsRead",
            method,
            one(&file),
            Enc::default()
                .leb(1)
                .leb(7)
                .str("File size (2147483648) is greater than 2 GiB")
                .0,
        );
    }
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn process_scope_is_exact_and_refusal_precedes_lookup() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("node", &["-e", "process.stdout.write('ok')"], b""),
        Enc::default().leb(0).str("ok").str("").leb(0).str("node").0,
    );
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("hd-missing-program-parity-778811", &[], b""),
        vec![1, 2],
    );
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("node-other", &[], b""),
        vec![1, 2],
    );
    assert_program(
        &scratch,
        p,
        vec![("Process", Limit::Entries(vec!["node".into()]))],
        || {},
    );
}

// module.process.command-env and module.process.command-dir: a child sees
// the program's environment with the command's pairs added (a later pair for
// a name wins) and starts in the command's directory; a directory that does
// not exist is a typed error, not a missing program, on both hosts.
#[test]
fn process_child_sees_its_environment_and_directory() {
    let scratch = Scratch::new();
    let dir = scratch.at("child-dir");
    fs::create_dir(&dir).unwrap();
    let missing = scratch.at("no-such-dir");
    let script = "process.stdout.write((process.env.HD_PARITY_VAR ?? '') + '|' + (process.env.HD_PARITY_OTHER ?? '') + '|' + require('node:path').basename(process.cwd()))";
    let env_only = "process.stdout.write((process.env.HD_PARITY_VAR ?? '') + '|' + (process.env.HD_PARITY_OTHER ?? ''))";
    let mut p = Program::new();
    p.call_waiting(
        "hd:Process",
        "run",
        process_command(
            "node",
            &["-e", script],
            &[
                ("HD_PARITY_VAR", "first"),
                ("HD_PARITY_OTHER", "kept"),
                ("HD_PARITY_VAR", "second"),
            ],
            Some(path(&dir)),
            b"",
        ),
        Enc::default()
            .leb(0)
            .str("second|kept|child-dir")
            .str("")
            .leb(0)
            .str("node")
            .0,
    );
    p.call_waiting(
        "hd:Process",
        "run",
        process_command("node", &["-e", env_only], &[], None, b""),
        Enc::default().leb(0).str("|").str("").leb(0).str("node").0,
    );
    p.call_waiting(
        "hd:Process",
        "run",
        process_command("node", &["-e", script], &[], Some(path(&missing)), b""),
        Enc::default()
            .leb(1)
            .leb(3)
            .str(&format!("working directory not found: {}", path(&missing)))
            .0,
    );
    assert_program(&scratch, p, vec![], || {});
}

// A SIGTERM-ended child reports 143, 128 plus the signal
// (`std-process.child.signal`), on both hosts.
#[cfg(unix)]
#[test]
fn process_signal_status_and_permission_denied() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let script = scratch.at("not-executable");
    fs::write(&script, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o600)).unwrap();
    let mut p = Program::new();
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("node", &["-e", "process.kill(process.pid,'SIGTERM')"], b""),
        Enc::default().leb(0).str("").str("").leb(286).str("node").0,
    );
    p.call_waiting(
        "hd:Process",
        "run",
        process_args(path(&script), &[], b""),
        Enc::default().leb(1).leb(1).str(path(&script)).0,
    );
    assert_program(&scratch, p, vec![], || {});
}

#[cfg(unix)]
#[test]
fn process_executable_without_shebang_uses_rust_standard_spawning_behavior() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let script = scratch.at("plain-executable");
    fs::write(
        &script,
        "printf 'out:%s' \"$1\"\nprintf 'err' >&2\nexit 19\n",
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o700)).unwrap();
    let mut p = Program::new();
    // Owner direction: Rust's platform behavior governs this case, not
    // Node's macOS-specific refusal to execute plain text.
    let expected = std::process::Command::new(&script)
        .arg("argument with spaces")
        .output();
    let encoded = match expected {
        Ok(output) => Enc::default()
            .leb(0)
            .str(&String::from_utf8_lossy(&output.stdout))
            .str(&String::from_utf8_lossy(&output.stderr))
            .leb(u64::try_from(output.status.code().unwrap_or(128)).unwrap() * 2)
            .str(path(&script)),
        Err(error) => Enc::default()
            .leb(1)
            .leb(3)
            .str(&format!("cannot run {:?}: {error}", path(&script))),
    };
    p.call_waiting(
        "hd:Process",
        "run",
        process_args(path(&script), &["argument with spaces"], b""),
        encoded.0,
    );
    let (wasm, expected) = p.finish(7);
    let result = hd_run_wasmtime::run_captured(
        hd_run_wasmtime::EngineConfig::default(),
        &wasm,
        &hd_run::HostSetup {
            providers: vec![],
            grants: hd_run::Grants::default(),
            limits: hd_run::Limits::default(),
            args: vec![],
            program: String::new(),
        },
    )
    .unwrap();
    assert_eq!(result.status, 7);
    assert_eq!(result.stdout, b"core-parity\n");
    assert_eq!(result.stderr, expected.as_bytes());
}

#[test]
fn process_invalid_names_and_arguments_return_typed_errors() {
    let mut p = Program::new();
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("", &[], b""),
        Enc::default().leb(1).leb(0).str("").0,
    );
    for (name, args, message) in [
        ("a\0b", vec![], "program name contains a NUL byte"),
        (
            "node",
            vec!["a\0b"],
            "process argument 0 contains a NUL byte",
        ),
    ] {
        p.call(
            "hd:Process",
            "run",
            process_args(name, &args, b""),
            Enc::default().leb(1).leb(3).str(message).0,
        );
    }
    assert_native_program(p);
}

#[test]
fn process_grant_refusal_precedes_invalid_name_and_argument_validation() {
    let scratch = Scratch::new();
    for (name, args) in [("", vec![]), ("a\0b", vec![]), ("node", vec!["a\0b"])] {
        let bytes = process_args(name, &args, b"");
        // Refusal takes precedence over native argument validation.
        let mut p = Program::new();
        p.call("hd:Process", "run", bytes, vec![1, 2]);
        assert_program(
            &scratch,
            p,
            vec![("Process", Limit::Entries(vec![]))],
            || {},
        );
    }
}

#[test]
fn repeated_identical_import_declarations_link_on_both_engines() {
    let scratch = Scratch::new();
    let file = scratch.at("file");
    fs::write(&file, "same").unwrap();
    let mut p = Program::new();
    for _ in 0..2 {
        p.call(
            "hd:FsRead",
            "read_text",
            one(&file),
            Enc::default().leb(0).str("same").0,
        );
        // Deliberately emit two import declarations of the same function.
        p.imported
            .remove(&("hd:FsRead".into(), "read_text.start".into()));
    }
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn synchronous_provider_finish_rejects_stale_handles() {
    let scratch = Scratch::new();
    for (module, methods) in [
        (
            "hd:FsRead",
            vec!["read_bytes", "read_text", "list_dir", "stat"],
        ),
        (
            "hd:FsWrite",
            vec![
                "write_bytes",
                "write_text",
                "append_text",
                "create_dir_all",
                "remove",
                "rename",
            ],
        ),
        ("hd:Process", vec!["run"]),
    ] {
        for method in methods {
            let mut p = Program::new();
            let finish = p.import(
                module,
                &format!("{method}.finish"),
                &[ValType::I32],
                &[ValType::I32],
            );
            p.instructions(&[I::I32Const(123_456), I::Call(finish), I::Drop]);
            let (wasm, _) = p.finish(0);
            let result = parity(&scratch, &wasm, vec![], || {});
            assert_eq!(result.status, 3);
            assert_eq!(
                result.stderr,
                b"panic: host-contract: a finish of an operation that is not pending\n"
            );
        }
    }
}

#[test]
fn malformed_structured_arguments_report_host_contract() {
    let scratch = Scratch::new();
    for (module, method, bytes) in [
        ("hd:FsRead", "read_bytes", vec![5, b'x']),
        ("hd:FsWrite", "rename", vec![1, b'x']),
        ("hd:Process", "run", vec![4, b'n']),
    ] {
        let mut p = Program::new();
        let call = p.import(
            module,
            &format!("{method}.start"),
            &[ValType::I32],
            &[ValType::I32, ValType::I32],
        );
        p.load_args(&bytes);
        p.instructions(&[
            I::I32Const(i32::try_from(bytes.len()).unwrap()),
            I::Call(call),
            I::Drop,
            I::Drop,
        ]);
        let (wasm, _) = p.finish(0);
        let result = parity(&scratch, &wasm, vec![], || {});
        assert_eq!(result.status, 3);
        assert_eq!(
            result.stderr,
            b"panic: host-contract: an argument does not decode\n"
        );
    }
    // A well-formed write outside the grant is refused and writes nothing.
    let outside = scratch.at("not-granted");
    // A malformed payload is the contract panic even outside the grant:
    // both engines decode every argument before the grant check.
    let mut bytes = Enc::default().str(path(&outside)).0;
    bytes.extend([5, b'x']);
    let mut p = Program::new();
    let call = p.import(
        "hd:FsWrite",
        "write_bytes.start",
        &[ValType::I32],
        &[ValType::I32, ValType::I32],
    );
    p.load_args(&bytes);
    p.instructions(&[
        I::I32Const(i32::try_from(bytes.len()).unwrap()),
        I::Call(call),
        I::Drop,
        I::Drop,
    ]);
    let (wasm, _) = p.finish(0);
    let result = parity(
        &scratch,
        &wasm,
        vec![("FsWrite", Limit::Entries(vec![]))],
        || {},
    );
    assert_eq!(result.status, 3);
    assert_eq!(
        result.stderr,
        b"panic: host-contract: an argument does not decode\n"
    );
    assert!(!outside.exists());
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "write_bytes",
        Enc::default().str(path(&outside)).bytes(&[1]).0,
        fs_error(&outside, 2),
    );
    assert_program(
        &scratch,
        p,
        vec![("FsWrite", Limit::Entries(vec![]))],
        || {},
    );
    assert!(!outside.exists());
}

#[test]
fn sys_all_reads_match_and_scope_refusals_name_the_method() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    for method in ["os", "arch", "hostname", "cpu_count"] {
        p.sys(method);
    }
    let (wasm, _) = p.finish(0);
    let result = parity(&scratch, &wasm, vec![], || {});
    assert_eq!(result.status, 0);
    let stderr = std::str::from_utf8(&result.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 4);
    assert!(
        stderr
            .lines()
            .all(|line| line.starts_with("00") && line.len() > 2)
    );
    for allowed in [vec![], vec!["os".into()]] {
        let mut p = Program::new();
        for method in ["os", "arch", "hostname", "cpu_count"] {
            if allowed.iter().any(|name| name == method) {
                continue;
            }
            p.sys(method);
            p.expected.push(Enc::default().leb(1).leb(0).str(method).0);
        }
        assert_program(&scratch, p, vec![("Sys", Limit::Entries(allowed))], || {});
    }
}

#[test]
fn random_fill_sizes_and_next_u64_have_observable_entropy() {
    let scratch = Scratch::new();
    let mut p = Program::new();
    let fill = p.import("hd:Random", "fill", &[ValType::I32], &[ValType::I32]);
    let next = p.import("hd:Random", "next_u64", &[], &[ValType::I64]);
    for (count, encoded) in [(0, 1), (1, 2), (64, 65), (128, 130)] {
        p.instructions(&[
            I::I32Const(count),
            I::Call(fill),
            I::I32Const(encoded),
            I::I32Eq,
            I::LocalSet(2),
            I::LocalGet(2),
            I::I32Const(0),
            I::I32Load8U(MEM),
            I::I32Const(if count < 128 { count } else { 128 }),
            I::I32Eq,
            I::I32And,
            I::LocalSet(2),
        ]);
        if count == 128 {
            p.instructions(&[
                I::LocalGet(2),
                I::I32Const(1),
                I::I32Load8U(MEM),
                I::I32Const(1),
                I::I32Eq,
                I::I32And,
                I::LocalSet(2),
            ]);
        }
        p.instructions(&[
            I::I32Const(0),
            I::LocalGet(2),
            I::I32Store8(MEM),
            I::I32Const(1),
            I::LocalSet(0),
        ]);
        p.dump();
        p.expected.push(vec![1]);
    }
    // No random bytes are compared between engines. Assert 64 bytes are
    // not all zero (failure probability 2^-512) and two u64s differ (2^-64).
    p.instructions(&[
        I::I32Const(64),
        I::Call(fill),
        I::Drop,
        I::I32Const(1),
        I::LocalSet(0),
        I::I32Const(0),
        I::LocalSet(2),
        I::Block(BlockType::Empty),
        I::Loop(BlockType::Empty),
        I::LocalGet(2),
        I::LocalGet(0),
        I::I32Load8U(MEM),
        I::I32Or,
        I::LocalSet(2),
        I::LocalGet(0),
        I::I32Const(1),
        I::I32Add,
        I::LocalTee(0),
        I::I32Const(65),
        I::I32LtU,
        I::BrIf(0),
        I::End,
        I::End,
        I::I32Const(0),
        I::LocalGet(2),
        I::I32Eqz,
        I::I32Eqz,
        I::I32Store8(MEM),
        I::I32Const(1),
        I::LocalSet(0),
    ]);
    p.dump();
    p.expected.push(vec![1]);
    p.instructions(&[
        I::Call(next),
        I::LocalSet(3),
        I::Call(next),
        I::LocalSet(4),
        I::I32Const(0),
        I::LocalGet(3),
        I::LocalGet(4),
        I::I64Ne,
        I::I32Store8(MEM),
        I::I32Const(1),
        I::LocalSet(0),
    ]);
    p.dump();
    p.expected.push(vec![1]);
    assert_program(&scratch, p, vec![], || {});
}

#[test]
fn filesystem_listing_uses_utf8_byte_order_and_preserves_trailing_slash() {
    let scratch = Scratch::new();
    let dir = scratch.at("listing");
    fs::create_dir(&dir).unwrap();
    for name in ["ä", "z", "A"] {
        fs::write(dir.join(name), b"x").unwrap();
    }
    let mut expected = Enc::default().leb(0).leb(3);
    for name in ["A", "z", "ä"] {
        expected = expected
            .str(&listed_path(&dir, name))
            .leb(0)
            .leb(1)
            .modified(&dir.join(name));
    }
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "list_dir",
        Enc::default().str(&format!("{}/", path(&dir))).0,
        expected.0,
    );
    assert_program(&scratch, p, vec![], || {});
}

/// Native messages are verified without treating another engine as authority.
fn assert_native_program(p: Program) {
    let (wasm, expected) = p.finish(7);
    let result = hd_run_wasmtime::run_captured(
        hd_run_wasmtime::EngineConfig::default(),
        &wasm,
        &hd_run::HostSetup {
            providers: vec![],
            grants: hd_run::Grants::default(),
            limits: hd_run::Limits::default(),
            args: vec![],
            program: String::new(),
        },
    )
    .unwrap();
    assert_eq!(result.status, 7);
    assert_eq!(result.stdout, b"core-parity\n");
    assert_eq!(result.stderr, expected.as_bytes());
}

#[test]
fn filesystem_text_preserves_utf8_bom_content() {
    let scratch = Scratch::new();
    let file = scratch.at("bom");
    fs::write(&file, "\u{feff}\u{feff}héllo").unwrap();
    let mut p = Program::new();
    p.call(
        "hd:FsRead",
        "read_text",
        one(&file),
        Enc::default().leb(0).str("\u{feff}\u{feff}héllo").0,
    );
    assert_native_program(p);
}

#[test]
fn filesystem_remove_nonempty_directory_keeps_native_context() {
    let scratch = Scratch::new();
    let dir = scratch.at("nonempty");
    fs::create_dir(&dir).unwrap();
    fs::write(dir.join("child"), b"keep").unwrap();
    let native = fs::remove_dir(&dir).unwrap_err();
    let message = format!("rmdir {:?}: {native}", path(&dir));
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "remove",
        one(&dir),
        Enc::default().leb(1).leb(7).str(&message).0,
    );
    assert_native_program(p);
    assert_eq!(fs::read(dir.join("child")).unwrap(), b"keep");
}

/// The writer cannot open the FIFO until the guest's sibling prints.
/// A host call that blocks the polling thread therefore fails this test,
/// even if it eventually returns the right bytes after the watchdog release.
#[cfg(unix)]
#[test]
fn filesystem_fifo_read_keeps_a_sibling_running_on_both_engines() {
    use hd_driver::{Executor, Goal, Host, NoClock};
    use hd_project::MemorySources;
    use hd_run::{Grants, HostSetup, Limits, Outcome};
    use hd_run_wasmtime::{EngineConfig, HostIo};
    use hd_sched::SerialOrder;
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::sync::{Arc, Mutex, mpsc};
    use std::time::Duration;

    #[derive(Clone)]
    struct ReleaseOnTick {
        bytes: Arc<Mutex<Vec<u8>>>,
        release: Arc<Mutex<Option<mpsc::Sender<()>>>>,
    }
    impl std::io::Write for ReleaseOnTick {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            let mut output = self.bytes.lock().unwrap();
            output.extend_from_slice(bytes);
            if output.windows(5).any(|part| part == b"tick\n")
                && let Some(release) = self.release.lock().unwrap().take()
            {
                let _ = release.send(());
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let template =
        fs::read_to_string(root.join("spec/conformance/cli/fs-pending-special/src/external.hd"))
            .unwrap();
    // Direct Node, supervised Node, then native, each with its own FIFO.
    for mode in 0..3 {
        let scratch = Scratch::new();
        let fifo = scratch.at("fifo");
        assert!(
            Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .unwrap()
                .success()
        );
        let (release, released) = mpsc::channel();
        let fifo_path = fifo.clone();
        let peer = std::thread::spawn(move || {
            // Release even a regressed host so it cannot leave this test
            // binary stuck in a blocking OS read. The boolean proves order.
            let sibling_ran = released.recv_timeout(Duration::from_secs(5)).is_ok();
            let until = std::time::Instant::now() + Duration::from_secs(20);
            loop {
                match rustix::fs::open(
                    fifo_path.as_path(),
                    rustix::fs::OFlags::WRONLY
                        | rustix::fs::OFlags::NONBLOCK
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                ) {
                    Ok(fd) => {
                        let mut file = fs::File::from(fd);
                        std::io::Write::write_all(&mut file, b"fifo contents").unwrap();
                        break;
                    }
                    Err(rustix::io::Errno::NXIO) if std::time::Instant::now() < until => {
                        std::thread::sleep(Duration::from_millis(5));
                    }
                    Err(error) => panic!("FIFO reader did not open: {error}"),
                }
            }
            sibling_ran
        });
        let source = format!(
            "{template}\npub fn main!() -> Result[void, FsError] $ FsRead + Clock + Console:\n    exercise!({:?})\n",
            path(&fifo),
        );
        let mut sources = MemorySources::default();
        sources.insert("main.hd", &source);
        let store = hd_cache::MemoryStore::default();
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let built = hd_driver::build(
            &host,
            "pending-fifo",
            &Goal::Program {
                entry: "main".into(),
            },
        );
        assert!(built.diags.is_empty(), "{}", built.render());
        let wasm = built.wasm.unwrap();
        let output = ReleaseOnTick {
            bytes: Arc::default(),
            release: Arc::new(Mutex::new(Some(release))),
        };
        if mode == 2 {
            let setup = HostSetup {
                providers: vec![],
                grants: Grants::default(),
                limits: Limits {
                    wall_ms: Some(20_000),
                    ..Limits::default()
                },
                args: vec![],
                program: String::new(),
            };
            let outcome = hd_run_wasmtime::run(
                EngineConfig::default(),
                &wasm,
                &setup,
                HostIo {
                    out: Box::new(output.clone()),
                    err: Box::new(Vec::<u8>::new()),
                    input: None,
                },
            )
            .unwrap();
            assert_eq!(outcome, Outcome::Exit(0));
        } else {
            let file = scratch.at("pending-fifo.wasm");
            fs::write(&file, wasm).unwrap();
            let config = scratch.at("run.json");
            fs::write(
                &config,
                if mode == 0 {
                    "{}"
                } else {
                    "{\"timeLimitMs\":20000}"
                },
            )
            .unwrap();
            let mut node = Command::new("node")
                .arg(root.join("host/run.mjs"))
                .arg(file)
                .arg(config)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let mut stdout = node.stdout.take().unwrap();
            let mut stderr = node.stderr.take().unwrap();
            let mut sink = output.clone();
            let out = std::thread::spawn(move || std::io::copy(&mut stdout, &mut sink).unwrap());
            let err = std::thread::spawn(move || {
                let mut bytes = Vec::new();
                stderr.read_to_end(&mut bytes).unwrap();
                bytes
            });
            let status = node.wait().unwrap();
            out.join().unwrap();
            let errors = err.join().unwrap();
            assert_eq!(
                status.code(),
                Some(0),
                "Node mode {mode}: {}",
                String::from_utf8_lossy(&errors)
            );
            assert!(errors.is_empty());
        }
        assert!(
            peer.join().unwrap(),
            "mode {mode}: sibling must run before the FIFO writer opens"
        );
        assert_eq!(*output.bytes.lock().unwrap(), b"tick\nfifo contents\n");
    }
}

/// Stdout stays unread until a guest sibling publishes progress through
/// `FsWrite`. Includes direct/supervised Node and a native subprocess using
/// real process streams, for normal exit and a trap with a write pending.
#[cfg(unix)]
#[test]
fn console_full_pipe_yields_and_drains_on_both_engines() {
    use hd_driver::{Executor, Goal, Host, NoClock};
    use hd_project::MemorySources;
    use hd_run::{Grants, HostSetup, Limits, Outcome};
    use hd_run_wasmtime::{EngineConfig, HostIo};
    use hd_sched::SerialOrder;
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    const CHILD: &str = "HD_816_NATIVE_OUTPUT_WASM";
    struct Child(std::process::Child);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    if let Some(wasm) = std::env::var_os(CHILD) {
        let setup = HostSetup {
            providers: vec![],
            grants: Grants::default(),
            limits: Limits::default(),
            args: vec![],
            program: String::new(),
        };
        let outcome = hd_run_wasmtime::run(
            EngineConfig::default(),
            &fs::read(wasm).unwrap(),
            &setup,
            HostIo::process(),
        )
        .unwrap();
        let Outcome::Exit(code) = outcome else {
            panic!("unexpected outcome: {outcome:?}")
        };
        std::process::exit(i32::from(code));
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let template = fs::read_to_string(
        root.join("spec/conformance/cli/console-pending-output/src/external.hd"),
    )
    .unwrap();
    for fail in [false, true] {
        for mode in 0..3 {
            let scratch = Scratch::new();
            let marker = scratch.at("progress");
            let entered = scratch.at("progress-entered");
            let source = format!(
                "{template}\npub fn main!() -> void $ Console + FsWrite + Clock:\n    exercise!({:?}, {fail})\n",
                path(&marker)
            );
            let mut sources = MemorySources::default();
            sources.insert("main.hd", &source);
            let store = hd_cache::MemoryStore::default();
            let host = Host {
                render_tir: &[],
                sources: &sources,
                store: &store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let built = hd_driver::build(
                &host,
                "pending-output",
                &Goal::Program {
                    entry: "main".into(),
                },
            );
            assert!(built.diags.is_empty(), "{}", built.render());
            let wasm = scratch.at("program.wasm");
            fs::write(&wasm, built.wasm.unwrap()).unwrap();
            let mut command = if mode == 2 {
                let mut command = Command::new(std::env::current_exe().unwrap());
                command
                    .args([
                        "--exact",
                        "core_parity::console_full_pipe_yields_and_drains_on_both_engines",
                        "--nocapture",
                        "--quiet",
                    ])
                    .env(CHILD, &wasm);
                command
            } else {
                let config = scratch.at("run.json");
                fs::write(
                    &config,
                    if mode == 0 {
                        "{}"
                    } else {
                        "{\"timeLimitMs\":20000}"
                    },
                )
                .unwrap();
                let mut command = Command::new("node");
                command
                    .arg(root.join("host/run.mjs"))
                    .arg(&wasm)
                    .arg(config);
                command
            };
            let mut child = Child(
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
            let until = Instant::now() + Duration::from_secs(10);
            while !marker.exists()
                && child.0.try_wait().unwrap().is_none()
                && Instant::now() < until
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            let progressed = marker.exists();
            let began = entered.exists();
            // Always release the reader, including the negative control,
            // so a regressed native writer cannot keep a process blocked.
            let mut stdout = child.0.stdout.take().unwrap();
            let mut stderr = child.0.stderr.take().unwrap();
            let out = std::thread::spawn(move || {
                let mut b = Vec::new();
                stdout.read_to_end(&mut b).unwrap();
                b
            });
            let err = std::thread::spawn(move || {
                let mut b = Vec::new();
                stderr.read_to_end(&mut b).unwrap();
                b
            });
            let until = Instant::now() + Duration::from_secs(15);
            let status = loop {
                if let Some(status) = child.0.try_wait().unwrap() {
                    break status;
                }
                assert!(Instant::now() < until, "output drains before deadline");
                std::thread::sleep(Duration::from_millis(5));
            };
            let stdout = out.join().unwrap();
            let stderr = err.join().unwrap();
            assert!(
                began && progressed,
                "mode {mode} fail={fail}: sibling runs before any stdout is read; stderr={}",
                String::from_utf8_lossy(&stderr)
            );
            assert_eq!(status.code(), Some(if fail { 3 } else { 0 }));
            let guest = if mode == 2 {
                stdout.strip_prefix(b"\nrunning 1 test\n").unwrap()
            } else {
                &stdout
            };
            assert_eq!(guest.len(), 2_097_152);
            assert!(
                guest.iter().all(|&b| b == b'x'),
                "every accepted byte is preserved"
            );
            if fail {
                assert!(String::from_utf8_lossy(&stderr).contains("output-panic-drain"));
            } else {
                assert!(stderr.is_empty());
            }
        }
    }
}

/// The session's out/err/ran frames share one writer. No stdout reader
/// exists until a sibling writes the independent progress marker.
#[cfg(unix)]
#[test]
fn node_session_output_frames_yield_and_keep_completion_last() {
    use hd_driver::{Executor, Goal, Host, NoClock, Packages, Session};
    use hd_project::MemorySources;
    use hd_sched::SerialOrder;
    use std::io::{Read, Write};
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let scratch = Scratch::new();
    let marker = scratch.at("progress");
    let template = fs::read_to_string(
        root.join("spec/conformance/cli/console-pending-output/src/external.hd"),
    )
    .unwrap();
    let source = format!(
        "{template}\nexercise!({:?}, false)\n$.use(Console).write_error!(\"after-output\\n\").expect(\"stderr\")\n",
        path(&marker)
    );
    let mut sources = MemorySources::default();
    sources.insert("repl#1", &source);
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let session = Session::start();
    let packages = Packages {
        session: Some(&session),
        inputs_from: Some(1),
        ..Packages::default()
    };
    let built = hd_driver::build_packages(
        &host,
        "output-session",
        &packages,
        &Goal::ReplInput { input: 1 },
    );
    assert!(built.diags.is_empty(), "{}", built.render());
    let wasm = scratch.at("session.wasm");
    fs::write(&wasm, built.wasm.unwrap()).unwrap();
    let url = format!("file://{}", path(&root.join("host/node.mjs")));
    let mut child = Command::new("node")
        .args([
            "--input-type=module",
            "-e",
            &format!("import {{ driveNodeSession }} from {url:?}; await driveNodeSession();"),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut input = child.stdin.take().unwrap();
    writeln!(input, "{{\"op\":\"open\",\"cap\":33554432}}").unwrap();
    writeln!(
        input,
        "{}",
        serde_json::json!({"op":"run","wasm":path(&wasm),"sources":[[1,source]],"live":1})
    )
    .unwrap();
    writeln!(input, "{{\"op\":\"close\"}}").unwrap();
    drop(input);
    let until = Instant::now() + Duration::from_secs(10);
    while !marker.exists() && child.try_wait().unwrap().is_none() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(5));
    }
    let progressed = marker.exists();
    let mut stdout = child.stdout.take().unwrap();
    let mut stderr = child.stderr.take().unwrap();
    let out = std::thread::spawn(move || {
        let mut b = Vec::new();
        stdout.read_to_end(&mut b).unwrap();
        b
    });
    let err = std::thread::spawn(move || {
        let mut b = Vec::new();
        stderr.read_to_end(&mut b).unwrap();
        b
    });
    let until = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() >= until {
            let _ = child.kill();
            let _ = child.wait();
            panic!("session drains its output");
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let stdout = out.join().unwrap();
    let stderr = err.join().unwrap();
    assert!(
        progressed,
        "a session sibling runs before stdout is read: {}",
        String::from_utf8_lossy(&stderr)
    );
    assert!(status.success());
    assert!(stderr.is_empty());
    let text = String::from_utf8(stdout).unwrap();
    let rows: Vec<_> = text.lines().collect();
    assert_eq!(rows.len(), 4, "ready, out, err, ran frames only");
    assert_eq!(rows[0], "ready");
    let bytes: String = serde_json::from_str(rows[1].strip_prefix("out\t").unwrap()).unwrap();
    assert_eq!(bytes, "x".repeat(2_097_152));
    assert_eq!(rows[2], "err\t\"after-output\\n\"");
    assert!(
        rows[3].starts_with("ran\t\t\t\tnull\t0\texit0\tnull\t1\t0"),
        "{}",
        rows[3]
    );
}
