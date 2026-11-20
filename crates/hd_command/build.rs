//! Embeds the Node host (`host/*.ts`) as plain JavaScript (#1697).
//!
//! Node strips a `.ts` module's types with a Wasm module of its own, which
//! costs every fresh process about 50 ms at V8's default tier and 350 ms
//! with `--no-liftoff`. `hd` writes the host to a fresh directory per run,
//! so no compile cache can keep the stripped text. The build strips it
//! once instead, with Node's own stripper (`scripts/ship-host.ts`), and
//! `node.rs` embeds the result as `HOST`.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

/// The listing `scripts/ship-host.ts` prints, kept beside a staged host.
const LISTING: &str = "ship-host.txt";

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    // A published package holds the host already stripped, as `host/`:
    // `scripts/publish-crates.ts` stages it, so `cargo install hd-lang` needs
    // no Node and reads nothing outside the package. A checkout never has
    // that directory, because the script stages outside the source tree.
    let staged = Path::new(&manifest).join("host");
    let (host, listing) = if staged.is_dir() {
        println!("cargo::rerun-if-changed={}", staged.display());
        let listing = std::fs::read_to_string(staged.join(LISTING))
            .unwrap_or_else(|e| panic!("the staged host has no {LISTING}: {e}"));
        (staged, listing)
    } else {
        strip_host(&manifest)
    };
    let mut code = String::from(
        "/// The Node host's modules as plain JavaScript, by their path in the\n\
         /// directory a run writes them to (`build.rs`).\n\
         pub(crate) const HOST: &[(&str, &str)] = &[\n",
    );
    let mut engine = None;
    for line in listing.lines() {
        if let Some(name) = line.strip_prefix("out ") {
            let _ = writeln!(
                code,
                "    ({name:?}, include_str!({:?})),",
                host.join(name).display()
            );
        } else if let Some(version) = line.strip_prefix("node ") {
            engine = Some(version.to_owned());
        } else if let Some(source) = line.strip_prefix("src ") {
            println!(
                "cargo::rerun-if-changed={}",
                Path::new(&manifest).join("../..").join(source).display()
            );
        } else {
            panic!("ship-host.ts printed an unknown line: {line}");
        }
    }
    code.push_str("];\n");
    let engine = engine.expect("ship-host.ts prints the `node` engine line");
    let _ = writeln!(
        code,
        "/// The oldest Node `hd test` runs on: package.json's `engines.node`.\n\
         pub(crate) const NODE_ENGINE: &str = {engine:?};"
    );
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    std::fs::write(Path::new(&out_dir).join("host.rs"), code).expect("write host.rs");
}

/// Strips the checkout's host into `OUT_DIR/host` with Node, and returns
/// that directory and the script's listing.
fn strip_host(manifest: &str) -> (PathBuf, String) {
    let repository = Path::new(manifest).join("../..");
    let out_dir = std::env::var("OUT_DIR").expect("OUT_DIR");
    let host = Path::new(&out_dir).join("host");
    // A module the host no longer loads must not linger in the embedded set.
    let _ = std::fs::remove_dir_all(&host);
    let script = repository.join("scripts/ship-host.ts");
    let output = Command::new("node")
        .arg("--no-warnings")
        .arg(&script)
        .arg(&host)
        .output()
        .unwrap_or_else(|e| {
            panic!("building hd_command runs Node to strip the host's types (`node` on PATH): {e}")
        });
    assert!(
        output.status.success(),
        "{} failed: {}",
        script.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    let listing = String::from_utf8(output.stdout).expect("ship-host.ts prints UTF-8");
    (host, listing)
}
