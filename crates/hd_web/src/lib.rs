#![deny(unsafe_code)]
//! `hd_web`: the playground's compiler (design-overview.md §2.1;
//! playground-wasm.md §3.1). A [`Session`] is one worker's state: the
//! playground's files and one in-memory cache store, kept across compiles
//! so an edit recompiles only what it changed. Each compile is one serial
//! driver run with a clock that reads zero, since `Instant::now()` panics
//! on wasm32. On wasm32 the crate exports four functions over that session
//! (the `exports` module); [`wire`] holds their request and response format.

use hd_cache::MemoryStore;
use hd_driver::diagnostics::{Diag, from_output};
use hd_driver::{
    Counters, Executor, Host, NoClock, Packages, TestCase, build_packages, test_programs,
};
use hd_project::{MemorySources, Root, in_program, module_below};
use hd_sched::SerialOrder;

pub mod wire;

// Owner decision 2026-10-12 (playground-wasm.md §6, Q1): the wasm exports
// carry `#[unsafe(no_mangle)]`. This module is the only exception to the
// crate's `deny(unsafe_code)`, and it has no `unsafe` blocks.
#[cfg(target_arch = "wasm32")]
#[allow(unsafe_code)]
mod exports;

/// The playground package's name, which its declarations print under.
pub const PACKAGE: &str = "playground";

/// The store size past which a session drops its store: the next compile
/// is then cold (playground-wasm.md §3.1).
pub const STORE_CAP_BYTES: usize = 64 << 20;

/// What a compile is for: the playground's Run, Check and Test buttons.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Goal {
    /// The entry file as a program, as `hd run` builds it; a file without
    /// a public `main` is a script, whose top-level statements run.
    Run,
    /// Every stage over the files, as `hd check` runs them.
    Check,
    /// The test program of every `tests:` block, as `hd test` builds it.
    Test,
}

/// What one compile produced.
pub struct CompileResult {
    /// The programs, when no diagnostic is an error: Run's one program;
    /// for Test, the unit-test program, then each integration test file's,
    /// each built on its own as `hd test` builds them
    /// ([`hd_driver::test_programs`]). Check builds none.
    pub programs: Vec<Program>,
    /// In content order, with the files as the playground named them; a
    /// diagnostic two test programs share is listed once.
    pub diagnostics: Vec<Diag>,
    /// For Test, the selected cases of every program, ordered by file and
    /// line as `hd test` reports them (`cli.test.report.order`), each with
    /// its program's index in [`CompileResult::programs`].
    pub tests: Vec<(usize, TestCase)>,
    /// What the first build did: tasks, and store hits and misses.
    pub counters: Counters,
}

/// One program of a compile.
pub struct Program {
    /// For Test, the integration test file it is; `None` for Run's program
    /// and the unit-test program.
    pub file: Option<String>,
    /// `None` when the build made no module.
    pub wasm: Option<Vec<u8>>,
}

/// One playground worker's state.
#[derive(Default)]
pub struct Session {
    /// The files of the last compile.
    sources: MemorySources,
    store: MemoryStore,
}

impl Session {
    #[must_use]
    pub fn new() -> Session {
        Session::default()
    }

    /// Compiles `files`, each a playground path (`src/main.hd`) and its
    /// text, for `goal`. `entry` is the file Run builds.
    pub fn compile<P, T>(
        &mut self,
        files: impl IntoIterator<Item = (P, T)>,
        entry: &str,
        goal: Goal,
    ) -> CompileResult
    where
        P: AsRef<str>,
        T: AsRef<str>,
    {
        self.sources = MemorySources::default();
        // Run builds one program: the library, the executables and the
        // entry, as `hd run FILE` narrows a package.
        let mut program = MemorySources::default();
        let mut count = 0;
        for (path, text) in files {
            let (path, text) = (path.as_ref(), text.as_ref());
            self.sources.insert(path, text);
            if in_program(entry, path) {
                program.insert(path, text);
            }
            count += 1;
        }
        // One file outside the `src` layout is a single-file program, as
        // `hd FILE.hd` builds a file with no `hd.toml` above it.
        let packages = Packages {
            single_file: count == 1 && Root::of(entry).0 == Root::Flat,
            ..Packages::default()
        };
        // Each build: its integration test file, its sources and its goal.
        let builds: Vec<(Option<String>, MemorySources, hd_driver::Goal)> = match goal {
            Goal::Run => vec![(
                None,
                program,
                hd_driver::Goal::Program {
                    entry: module_below(entry),
                },
            )],
            Goal::Check => vec![(None, self.sources.clone(), hd_driver::Goal::Analyze)],
            Goal::Test => test_programs(PACKAGE, &self.sources, None, &|_| false)
                .into_iter()
                .map(|t| {
                    let mut sources = self.sources.clone();
                    sources.retain(|p| t.keep.iter().any(|k| k == p));
                    let goal = hd_driver::Goal::Tests {
                        module: t.module,
                        filter: None,
                    };
                    (t.file, sources, goal)
                })
                .collect(),
        };
        let mut result = CompileResult {
            programs: Vec::new(),
            diagnostics: Vec::new(),
            tests: Vec::new(),
            counters: Counters::default(),
        };
        let mut built = Vec::new();
        for (file, sources, goal) in &builds {
            let host = Host {
                render_tir: &[],
                sources,
                store: &self.store,
                clock: &NoClock,
                executor: Executor::Serial(SerialOrder::Priority),
            };
            let out = build_packages(&host, PACKAGE, &packages, goal);
            for d in from_output(&out, None) {
                if !result.diagnostics.contains(&d) {
                    result.diagnostics.push(d);
                }
            }
            let index = built.len();
            if index == 0 {
                result.counters = out.counters;
            }
            result
                .tests
                .extend(out.tests.into_iter().map(|case| (index, case)));
            built.push((file.clone(), out.wasm));
        }
        // No host writes the entries back yet, so their list only grows.
        self.store.drain_new();
        if self.store.bytes() > STORE_CAP_BYTES {
            self.store = MemoryStore::default();
        }
        if goal != Goal::Check && !result.diagnostics.iter().any(Diag::is_error) {
            result.programs = built
                .into_iter()
                .map(|(file, wasm)| Program { file, wasm })
                .collect();
        }
        result
            .tests
            .sort_by(|(_, a), (_, b)| (&a.file, a.line).cmp(&(&b.file, b.line)));
        result
    }
}
