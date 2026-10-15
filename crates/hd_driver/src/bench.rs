//! `cargo run -p hd_driver --example bench N`: a synthetic two-folder
//! package with N functions per folder, compiled cold, warm, after a
//! private body edit and after a comment edit; and `--example rebuild`:
//! the rebuild scope of body and signature edits on a fan-out package
//! (#49). Eyeballed, not gated. The example supplies the clock.

use std::fmt::Write as _;
use std::sync::Arc;

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
    let _ = writeln!(out, "   graph parts computed {:?}", c.parts_computed);
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
            "pub fn shape_{i}(a: i32, b: i32) -> i32:\n    let x = a * {i} + b\n    let i = +0\n    while i < 3:\n        if x % 2 == 0:\n            x = x / 2\n        else:\n            x = x * 3 + 1\n        i = i + 1\n    return x\n\n"
        );
    }
    let mut app = String::from("use pkg.geo.shapes.{");
    app.push_str(
        &(0..n)
            .map(|i| format!("shape_{i}"))
            .collect::<Vec<_>>()
            .join(", "),
    );
    app.push_str("}\n\n");
    for i in 0..n {
        let _ = write!(
            app,
            "fn step_{i}(a: i32) -> i32:\n    return shape_{i}(a, {i}) + 1\n\n"
        );
    }
    app.push_str("pub fn main() -> void $ Console:\n    let s = +0\n");
    for i in 0..n {
        let _ = writeln!(app, "    s = s + step_{i}(s % 100)");
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

/// A fan-out package: `core/util.hd`, imported by `n` leaf folders
/// `f0/leaf.hd` .. `f{n-1}/leaf.hd`, each imported by `app/main.hd`.
/// Every leaf calls all of `core.util`'s functions, one of them with a
/// closure, so the optimized pipeline's `closure_spec` sees a cross-module
/// callee.
#[must_use]
pub fn fanout_sources(n: usize) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("core/util.hd", FANOUT_CORE);
    let mut main = String::new();
    for i in 0..n {
        let _ = writeln!(main, "use pkg.f{i}.leaf.{{run_{i}}}");
        s.insert(
            &format!("f{i}/leaf.hd"),
            &format!(
                "use pkg.core.util.{{scale, apply, clamp}}\n\nfn helper(a: i32) -> i32:\n    return a + {i}\n\npub fn run_{i}(a: i32) -> i32:\n    return clamp(helper(scale(a, {i})) + apply(fn(y: i32) -> i32: y + {i}, a))\n"
            ),
        );
    }
    main.push_str("\npub fn main() -> void $ Console:\n    let s = +0\n");
    for i in 0..n {
        let _ = writeln!(main, "    s = s + run_{i}(s % 100)");
    }
    main.push_str("    println(s)\n");
    s.insert("app/main.hd", &main);
    s
}

/// `core/util.hd` of [`fanout_sources`].
pub const FANOUT_CORE: &str = "\
pub fn scale(a: i32, k: i32) -> i32:
    let x = a * k
    let i = +0
    while i < 3:
        x = x + i
        i = i + 1
    return x

pub fn apply(f: fn(i32) -> i32, x: i32) -> i32:
    f(x)

pub fn clamp(a: i32) -> i32:
    if a < 0:
        return 0
    return a % 1000
";

/// The edits [`rebuild_scope`] measures, as (label, file, from, to).
pub const FANOUT_EDITS: [(&str, &str, &str, &str); 4] = [
    (
        "(a) body edit in a leaf",
        "f1/leaf.hd",
        "return clamp(helper(",
        "return clamp(1 + helper(",
    ),
    (
        "(b) body edit in core.util",
        "core/util.hd",
        "        x = x + i\n",
        "        x = x + i + 1\n",
    ),
    (
        "(b2) body edit in core.util that stops calling a closure param",
        "core/util.hd",
        "    f(x)\n",
        "    x\n",
    ),
    (
        "(c) signature edit in core.util",
        "core/util.hd",
        "pub fn clamp(a: i32) -> i32:\n    if a < 0:\n        return 0\n    return a % 1000\n",
        "pub fn clamp(a: i32, hi: i32 = 1000) -> i32:\n    if a < 0:\n        return 0\n    return a % hi\n",
    ),
];

/// [`fanout_sources`] with `file`'s `from` replaced by `to`.
#[must_use]
pub fn fanout_edited(n: usize, file: &str, from: &str, to: &str) -> MemorySources {
    let mut s = fanout_sources(n);
    let text = hd_project::SourceSet::read(&s, file).unwrap_or_else(|| Arc::from(&b""[..]));
    let text = String::from_utf8_lossy(&text).into_owned();
    assert!(text.contains(from), "{file} has no `{from}`");
    s.insert(file, &text.replace(from, to));
    s
}

/// The fan-out package with `n` leaves at `level`: a cold build, a warm
/// rebuild, then each of [`FANOUT_EDITS`] on the warm store, with the
/// counters of each run and whether its Wasm equals a clean build's.
pub fn rebuild_scope(
    n: usize,
    level: crate::OptLevel,
    clock: &dyn Clock,
    executor: Executor,
) -> Result<String, String> {
    let mut out = String::new();
    let _ = writeln!(out, "fan-out: {n} leaves, {level:?}, {executor:?}");
    let goal = Goal::Program {
        entry: "app.main".into(),
    };
    let packages = crate::Packages {
        level,
        ..crate::Packages::default()
    };
    let run = |store: &MemoryStore, src: &MemorySources| {
        let host = Host {
            render_tir: &[],
            sources: src,
            store,
            clock,
            executor,
        };
        let t = clock.now_ns();
        let r = crate::build_packages(&host, "fan", &packages, &goal);
        (r, clock.now_ns().saturating_sub(t))
    };
    let store = MemoryStore::default();
    let base = fanout_sources(n);
    let (cold, ns) = run(&store, &base);
    if cold.wasm.is_none() {
        return Err(cold.render());
    }
    report(&mut out, "cold", &cold.counters, ns);
    let (warm, ns) = run(&store, &base);
    report(&mut out, "warm, no edit", &warm.counters, ns);
    for (label, file, from, to) in FANOUT_EDITS {
        let src = fanout_edited(n, file, from, to);
        let (r, ns) = run(&store, &src);
        if r.wasm.is_none() {
            return Err(r.render());
        }
        report(&mut out, label, &r.counters, ns);
        let (clean, _) = run(&MemoryStore::default(), &src);
        let same = clean.wasm == r.wasm;
        let _ = writeln!(out, "   wasm equals a clean build's: {same}");
        if !same {
            return Err(out);
        }
        // Back to the base, so each edit starts from the same warm store.
        run(&store, &base);
    }
    Ok(out)
}
