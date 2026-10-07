//! Runs every stage of the design over one package and prints how far each
//! stage gets: `cargo run -p hd_driver --example stages -- DIR [PACKAGE]`.
//! With no argument it reads `lib/std` as package `std`.

use std::path::{Path, PathBuf};

use hd_driver::architecture::analyze_package;
use hd_project::MemorySources;

fn walk(root: &Path, dir: &Path, out: &mut MemorySources) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            walk(root, &p, out);
        } else if p.extension().is_some_and(|x| x == "hd") {
            let rel = p
                .strip_prefix(root)
                .expect("under root")
                .to_string_lossy()
                .replace('\\', "/");
            if let Ok(text) = std::fs::read_to_string(&p) {
                out.insert(&rel, &text);
            }
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let default = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib/std");
    let root = args.first().map_or(default, PathBuf::from);
    let package = args.get(1).map_or("std", String::as_str);
    let mut sources = MemorySources::default();
    walk(&root, &root, &mut sources);
    let report = analyze_package(package, &sources);
    print!("{}", report.render());
    println!("\nmost frequent not-implemented reasons:");
    let mut reasons: Vec<(&String, &usize)> = report.reasons.iter().collect();
    reasons.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    for (r, n) in reasons.into_iter().take(15) {
        println!("{n:>4}  {r}");
    }
}
