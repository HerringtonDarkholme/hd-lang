#![deny(unsafe_code)]
//! `hd_web`: the playground's compiler (design-overview.md §2.1;
//! playground-wasm.md §3.1). A [`Session`] is one worker's state: the
//! playground's files and one in-memory cache store, kept across compiles
//! so an edit recompiles only what it changed. Each compile is one serial
//! driver run with a clock that reads zero, since `Instant::now()` panics
//! on wasm32. On wasm32 the crate exports four functions over that session
//! (the `exports` module); [`wire`] holds their request and response format.
//!
//! A session also holds one REPL session (repl-plan.md job W3): the
//! playground's files as its package, and the inputs that joined it. Each
//! REPL request builds one module of that session, which the JS host links
//! to the modules before it and runs; the host then reports how the module
//! ended, and only a module that ended with status 0 joins the session
//! (`cli.repl.panic.binding`, `cli.repl.panic.declarations`).

use hd_cache::MemoryStore;
use hd_driver::diagnostics::{Diag, from_output};
use hd_driver::{
    BuildProfile, Counters, EngineTarget, Executor, Host, NoClock, OptLevel, Packages, TestCase,
    build_packages, test_programs,
};
use hd_project::{MemorySources, Root, in_program, module_below, repl_file, repl_input};
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

/// The pipeline of every build (opt-pipeline.md §5): the playground's
/// programs and REPL modules run in a browser, unoptimized.
const PIPELINE: (EngineTarget, OptLevel) = (EngineTarget::Browser, OptLevel::Dev);

#[cfg(test)]
thread_local! {
    /// The pipeline of each build this thread ran, for the tests that
    /// check what each request builds with.
    static BUILT: std::cell::RefCell<Vec<(EngineTarget, OptLevel)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Notes the pipeline that built `out`, as the driver reports it.
#[cfg(test)]
fn note_built(out: &hd_driver::Output) {
    BUILT.with_borrow_mut(|built| built.push((out.target, out.level)));
}

#[cfg(not(test))]
const fn note_built(_: &hd_driver::Output) {}

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

/// What one REPL request built: one module of the session.
pub struct ReplBuild {
    /// The module's number: 0 for the session's prelude, then 1, 2, ... for
    /// the inputs, as diagnostics name them (`repl#3`). A failed input uses
    /// its number up, as on the CLI.
    pub input: u32,
    /// The module, when no diagnostic is an error. The session waits for
    /// its outcome ([`Session::repl_outcome`]) before it builds another.
    pub wasm: Option<Vec<u8>>,
    /// The module's exports that are session keys
    /// ([`hd_wasm::is_session_key`]), in export order: the host registers
    /// them under [`hd_wasm::SESSION_MODULE`] for the modules after it,
    /// once the module joined. The entry exports (`hd.init`, `hd.poll`,
    /// `hd.wake`) are not among them.
    pub session_keys: Vec<String>,
    /// In content order; an input's file is `repl#N`.
    pub diagnostics: Vec<Diag>,
}

/// Why a REPL request does not fit the session's state: no session, a
/// module still waiting for its outcome, or an outcome for another module.
#[derive(Debug, PartialEq, Eq)]
pub struct ReplError(pub String);

/// The REPL session of a worker.
struct Repl {
    /// The playground's files, and the `repl#N` file of each input that
    /// joined or waits for its outcome.
    sources: MemorySources,
    /// What the modules that joined export, for the next build.
    built: hd_driver::Session,
    /// The number of the last module built.
    last: u32,
    /// The module that waits for its outcome, with what it adds to the
    /// session once it ran with status 0.
    waiting: Option<(u32, hd_driver::Session)>,
}

/// One playground worker's state.
#[derive(Default)]
pub struct Session {
    /// The files of the last compile.
    sources: MemorySources,
    store: MemoryStore,
    /// The REPL session, once one started and while its prelude ran.
    repl: Option<Repl>,
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
            target: PIPELINE.0,
            level: PIPELINE.1,
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
            note_built(&out);
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
        self.tidy_store();
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

impl Session {
    /// Drops the store's new-entry list, and the store itself once it
    /// passes [`STORE_CAP_BYTES`].
    fn tidy_store(&mut self) {
        // No host writes the entries back yet, so their list only grows.
        self.store.drain_new();
        if self.store.bytes() > STORE_CAP_BYTES {
            self.store = MemoryStore::default();
        }
    }

    /// Starts a REPL session over `files`, which act as its package
    /// (`cli.repl.package.lib`: `src/lib.hd` when present), and builds its
    /// prelude, module 0 ([`hd_driver::build_repl_prelude`]). A session
    /// that ran before is dropped. The prelude waits for its outcome as
    /// every module does; a prelude that does not build ends the session.
    ///
    /// # Errors
    ///
    /// A file named as an input is (`repl#3`).
    pub fn repl_start<P, T>(
        &mut self,
        files: impl IntoIterator<Item = (P, T)>,
    ) -> Result<ReplBuild, ReplError>
    where
        P: AsRef<str>,
        T: AsRef<str>,
    {
        self.repl = None;
        let mut sources = MemorySources::default();
        for (path, text) in files {
            let path = path.as_ref();
            if repl_input(path).is_some() {
                return Err(ReplError(format!("the file {path} is named as an input")));
            }
            sources.insert(path, text.as_ref());
        }
        let host = Host {
            render_tir: &[],
            sources: &sources,
            store: &self.store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let out = hd_driver::build_repl_prelude(&host, BuildProfile::Checked, PIPELINE.0);
        let (build, added) = finish(0, out);
        self.tidy_store();
        if let Some(added) = added {
            self.repl = Some(Repl {
                sources,
                built: hd_driver::Session::start(),
                last: 0,
                waiting: Some((0, added)),
            });
        }
        Ok(build)
    }

    /// Builds `text` as the session's next input (`Goal::ReplInput`) over
    /// the inputs that joined, numbered one after the last module built. An
    /// input that does not build is dropped at once; one that builds waits
    /// for its outcome.
    ///
    /// # Errors
    ///
    /// No session started, or the last module still waits for its outcome.
    pub fn repl_input(&mut self, text: &str) -> Result<ReplBuild, ReplError> {
        let last = self.repl.as_ref().ok_or_else(no_session)?.last;
        self.repl_input_as(last + 1, text)
    }

    /// Builds `text` as the session's input `number`, as [`repl_input`]
    /// does. A host that numbers its inputs itself passes the number, so a
    /// session it rebuilds in a fresh worker keeps the numbers that its
    /// diagnostics, `dbg` lines and journal already show: it builds the
    /// kept inputs again with their old numbers, skipping the dropped ones.
    ///
    /// [`repl_input`]: Session::repl_input
    ///
    /// # Errors
    ///
    /// No session started, the last module still waits for its outcome, or
    /// `number` does not come after the last module built.
    pub fn repl_input_as(&mut self, number: u32, text: &str) -> Result<ReplBuild, ReplError> {
        let repl = self.repl.as_mut().ok_or_else(no_session)?;
        if let Some((n, _)) = &repl.waiting {
            return Err(ReplError(format!("module {n} has no outcome yet")));
        }
        if number <= repl.last {
            let last = repl.last;
            return Err(ReplError(format!(
                "input {number} does not come after module {last}"
            )));
        }
        repl.last = number;
        let file = repl_file(number);
        repl.sources.insert(&file, text);
        let host = Host {
            render_tir: &[],
            sources: &repl.sources,
            store: &self.store,
            clock: &NoClock,
            executor: Executor::Serial(SerialOrder::Priority),
        };
        let packages = Packages {
            build_profile: BuildProfile::Checked,
            target: PIPELINE.0,
            level: PIPELINE.1,
            session: Some(&repl.built),
            ..Packages::default()
        };
        let goal = hd_driver::Goal::ReplInput { input: number };
        let out = build_packages(&host, PACKAGE, &packages, &goal);
        let (build, added) = finish(number, out);
        match added {
            Some(added) => repl.waiting = Some((number, added)),
            None => repl.sources.retain(|p| p != file),
        }
        self.tidy_store();
        Ok(build)
    }

    /// Takes the outcome of module `input`, the one that waits: with
    /// `joined`, it ran and ended with status 0, so it joins the session
    /// and later inputs see its names. Otherwise it failed to instantiate,
    /// panicked, was stopped or ended with another status, and adds
    /// nothing (`cli.repl.panic.binding`, `cli.repl.panic.declarations`);
    /// a prelude that did not join ends the session.
    ///
    /// # Errors
    ///
    /// No session started, or `input` is not the module that waits.
    pub fn repl_outcome(&mut self, input: u32, joined: bool) -> Result<(), ReplError> {
        let repl = self.repl.as_mut().ok_or_else(no_session)?;
        let Some((_, added)) = repl.waiting.take_if(|(n, _)| *n == input) else {
            return Err(ReplError(format!(
                "module {input} does not wait for its outcome"
            )));
        };
        if joined {
            repl.built.add(added);
        } else if input == 0 {
            self.repl = None;
        } else {
            let file = repl_file(input);
            repl.sources.retain(|p| p != file);
        }
        Ok(())
    }
}

fn no_session() -> ReplError {
    ReplError("no REPL session: start one first".to_owned())
}

/// A REPL build's answer from the driver's output, and what the module
/// adds to the session once it ran; `None` when it did not build.
fn finish(input: u32, out: hd_driver::Output) -> (ReplBuild, Option<hd_driver::Session>) {
    note_built(&out);
    let mut diagnostics =
        hd_driver::diagnostics::for_input(from_output(&out, None), &repl_file(input));
    let mut build = ReplBuild {
        input,
        wasm: None,
        session_keys: Vec::new(),
        diagnostics: Vec::new(),
    };
    let mut added = None;
    if !diagnostics.iter().any(Diag::is_error) {
        if let (Some(wasm), Some(adds)) = (out.wasm, out.session_added) {
            build.session_keys = session_keys(&wasm);
            build.wasm = Some(wasm);
            added = Some(adds);
        } else {
            let file = repl_file(input);
            diagnostics.push(Diag::error(None, "no Wasm produced").at(&file, None));
        }
    }
    build.diagnostics = diagnostics;
    (build, added)
}

/// The exports of `wasm` that are session keys, in export order.
fn session_keys(wasm: &[u8]) -> Vec<String> {
    let mut keys = Vec::new();
    for payload in wasmparser::Parser::new(0).parse_all(wasm) {
        if let Ok(wasmparser::Payload::ExportSection(exports)) = payload {
            for export in exports.into_iter().flatten() {
                if hd_wasm::is_session_key(export.name) {
                    keys.push(export.name.to_owned());
                }
            }
        }
    }
    keys
}

#[cfg(test)]
mod pipeline_tests {
    use super::{BUILT, Goal, Session};
    use hd_driver::{EngineTarget, OptLevel};

    /// Every build of a session, its Run, Check and Test compiles and its
    /// REPL modules, is tuned for the browser, unoptimized
    /// (opt-pipeline.md §5).
    #[test]
    fn every_build_is_for_the_browser_in_the_dev_pipeline() {
        let browser = (EngineTarget::Browser, OptLevel::Dev);
        let mut session = Session::new();
        let files = [(
            "main.hd",
            "pub fn main() -> void $ Console:\n    println(\"open\")\n\ntests:\n    it(\"runs\"):\n        pass\n",
        )];
        BUILT.take();
        for goal in [Goal::Run, Goal::Check, Goal::Test] {
            let result = session.compile(files, "main.hd", goal);
            assert!(result.diagnostics.is_empty(), "{goal:?}");
            assert_eq!(BUILT.take(), [browser], "{goal:?}");
        }
        let prelude = session.repl_start(files).expect("a session");
        assert!(prelude.wasm.is_some());
        session.repl_outcome(0, true).expect("the prelude joins");
        let input = session.repl_input("x := +1\n").expect("an input");
        assert!(input.diagnostics.is_empty());
        assert_eq!(BUILT.take(), [browser, browser]);
    }
}
