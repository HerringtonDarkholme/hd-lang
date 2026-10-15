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
            .0,
    );
    p.call(
        "hd:FsRead",
        "stat",
        one(&sub),
        Enc::default().leb(0).leb(1).str(path(&sub)).leb(1).leb(0).0,
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
            .str(&listed_path(&dir, "z"))
            .leb(1)
            .leb(0)
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
            .str(&listed_path(&allowed, "file"))
            .leb(0)
            .leb(5)
            .str(&listed_path(&allowed, "good"))
            .leb(2)
            .leb(path(&file).len() as u64)
            .0,
    );
    p.call(
        "hd:FsRead",
        "read_text",
        one(&escaped),
        fs_error(&escaped, 2),
    );
    p.call("hd:FsRead", "stat", one(&bad), fs_error(&bad, 2));
    p.call(
        "hd:FsWrite",
        "create_dir_all",
        one(&nonexistent),
        fs_error(&nonexistent, 2),
    );
    p.call(
        "hd:FsWrite",
        "rename",
        Enc::default().str(path(&file)).str(path(&escaped)).0,
        fs_error(&escaped, 2),
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
    let mut e = Enc::default().str(name).leb(argv.len() as u64);
    for arg in argv {
        e = e.str(arg);
    }
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
        Enc::default().leb(0).str("\0�é").str("�\0é").leb(46).0,
    );
    p.call_waiting(
        "hd:Process",
        "run",
        process_args("hd-missing-program-parity-778811", &[], b""),
        vec![1, 0],
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
        Enc::default().leb(0).str("out").str("err").leb(14).0,
    );
    assert_native_program(p);
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
        Enc::default().leb(0).str("ok").str("").leb(0).0,
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
        Enc::default().leb(0).str("").str("").leb(286).0,
    );
    p.call_waiting(
        "hd:Process",
        "run",
        process_args(path(&script), &[], b""),
        vec![1, 1],
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
            .leb(u64::try_from(output.status.code().unwrap_or(128)).unwrap() * 2),
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
    p.call_waiting("hd:Process", "run", process_args("", &[], b""), vec![1, 0]);
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
    // fsOp checks its path grant before decoding the write payload.
    let outside = scratch.at("not-granted");
    let mut p = Program::new();
    p.call(
        "hd:FsWrite",
        "write_bytes",
        one(&outside),
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
        expected = expected.str(&listed_path(&dir, name)).leb(0).leb(1);
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
