//! `cargo run -p hd_driver --example bench N`: a synthetic two-folder package with N functions per folder,
//! compiled cold, warm, after a private body edit and after a comment edit,
//! each run timed. Eyeballed, not gated.

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use hd_cache::MemStore;

use crate::{Counters, SourceFile, run};

fn report(out: &mut String, label: &str, c: &Counters, total: Duration) {
    let _ = writeln!(out, "== {label}: total {total:?}");
    let _ = writeln!(out, "   tasks {:?}", c.tasks);
    let _ = writeln!(out, "   hits {:?} misses {:?}", c.hits, c.misses);
    let _ = writeln!(
        out,
        "   modules checked {:?}, interfaces built {:?}, TIR decoded {:?}, emitted {}",
        c.modules_checked, c.ifaces_built, c.tir_decoded, c.emitted
    );
    let mut times: Vec<_> = c.stage_time.iter().collect();
    times.sort_by_key(|(_, d)| std::cmp::Reverse(**d));
    let _ = writeln!(out, "   stage times {times:?}");
}

/// The bench's sources: `app/main.hd` and `geo/shapes.hd`.
#[must_use]
pub fn sources(n: usize) -> (String, String) {
    let mut geo = String::from("pub data Point:\n    pub x: i32\n    pub y: i32\n\n");
    for i in 0..n {
        let _ = write!(
            geo,
            "pub fn g{i}(a: i32, b: i32) -> i32:\n    x := a * {i} + b\n    i := +0\n    while i < 3:\n        if x % 2 == 0:\n            x = x / 2\n        else:\n            x = x * 3 + 1\n        i = i + 1\n    return x\n\n"
        );
    }
    let mut app = String::from("use pkg.geo.shapes.{");
    app.push_str(
        &(0..n)
            .map(|i| format!("g{i}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    app.push_str("}\n\n");
    for i in 0..n {
        let _ = write!(
            app,
            "fn f{i}(a: i32) -> i32:\n    return g{i}(a, {i}) + 1\n\n"
        );
    }
    app.push_str("fn main():\n    s := +0\n");
    for i in 0..n {
        let _ = writeln!(app, "    s = s + f{i}(s % 100)");
    }
    app.push_str("    println(s)\n");
    (app, geo)
}

/// Runs the bench and returns its report and the cold run's Wasm.
pub fn bench(n: usize) -> Result<(String, Vec<u8>), String> {
    let (app, geo) = sources(n);
    let files = |geo: &str| {
        vec![
            SourceFile {
                path: "app/main.hd".into(),
                text: app.clone(),
            },
            SourceFile {
                path: "geo/shapes.hd".into(),
                text: geo.to_owned(),
            },
        ]
    };
    let mut out = String::new();
    let lines = app.lines().count() + geo.lines().count();
    let _ = writeln!(out, "bench: {n} functions per folder, {lines} lines");
    let mut store = MemStore::default();
    let t = Instant::now();
    let r = run(&mut store, &files(&geo), "pkg.app.main");
    report(&mut out, "cold", &r.counters, t.elapsed());
    if !r.diagnostics.is_empty() {
        return Err(r.diagnostics[..r.diagnostics.len().min(5)].join("\n"));
    }
    let wasm = r.wasm.ok_or("no wasm")?;
    let _ = writeln!(out, "   wasm {} bytes", wasm.len());
    let t = Instant::now();
    let r = run(&mut store, &files(&geo), "pkg.app.main");
    report(&mut out, "warm, no edit", &r.counters, t.elapsed());
    let edited = geo.replace("x = x * 3 + 1", "x = x * 3 + 2");
    let t = Instant::now();
    let r = run(&mut store, &files(&edited), "pkg.app.main");
    report(
        &mut out,
        "private body edit in geo (all bodies)",
        &r.counters,
        t.elapsed(),
    );
    let edited = geo.replacen("    return x\n", "    # note\n    return x\n", 1);
    let t = Instant::now();
    let r = run(&mut store, &files(&edited), "pkg.app.main");
    report(&mut out, "comment edit in geo", &r.counters, t.elapsed());
    Ok((out, wasm))
}
