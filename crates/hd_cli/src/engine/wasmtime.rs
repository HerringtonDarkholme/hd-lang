//! The wasmtime engine behind `--engine wasmtime`, built with the
//! `wasmtime` feature. `no_wasmtime.rs` has the same items for a build
//! without it; `engine.rs` picks one and holds the rest of engine
//! selection.

use hd_run::{HostSetup, Outcome};

use super::{CaseRun, HostCase, ProgramLimits, TestEnv, TestPrograms};

/// Why this build lacks the engine: nothing, it has it.
pub(crate) const UNAVAILABLE: Option<&str> = None;

/// A test module prepared for the engine's workers.
pub(crate) type Prepared = hd_run_wasmtime::PreparedTest;

/// The named child programs of a test run: name, module and grants. They
/// stay typed here; only Node needs a scratch-file configuration.
pub(crate) struct Programs(pub(crate) Vec<(String, Vec<u8>, hd_run::Grants)>);

impl TryFrom<Vec<(String, Vec<u8>, hd_run::Grants)>> for Programs {
    type Error = String;

    fn try_from(list: Vec<(String, Vec<u8>, hd_run::Grants)>) -> Result<Self, String> {
        Ok(Self(list))
    }
}

/// The code cache of this command's wasmtime engines
/// (`docs/compiler/engines-and-test-runner.md` §18.2): compiled functions
/// kept in the compiled cache, under its cap, while the value lives. An
/// engine made after it reuses the functions an earlier run compiled.
pub(crate) fn code_cache() -> Option<hd_run_wasmtime::CodeCache> {
    hd_run_wasmtime::install_code_cache(crate::disk_store())
}

/// One engine and compiled child-program list for a package's test run.
pub(crate) struct Session {
    engine: hd_run_wasmtime::TestEngine,
    children: Vec<hd_run_wasmtime::NamedProgram>,
    _code: Option<hd_run_wasmtime::CodeCache>,
}

impl Session {
    pub(crate) fn new(
        level: hd_driver::OptLevel,
        jobs: usize,
        programs: Option<&TestPrograms>,
    ) -> Result<Self, String> {
        let code = code_cache();
        let config = hd_run_wasmtime::EngineConfig::pipeline(level);
        let workers = u32::try_from(jobs).map_err(|_| "too many test workers")?;
        let engine = hd_run_wasmtime::TestEngine::new(
            config,
            hd_run_wasmtime::TestPoolConfig {
                workers,
                shrink_spares: workers,
            },
        )?;
        let children = match programs {
            Some(TestPrograms::Wasmtime(Programs(list))) => list
                .iter()
                .map(|(name, wasm, grants)| {
                    hd_run_wasmtime::NamedProgram::compile(
                        config,
                        name.clone(),
                        wasm,
                        grants.clone(),
                    )
                })
                .collect::<Result<Vec<_>, _>>()?,
            None => Vec::new(),
            Some(TestPrograms::Node(_)) => {
                return Err("internal: Node child programs on Wasmtime".into());
            }
        };
        Ok(Self {
            engine,
            children,
            _code: code,
        })
    }

    pub(crate) fn prepare(&self, wasm: &[u8], integration: bool) -> Result<Prepared, String> {
        let module = self.engine.compile(wasm)?;
        let kind = if integration {
            hd_run_wasmtime::TestHostKind::Integration
        } else {
            hd_run_wasmtime::TestHostKind::Unit
        };
        self.engine.prepare(&module, kind)
    }

    pub(crate) fn run(
        &self,
        prepared: &Prepared,
        env: &TestEnv<'_>,
        cases: &[HostCase],
        limits: ProgramLimits,
        done: &mut dyn FnMut(CaseRun),
    ) -> Result<(), String> {
        let cwd = match env.cwd {
            Some(cwd) => cwd.to_path_buf(),
            None => std::env::current_dir().map_err(|error| error.to_string())?,
        };
        let environment = hd_run_wasmtime::TestEnvironment {
            cwd,
            program: env.program.to_owned(),
            children: if env.programs.is_some() {
                self.children.clone()
            } else {
                Vec::new()
            },
        };
        for case in cases {
            let snapshot = &case.snapshot;
            let test_case = hd_run_wasmtime::testing::TestCase {
                test: case.test,
                init: case.init,
                kind: case.kind.clone(),
                temp_dir: Some(case.temp_dir.clone()),
                seed: case.seed,
                snapshot: Some(hd_run_wasmtime::testing::Snapshot {
                    dir: snapshot.root.join(&snapshot.folder),
                    shown: Some(snapshot.folder.clone()),
                    slug: snapshot.slug.clone(),
                    update: snapshot.update,
                }),
                regression: case.seed.map(|_| {
                    snapshot
                        .root
                        .join(&snapshot.regressions)
                        .join(&snapshot.slug)
                }),
            };
            let setup = HostSetup {
                providers: Vec::new(),
                grants: case.grants.clone(),
                limits: limits.into(),
                args: Vec::new(),
                program: env.program.to_owned(),
            };
            let mut runner = hd_run_wasmtime::NativeBodyRunner::new(
                prepared.clone(),
                test_case.clone(),
                environment.clone(),
                setup,
            );
            hd_run_wasmtime::testing::execute_case(&test_case, &mut runner, &mut |case| {
                done(case_run(case));
            })?;
        }
        Ok(())
    }
}

/// The result the test command judges, with the panic the host recorded.
fn case_run(case: hd_run_wasmtime::testing::CaseRun) -> CaseRun {
    CaseRun {
        panic: case.panic.map(|p| super::Panic {
            category: p.category,
            message: p.message,
        }),
        test: case.test,
        status: case.status,
        trapped: case.trapped,
        stdout: case.stdout,
        stderr: case.stderr,
        us: case.us,
        row: case.row,
        input: case.input,
        frames: case.frames,
    }
}

/// Runs a built program to its end with the process's standard streams.
pub(crate) fn run_program(
    level: hd_driver::OptLevel,
    wasm: &[u8],
    host: &HostSetup,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    let _code = code_cache();
    hd_run_wasmtime::run_traced(
        hd_run_wasmtime::EngineConfig::pipeline(level),
        wasm,
        host,
        hd_run_wasmtime::HostIo::process(),
    )
}

/// The stack a test worker thread needs to host a guest.
pub(crate) fn worker_stack(level: hd_driver::OptLevel) -> usize {
    hd_run_wasmtime::thread_stack(&hd_run_wasmtime::EngineConfig::pipeline(level))
}
