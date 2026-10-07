use std::time::Instant;

use hd_driver::{Counters, MemStore, SourceFile, run};

fn files(app: &str, geo: &str) -> Vec<SourceFile> {
    vec![
        SourceFile {
            path: "app/main.hd".into(),
            text: app.to_owned(),
        },
        SourceFile {
            path: "geo/shapes.hd".into(),
            text: geo.to_owned(),
        },
    ]
}

fn print_report(label: &str, total: std::time::Duration, counters: &Counters) {
    let mut stages: Vec<_> = counters.stage_time.iter().collect();
    stages.sort_by_key(|(_, duration)| std::cmp::Reverse(**duration));
    println!("== {label}: total {total:?}");
    println!("   stage times {stages:?}");
    println!("   hits {:?} misses {:?}", counters.hits, counters.misses);
    println!(
        "   checked {:?}, interfaces {:?}, emitted {}",
        counters.modules_checked, counters.ifaces_built, counters.emitted
    );
}

fn main() {
    let n = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let (app, geo) = hd_driver::bench::sources(n);
    let mut store = MemStore::default();

    let start = Instant::now();
    let cold = run(&mut store, &files(&app, &geo), "pkg.app.main");
    print_report("cold seed", start.elapsed(), &cold.counters);
    if !cold.diagnostics.is_empty() {
        panic!("cold diagnostics: {:?}", cold.diagnostics);
    }

    let signature = geo.replacen(
        "pub fn g0(a: i32, b: i32) -> i32:",
        "pub fn g0(a: i32, b: i32, unused: i32) -> i32:",
        1,
    );
    let caller = app.replacen("g0(a, 0)", "g0(a, 0, 0)", 1);
    let start = Instant::now();
    let edited = run(&mut store, &files(&caller, &signature), "pkg.app.main");
    print_report(
        "public signature and caller edit",
        start.elapsed(),
        &edited.counters,
    );
    if !edited.diagnostics.is_empty() {
        panic!("signature diagnostics: {:?}", edited.diagnostics);
    }
}
