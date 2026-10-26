//! Embeds `lib/std`'s sources: until the std pack writer exists (slice 4),
//! the driver carries std as source text and builds its interfaces like any
//! dependency's (`docs/compiler/design-overview.md` §2.2 rule 2, `hd_stdpack`).

use hd_base::SliceSort as _;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

#[path = "src/strip_comments.rs"]
mod strip_comments;

fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(root, &p, out);
        } else if p.extension().is_some_and(|x| x == "hd")
            && let Ok(rel) = p.strip_prefix(root)
        {
            out.push((rel.to_string_lossy().replace('\\', "/"), p.clone()));
        }
    }
}

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let root = Path::new(&manifest).join("../../lib/std");
    let root = root.canonicalize().unwrap_or(root);
    println!("cargo::rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files.sort_stable();
    let out = std::env::var("OUT_DIR").unwrap_or_default();
    // The embedded text carries no plain `#` comments: neither the compiler
    // nor `hd doc` reads them, and they are a third of the browser build's
    // download (#363). The browser build reads no `##` doc comments either,
    // so its copy drops them too (#509); the native build keeps them for
    // `hd doc`. Both copies are written, and `STD_FILES` names the one this
    // target embeds: a cargo feature would be unified across the workspace
    // and strip the native `hd` too, while the target is per build.
    let web =
        std::env::var("CARGO_CFG_TARGET_FAMILY").is_ok_and(|f| f.split(',').any(|f| f == "wasm"));
    let mut code = String::new();
    for (name, dir, strip) in [
        (
            "STD_FILES_WITH_DOCS",
            "std",
            strip_comments::strip_plain_comments as fn(&str) -> String,
        ),
        (
            "STD_FILES_WITHOUT_DOCS",
            "std-no-docs",
            strip_comments::strip_all_comments,
        ),
    ] {
        let _ = writeln!(
            code,
            "/// `lib/std`'s files: (relative path, text).\npub const {name}: &[(&str, &str)] = &["
        );
        let stripped = Path::new(&out).join(dir);
        for (rel, abs) in &files {
            if web && matches!(rel.as_str(), "rt/checked.hd" | "rt/checked_report.hd") {
                continue;
            }
            let text = std::fs::read_to_string(abs).unwrap_or_default();
            let dest = stripped.join(rel);
            if let Some(dir) = dest.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(&dest, strip(&text));
            let _ = writeln!(
                code,
                "    ({rel:?}, include_str!({:?})),",
                dest.display().to_string()
            );
        }
        code.push_str("];\n");
    }
    let chosen = if web {
        "STD_FILES_WITHOUT_DOCS"
    } else {
        "STD_FILES_WITH_DOCS"
    };
    let _ = writeln!(
        code,
        "/// The std files this build embeds: without `##` doc comments for the\n/// browser build, with them otherwise.\npub static STD_FILES: &[(&str, &str)] = {chosen};"
    );
    let _ = std::fs::write(Path::new(&out).join("std_files.rs"), code);
    build_id(&manifest);
}

/// The compiler build id of `crates/hd_cache/DESIGN.md` §5.3's `toolchain_key`: a hash of every
/// compiler crate's Rust sources and manifests, so an entry written by one
/// compiler build never decodes under another.
fn build_id(manifest: &str) {
    let crates = Path::new(manifest).join("..");
    let crates = crates.canonicalize().unwrap_or(crates);
    println!("cargo::rerun-if-changed={}", crates.display());
    let mut files = Vec::new();
    sources(&crates, &mut files);
    files.sort_stable();
    // FNV-1a over (path, contents): stable across toolchains and runs.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for path in &files {
        let rel = path
            .strip_prefix(&crates)
            .unwrap_or(path)
            .to_string_lossy()
            .into_owned();
        for b in rel
            .bytes()
            .chain([0])
            .chain(std::fs::read(path).unwrap_or_default())
        {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    }
    println!("cargo::rustc-env=HD_BUILD_ID={h:016x}");
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs")
            || p.file_name().is_some_and(|n| n == "Cargo.toml")
        {
            out.push(p);
        }
    }
}
