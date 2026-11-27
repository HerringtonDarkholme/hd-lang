//! C providers on Wasmtime (#2388 step 4): `hd run` opens a package's
//! `native/<target-triple>/` library, takes its entry symbol, checks the
//! table and calls `create`, binds every called slot, and calls `destroy`
//! at the instance's close, trap or not (`cli.host.c.file`, `entry`,
//! `check`, `create`, `thread`). The library is `c_providers/probe.rs`,
//! a Rust `cdylib` with the C ABI, built by `rustc` inside the test from
//! the header `hd build` writes; no binary is checked in.

use std::path::{Path, PathBuf};

use hd_testkit::Scratch;

/// `hd ARGS` in `root`, with its own cache and no engine from the
/// environment.
fn hd(root: &Path, args: &[&str]) -> std::process::Output {
    let mut command = hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"));
    command
        .env(
            "HD_CACHE",
            crate::shared_objects(root.with_extension("cache")),
        )
        .env_remove("HD_JOBS")
        .env_remove("HD_ENGINE")
        .env_remove("HD_WAT")
        .current_dir(root)
        .args(args);
    // A run that hangs fails the test instead of the whole binary.
    hd_testkit::cdylib::output_within(command, std::time::Duration::from_mins(2))
        .unwrap_or_else(|why| panic!("{why}"))
}

/// The library's file on this target.
fn library(root: &Path) -> PathBuf {
    let name = if cfg!(windows) {
        "hd_probe.dll"
    } else if cfg!(target_vendor = "apple") {
        "libhd_probe.dylib"
    } else {
        "libhd_probe.so"
    };
    root.join("native")
        .join(env!("HD_TARGET_TRIPLE"))
        .join(name)
}

/// The probe package: its manifest and program, built once so `hd build`
/// writes its header.
fn package(name: &str, main: &str) -> Scratch {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), name);
    let dir = root.path();
    std::fs::create_dir_all(dir.join("src")).expect("src");
    std::fs::write(
        dir.join("hd.toml"),
        "[package]\nname = \"probe_app\"\n\n[host]\n\"main.Probe\" = { c = \"hd_probe\" }\n\n[capabilities]\nConsole = true\n\"pkg.main.Probe\" = true\n",
    )
    .expect("manifest");
    std::fs::write(dir.join("src/main.hd"), main).expect("main");
    let built = hd(dir, &["build"]);
    assert!(
        built.status.success(),
        "{}",
        String::from_utf8_lossy(&built.stdout)
    );
    root
}

/// The contract hash `hd build` wrote for `Probe`, as Rust array text.
fn hash(root: &Path) -> String {
    let header = std::fs::read_to_string(root.join("native/include/probe_app.h")).expect("header");
    let line = header
        .lines()
        .find_map(|l| l.strip_prefix("#define HD_CONTRACT_HASH_hd_probe_Probe { "))
        .expect("the contract hash macro");
    format!("[{}]", line.trim_end_matches(" }"))
}

/// Builds the probe library into `root`'s `native/<triple>/` folder, with
/// its table's hash, ABI version and entry symbol.
fn build_library(root: &Path, hash: &str, abi: u32, entry: &str) {
    let source = include_str!("c_providers/probe.rs")
        .replace("__HASH__", hash)
        .replace("__ABI__", &abi.to_string())
        .replace("__ENTRY__", entry);
    let dir = root.join("probe-src");
    std::fs::create_dir_all(&dir).expect("library source");
    std::fs::write(dir.join("lib.rs"), source).expect("library source");
    let out = library(root);
    std::fs::create_dir_all(out.parent().expect("folder")).expect("native folder");
    hd_testkit::cdylib::build(&dir.join("lib.rs"), &out);
}

fn text(out: &[u8]) -> String {
    String::from_utf8_lossy(out).replace("\r\n", "\n")
}

/// Every row of the C value table crosses through the bound slots: each
/// integer width and sign (stack arguments past the integer registers
/// included), `f32` and `f64` (past the float registers too), `bool`, a
/// narrow and a 64-bit result, a string's bytes, a list's elements, and
/// `&mut` writes through a list, a number and a `&f64`. With no
/// `--engine`, a module whose every root trait has a C provider runs on
/// Wasmtime (decision 7). `destroy` runs at the close of a run that ends
/// and of one that traps.
#[test]
fn a_c_provider_binds_every_value_table_row_and_destroys_at_close() {
    let main = include_str!("c_providers/main.hd");
    let root = package("c-provider-round-trip", main);
    let dir = root.path();
    build_library(dir, &hash(dir), 1, "hd_probe_Probe");
    let run = hd(dir, &["run"]);
    assert!(run.status.success(), "{}", text(&run.stderr));
    assert_eq!(
        text(&run.stdout),
        "ints -123343511\nfloats 11236.0\nhalf 0.75 flag false true\nnarrow -7 wide 18446744073709551614\ntext 506\nsum 256\nfill 1 2 3 kept 0 at 44\ncount 1 2\n"
    );
    assert_eq!(text(&run.stderr), "probe destroyed\n");
    // The same module prebuilt, on a named Wasmtime.
    let prebuilt = hd(dir, &["build/debug/probe_app.wasm", "--engine", "wasmtime"]);
    assert_eq!(text(&prebuilt.stdout), text(&run.stdout));
    // A trap still closes the instance, and `destroy` runs.
    std::fs::write(
        dir.join("src/main.hd"),
        format!("{main}    panic(\"boom\")\n"),
    )
    .expect("main");
    let trapped = hd(dir, &["run"]);
    assert!(!trapped.status.success());
    let stderr = text(&trapped.stderr);
    assert!(stderr.contains("panic: explicit-panic: boom"), "{stderr}");
    assert!(stderr.contains("probe destroyed"), "{stderr}");
}

/// Each library that cannot serve the trait refuses the run before the
/// guest starts, naming the package, the target triple and the trait: a
/// missing file for the triple (`cli.host.c.target-missing`), a missing
/// entry symbol (`cli.host.c.entry`), another ABI version and another
/// contract hash (`cli.host.c.check`).
#[test]
fn a_library_that_cannot_serve_its_trait_refuses_the_run_at_startup() {
    let root = package("c-provider-refusals", include_str!("c_providers/main.hd"));
    let dir = root.path();
    let triple = env!("HD_TARGET_TRIPLE");
    let refused = |what: &str, needles: &[&str]| {
        let run = hd(dir, &["run", "--engine", "wasmtime"]);
        let stderr = text(&run.stderr);
        assert_eq!(run.status.code(), Some(101), "{what}: {stderr}");
        assert!(run.stdout.is_empty(), "{what}: the guest started");
        assert!(!stderr.contains("probe destroyed"), "{what}: {stderr}");
        for needle in ["probe_app", triple, "main.Probe"].iter().chain(needles) {
            assert!(stderr.contains(needle), "{what}: {needle:?} in {stderr}");
        }
    };
    refused("missing file", &["native-provider-missing", "hd_probe"]);
    let good = hash(dir);
    build_library(dir, &good, 1, "other_Probe");
    refused("missing symbol", &["exports no `hd_probe_Probe`"]);
    build_library(dir, &good, 2, "hd_probe_Probe");
    refused("ABI version", &["C ABI version 2", "needs version 1"]);
    build_library(dir, "[0; 16]", 1, "hd_probe_Probe");
    refused(
        "contract hash",
        &["00000000000000000000000000000000", "another version"],
    );
    // Node has no provider for a C-only trait (`cli.host.c.engine`).
    let node = hd(dir, &["run", "--engine", "node"]);
    assert!(
        text(&node.stderr).contains("native-provider-missing"),
        "{}",
        text(&node.stderr)
    );
}

/// A C-bound trait returns handles of another C-bound trait from the same
/// library (`module.host.package.c-handles`, decision 1): the header lists
/// both tables, `hd run` calls each handle's own object and destroys it at
/// its last release, and `hd test` on Wasmtime runs a task that reaches the
/// library (`cli.host.c.create`).
#[test]
fn a_c_provider_returns_handles_and_hd_test_reaches_it() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "c-provider-handles");
    let dir = root.path();
    for folder in ["src", "tasks", "tests"] {
        std::fs::create_dir_all(dir.join(folder)).expect("folder");
    }
    for (path, text) in [
        ("hd.toml", include_str!("c_providers/store_app/hd.toml")),
        (
            "src/store.hd",
            include_str!("c_providers/store_app/src/store.hd"),
        ),
        (
            "src/lib.hd",
            include_str!("c_providers/store_app/src/lib.hd"),
        ),
        (
            "tasks/walk.hd",
            include_str!("c_providers/store_app/tasks/walk.hd"),
        ),
        (
            "tests/walk.hd",
            include_str!("c_providers/store_app/tests/walk.hd"),
        ),
    ] {
        std::fs::write(dir.join(path), text).expect("package file");
    }
    let built = hd(dir, &["build"]);
    assert!(built.status.success(), "{}", text(&built.stdout));
    let header = std::fs::read_to_string(dir.join("native/include/store_app.h")).expect("header");
    assert!(
        header.contains("    void *(*open)(void *self, int32_t from);"),
        "{header}"
    );
    assert!(
        header.contains("const hd_Cursor *hd_store_Cursor(void);"),
        "{header}"
    );
    assert!(
        header.contains("NULL: a method's result makes each handle"),
        "{header}"
    );
    let hash_of = |entry: &str| {
        let line = header
            .lines()
            .find_map(|l| l.strip_prefix(&format!("#define HD_CONTRACT_HASH_{entry} {{ ")))
            .expect("the contract hash macro");
        format!("[{}]", line.trim_end_matches(" }"))
    };
    let source = include_str!("c_providers/store.rs")
        .replace("__STORE__", &hash_of("hd_store_Store"))
        .replace("__CURSOR__", &hash_of("hd_store_Cursor"));
    let src = dir.join("store-src");
    std::fs::create_dir_all(&src).expect("library source");
    std::fs::write(src.join("lib.rs"), source).expect("library source");
    let out = library(dir).with_file_name(if cfg!(windows) {
        "hd_store.dll"
    } else if cfg!(target_vendor = "apple") {
        "libhd_store.dylib"
    } else {
        "libhd_store.so"
    });
    std::fs::create_dir_all(out.parent().expect("folder")).expect("native folder");
    hd_testkit::cdylib::build(&src.join("lib.rs"), &out);
    let run = hd(dir, &["run", "walk"]);
    assert!(run.status.success(), "{}", text(&run.stderr));
    assert_eq!(text(&run.stdout), "11 12 21\n");
    let stderr = text(&run.stderr);
    for line in [
        "cursor 12 destroyed",
        "cursor 21 destroyed",
        "store destroyed",
    ] {
        assert!(stderr.contains(line), "{line}: {stderr}");
    }
    let test = hd(dir, &["test", "--engine", "wasmtime"]);
    assert!(
        test.status.success(),
        "{}{}",
        text(&test.stdout),
        text(&test.stderr)
    );
    assert!(
        text(&test.stdout).contains("1 passed"),
        "{}",
        text(&test.stdout)
    );
}

/// Builds the reader library into `dir`'s `native/<triple>/` folder from
/// `c_providers/reader.rs`, with its `read` and `read_abort` slots.
fn build_reader(dir: &Path, read: &str, read_abort: &str) {
    let header = std::fs::read_to_string(dir.join("native/include/reader_app.h")).expect("header");
    let line = header
        .lines()
        .find_map(|l| l.strip_prefix("#define HD_CONTRACT_HASH_hd_reader_Reader { "))
        .expect("the contract hash macro");
    let source = include_str!("c_providers/reader.rs")
        .replace("__HASH__", &format!("[{}]", line.trim_end_matches(" }")))
        .replace("__READ__", read)
        .replace("__READ_ABORT__", read_abort);
    let src = dir.join("reader-src");
    std::fs::create_dir_all(&src).expect("library source");
    std::fs::write(src.join("lib.rs"), source).expect("library source");
    let out = library(dir).with_file_name(if cfg!(windows) {
        "hd_reader.dll"
    } else if cfg!(target_vendor = "apple") {
        "libhd_reader.dylib"
    } else {
        "libhd_reader.so"
    });
    std::fs::create_dir_all(out.parent().expect("folder")).expect("native folder");
    hd_testkit::cdylib::build(&src.join("lib.rs"), &out);
}

/// A C provider's `!` methods complete from threads of the library's own
/// (`cli.host.c.thread`): a `List[u8]` and a `Result`'s `i32` error
/// (`cli.host.c.bytes`), and an `i64`. A `race!` whose timer wins cancels a
/// pending C read: its abort slot runs once, before the guest goes on, with
/// `completed` false, and the provider is destroyed after it
/// (`cli.host.c.abort`, `cli.host.c.token`). Before that, a library that
/// leaves a called method's slot or its abort slot empty refuses the run
/// before the guest starts or any `create` runs (`cli.host.c.slot`,
/// `cli.host.c.abort-slot`).
#[test]
fn a_race_cancels_a_c_read_and_runs_its_abort_slot_once() {
    let root = Scratch::new(env!("CARGO_TARGET_TMPDIR"), "c-provider-suspends");
    let dir = root.path();
    std::fs::create_dir_all(dir.join("src")).expect("folder");
    for (path, text) in [
        ("hd.toml", include_str!("c_providers/reader_app/hd.toml")),
        (
            "src/main.hd",
            include_str!("c_providers/reader_app/src/main.hd"),
        ),
    ] {
        std::fs::write(dir.join(path), text).expect("package file");
    }
    let built = hd(dir, &["build"]);
    assert!(built.status.success(), "{}", text(&built.stdout));
    for (read, abort, empty) in [
        ("None", "Some(abort)", "slot"),
        ("Some(read)", "None", "abort slot"),
    ] {
        build_reader(dir, read, abort);
        let run = hd(dir, &["run"]);
        let stderr = text(&run.stderr);
        assert_eq!(run.status.code(), Some(101), "{stderr}");
        assert!(run.stdout.is_empty(), "the guest started: {stderr}");
        for needle in [
            format!("`hd_reader` of package `reader_app` leaves the {empty} of"),
            "main.Reader\"'s method `read` empty".to_owned(),
        ] {
            assert!(stderr.contains(&needle), "{needle}: {stderr}");
        }
        assert!(!stderr.contains("reader destroyed"), "created: {stderr}");
    }
    build_reader(dir, "Some(read)", "Some(abort)");
    let run = hd(dir, &["run"]);
    assert!(run.status.success(), "{}", text(&run.stderr));
    assert_eq!(
        text(&run.stdout),
        "ok 2 [104, 105]\nerror 5\ntotal 400000000000\nerror -1\naborts 1 discarded 0\n"
    );
    assert_eq!(text(&run.stderr), "read aborted\nreader destroyed\n");
}
