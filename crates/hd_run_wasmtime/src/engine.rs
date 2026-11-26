use std::sync::Arc;

use hd_base::HashSet;
use hd_cache::CacheStore;
use hd_run::{Engine, HostSetup, Instance, LoadError, StartError};
use wasmtime::{Linker, Store};

use super::instance::WasmtimeInstance;
use crate::host::{
    Host, HostIo, Stop, define, package, start_clock, supported, unsupported_message,
};
use crate::runtime::epochs;
use crate::{cache, limits};

/// The wasmtime configuration of §18.1, as data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EngineConfig {
    pub cranelift_opt: OptLevel,
    /// Compiler workers for one module; fixture pools leave this at one.
    pub compilation_jobs: usize,
    /// `Config::max_wasm_stack`: the profile's stack limit.
    pub max_wasm_stack: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum OptLevel {
    /// The dev pipeline: no optimization, backtracking register allocator
    /// (spike T1 has not chosen the single-pass allocator; §18.6).
    None,
    /// The optimized pipeline, `--release`.
    Speed,
}

impl EngineConfig {
    /// The engine options of a build's optimization level
    /// (`crates/hd_opt/DESIGN.md` (optimization pipelines) §3.1): Cranelift `None` for the dev pipeline
    /// (`hd run`), `Speed` for the optimized one (`--release`).
    #[must_use]
    pub fn pipeline(level: hd_base::OptLevel) -> EngineConfig {
        EngineConfig {
            cranelift_opt: match level {
                hd_base::OptLevel::Dev => OptLevel::None,
                hd_base::OptLevel::Optimized => OptLevel::Speed,
            },
            compilation_jobs: 1,
            max_wasm_stack: 4 << 20,
        }
    }
}

impl Default for EngineConfig {
    fn default() -> Self {
        EngineConfig::pipeline(hd_base::OptLevel::Dev)
    }
}

/// The native stack of the thread a program runs on: its Wasm stack limit
/// and room for the host and the compiler. A caller runs `run` (or the
/// `Engine` methods) on a thread with at least this much stack.
#[must_use]
pub fn thread_stack(config: &EngineConfig) -> usize {
    config.max_wasm_stack + (8 << 20)
}

pub struct WasmtimeEngine {
    pub config: EngineConfig,
    pub(crate) engine: Arc<wasmtime::Engine>,
    pub(crate) profile: crate::profiles::RuntimeProfile,
}

impl WasmtimeEngine {
    /// The engine of `config`, or the reason wasmtime refused it.
    pub fn new(config: EngineConfig) -> Result<WasmtimeEngine, String> {
        Self::new_with_profile(config, crate::profiles::RuntimeProfile::Default)
    }

    /// Creates an engine with explicitly scoped conformance fixture providers.
    /// Ordinary engines never accept the fixture-only imports.
    pub fn new_with_profile(
        config: EngineConfig,
        profile: crate::profiles::RuntimeProfile,
    ) -> Result<WasmtimeEngine, String> {
        start_clock();
        let shared = cache::ondemand(&config)?;
        Ok(WasmtimeEngine {
            config,
            engine: Arc::clone(&shared.engine),
            profile,
        })
    }

    /// Instantiates `m` with its standard streams on `io`.
    pub fn instantiate_with(
        &self,
        m: &Module,
        setup: &HostSetup,
        io: HostIo,
    ) -> Result<WasmtimeInstance, StartError> {
        self.instantiate_host(m, host_of(setup, io))
    }

    /// [`WasmtimeEngine::instantiate_with`] with an embedder's native
    /// package providers (`cli.host.package.native-registration`): the one
    /// registry its generated companion built, passed by value.
    ///
    /// Preflight (`host/package.rs`) resolves every root trait of the
    /// module's contract to the registration of its nominal identity before
    /// anything is linked or the guest starts, and refuses a missing,
    /// mismatched, duplicate or unrequired registration naming the trait
    /// (`native-provider-missing`, `native-mismatch`, `duplicate`). A
    /// registered trait runs only when `setup.grants.consent`, the app
    /// manifest's consent, grants it (`native-two-keys`): `hd` fills it from
    /// `hd.toml`, and an embedder from its own manifest with
    /// [`hd_run::Grants::resolve`] or [`hd_run::Consent::new`].
    pub fn instantiate_with_bindings(
        &self,
        m: &Module,
        setup: &HostSetup,
        io: HostIo,
        bindings: crate::native::Bindings,
    ) -> Result<WasmtimeInstance, StartError> {
        self.instantiate_native(m, host_of(setup, io), Some(bindings), "hd.poll")
    }

    /// [`WasmtimeEngine::instantiate_with`], for a run the host may cancel
    /// through `interrupt` (`cancel.rs`), with the cleanup bound `bound`.
    pub(crate) fn instantiate_cancellable(
        &self,
        m: &Module,
        setup: &HostSetup,
        io: HostIo,
        interrupt: &crate::HostInterrupt,
        bound: std::time::Duration,
    ) -> Result<WasmtimeInstance, StartError> {
        let mut host = host_of(setup, io);
        host.interrupt = Some(Arc::clone(&interrupt.flag));
        let mut inst = self.instantiate_host(m, host)?;
        *interrupt
            .waker
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(inst.store.data().reactor.waker());
        // A request made before the reactor had a waker set the flag only.
        if interrupt.flag.load(std::sync::atomic::Ordering::Relaxed) {
            interrupt.request();
        }
        inst.host_cancel = Some(crate::cancel::HostCancel::new(bound));
        Ok(inst)
    }

    /// Instantiates `m` with no native package provider: a module whose
    /// contract requires a root trait is refused before the guest starts
    /// (`cli.host.package.native-cli`).
    pub(crate) fn instantiate_host(
        &self,
        m: &Module,
        host: Host,
    ) -> Result<WasmtimeInstance, StartError> {
        self.instantiate_native(m, host, None, "hd.poll")
    }

    /// A worker instance of `m` (`std-remote.workers.serve`): `hd.init`,
    /// then `hd.worker`.
    pub(crate) fn instantiate_worker(
        &self,
        m: &Module,
        host: Host,
    ) -> Result<WasmtimeInstance, StartError> {
        self.instantiate_native(m, host, None, "hd.worker")
    }

    fn instantiate_native(
        &self,
        m: &Module,
        mut host: Host,
        bindings: Option<crate::native::Bindings>,
        poll: &str,
    ) -> Result<WasmtimeInstance, StartError> {
        // A send starts a worker on this module (`std-remote.workers.host`).
        host.workers.spawn = Some(crate::workers::Spawn {
            config: self.config,
            engine: Arc::clone(&self.engine),
            profile: self.profile,
            module: Arc::clone(&m.0),
            test: None,
        });
        let package = m.2.as_deref();
        host.natives = package::preflight(package, bindings, &host.grants.consent)?;
        let mut store = new_store(&self.engine, host)?;
        let mut linker = Linker::new(&self.engine);
        let mut defined = HashSet::default();
        package::define(&mut linker, &m.0, package, &mut defined)?;
        define(&mut linker, &m.0, self.profile, &mut defined).map_err(StartError::Engine)?;
        link_heap(&mut linker, &mut store, &m.0).map_err(StartError::Engine)?;
        let instance = match linker.instantiate(&mut store, &m.0) {
            Ok(instance) => instance,
            Err(error)
                if limits::is_heap_exhausted(&error)
                    || matches!(error.downcast_ref::<Stop>(), Some(Stop::TimeLimit)) =>
            {
                let mut stopped = WasmtimeInstance::stopped(store, Arc::clone(&self.engine));
                stopped.trapped(&error);
                return Ok(stopped);
            }
            Err(error) => return Err(StartError::Engine(error.to_string())),
        };
        let mut run = WasmtimeInstance::from_instance(
            store,
            Arc::clone(&self.engine),
            instance,
            "hd.init",
            poll,
        )?;
        run.report.clone_from(&m.1);
        Ok(run)
    }
}

/// The host state of one instance of `setup`, its standard streams on `io`.
fn host_of(setup: &HostSetup, io: HostIo) -> Host {
    Host::new(
        io,
        setup.args.clone(),
        setup.program.clone(),
        setup.grants.clone(),
        setup.limits.wall_ms,
    )
}

#[must_use]
pub fn wasmtime_config(config: EngineConfig) -> wasmtime::Config {
    let mut c = wasmtime::Config::new();
    c.wasm_gc(false)
        .wasm_multi_value(true)
        .wasm_bulk_memory(true)
        // The atomics of `std.rt.memory`, and the shared heap of a program
        // that starts threads (`module.rt.thread.heap`, a tier 2 feature).
        .wasm_threads(true)
        .shared_memory(true)
        .wasm_extended_const(true)
        .wasm_tail_call(false)
        .epoch_interruption(true)
        // Guest faults use signal handlers on macOS too, as on Linux. The
        // Mach-port handler thread aborts the process when any handled
        // signal (tokio's SIGCHLD for `Process.run!`, Ctrl-C) lands on it
        // (#160, `tests/child_signals.rs`). The trap handler is
        // process-wide: every engine in the process must agree.
        .macos_use_mach_ports(false)
        // A panic report lists every frame (cli.test.report.frame), not
        // wasmtime's default twenty. A frame takes at least 16 bytes of the
        // Wasm stack (a return address and a frame pointer), so this bound
        // drops none. A backtrace is taken only on a trap, into a vector of
        // this capacity.
        .wasm_backtrace_max_frames(std::num::NonZeroUsize::new(
            (config.max_wasm_stack / 16).max(1),
        ))
        .max_wasm_stack(config.max_wasm_stack)
        // No fiber runs Wasm (no async support); wasmtime still checks
        // the Wasm stack against this size.
        .async_stack_size(config.max_wasm_stack)
        // Single-program callers opt in within their job budget; fixture
        // pools already parallelize across modules and stay serial here.
        .parallel_compilation(config.compilation_jobs > 1)
        .cranelift_opt_level(match config.cranelift_opt {
            OptLevel::None => wasmtime::OptLevel::None,
            OptLevel::Speed => wasmtime::OptLevel::Speed,
        });
    c
}

/// Links the shared heap of a program that starts threads
/// (`module.rt.thread.heap`): the store's own, a thread's, or a new one of
/// the module's import type, which the store keeps for the threads it
/// starts. Nothing for a module that imports no heap.
pub(crate) fn link_heap(
    linker: &mut Linker<Host>,
    store: &mut Store<Host>,
    module: &wasmtime::Module,
) -> Result<(), String> {
    let Some(ty) = module.imports().find_map(|i| {
        (i.module() == "hd:rt" && i.name() == "heap")
            .then(|| i.ty().memory().cloned())
            .flatten()
    }) else {
        return Ok(());
    };
    let heap = match store.data().workers.heap.clone() {
        Some(heap) => heap,
        None => wasmtime::SharedMemory::new(store.engine(), ty).map_err(|e| format!("{e:#}"))?,
    };
    store.data_mut().workers.heap = Some(heap.clone());
    linker
        .define(&mut *store, "hd:rt", "heap", heap)
        .map(|_| ())
        .map_err(|e| format!("{e:#}"))
}

pub(crate) fn new_store(
    engine: &Arc<wasmtime::Engine>,
    host: Host,
) -> Result<Store<Host>, StartError> {
    // A typed allocation failure of the store's metadata is a startup
    // error, not a guest heap-exhausted panic or a Rust panic.
    let mut store = Store::try_new(engine, host).map_err(|error| {
        StartError::Engine(format!("cannot create the wasmtime store: {error:#}"))
    })?;
    if store.data().deadline().is_some() || store.data().interrupt.is_some() {
        epochs::register(engine).map_err(StartError::Engine)?;
        store.epoch_deadline_callback(|cx| {
            cx.data().check_deadline()?;
            Ok(wasmtime::UpdateDeadline::Continue(1))
        });
        store.set_epoch_deadline(1);
    } else {
        store.epoch_deadline_trap();
        store.set_epoch_deadline(u64::MAX / 2);
    }
    Ok(store)
}

/// A compiled module. The artifact is shared: loading identical bytes
/// returns the same compilation (see `cache`). Its package host contract,
/// when it has one, is decoded and indexed once here.
pub struct Module(
    pub(crate) Arc<wasmtime::Module>,
    pub(crate) Option<crate::checked_report::Report>,
    pub(crate) Option<Arc<package::Package>>,
);

impl Engine for WasmtimeEngine {
    type Module = Module;
    fn load(&self, wasm: &[u8], _cache: &dyn CacheStore) -> Result<Module, LoadError> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err(LoadError::NotHd("not a Wasm module".into()));
        }
        // The package imports the contract describes are resolved at
        // instantiation, against the embedder's registry, if any.
        let package = hd_host_abi::schema::from_wasm(wasm)
            .map_err(|why| LoadError::Engine(format!("invalid package host contract: {why}")))?
            .map(|contract| Arc::new(package::Package::new(contract)));
        let imports = hd_run::module_imports(wasm).map_err(LoadError::NotHd)?;
        if let Some((m, n)) = imports.iter().find(|(m, n)| {
            !supported(m, n, self.profile) && !package.as_ref().is_some_and(|p| p.describes(m, n))
        }) {
            return Err(LoadError::Engine(unsupported_message(m, n)));
        }
        let shared = cache::ondemand(&self.config).map_err(LoadError::Engine)?;
        shared
            .cache
            .get_or_compile(&shared.engine, wasm)
            .map(|module| Module(module, crate::checked_report::Report::read(wasm), package))
            .map_err(|e| LoadError::Engine(format!("{e:#}")))
    }
    fn instantiate(&self, m: &Module, host: &HostSetup) -> Result<Box<dyn Instance>, StartError> {
        Ok(Box::new(self.instantiate_with(
            m,
            host,
            HostIo::process(),
        )?))
    }
}
