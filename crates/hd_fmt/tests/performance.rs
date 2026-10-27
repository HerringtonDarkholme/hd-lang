use std::{
    fs,
    hint::black_box,
    path::{Path, PathBuf},
    time::Instant,
};

use hd_base::SliceSort as _;

fn hd_files(path: &Path, files: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = fs::read_dir(path)
        .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
        .map(|entry| entry.expect("stdlib directory entry").path())
        .collect();
    entries.sort_stable();
    for entry in entries {
        if entry.is_dir() {
            hd_files(&entry, files);
        } else if entry.extension().is_some_and(|extension| extension == "hd") {
            files.push(entry);
        }
    }
}

fn median(samples: &mut [u128]) -> u128 {
    samples.sort_stable();
    samples[samples.len() / 2]
}

fn measure(sources: &[String], format: bool) -> u128 {
    let started = Instant::now();
    for source in sources {
        if format {
            black_box(hd_fmt::format(black_box(source)).expect("stdlib source formats"));
        } else {
            black_box(hd_syntax::parse(black_box(source.as_bytes())));
        }
    }
    started.elapsed().as_nanos()
}

fn ratio_thousandths(numerator: u128, denominator: u128) -> (u128, u128) {
    let thousandths = numerator * 1_000 / denominator;
    (thousandths / 1_000, thousandths % 1_000)
}

#[test]
fn formatting_the_stdlib_stays_within_three_times_parsing() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = Vec::new();
    hd_files(&root.join("lib/std"), &mut files);
    assert!(!files.is_empty(), "the stdlib performance corpus is empty");
    let authored: Vec<_> = files
        .iter()
        .map(|path| {
            fs::read_to_string(path)
                .unwrap_or_else(|error| panic!("could not read {}: {error}", path.display()))
        })
        .collect();
    // The source rewrite deliberately lands after the formatter. Measure the
    // canonical stdlib projection that the rewrite will commit, so this test
    // is stable before and after that mechanical pass.
    let sources: Vec<_> = authored
        .iter()
        .map(|source| hd_fmt::format(source).expect("stdlib source formats"))
        .collect();

    black_box(measure(&sources, false));
    black_box(measure(&sources, true));
    let mut parse = Vec::new();
    let mut format = Vec::new();
    for round in 0..9 {
        if round % 2 == 0 {
            parse.push(measure(&sources, false));
            format.push(measure(&sources, true));
        } else {
            format.push(measure(&sources, true));
            parse.push(measure(&sources, false));
        }
    }
    let parse = median(&mut parse);
    let format = median(&mut format);
    let (ratio_whole, ratio_fraction) = ratio_thousandths(format, parse);
    eprintln!(
        "stdlib formatter budget: parse={parse}ns format={format}ns \
         ratio={ratio_whole}.{ratio_fraction:03}"
    );
    assert!(
        format <= parse * 3,
        "formatting exceeded the 3x parse budget: parse={parse}ns format={format}ns \
         ratio={ratio_whole}.{ratio_fraction:03}"
    );
}
