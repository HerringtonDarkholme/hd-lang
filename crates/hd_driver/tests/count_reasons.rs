//! The count-reason corpus walk (#1399, #1362): builds every corpus module
//! in release (or dev, below), tallies each emitted count operation by
//! corpus part, side (`app` or `std` bodies) and reason, and records a
//! digest of each module's plain Wasm for byte-identity checks between two
//! commits.
//!
//! `cargo test -p hd_driver --test count_reasons -- --ignored --nocapture`
//!
//! - `HD_CORPUS_EVERY=N` builds every Nth module of each part (default 1).
//! - `HD_CORPUS_OUT=DIR` writes `digests.tsv` and `reasons.tsv` there,
//!   and `counts.tsv`: each module's items with their count operations.
//! - `HD_CORPUS_DEV=1` builds in dev (checked profile, dev pipeline)
//!   instead of release.
//!
//! The parts: `conformance` (runtime cases of `spec/conformance`), `stage`
//! (every crate's `tests/cases`), `examples`, `tour` and `guide` (hd blocks
//! of `website/tour` and `website/guide`), `bench` (`bench/study`,
//! `bench/runtime/progs`). std's bodies are the `std` side of each part. A
//! module that does not build is counted as skipped.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use hd_base::SliceSort as _;
use hd_cache::MemoryStore;
use hd_driver::{BuildProfile, Executor, Goal, Host, NoClock, OptLevel, Packages, build_packages};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

struct Module {
    part: &'static str,
    name: String,
    files: Vec<(String, String)>,
    program: bool,
}

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut paths: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
    paths.sort_stable();
    for p in paths {
        if p.is_dir() {
            walk(&p, ext, out);
        } else if p.extension().is_some_and(|e| e == ext) {
            out.push(p);
        }
    }
}

fn rel(p: &Path) -> String {
    p.strip_prefix(root())
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

/// A stage case: `# @file: x.hd` starts another virtual module.
fn split_files(text: &str) -> Vec<(String, String)> {
    let mut files = vec![("main.hd".to_owned(), String::new())];
    for line in text.lines() {
        if let Some(name) = line.strip_prefix("# @file: ") {
            let name = name.trim().to_owned();
            if files.len() == 1 && files[0].1.trim().lines().all(|l| l.starts_with('#')) {
                files[0].0 = name;
            } else {
                files.push((name, String::new()));
            }
            continue;
        }
        let last = files.last_mut().expect("a file");
        last.1.push_str(line);
        last.1.push('\n');
    }
    files
}

fn modules() -> Vec<Module> {
    let r = root();
    let mut out = Vec::new();
    // Conformance: runtime cases, each one file.
    let tsv = std::fs::read_to_string(r.join("spec/conformance/cases.tsv")).expect("cases.tsv");
    for line in tsv.lines().skip(1) {
        let f: Vec<&str> = line.split('\t').collect();
        if f.get(1) != Some(&"runtime") {
            continue;
        }
        let path = r.join("spec/conformance").join(f[0]);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        out.push(Module {
            part: "conformance",
            name: rel(&path),
            files: vec![("main.hd".into(), text)],
            program: true,
        });
    }
    // Stage cases of every crate.
    let mut cases = Vec::new();
    for c in ["hd_syntax", "hd_check", "hd_wasm_linear", "hd_driver"] {
        walk(
            &r.join("crates").join(c).join("tests/cases"),
            "hd",
            &mut cases,
        );
    }
    for p in cases {
        let text = std::fs::read_to_string(&p).expect("case");
        out.push(Module {
            part: "stage",
            name: rel(&p),
            files: split_files(&text),
            program: false,
        });
    }
    // Examples: a top-level file alone; a directory as one package, rooted
    // at `src/` when it has one.
    let mut entries: Vec<PathBuf> = std::fs::read_dir(r.join("examples"))
        .expect("examples")
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort_stable();
    for e in entries {
        let (base, mut paths) = if e.is_dir() {
            let base = if e.join("src").is_dir() {
                e.join("src")
            } else {
                e.clone()
            };
            let mut v = Vec::new();
            walk(&base, "hd", &mut v);
            (base, v)
        } else if e.extension().is_some_and(|x| x == "hd") {
            (e.parent().expect("dir").to_path_buf(), vec![e.clone()])
        } else {
            continue;
        };
        if paths.is_empty() {
            continue;
        }
        let single = paths.len() == 1;
        let files: Vec<(String, String)> = paths
            .drain(..)
            .map(|p| {
                let name = if single {
                    "main.hd".to_owned()
                } else {
                    p.strip_prefix(&base)
                        .expect("base")
                        .to_string_lossy()
                        .into_owned()
                };
                (name, std::fs::read_to_string(&p).expect("example"))
            })
            .collect();
        out.push(Module {
            part: "examples",
            name: rel(&e),
            program: files.iter().any(|(_, t)| t.contains("fn main")),
            files,
        });
    }
    // Tour and guide: each ```hd block.
    for (part, dir) in [("tour", "website/tour"), ("guide", "website/guide")] {
        let mut mds = Vec::new();
        walk(&r.join(dir), "md", &mut mds);
        for md in mds {
            let text = std::fs::read_to_string(&md).expect("md");
            let mut block: Option<String> = None;
            let mut k = 0;
            for line in text.lines() {
                match &mut block {
                    None if line.trim_start() == "```hd" => block = Some(String::new()),
                    Some(b) if line.trim_start() == "```" => {
                        k += 1;
                        let program = b.contains("fn main");
                        out.push(Module {
                            part,
                            name: format!("{}#{k}", rel(&md)),
                            files: vec![("main.hd".into(), std::mem::take(b))],
                            program,
                        });
                        block = None;
                    }
                    Some(b) => {
                        b.push_str(line);
                        b.push('\n');
                    }
                    None => {}
                }
            }
        }
    }
    // Bench programs.
    for dir in ["bench/study", "bench/runtime/progs"] {
        let mut v = Vec::new();
        let Ok(rd) = std::fs::read_dir(r.join(dir)) else {
            continue;
        };
        v.extend(
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "hd")),
        );
        v.sort_stable();
        for p in v {
            out.push(Module {
                part: "bench",
                name: rel(&p),
                files: vec![(
                    "main.hd".into(),
                    std::fs::read_to_string(&p).expect("bench"),
                )],
                program: true,
            });
        }
    }
    out
}

/// A count operation's reason, from its debug form (`reason: LastUse`,
/// `reason: Conservative("entry")`) as `last use`, `conservative: entry`.
fn reason_of(debug: &str) -> String {
    let Some(at) = debug.rfind("reason: ") else {
        return "none".into();
    };
    let r = debug[at + 8..].trim_end_matches([' ', '}']);
    if let Some(site) = r.strip_prefix("Conservative(\"") {
        return format!("conservative: {}", site.trim_end_matches("\")"));
    }
    let mut s = String::new();
    for (i, c) in r.chars().enumerate() {
        if c.is_uppercase() && i > 0 {
            s.push(' ');
        }
        s.push(c.to_ascii_lowercase());
    }
    s
}

fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0100_0000_01b3)
    })
}

type Tally = BTreeMap<(String, String, String, String), usize>;

fn build(store: &MemoryStore, m: &Module, origins: bool) -> hd_driver::Output {
    let mut sources = MemorySources::default();
    for (name, text) in &m.files {
        sources.insert(name, text);
    }
    let host = Host {
        render_tir: &[],
        sources: &sources,
        store,
        clock: &NoClock,
        executor: Executor::Serial(SerialOrder::Priority),
    };
    let dev = std::env::var_os("HD_CORPUS_DEV").is_some();
    let packages = Packages {
        build_profile: if dev {
            BuildProfile::Checked
        } else {
            BuildProfile::Release
        },
        level: if dev {
            OptLevel::Dev
        } else {
            OptLevel::Optimized
        },
        instruction_origins: origins,
        ..Packages::default()
    };
    let goal = if m.program {
        Goal::Program {
            entry: "main".into(),
        }
    } else {
        Goal::Fixture {
            module: m.files[0].0.trim_end_matches(".hd").into(),
        }
    };
    build_packages(&host, "app", &packages, &goal)
}

fn run(mods: &[&Module], tally: &mut Tally, digests: &mut Vec<String>) {
    let store = MemoryStore::default();
    for m in mods {
        let plain =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| build(&store, m, false)));
        let Ok(plain) = plain else {
            digests.push(format!("{}\t{}\tpanic", m.part, m.name));
            continue;
        };
        let Some(wasm) = plain.wasm.as_ref().filter(|_| !plain.diags.has_errors()) else {
            digests.push(format!("{}\t{}\tskipped", m.part, m.name));
            *tally
                .entry((m.part.into(), "-".into(), "skipped".into(), "-".into()))
                .or_default() += 1;
            continue;
        };
        digests.push(format!("{}\t{}\t{:016x}", m.part, m.name, fnv(wasm)));
        *tally
            .entry((m.part.into(), "-".into(), "built".into(), "-".into()))
            .or_default() += 1;
        let mapped = build(&store, m, true);
        let mut per_item: BTreeMap<&str, usize> = BTreeMap::new();
        for f in &mapped.instruction_origins {
            let Some(item) = &f.item else { continue };
            *per_item.entry(item).or_default() += f.counts.len();
            let side = if item.starts_with("std/") {
                "std"
            } else {
                "app"
            };
            for c in &f.counts {
                let key = (
                    m.part.to_owned(),
                    side.to_owned(),
                    reason_of(&format!("{c:?}")),
                    c.action.name().to_owned(),
                );
                *tally.entry(key).or_default() += 1;
            }
        }
        for (item, n) in per_item.into_iter().filter(|&(_, n)| n > 0) {
            digests.push(format!("~{}\t{}\t{item}\t{n}", m.part, m.name));
        }
    }
}

#[test]
#[ignore = "corpus walk; run by hand (#1399)"]
fn count_reasons() {
    let every: usize = std::env::var("HD_CORPUS_EVERY")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1)
        .max(1);
    let all = modules();
    let mut by_part: BTreeMap<&str, usize> = BTreeMap::new();
    let mods: Vec<&Module> = all
        .iter()
        .filter(|m| {
            let k = by_part.entry(m.part).or_default();
            *k += 1;
            (*k - 1).is_multiple_of(every)
        })
        .collect();
    let threads = std::env::var("RUST_TEST_THREADS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(2usize)
        .max(1);
    let chunks: Vec<Vec<&Module>> = (0..threads)
        .map(|t| mods.iter().skip(t).step_by(threads).copied().collect())
        .collect();
    let results: Vec<(Tally, Vec<String>)> = std::thread::scope(|s| {
        let hs: Vec<_> = chunks
            .iter()
            .map(|c| {
                s.spawn(move || {
                    let (mut t, mut d) = (Tally::new(), Vec::new());
                    run(c, &mut t, &mut d);
                    (t, d)
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().expect("worker")).collect()
    });
    let (mut tally, mut digests) = (Tally::new(), Vec::new());
    for (t, d) in results {
        for (k, n) in t {
            *tally.entry(k).or_default() += n;
        }
        digests.extend(d);
    }
    digests.sort_stable();
    let mut reasons = String::from("part\tside\treason\taction\tcount\n");
    for ((part, side, reason, action), n) in &tally {
        let _ = writeln!(reasons, "{part}\t{side}\t{reason}\t{action}\t{n}");
    }
    print!("{reasons}");
    if let Ok(dir) = std::env::var("HD_CORPUS_OUT") {
        let dir = PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("out dir");
        std::fs::write(dir.join("reasons.tsv"), &reasons).expect("reasons");
        let (counts, digests): (Vec<String>, Vec<String>) =
            digests.into_iter().partition(|d| d.starts_with('~'));
        let counts: Vec<&str> = counts.iter().map(|c| &c[1..]).collect();
        std::fs::write(dir.join("digests.tsv"), digests.join("\n") + "\n").expect("digests");
        std::fs::write(dir.join("counts.tsv"), counts.join("\n") + "\n").expect("counts");
    }
}
