//! Embeds `lib/std`'s sources: until the std pack writer exists (slice 4),
//! the driver carries std as source text and builds its interfaces like any
//! dependency's (design-overview.md §2.1, `hd_stdpack`).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

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
    let root = Path::new(&manifest).join("../../../lib/std");
    let root = root.canonicalize().unwrap_or(root);
    println!("cargo::rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    walk(&root, &root, &mut files);
    files.sort();
    let mut code = String::from(
        "/// `lib/std`'s files: (relative path, text).\npub static STD_FILES: &[(&str, &str)] = &[\n",
    );
    for (rel, abs) in &files {
        let _ = writeln!(
            code,
            "    ({rel:?}, include_str!({:?})),",
            abs.display().to_string()
        );
    }
    code.push_str("];\n");
    let out = std::env::var("OUT_DIR").unwrap_or_default();
    let _ = std::fs::write(Path::new(&out).join("std_files.rs"), code);
}
