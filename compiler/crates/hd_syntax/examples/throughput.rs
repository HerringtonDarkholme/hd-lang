use std::fs;
use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use hd_syntax::{lex, skim};

fn files(root: &Path, output: &mut Vec<PathBuf>) {
    for entry in
        fs::read_dir(root).unwrap_or_else(|error| panic!("read {}: {error}", root.display()))
    {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            files(&path, output);
        } else if path.extension().is_some_and(|extension| extension == "hd") {
            output.push(path);
        }
    }
}

fn measure(mut operation: impl FnMut(), bytes: usize) -> f64 {
    let start = Instant::now();
    let mut rounds = 0_u64;
    while start.elapsed() < Duration::from_millis(750) {
        operation();
        rounds += 1;
    }
    let seconds = start.elapsed().as_secs_f64();
    #[allow(clippy::cast_precision_loss)]
    let processed = bytes as f64 * rounds as f64;
    processed / seconds / (1024.0 * 1024.0)
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .expect("repository root")
        .join("lib/std");
    let mut paths = Vec::new();
    files(&root, &mut paths);
    paths.sort();
    let sources: Vec<Vec<u8>> = paths
        .iter()
        .map(|path| fs::read(path).expect("read std source"))
        .collect();
    let bytes = sources.iter().map(Vec::len).sum();
    let lex_mbps = measure(
        || {
            for source in &sources {
                black_box(lex(black_box(source)));
            }
        },
        bytes,
    );
    let skim_mbps = measure(
        || {
            for source in &sources {
                black_box(skim(black_box(source)));
            }
        },
        bytes,
    );
    println!("lib/std: {bytes} bytes; lex {lex_mbps:.1} MB/s; skim {skim_mbps:.1} MB/s");
}
