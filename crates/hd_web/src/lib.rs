#![forbid(unsafe_code)]
//! `hd_web`: the playground's compiler (design-overview.md §2.1;
//! playground-wasm.md §3.1). A [`Session`] is one worker's state: the
//! playground's files and one in-memory cache store, kept across compiles
//! so an edit recompiles only what it changed. Each compile is one serial
//! driver run with a clock that reads zero, since `Instant::now()` panics
//! on wasm32. The wasm exports are a later step.

use hd_cache::MemoryStore;
use hd_driver::diagnostics::{Diag, from_output};
use hd_driver::{Counters, Executor, Host, NoClock, Packages, TestCase, build_packages};
use hd_project::{MemorySources, Root, in_program, module_below};
use hd_sched::SerialOrder;

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
    /// The program, for Run and Test, when no diagnostic is an error.
    pub wasm: Option<Vec<u8>>,
    /// In content order, with the files as the playground named them.
    pub diagnostics: Vec<Diag>,
    /// For Test, the selected cases in content order.
    pub tests: Vec<TestCase>,
    /// What the run did: tasks, and store hits and misses.
    pub counters: Counters,
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
        let (sources, goal) = match goal {
            Goal::Run => (
                &program,
                hd_driver::Goal::Program {
                    entry: module_below(entry),
                },
            ),
            Goal::Check => (&self.sources, hd_driver::Goal::Analyze),
            Goal::Test => (
                &self.sources,
                hd_driver::Goal::Tests {
                    module: None,
                    filter: None,
                },
            ),
        };
        let host = Host {
            render_tir: &[],
            sources,
            store: &self.store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        // One file outside the `src` layout is a single-file program, as
        // `hd FILE.hd` builds a file with no `hd.toml` above it.
        let packages = Packages {
            single_file: count == 1 && Root::of(entry).0 == Root::Flat,
            ..Packages::default()
        };
        let out = build_packages(&host, PACKAGE, &packages, &goal);
        // No host writes the entries back yet, so their list only grows.
        self.store.drain_new();
        if self.store.bytes() > STORE_CAP_BYTES {
            self.store = MemoryStore::default();
        }
        CompileResult {
            diagnostics: from_output(&out, None),
            wasm: out.wasm,
            tests: out.tests,
            counters: out.counters,
        }
    }
}
