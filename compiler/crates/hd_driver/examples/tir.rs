//! Builds a program and prints the checked TIR of the named items:
//! `cargo run -p hd_driver --example tir -- DIR ENTRY ITEM...`, with items
//! as `std/console/println` or `app/main/main`.

use std::path::{Path, PathBuf};

use hd_driver::{Executor, Goal, Host, NoClock, build};
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
    let Some(dir) = args.first() else {
        eprintln!("usage: tir DIR ENTRY ITEM...");
        return;
    };
    let root = PathBuf::from(dir);
    let mut sources = MemorySources::default();
    walk(&root, &root, &mut sources);
    let entry = args.get(1).cloned().unwrap_or_else(|| "main".into());
    let items: Vec<&str> = args.iter().skip(2).map(String::as_str).collect();
    let store = hd_cache::MemoryStore::default();
    let host = Host {
        render_tir: &items,
        sources: &sources,
        store: &store,
        clock: &NoClock,
        executor: Executor::Serial(hd_sched::SerialOrder::Priority),
    };
    let out = build(&host, "app", &Goal::Program { entry });
    for (path, text) in &out.tir_text {
        println!("== {path}\n{text}");
    }
    print!("{}", out.render());
    println!("wasm: {:?}", out.wasm.as_ref().map(Vec::len));
}
