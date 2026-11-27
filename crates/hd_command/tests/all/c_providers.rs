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
    hd_testkit::node::child(env!("CARGO_BIN_EXE_hd"))
        .env(
            "HD_CACHE",
            crate::shared_objects(root.with_extension("cache")),
        )
        .env_remove("HD_JOBS")
        .env_remove("HD_ENGINE")
        .env_remove("HD_WAT")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run hd")
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
    let rustc = std::process::Command::new("rustc")
        .args(["--crate-type", "cdylib", "--edition", "2024", "-O", "-o"])
        .arg(&out)
        .arg(dir.join("lib.rs"))
        .output()
        .expect("rustc builds the test library");
    assert!(
        rustc.status.success(),
        "{}",
        String::from_utf8_lossy(&rustc.stderr)
    );
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
