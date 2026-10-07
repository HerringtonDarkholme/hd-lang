//! `hd-walk run DIR [ENTRY]`: the skeleton's `hd run`. Compiles every `.hd`
//! file under DIR (folder = directory), writes `DIR/../walk-out.wasm` and runs
//! it on V8 through `host/run.mjs`. `hd-walk bench N`: a synthetic two-folder
//! package with N functions per folder, cold, warm and edited runs, timed.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use hd_walk::driver::{Counters, MemStore, SourceFile, run};

fn collect(dir: &Path, root: &Path, out: &mut Vec<SourceFile>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).expect("read dir").flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::path);
    for e in entries {
        let p = e.path();
        if p.is_dir() {
            collect(&p, root, out);
        } else if p.extension().is_some_and(|x| x == "hd") {
            let rel = p.strip_prefix(root).expect("prefix").to_string_lossy().replace('\\', "/");
            out.push(SourceFile { path: rel, text: std::fs::read_to_string(&p).expect("read") });
        }
    }
}

fn report(label: &str, c: &Counters, total: std::time::Duration) {
    eprintln!("== {label}: total {total:?}");
    eprintln!("   tasks {:?}", c.tasks);
    eprintln!("   hits {:?} misses {:?}", c.hits, c.misses);
    eprintln!("   modules checked {:?}, interfaces built {:?}, emitted {}", c.modules_checked, c.ifaces_built, c.emitted);
    let mut times: Vec<_> = c.stage_time.iter().collect();
    times.sort_by_key(|(_, d)| std::cmp::Reverse(**d));
    eprintln!("   stage times {times:?}");
}

fn node(wasm: &[u8], path: &Path) -> String {
    std::fs::write(path, wasm).expect("write wasm");
    let host = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("host/run.mjs");
    let out = Command::new("node").arg(host).arg(path).output().expect("node");
    eprint!("{}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn bench(n: usize) {
    let mut geo = String::from("pub data Point:\n    pub x: i32\n    pub y: i32\n\n");
    for i in 0..n {
        geo.push_str(&format!(
            "pub fn g{i}(a: i32, b: i32) -> i32:\n    x := a * {i} + b\n    i := +0\n    while i < 3:\n        if x % 2 == 0:\n            x = x / 2\n        else:\n            x = x * 3 + 1\n        i = i + 1\n    return x\n\n"
        ));
    }
    let mut app = String::from("use pkg.geo.shapes.{");
    app.push_str(&(0..n).map(|i| format!("g{i}")).collect::<Vec<_>>().join(", "));
    app.push_str("}\n\n");
    for i in 0..n {
        app.push_str(&format!("fn f{i}(a: i32) -> i32:\n    return g{i}(a, {i}) + 1\n\n"));
    }
    app.push_str("fn main():\n    s := +0\n");
    for i in 0..n {
        app.push_str(&format!("    s = s + f{i}(s % 100)\n"));
    }
    app.push_str("    println(s)\n");
    let files = |geo: &str| {
        vec![
            SourceFile { path: "app/main.hd".into(), text: app.clone() },
            SourceFile { path: "geo/shapes.hd".into(), text: geo.to_owned() },
        ]
    };
    let lines = app.lines().count() + geo.lines().count();
    eprintln!("bench: {n} functions per folder, {lines} lines");
    let mut store = MemStore::default();
    let t = Instant::now();
    let r = run(&mut store, &files(&geo), "pkg.app.main");
    report("cold", &r.counters, t.elapsed());
    assert!(r.diagnostics.is_empty(), "{:?}", &r.diagnostics[..r.diagnostics.len().min(5)]);
    let wasm = r.wasm.expect("wasm");
    eprintln!("   wasm {} bytes", wasm.len());
    let out = std::env::temp_dir().join(format!("hd-walk-bench-{}.wasm", std::process::id()));
    print!("{}", node(&wasm, &out));
    let t = Instant::now();
    let r = run(&mut store, &files(&geo), "pkg.app.main");
    report("warm, no edit", &r.counters, t.elapsed());
    let edited = geo.replace("x = x * 3 + 1", "x = x * 3 + 2");
    let t = Instant::now();
    let r = run(&mut store, &files(&edited), "pkg.app.main");
    report("private body edit in geo (all bodies)", &r.counters, t.elapsed());
    let edited = geo.replacen("    return x\n", "    # note\n    return x\n", 1);
    let t = Instant::now();
    let r = run(&mut store, &files(&edited), "pkg.app.main");
    report("comment edit in geo", &r.counters, t.elapsed());
    let _ = std::fs::remove_file(out);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("run") => {
            let dir = PathBuf::from(args.get(2).expect("DIR"));
            let entry = args.get(3).cloned().unwrap_or_else(|| "pkg.app.main".into());
            let mut files = Vec::new();
            collect(&dir, &dir, &mut files);
            let mut store = MemStore::default();
            let t = Instant::now();
            let r = run(&mut store, &files, &entry);
            if !r.diagnostics.is_empty() {
                for d in &r.diagnostics {
                    eprintln!("error: {d}");
                }
                std::process::exit(1);
            }
            report("compile", &r.counters, t.elapsed());
            let out = std::env::temp_dir().join(format!("hd-walk-{}.wasm", std::process::id()));
            print!("{}", node(&r.wasm.expect("wasm"), &out));
            let _ = std::fs::remove_file(out);
        }
        Some("bench") => bench(args.get(2).and_then(|n| n.parse().ok()).unwrap_or(200)),
        _ => eprintln!("usage: hd-walk run DIR [ENTRY] | hd-walk bench [N]"),
    }
}
