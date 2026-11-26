//! Compiled machine code reused across runs
//! (`docs/compiler/engines-and-test-runner.md` §18.2): Cranelift's
//! incremental compilation cache, kept in `clpack` entries of the
//! compiled cache, so a warm run compiles no function.
//!
//! Loading a precompiled module (`Module::deserialize`) is `unsafe`, so
//! this caches one step lower. wasmtime still parses and validates the
//! module and translates each function to CLIF. Cranelift then looks the
//! function up by its own key and takes the machine code it emitted
//! before, through a safe decoder that also checks Cranelift's version.
//!
//! - **One pack per module.** wasmtime asks for one function at a time,
//!   and a file per function costs more to publish and open than
//!   Cranelift spends on most functions. So one `clpack` entry holds
//!   every function of one module, keyed by the module's bytes and the
//!   engine's compilation settings
//!   (`Engine::precompile_compatibility_hash`: the target, Cranelift's
//!   flags, the tunables, the Wasm features and wasmtime's version).
//!   Inside a pack, Cranelift's key finds each function: a hash of its
//!   CLIF with the target and flags. A module that changed misses whole.
//! - **The open pack.** Scoped compiler workers inherit one shared session
//!   for the module being compiled. Separate compiles keep separate
//!   sessions, including when they share an engine. A mutex protects only
//!   a function lookup or insertion, never translation or compilation.
//! - **Stable keys.** Cranelift's key also covers state that a reused
//!   compiler context keeps from the function before; see
//!   [`configure_with`] for how the engines keep it out.
//! - **Native code.** A pack holds machine code that wasmtime runs, so it
//!   is a private entry: written owner-only and read only when the reader
//!   owns it alone (`DiskStore::get_private`). The pack's header repeats
//!   its key and the entry's checksum covers its bytes, so a moved or torn
//!   file is a miss. Platforms without Unix permissions keep no packs.
//! - **One store per process.** The engines are shared process-wide, so
//!   the store is too: [`install_code_cache`] sets it until its last
//!   [`CodeCache`] drops, which publishes the store's sizes and runs its
//!   bounded eviction under the cache's cap. An engine made while no store
//!   is installed never uses one.

use hd_base::SliceSort as _;
use std::borrow::Cow;
use std::cell::RefCell;
use std::hash::{Hash as _, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use hd_base::{Hash128, HashMap, StableHasher};
use hd_cache::{EntryKind, decode_entry, encode_entry};

/// The pack layout; a change misses instead of misreading.
const LAYOUT: u16 = 1;
/// How many writes of its pack a function may sit out before it is
/// dropped (see [`configure_with`] for why a pack keeps more than one
/// compile looked up).
pub const MAX_IDLE: u8 = 8;
/// The one section of a pack: its functions.
const FUNCTIONS: u16 = 1;

/// Where packs live: the compiled cache, or a test's memory.
pub trait Packs: Send + Sync {
    fn read(&self, key: &Hash128) -> Option<Arc<[u8]>>;
    fn write(&self, key: &Hash128, bytes: &[u8]);
}

/// The compiled cache's private `clpack` entries of the running user.
#[cfg(unix)]
struct Private {
    store: hd_cache::DiskStore,
    owner: u32,
}

#[cfg(unix)]
impl Packs for Private {
    fn read(&self, key: &Hash128) -> Option<Arc<[u8]>> {
        self.store.get_private(EntryKind::Clpack, key, self.owner)
    }
    fn write(&self, key: &Hash128, bytes: &[u8]) {
        self.store.put_private(EntryKind::Clpack, key, bytes);
    }
}

/// A test's packs in memory.
#[cfg(feature = "testing")]
impl Packs for hd_cache::MemoryStore {
    fn read(&self, key: &Hash128) -> Option<Arc<[u8]>> {
        hd_cache::CacheStore::get(self, EntryKind::Clpack, key)
    }
    fn write(&self, key: &Hash128, bytes: &[u8]) {
        hd_cache::CacheStore::put(self, EntryKind::Clpack, key, bytes);
    }
}

struct Installed {
    packs: Option<Arc<dyn Packs>>,
    holders: usize,
}

static INSTALLED: Mutex<Installed> = Mutex::new(Installed {
    packs: None,
    holders: 0,
});

static HITS: AtomicU64 = AtomicU64::new(0);
static COMPILED: AtomicU64 = AtomicU64::new(0);

/// Keeps the code cache installed; the last one to drop removes it.
#[must_use = "the code cache stays installed only while this lives"]
pub struct CodeCache(());

impl Drop for CodeCache {
    fn drop(&mut self) {
        let removed = {
            let mut installed = INSTALLED.lock().unwrap_or_else(PoisonError::into_inner);
            installed.holders -= 1;
            if installed.holders == 0 {
                installed.packs.take()
            } else {
                None
            }
        };
        // The store settles outside the lock.
        drop(removed);
    }
}

/// Keeps compiled code in `store`'s `clpack` entries for the engines made
/// while the returned value lives. A store already installed stays, and
/// `store` is dropped. `None` on a platform without Unix permissions,
/// where no pack is kept.
#[must_use]
pub fn install_code_cache(store: hd_cache::DiskStore) -> Option<CodeCache> {
    #[cfg(unix)]
    {
        let packs = Private {
            store,
            owner: rustix::process::geteuid().as_raw(),
        };
        Some(install(Arc::new(packs)))
    }
    #[cfg(not(unix))]
    {
        drop(store);
        None
    }
}

#[cfg(unix)]
fn install(packs: Arc<dyn Packs>) -> CodeCache {
    let mut installed = INSTALLED.lock().unwrap_or_else(PoisonError::into_inner);
    installed.holders += 1;
    if installed.packs.is_none() {
        installed.packs = Some(packs);
    }
    CodeCache(())
}

fn installed() -> Option<Arc<dyn Packs>> {
    INSTALLED
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .packs
        .clone()
}

/// What the code cache did in this process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CodeCacheStats {
    /// Functions whose machine code came from a pack.
    pub hits: u64,
    /// Functions Cranelift compiled while a pack was open.
    pub compiled: u64,
}

/// The code cache's counts so far in this process.
#[must_use]
pub fn code_cache_stats() -> CodeCacheStats {
    CodeCacheStats {
        hits: HITS.load(Ordering::Relaxed),
        compiled: COMPILED.load(Ordering::Relaxed),
    }
}

/// Turns the code cache on for an engine made from `config` when a store
/// is installed; the engine's [`salt`] then goes to [`compile`].
pub(crate) fn configure(config: &mut wasmtime::Config) -> bool {
    installed().is_some() && configure_with(config).is_ok()
}

/// Turns Cranelift's function cache on in `config`.
///
/// Cranelift's key covers state that a pooled compiler context keeps from
/// earlier work: Cranelift 0.136 keeps the exception table of the last
/// trampoline the context compiled, and a compile clears it while a cache
/// hit does not. Two measures keep the keys of a warm compile the cold
/// compile's:
///
/// - **The inlining pipeline, inlining nothing.** `Inlining::Intrinsics`
///   inlines only wasmtime's own intrinsics, which core modules do not
///   call, so the code and the backtraces stay the same. Its pipeline
///   translates every function of a module before it compiles any, so
///   within one compile no compile comes between a function's
///   translation and its key. wasmtime's default pipeline translates and
///   compiles one function after another in one context, so a warm
///   compile, which compiles nothing, would look up other keys than the
///   cold compile wrote.
/// - **A few keys per function.** Compiles running at once on one engine
///   share its context pool, so which trampoline left the context a
///   function is translated in depends on their interleaving. Such a
///   function has a few keys, and its pack keeps each one a compile used
///   within the last [`MAX_IDLE`] writes of the pack. A process that
///   compiles one module at a time (`hd run`, `hd test`, the REPL)
///   compiles nothing on its second run; a harness that compiles many at
///   once misses less on each run.
pub fn configure_with(config: &mut wasmtime::Config) -> wasmtime::Result<()> {
    config.compiler_inlining(wasmtime::Inlining::Intrinsics);
    config.enable_incremental_compilation(Arc::new(Functions))?;
    Ok(())
}

/// The part of a pack key that stands for `engine`'s compilation settings.
#[must_use]
pub fn salt(engine: &wasmtime::Engine) -> Hash128 {
    /// Feeds a `std::hash::Hash` into the stable hasher.
    struct Feed(StableHasher);
    impl Hasher for Feed {
        fn write(&mut self, bytes: &[u8]) {
            self.0.bytes(bytes);
        }
        fn finish(&self) -> u64 {
            unreachable!("only `StableHasher::finish` reads the hash")
        }
    }
    let mut feed = Feed(StableHasher::new("wasmtime-engine"));
    engine.precompile_compatibility_hash().hash(&mut feed);
    feed.0.finish()
}

fn pack_key(salt: Hash128, wasm: &[u8]) -> Hash128 {
    let mut h = StableHasher::new("clpack");
    h.hash(salt);
    h.u64(wasm.len() as u64);
    h.bytes(wasm);
    h.finish()
}

/// Compiles `wasm` on `engine`, an engine [`configure`] turned the cache
/// on for, with the installed store's pack of the module.
pub(crate) fn compile(
    engine: &wasmtime::Engine,
    salt: Hash128,
    wasm: &[u8],
    jobs: usize,
) -> wasmtime::Result<wasmtime::Module> {
    match installed() {
        Some(packs) => compile_with_jobs(&*packs, engine, salt, wasm, jobs).0,
        None => compile_module(engine, wasm, jobs),
    }
}

/// [`compile`] over `packs`, with this compile's counts.
#[cfg(feature = "testing")]
pub fn compile_with(
    packs: &dyn Packs,
    engine: &wasmtime::Engine,
    salt: Hash128,
    wasm: &[u8],
) -> (wasmtime::Result<wasmtime::Module>, CodeCacheStats) {
    compile_with_jobs(packs, engine, salt, wasm, 1)
}

/// [`compile_with`] with a caller's compiler-worker budget.
pub fn compile_with_jobs(
    packs: &dyn Packs,
    engine: &wasmtime::Engine,
    salt: Hash128,
    wasm: &[u8],
    jobs: usize,
) -> (wasmtime::Result<wasmtime::Module>, CodeCacheStats) {
    let key = pack_key(salt, wasm);
    let functions = packs
        .read(&key)
        .and_then(|bytes| decode(&bytes, key))
        .unwrap_or_default();
    let session = Arc::new(Mutex::new(Session {
        functions,
        dirty: false,
        stats: CodeCacheStats::default(),
    }));
    let _scope = SessionScope::enter(Some(Arc::clone(&session)));
    let compiled = compile_module(engine, wasm, jobs);
    let session = session.lock().unwrap_or_else(PoisonError::into_inner);
    HITS.fetch_add(session.stats.hits, Ordering::Relaxed);
    COMPILED.fetch_add(session.stats.compiled, Ordering::Relaxed);
    if compiled.is_ok() && session.dirty {
        packs.write(&key, &encode(key, salt, &session.functions));
    }
    (compiled, session.stats)
}

/// One function's machine code, as Cranelift serialized it.
pub struct Function {
    pub code: Box<[u8]>,
    /// Looked up or compiled by this compile.
    pub used: bool,
    /// The writes of its pack since a compile last used it.
    pub idle: u8,
}

/// The pack of one module, shared only by that compile's workers.
#[derive(Default)]
struct Session {
    functions: HashMap<Box<[u8]>, Function>,
    /// Cranelift compiled a function: the pack is written again.
    dirty: bool,
    stats: CodeCacheStats,
}

thread_local! {
    static SESSION: RefCell<Option<Arc<Mutex<Session>>>> = const { RefCell::new(None) };
}

/// Restores the caller's session even if compilation unwinds.
struct SessionScope(Option<Arc<Mutex<Session>>>);

impl SessionScope {
    fn enter(session: Option<Arc<Mutex<Session>>>) -> Self {
        Self(SESSION.replace(session))
    }
}

impl Drop for SessionScope {
    fn drop(&mut self) {
        SESSION.set(self.0.take());
    }
}

/// Runs only after the in-process module cache misses. The serial path
/// never creates a Rayon pool. Wasmtime uses the current scoped pool,
/// whose workers inherit the same pack through Rayon's scoped-TLS pattern
/// (`ThreadPoolBuilder::build_scoped`), rather than the global pool.
pub(crate) fn compile_module(
    engine: &wasmtime::Engine,
    wasm: &[u8],
    jobs: usize,
) -> wasmtime::Result<wasmtime::Module> {
    let _trace = hd_base::trace::span("phase", || format!("wasmtime compile jobs={}", jobs.max(1)));
    if jobs <= 1 {
        return wasmtime::Module::new(engine, wasm);
    }
    let session = SESSION.with_borrow(Clone::clone);
    rayon::ThreadPoolBuilder::new()
        .num_threads(jobs)
        .thread_name(|i| format!("hd-cranelift-{i}"))
        .build_scoped(
            |thread| {
                let _scope = SessionScope::enter(session.clone());
                thread.run();
            },
            |pool| pool.install(|| wasmtime::Module::new(engine, wasm)),
        )
        .map_err(wasmtime::Error::new)?
}

/// Cranelift's view of the pack: the function store wasmtime calls while
/// it compiles, on the compiling thread.
#[derive(Debug)]
struct Functions;

impl wasmtime::CacheStore for Functions {
    fn get(&self, key: &[u8]) -> Option<Cow<'_, [u8]>> {
        SESSION.with_borrow(|session| {
            let mut session = session
                .as_ref()?
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let code = {
                let function = session.functions.get_mut(key)?;
                function.used = true;
                function.code.to_vec()
            };
            session.stats.hits += 1;
            Some(Cow::Owned(code))
        })
    }

    fn insert(&self, key: &[u8], value: Vec<u8>) -> bool {
        SESSION.with_borrow(|session| {
            let Some(session) = session.as_ref() else {
                return false;
            };
            let mut session = session.lock().unwrap_or_else(PoisonError::into_inner);
            session.stats.compiled += 1;
            session.dirty = true;
            session.functions.insert(
                key.into(),
                Function {
                    code: value.into(),
                    used: true,
                    idle: 0,
                },
            );
            true
        })
    }
}

/// A pack: its functions in key order, each as its idle count, key
/// length, key, code length and code. A function this compile did not use
/// ages by one write, and one idle for more than [`MAX_IDLE`] is dropped.
#[must_use]
pub fn encode(key: Hash128, salt: Hash128, functions: &HashMap<Box<[u8]>, Function>) -> Vec<u8> {
    let mut kept: Vec<_> = functions
        .iter()
        .filter_map(|(k, f)| {
            let idle = if f.used { 0 } else { f.idle.saturating_add(1) };
            (idle <= MAX_IDLE).then_some((k, f, idle))
        })
        .collect();
    kept.sort_stable_by(|a, b| a.0.cmp(b.0));
    let mut body = Vec::new();
    for (k, f, idle) in &kept {
        body.push(*idle);
        for part in [&k[..], &f.code[..]] {
            let len = u32::try_from(part.len()).expect("a function under 4 GiB");
            body.extend_from_slice(&len.to_le_bytes());
            body.extend_from_slice(part);
        }
    }
    let rows = u32::try_from(kept.len()).expect("under 4 Gi functions");
    encode_entry(
        EntryKind::Clpack,
        LAYOUT,
        key,
        salt,
        0,
        &[(FUNCTIONS, rows, &body)],
    )
}

/// The functions of a pack stored under `key`; `None` for anything else,
/// which is a miss.
#[must_use]
pub fn decode(bytes: &[u8], key: Hash128) -> Option<HashMap<Box<[u8]>, Function>> {
    let (header, sections) = decode_entry(bytes).ok()?;
    if header.kind != EntryKind::Clpack || header.layout != LAYOUT || header.key != key {
        return None;
    }
    let [(section, mut body)] = sections[..] else {
        return None;
    };
    if section.kind != FUNCTIONS {
        return None;
    }
    let mut functions = HashMap::default();
    for _ in 0..section.rows {
        let [idle] = *take(&mut body, 1)? else {
            return None;
        };
        let k = take_part(&mut body)?;
        let code = take_part(&mut body)?;
        functions.insert(
            k.into(),
            Function {
                code: code.into(),
                used: false,
                idle,
            },
        );
    }
    Some(functions)
}

/// The next `n` bytes of `body`.
fn take<'a>(body: &mut &'a [u8], n: usize) -> Option<&'a [u8]> {
    let (head, rest) = body.split_at_checked(n)?;
    *body = rest;
    Some(head)
}

/// The next length-prefixed part of `body`.
fn take_part<'a>(body: &mut &'a [u8]) -> Option<&'a [u8]> {
    let len = u32::from_le_bytes(take(body, 4)?.try_into().ok()?);
    take(body, usize::try_from(len).ok()?)
}
