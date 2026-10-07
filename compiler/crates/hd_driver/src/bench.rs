//! `cargo run -p hd_driver --example bench N`: a synthetic two-folder
//! package with N functions per folder, compiled cold, warm, after a
//! private body edit and after a comment edit. Eyeballed, not gated. The
//! example supplies the clock.

use std::fmt::Write as _;

use hd_cache::MemoryStore;
use hd_project::MemorySources;

use crate::{Clock, Counters, Executor, Goal, Host, build};

fn report(out: &mut String, label: &str, c: &Counters, total_ns: u64) {
    let _ = writeln!(
        out,
        "== {label}: total {:?}",
        std::time::Duration::from_nanos(total_ns)
    );
    let _ = writeln!(out, "   tasks {:?}", c.tasks);
    let _ = writeln!(out, "   hits {:?} misses {:?}", c.hits, c.misses);
    let _ = writeln!(
        out,
        "   modules checked {:?}, interfaces built {:?}, TIR decoded {:?}, emitted {}",
        c.modules_checked, c.ifaces_built, c.tir_decoded, c.emitted
    );
    let mut times: Vec<_> = c.stage_ns.iter().collect();
    times.sort_by_key(|(_, d)| std::cmp::Reverse(**d));
    let _ = writeln!(out, "   stage ns {times:?}");
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
pub fn bench(n: usize, clock: &dyn Clock, executor: Executor) -> Result<(String, Vec<u8>), String> {
    let (app, geo) = sources(n);
    let files = |geo: &str| {
        let mut s = MemorySources::default();
        s.insert("app/main.hd", &app);
        s.insert("geo/shapes.hd", geo);
        s
    };
    let mut out = String::new();
    let lines = app.lines().count() + geo.lines().count();
    let _ = writeln!(
        out,
        "bench: {n} functions per folder, {lines} lines, {executor:?}"
    );
    let store = MemoryStore::default();
    let goal = Goal::Program {
        entry: "app.main".into(),
    };
    let run = |label: &str, src: &MemorySources, out: &mut String| {
        let host = Host {
            render_tir: &[],
            sources: src,
            store: &store,
            clock,
            executor,
        };
        let t = clock.now_ns();
        let r = build(&host, "bench", &goal);
        report(out, label, &r.counters, clock.now_ns().saturating_sub(t));
        r
    };
    let r = run("cold", &files(&geo), &mut out);
    let Some(wasm) = r.wasm.clone() else {
        return Err(r.render());
    };
    let _ = writeln!(out, "   wasm {} bytes", wasm.len());
    run("warm, no edit", &files(&geo), &mut out);
    run(
        "private body edit in geo (all bodies)",
        &files(&geo.replace("x = x * 3 + 1", "x = x * 3 + 2")),
        &mut out,
    );
    run(
        "comment edit in geo",
        &files(&geo.replacen("    return x\n", "    # note\n    return x\n", 1)),
        &mut out,
    );
    Ok((out, wasm))
}
