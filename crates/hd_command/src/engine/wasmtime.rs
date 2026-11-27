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
pub(crate) struct Programs {
    pub(crate) native: Vec<(String, Vec<u8>, hd_run::Grants, Vec<crate::node::CLibrary>)>,
    node: Option<crate::node::Programs>,
}

impl Programs {
    pub(crate) fn new(
        list: Vec<super::TestProgram>,
        cwd: &std::path::Path,
    ) -> Result<Self, String> {
        let (node, native): (Vec<_>, Vec<_>) = list
            .into_iter()
            .partition(|child| child.engine == super::EngineKind::Node);
        let node = if node.is_empty() {
            None
        } else {
            Some(crate::node::Programs::new(&node, cwd)?)
        };
        Ok(Self {
            native: native
                .into_iter()
                .map(|child| (child.name, child.wasm, child.grants, child.c))
                .collect(),
            node,
        })
    }
}

/// The selected C libraries as the Wasmtime host takes them.
fn libraries(c: &[crate::node::CLibrary]) -> Vec<hd_run_wasmtime::native::c::Library> {
    c.iter()
        .map(|l| hd_run_wasmtime::native::c::Library {
            module: l.module.clone(),
            path: l.path.clone(),
            name: l.name.clone(),
            package: l.package.clone(),
            triple: l.triple.clone(),
        })
        .collect()
}

/// The code cache of this command's wasmtime engines
/// (`docs/compiler/engines-and-test-runner.md` §18.2): compiled functions
/// kept in the compiled cache, under its cap, while the value lives. An
/// engine made after it reuses the functions an earlier run compiled.
pub(crate) fn code_cache() -> Option<hd_run_wasmtime::CodeCache> {
    hd_run_wasmtime::install_code_cache(crate::open_disk_store())
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
        let mut config = hd_run_wasmtime::EngineConfig::pipeline(level);
        config.compilation_jobs = jobs;
        let workers = u32::try_from(jobs).map_err(|_| "too many test workers")?;
        let engine = hd_run_wasmtime::TestEngine::new(
            config,
            hd_run_wasmtime::TestPoolConfig {
                workers,
                shrink_spares: workers,
            },
        )?;
        let children = match programs {
            Some(TestPrograms::Wasmtime(programs)) => {
                let mut children = programs
                    .native
                    .iter()
                    .map(|(name, wasm, grants, c)| {
                        hd_run_wasmtime::NamedProgram::compile_with_c(
                            config,
                            name.clone(),
                            wasm,
                            grants.clone(),
                            libraries(c),
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if let Some(node) = &programs.node {
                    for (name, args) in node.commands() {
                        children.push(hd_run_wasmtime::NamedProgram::command(
                            name.clone(),
                            crate::node::node_program().to_string_lossy().into_owned(),
                            args.clone(),
                        ));
                    }
                }
                children
            }
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
        failure: case.failure,
        frames: case.frames,
        snapshots: case
            .snapshots
            .into_iter()
            .map(|r| crate::node::Recorded { text: r.text })
            .collect(),
    }
}

/// Runs a built program to its end with the process's standard streams.
/// With a cleanup bound, each Ctrl-C is a request to cancel its entry
/// (`cli.cancel.unhandled-ctrl-c`). The C providers `c` are bound before
/// the guest starts (`hd_run_wasmtime::run_with_c_providers`).
#[allow(unsafe_code)]
pub(crate) fn run_program(
    level: hd_driver::OptLevel,
    image: hd_run::artifact::Image<'_>,
    host: &HostSetup,
    c: &[crate::node::CLibrary],
    sampling: Option<&std::path::Path>,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    let _code = match image {
        hd_run::artifact::Image::Wasm(_) => code_cache(),
        hd_run::artifact::Image::Native(_) => None,
    };
    let mut config = hd_run_wasmtime::EngineConfig::pipeline(level);
    config.compilation_jobs = crate::default_jobs();
    let _configured = hd_base::trace::span("engine", || {
        format!("wasmtime OptLevel::{:?}", config.cranelift_opt)
    });
    let io = hd_run_wasmtime::HostIo::process();
    let interrupt = host
        .limits
        .cleanup_ms
        .map(|_| hd_run_wasmtime::HostInterrupt::new());
    let _signals = interrupt.as_ref().map(|interrupt| {
        let handler = interrupt.clone();
        super::on_ctrl_c(move |_| handler.request())
    });
    let cancel = interrupt
        .as_ref()
        .zip(host.limits.cleanup_ms)
        .map(|(interrupt, ms)| (interrupt, std::time::Duration::from_millis(ms)));
    if !c.is_empty() {
        let libraries = libraries(c);
        let config = match image {
            hd_run::artifact::Image::Wasm(_) => config,
            hd_run::artifact::Image::Native(_) => {
                hd_run_wasmtime::EngineConfig::pipeline(hd_base::OptLevel::Optimized)
            }
        };
        // SAFETY: as `run_native_image`: the CLI passes only an explicitly
        // selected native artifact or its own build output, held open for
        // this run; each library is one the root manifest consented to.
        return unsafe {
            hd_run_wasmtime::run_with_c_providers(
                config, image, host, io, &libraries, cancel, sampling,
            )
        };
    }
    match image {
        hd_run::artifact::Image::Wasm(wasm) => match (sampling, cancel) {
            (Some(output), cancel) => {
                hd_run_wasmtime::run_sampled(config, wasm, host, io, output, cancel)
            }
            (None, Some((interrupt, bound))) => {
                hd_run_wasmtime::run_cancellable(config, wasm, host, io, interrupt, bound)
            }
            (None, None) => hd_run_wasmtime::run_traced(config, wasm, host, io),
        },
        hd_run::artifact::Image::Native(native) => {
            run_native_image(native, host, io, cancel, sampling)
        }
    }
}

pub(crate) fn native_key() -> Result<hd_base::Hash128, String> {
    hd_run_wasmtime::precompile_key()
}
pub(crate) fn precompile(wasm: &[u8]) -> Result<Vec<u8>, String> {
    hd_run_wasmtime::precompile(wasm, crate::default_jobs())
}

/// The stack a test worker thread needs to host a guest.
pub(crate) fn worker_stack(level: hd_driver::OptLevel) -> usize {
    hd_run_wasmtime::thread_stack(&hd_run_wasmtime::EngineConfig::pipeline(level))
}

#[allow(unsafe_code)]
fn run_native_image(
    native: &hd_run::artifact::Native,
    host: &HostSetup,
    io: hd_run_wasmtime::HostIo,
    cancel: Option<(&hd_run_wasmtime::HostInterrupt, std::time::Duration)>,
    sampling: Option<&std::path::Path>,
) -> Result<(Outcome, Vec<(u32, u32)>), String> {
    // SAFETY: the CLI only passes an explicitly selected native artifact
    // or its own build/install output, held open for this run. Its writers
    // publish replacements instead of modifying existing mapped files.
    unsafe {
        match sampling {
            Some(output) => hd_run_wasmtime::run_native_sampled(native, host, io, output, cancel),
            None => hd_run_wasmtime::run_native(native, host, io, cancel),
        }
    }
}
