//! The architecture driver over the real `lib/std`: every stage is called,
//! none panics, and the stages that are real today get every file through.

use std::path::{Path, PathBuf};

use hd_base::Stage;
use hd_driver::analyze_package;
use hd_project::MemorySources;

fn walk(root: &Path, dir: &Path, out: &mut MemorySources, n: &mut usize) {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .expect("dir")
        .flatten()
        .map(|e| e.path())
        .collect();
    paths.sort();
    for p in paths {
        if p.is_dir() {
            walk(root, &p, out, n);
        } else if p.extension().is_some_and(|x| x == "hd") {
            let rel = p
                .strip_prefix(root)
                .expect("root")
                .to_string_lossy()
                .into_owned();
            out.insert(&rel, &std::fs::read_to_string(&p).expect("read"));
            *n += 1;
        }
    }
}

#[test]
fn every_stage_runs_on_std() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../lib/std");
    let mut s = MemorySources::default();
    let mut files = 0;
    walk(&root, &root, &mut s, &mut files);
    let r = analyze_package("std", &s);
    println!("{}", r.render());
    assert_eq!(r.tally(Stage::Skim).ok, files);
    assert_eq!(
        r.tally(Stage::Parse).ok + r.tally(Stage::Parse).not_implemented,
        files
    );
    assert_eq!(r.tally(Stage::FolderGraph).ok, 1);
    for st in Stage::ALL {
        let t = r.tally(st);
        assert!(
            t.ok + t.not_implemented + t.blocked > 0,
            "stage {} was never called",
            st.name()
        );
    }
}
