//! Parses each `.hd` file under the given paths and prints every
//! diagnostic as `hd check` does, `error: path:line:column: code: message`:
//! `cargo run -p hd_syntax --example parse_check -- PATH...`. With
//! `--tree`, prints each file's tree. Exits 101 when any file has a
//! diagnostic, so `scripts/hd-conformance.sh` serves it as the `parse`
//! action of the conformance command contract.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use hd_syntax::parse;

fn files(root: &Path, output: &mut Vec<PathBuf>) {
    if root.is_file() {
        output.push(root.to_path_buf());
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "hd") {
            output.push(path);
        }
    }
}

fn main() -> ExitCode {
    let mut tree = false;
    let mut paths = Vec::new();
    for argument in std::env::args().skip(1) {
        if argument == "--tree" {
            tree = true;
        } else {
            files(Path::new(&argument), &mut paths);
        }
    }
    paths.sort();
    let mut clean = 0;
    for path in &paths {
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let parsed = parse(source.as_bytes());
        if tree {
            print!("{}", parsed.tree.debug_tree(&parsed.tokens, &source));
        }
        if parsed.diagnostics.is_empty() {
            clean += 1;
        }
        for diagnostic in &parsed.diagnostics {
            let lo = diagnostic.primary.lo as usize;
            let line = parsed.tokens.line_of(diagnostic.primary.lo) + 1;
            let before = source.get(..lo).unwrap_or(&source);
            let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            println!(
                "error: {}:{line}:{column}: {}: {}",
                path.display(),
                diagnostic.code.as_str(),
                diagnostic.message
            );
        }
    }
    println!("{clean}/{} clean", paths.len());
    if paths.is_empty() {
        eprintln!("error: no .hd file under the given paths");
        return ExitCode::from(101);
    }
    if clean == paths.len() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(101)
    }
}
