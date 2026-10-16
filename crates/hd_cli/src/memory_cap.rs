//! Process-heap accounting for compilation, not a resident-memory limit.
//! Tracking and its reporter are enabled only when a compiler cap is requested.
//! The safe allocator wrapper still reserves metadata even when disabled.
//! Tracking remains enabled while a guest runs so frees cannot unbalance the
//! counter. Guest entry disarms compiler enforcement, because guest workers
//! and caches may retain native allocations beyond teardown. A command that
//! compiles again after a guest ran (`hd test` over several workspace members)
//! rearms with [`rearm`] before that compile; the bytes the guest phase left
//! live are then excluded from the compiler's count (see [`rearm`]).
//! Temporary suspension remains available for
//! accounting probes. The external safe allocator reports
//! successful allocations before returning them to the compiler. If the system
//! allocator itself fails first, Rust's OOM handler can abort before our callback.

use std::cell::Cell;
use std::ffi::OsString;
use std::io::Write;
use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

use hd_driver::{Goal, Host, Output, Packages, Stage, StageObserver};
use tracking_allocator::{AllocationGroupId, AllocationRegistry, AllocationTracker, Allocator};

use crate::report::{Diag, Summary};

#[global_allocator]
static ALLOCATOR: Allocator<std::alloc::System> = Allocator::system();

static LIVE: AtomicU64 = AtomicU64::new(0);
static LIMIT: AtomicU64 = AtomicU64::new(0);
static ARMED: AtomicBool = AtomicBool::new(false);
static SUSPENSIONS: AtomicUsize = AtomicUsize::new(0);
static TRIPPED: AtomicBool = AtomicBool::new(false);
static REPORT_READY: AtomicBool = AtomicBool::new(false);
static FAILURE_STAGE: AtomicUsize = AtomicUsize::new(STARTUP);
// Live bytes that are not compiler memory: what earlier guest phases left
// live. The compiler's count is `LIVE - GUEST_RETAINED`; zero until a guest
// phase ends with a rearm, so a single-program command never changes it.
static GUEST_RETAINED: AtomicU64 = AtomicU64::new(0);
// The compiler's count when the latest guest phase began.
static GUEST_ENTRY: AtomicU64 = AtomicU64::new(0);
static EMERGENCY_THREAD: OnceLock<std::thread::Thread> = OnceLock::new();
static REPORT: OnceLock<Report> = OnceLock::new();
// Wasmtime's private compilation pool has no observer hook. An explicit stage
// on a driver thread always wins; otherwise a precompile scope supplies this
// process-wide fallback for allocations on those internal workers.
static PRECOMPILE_SCOPES: AtomicUsize = AtomicUsize::new(0);
static DEFAULT_STAGE: AtomicUsize = AtomicUsize::new(STARTUP);

// The two non-driver phases precede Stage::ALL in the preallocated report.
const STARTUP: usize = 0;
const COMPILER: usize = 1;

thread_local! {
    static ACTIVE_STAGE: Cell<usize> = const { Cell::new(STARTUP) };
    static REPORTING_THREAD: Cell<bool> = const { Cell::new(false) };
}

// Unit tests check accounting balance on their own thread. `LIVE` is
// process-wide, and libtest's runner thread and the reporter thread allocate
// at times the test cannot order against. A const `Cell` needs no destructor,
// so updating it never allocates.
#[cfg(test)]
thread_local! {
    static THREAD_LIVE: Cell<i64> = const { Cell::new(0) };
}

#[cfg(test)]
fn count_thread_bytes(bytes: usize, allocated: bool) {
    let bytes = i64::try_from(bytes).unwrap_or(i64::MAX);
    let delta = if allocated { bytes } else { -bytes };
    let _ = THREAD_LIVE.try_with(|n| n.set(n.get().wrapping_add(delta)));
}

struct Report {
    lines: Vec<Vec<u8>>,
    stream: Option<std::fs::File>,
}

struct Tracker;

impl AllocationTracker for Tracker {
    fn allocated(&self, _: usize, _: usize, wrapped_size: usize, _: AllocationGroupId) {
        #[cfg(test)]
        count_thread_bytes(wrapped_size, true);
        let live = add_bytes(&LIVE, wrapped_size);
        if !REPORTING_THREAD.try_with(Cell::get).unwrap_or(false) {
            enforce(live);
        }
    }

    fn deallocated(
        &self,
        _: usize,
        _: usize,
        wrapped_size: usize,
        _: AllocationGroupId,
        _: AllocationGroupId,
    ) {
        #[cfg(test)]
        count_thread_bytes(wrapped_size, false);
        remove_bytes(&LIVE, wrapped_size);
    }
}

fn add_bytes(counter: &AtomicU64, bytes: usize) -> u64 {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    let previous = counter
        .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
            Some(n.saturating_add(bytes))
        })
        .unwrap_or_else(|current| current);
    previous.saturating_add(bytes)
}

fn remove_bytes(counter: &AtomicU64, bytes: usize) {
    let bytes = u64::try_from(bytes).unwrap_or(u64::MAX);
    let _ = counter.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
        Some(n.saturating_sub(bytes))
    });
}

/// Inspect only global options, before collecting the retained arguments. Values
/// after `--` belong to the guest and must not enable compiler accounting.
fn requested(args: impl IntoIterator<Item = OsString>, environment: bool) -> bool {
    environment
        || args
            .into_iter()
            .take_while(|arg| arg != "--")
            .any(|arg| arg == "--max-memory" || arg.to_string_lossy().starts_with("--max-memory="))
}

/// Call before collecting arguments. Disabled invocations install no tracker
/// and start no reporter. Allocation metadata distinguishes earlier, untracked
/// allocations, so their eventual frees do not decrement the counter.
pub(crate) fn startup(
    args: impl IntoIterator<Item = OsString>,
    environment: bool,
) -> Result<(), &'static str> {
    if !requested(args, environment) {
        return Ok(());
    }
    AllocationRegistry::set_global_tracker(Tracker)
        .map_err(|_| "cannot install compiler memory tracker")?;
    AllocationRegistry::enable_tracking();
    // Exit from a dedicated prepared thread: std::process::exit flushes stdout,
    // which could reenter a borrowed stdout buffer on the allocating thread.
    // All thread setup is tracked, but happens before enforcement is armed.
    let (ready, prepared) = std::sync::mpsc::sync_channel(0);
    std::thread::Builder::new()
        .name("hd-memory-limit".to_owned())
        .spawn(move || {
            REPORTING_THREAD.with(|s| s.set(true));
            if ready.send(std::thread::current()).is_err() {
                return;
            }
            await_report(&REPORT_READY);
            emergency_exit();
        })
        .map_err(|_| "cannot start compiler memory reporter")?;
    let thread = prepared
        .recv()
        .map_err(|_| "cannot prepare compiler memory reporter")?;
    EMERGENCY_THREAD
        .set(thread)
        .map_err(|_| "compiler memory reporter already started")?;
    Ok(())
}

/// Block the reporter until a trip has published its report. The flag is
/// checked before every park: a thread has one unpark token, and the
/// rendezvous `send` above parks on the same token. When the sender arrives
/// first, the receiver's wakeup and an immediate trip's wakeup (`--max-memory=0`
/// trips while arming) merge into the one token `send` consumes, so parking
/// before checking would sleep forever with the report ready.
fn await_report(ready: &AtomicBool) {
    while !ready.load(Ordering::Acquire) {
        std::thread::park();
    }
}

/// Configure once, after global options have been parsed but before project I/O.
/// Building the emergency output is itself counted, then checked immediately.
pub(crate) fn arm(limit: Option<u64>, json: bool, to_stderr: bool) {
    let Some(limit) = limit else {
        return;
    };
    let labels = std::iter::once("starting compiler")
        .chain(std::iter::once("preparing compilation"))
        .chain(Stage::ALL.into_iter().map(stage_description));
    let lines = labels.map(|label| diagnostic(limit, label, json)).collect();
    // Duplicate the selected stream before arming. Using the stdio locks from
    // an allocator callback could deadlock or reenter an already borrowed buffer.
    let report = Report {
        lines,
        stream: emergency_stream(to_stderr || !json).ok(),
    };
    if REPORT.set(report).is_err() {
        return;
    }
    LIMIT.store(limit, Ordering::SeqCst);
    ARMED.store(true, Ordering::SeqCst);
    enforce(LIVE.load(Ordering::SeqCst));
    // The initial arm check describes startup; later unobserved work is honest
    // generic compilation preparation, rather than falsely claiming startup.
    DEFAULT_STAGE.store(COMPILER, Ordering::SeqCst);
}

#[cfg(unix)]
fn emergency_stream(stderr: bool) -> std::io::Result<std::fs::File> {
    use std::os::fd::AsFd;
    let descriptor = if stderr {
        std::io::stderr().as_fd().try_clone_to_owned()?
    } else {
        std::io::stdout().as_fd().try_clone_to_owned()?
    };
    Ok(std::fs::File::from(descriptor))
}

#[cfg(windows)]
fn emergency_stream(stderr: bool) -> std::io::Result<std::fs::File> {
    use std::os::windows::io::AsHandle;
    let handle = if stderr {
        std::io::stderr().as_handle().try_clone_to_owned()?
    } else {
        std::io::stdout().as_handle().try_clone_to_owned()?
    };
    Ok(std::fs::File::from(handle))
}

#[cfg(not(any(unix, windows)))]
fn emergency_stream(_: bool) -> std::io::Result<std::fs::File> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

fn diagnostic(limit: u64, label: &str, json: bool) -> Vec<u8> {
    let diag = Diag::error(
        Some(hd_diag::Code::MemoryLimit),
        &format!("compiler memory limit of {limit} bytes exceeded while {label}"),
    );
    let mut line = if json { diag.json() } else { diag.text() };
    line.push('\n');
    if json {
        line.push_str(
            &Summary {
                errors: 1,
                status: 101,
                ..Summary::default()
            }
            .json(),
        );
        line.push('\n');
    }
    line.into_bytes()
}

fn enforce(live: u64) {
    if !ARMED.load(Ordering::SeqCst)
        || SUSPENSIONS.load(Ordering::SeqCst) != 0
        || live.saturating_sub(GUEST_RETAINED.load(Ordering::SeqCst))
            <= LIMIT.load(Ordering::SeqCst)
    {
        return;
    }
    // Only the winner writes. Other allocating threads cannot race ahead to
    // another OS operation or exit before the winning thread flushes its report.
    if !TRIPPED.swap(true, Ordering::SeqCst) {
        let local = ACTIVE_STAGE.try_with(Cell::get).unwrap_or(STARTUP);
        let stage = if local == STARTUP {
            if PRECOMPILE_SCOPES.load(Ordering::SeqCst) == 0 {
                DEFAULT_STAGE.load(Ordering::SeqCst)
            } else {
                stage_index(Stage::Precompile)
            }
        } else {
            local
        };
        FAILURE_STAGE.store(stage, Ordering::Relaxed);
        REPORT_READY.store(true, Ordering::Release);
        if let Some(thread) = EMERGENCY_THREAD.get() {
            thread.unpark();
        }
    }
    loop {
        std::hint::spin_loop();
    }
}

fn emergency_exit() -> ! {
    if let Some(report) = REPORT.get() {
        let stage = FAILURE_STAGE.load(Ordering::Relaxed);
        let line = report.lines.get(stage).unwrap_or(&report.lines[COMPILER]);
        if let Some(mut stream) = report.stream.as_ref() {
            let _ = stream.write_all(line);
        }
    }
    exit_now(101)
}

/// Ends the process without std's exit cleanup.
///
/// Why `unsafe` here (owner-approved, 2026-10-11, task #298): on linux-gnu,
/// `std::process::exit` runs cleanup that takes std's lock for per-thread
/// stack-overflow info. A thread that is starting holds that lock while it
/// allocates; if that allocation is the one that crosses the compiler memory
/// cap, the tracking allocator spins in it forever with the lock held, and
/// `process::exit` then blocks forever after the report is written. `_exit`
/// takes no lock and runs no cleanup. Nothing is lost: the report went to a
/// duplicated raw descriptor above, not through std's buffered stdout.
#[cfg(unix)]
#[allow(unsafe_code)]
fn exit_now(code: i32) -> ! {
    // SAFETY: `_exit` only terminates the process; it touches no Rust state,
    // and nothing runs after it.
    unsafe { libc::_exit(code) }
}

/// Elsewhere std's exit cleanup takes no such lock.
#[cfg(not(unix))]
fn exit_now(code: i32) -> ! {
    std::process::exit(code)
}

fn stage_index(stage: Stage) -> usize {
    Stage::ALL.iter().position(|s| *s == stage).unwrap_or(0) + 2
}

fn stage_description(stage: Stage) -> &'static str {
    match stage {
        Stage::Discover => "discovering modules",
        Stage::Skim => "skimming declarations",
        Stage::Parse => "parsing modules",
        Stage::FolderGraph => "building the folder graph",
        Stage::FolderIface => "building folder interfaces",
        Stage::HeaderCheck => "checking headers",
        Stage::ModulePrep => "preparing modules",
        Stage::Body => "checking bodies",
        Stage::ModuleFinish => "finishing modules",
        Stage::Coherence => "checking coherence",
        Stage::InitOrder => "ordering initializers",
        Stage::PackageResult => "assembling package results",
        Stage::Collect => "collecting reachable definitions",
        Stage::Emit => "emitting WebAssembly",
        Stage::Link => "linking WebAssembly",
        Stage::Precompile => "precompiling WebAssembly",
        Stage::Run => "preparing execution",
    }
}

/// A stage scope must be restored on the same thread that entered it.
pub(crate) struct StageGuard {
    previous: usize,
    precompile: bool,
    _same_thread: PhantomData<Rc<()>>,
}

impl Drop for StageGuard {
    fn drop(&mut self) {
        let _ = ACTIVE_STAGE.try_with(|s| s.set(self.previous));
        if self.precompile {
            PRECOMPILE_SCOPES.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

/// Driver stage names (not free-form user text); unknown names use the generic
/// compilation label. In particular, use `Precompile` around Cranelift loading.
pub(crate) fn stage(name: &'static str) -> StageGuard {
    let index = Stage::ALL
        .into_iter()
        .find(|s| s.name() == name)
        .map_or(COMPILER, stage_index);
    let precompile = name == "Precompile";
    if precompile {
        PRECOMPILE_SCOPES.fetch_add(1, Ordering::SeqCst);
    }
    StageGuard {
        previous: ACTIVE_STAGE.with(|s| s.replace(index)),
        precompile,
        _same_thread: PhantomData,
    }
}

struct Observer;

impl StageObserver for Observer {
    fn enter(&self, stage: Stage) -> usize {
        ACTIVE_STAGE.with(|s| s.replace(stage_index(stage)))
    }

    fn exit(&self, token: usize) {
        let _ = ACTIVE_STAGE.try_with(|s| s.set(token));
    }
}

pub(crate) fn build_packages(
    host: &Host<'_>,
    package: &str,
    packages: &Packages<'_>,
    goal: &Goal,
) -> Output {
    hd_driver::build_packages_observed(host, package, packages, goal, &Observer)
}

/// Keep alive through guest initialization, execution, and instance destruction.
/// Counter updates continue, including allocations freed by other threads.
pub(crate) struct Suspension {
    _private: (),
}

pub(crate) fn suspend() -> Suspension {
    SUSPENSIONS.fetch_add(1, Ordering::SeqCst);
    Suspension { _private: () }
}

/// Finish the compiler phase before guest initialization. Unlike a temporary
/// suspension, dropping the guard never rearms: native guest-origin
/// worker/cache allocations retained after teardown must not become compiler
/// memory-limit errors. Only [`rearm`], before a further compile, rearms.
pub(crate) fn enter_guest() -> Suspension {
    if ARMED.swap(false, Ordering::SeqCst) {
        let live = LIVE.load(Ordering::SeqCst);
        GUEST_ENTRY.store(
            live.saturating_sub(GUEST_RETAINED.load(Ordering::SeqCst)),
            Ordering::SeqCst,
        );
    }
    suspend()
}

/// Start a further compile phase after a guest ran: rearm the cap that
/// [`enter_guest`] disarmed. Does nothing without a cap, or while still armed.
///
/// Baseline: the bytes that went live between guest entry and now are taken
/// as the guest's, and excluded from the compiler's count from here on. So
/// the next compile starts from at most the compiler's own count at guest
/// entry, which never exceeded the cap. Compiler memory freed during the guest
/// phase lowers the count as usual. When retained guest memory is freed later,
/// the count drops by it too, so the compiler gains at most that much slack.
pub(crate) fn rearm() {
    if REPORT.get().is_none() || ARMED.load(Ordering::SeqCst) {
        return;
    }
    let live = LIVE.load(Ordering::SeqCst);
    let entry = GUEST_ENTRY.load(Ordering::SeqCst);
    let retained = GUEST_RETAINED.load(Ordering::SeqCst);
    if live.saturating_sub(retained) > entry {
        GUEST_RETAINED.store(live - entry, Ordering::SeqCst);
    }
    ARMED.store(true, Ordering::SeqCst);
    enforce(LIVE.load(Ordering::SeqCst));
}

impl Drop for Suspension {
    fn drop(&mut self) {
        if SUSPENSIONS.fetch_sub(1, Ordering::SeqCst) == 1 {
            enforce(LIVE.load(Ordering::SeqCst));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Net tracked bytes this thread allocated, counted by the same callbacks
    /// that update `LIVE`. Exact balance checks use it, because libtest's
    /// runner thread and the reporter thread can move `LIVE` mid-check.
    fn thread_live() -> i64 {
        THREAD_LIVE.with(Cell::get)
    }

    // An isolated process is required: a real allocator limit exits, and the
    // global tracker cannot be reset safely between parallel unit tests.
    // A lost report wakeup leaves the child spinning, so bound every run.
    fn child(mode: &str) -> std::process::Output {
        use std::process::Stdio;
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "memory_cap::tests::allocation_trip_child",
                "--nocapture",
            ])
            .env("HD_MEMORY_CAP_UNIT_CHILD", mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while child.try_wait().unwrap().is_none() {
            if std::time::Instant::now() >= until {
                let _ = child.kill();
                let output = child.wait_with_output().unwrap();
                panic!("memory-limit child `{mode}` hung: {output:?}");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        child.wait_with_output().unwrap()
    }

    #[test]
    fn allocation_trip_child() {
        let Ok(mode) = std::env::var("HD_MEMORY_CAP_UNIT_CHILD") else {
            return;
        };
        if mode == "disabled" {
            startup([OsString::from("check"), OsString::from("file.hd")], false).unwrap();
            assert!(EMERGENCY_THREAD.get().is_none());
            assert!(REPORT.get().is_none());
            assert!(!ARMED.load(Ordering::SeqCst));
            let allocation = std::hint::black_box(vec![0u8; 512 * 1024]);
            assert_eq!(LIVE.load(Ordering::SeqCst), 0);
            drop(allocation);
            assert_eq!(LIVE.load(Ordering::SeqCst), 0);
            return;
        }
        // This allocation predates tracker setup. Freeing it must never
        // subtract bytes belonging to a later tracked allocation.
        let earlier = std::hint::black_box(vec![0u8; 32 * 1024]);
        startup([OsString::from("--max-memory=128K")], false).unwrap();
        let before = thread_live();
        drop(earlier);
        assert_eq!(thread_live(), before);
        if mode == "arm" {
            arm(Some(0), true, true);
            panic!("arming over the cap returned to compiler");
        }
        arm(Some(128 * 1024), mode == "json", true);
        if mode == "retained-guest" {
            let guest = enter_guest();
            let before = thread_live();
            let retained = std::hint::black_box(vec![0u8; 512 * 1024]);
            assert!(thread_live() >= before + 512 * 1024);
            drop(guest);
            // Native guest allocations can still be held by a detached input,
            // DNS/HTTP worker, or Wasmtime cache after the instance is gone.
            assert!(!ARMED.load(Ordering::SeqCst));
            let later = std::hint::black_box(vec![0u8; 512 * 1024]);
            drop(later);
            drop(retained);
            assert_eq!(thread_live(), before);
            return;
        }
        if mode == "rearm" {
            let guest = enter_guest();
            let _retained = std::hint::black_box(vec![0u8; 512 * 1024]);
            drop(guest);
            rearm();
            assert!(ARMED.load(Ordering::SeqCst));
            // The retained guest bytes exceed the cap on their own; they are
            // not the next compile's. Rearming did not trip, and a small
            // compiler allocation fits.
            drop(std::hint::black_box(vec![0u8; 16 * 1024]));
            let _stage = stage("Body");
            let _allocation = std::hint::black_box(vec![0u8; 512 * 1024]);
            panic!("over-cap allocation after rearm returned to compiler");
        }
        if mode == "suspended" {
            let suspension = suspend();
            let before = thread_live();
            let allocation = std::hint::black_box(vec![0u8; 512 * 1024]);
            assert!(thread_live() >= before + 512 * 1024);
            drop(allocation);
            assert_eq!(thread_live(), before);
            drop(suspension);
            return;
        }
        let _stage = stage(if matches!(mode.as_str(), "worker" | "hidden-worker") {
            "Precompile"
        } else {
            "Body"
        });
        if mode == "hidden-worker" {
            std::thread::spawn(|| {
                // Models a private Cranelift worker with no CLI stage observer.
                let _allocation = std::hint::black_box(vec![0u8; 512 * 1024]);
            })
            .join()
            .unwrap();
        }
        if mode == "worker" {
            std::thread::spawn(|| {
                let _stage = stage("Emit");
                let _allocation = std::hint::black_box(vec![0u8; 512 * 1024]);
            })
            .join()
            .unwrap();
        }
        if mode == "stdout-borrow" {
            struct AllocatingDisplay;
            impl std::fmt::Display for AllocatingDisplay {
                fn fmt(&self, _: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    let _allocation = std::hint::black_box(vec![0u8; 512 * 1024]);
                    Ok(())
                }
            }
            // Display runs while stdout's buffer is already mutably borrowed.
            // Exiting on this thread would make stdio cleanup panic.
            print!("{AllocatingDisplay}");
        }
        let _allocation = std::hint::black_box(vec![0u8; 512 * 1024]);
        panic!("over-cap allocation returned to compiler");
    }

    #[test]
    fn no_limit_keeps_tracking_and_reporter_disabled() {
        let output = child("disabled");
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }

    #[test]
    fn only_global_compiler_caps_request_tracking() {
        let words = |args: &[&str]| args.iter().map(OsString::from).collect::<Vec<_>>();
        assert!(!requested(words(&["check", "file.hd"]), false));
        assert!(!requested(words(&["run", "--max-heap=1G"]), false));
        assert!(!requested(words(&["run", "--", "--max-memory=1G"]), false));
        assert!(requested(words(&["check", "--max-memory", "1G"]), false));
        assert!(requested(words(&["check", "--max-memory=1G"]), false));
        assert!(requested(words(&["check", "file.hd"]), true));
    }

    #[test]
    fn real_allocator_stops_before_returning_over_cap_allocation() {
        let output = child("text");
        assert_eq!(output.status.code(), Some(101));
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(error.lines().count(), 1, "{error}");
        assert!(error.contains("memory-limit"), "{error}");
        assert!(error.contains("while checking bodies"), "{error}");
        assert!(!error.contains("panic"), "{error}");
    }

    #[test]
    fn real_allocator_json_is_one_diagnostic_and_failure_summary() {
        let output = child("json");
        assert_eq!(output.status.code(), Some(101));
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(error.lines().count(), 2, "{error}");
        assert_eq!(error.matches("\"kind\":\"diagnostic\"").count(), 1);
        assert!(error.contains("while checking bodies"), "{error}");
        assert!(error.contains("\"status\":101"), "{error}");
    }

    #[test]
    fn guest_suspension_keeps_real_allocation_and_free_accounting_balanced() {
        let output = child("suspended");
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }

    #[test]
    fn retained_guest_allocations_do_not_rearm_compiler_enforcement() {
        let output = child("retained-guest");
        assert!(output.status.success(), "{output:?}");
        assert!(output.stderr.is_empty(), "{output:?}");
    }

    #[test]
    fn rearm_excludes_retained_guest_bytes_and_limits_the_next_compile() {
        let output = child("rearm");
        assert_eq!(output.status.code(), Some(101), "{output:?}");
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(error.lines().count(), 1, "{error}");
        assert!(error.contains("while checking bodies"), "{error}");
        assert!(!error.contains("preparing compilation"), "{error}");
    }

    #[test]
    fn emergency_exit_does_not_reenter_allocating_stdout_buffer() {
        let output = child("stdout-borrow");
        assert_eq!(output.status.code(), Some(101));
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(error.lines().count(), 1, "{error}");
        assert!(error.contains("while checking bodies"), "{error}");
        assert!(!error.contains("panic"), "{error}");
    }

    #[test]
    fn emergency_report_uses_allocating_workers_stage_not_main_thread_stage() {
        let output = child("worker");
        assert_eq!(output.status.code(), Some(101));
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(error.lines().count(), 1, "{error}");
        assert!(error.contains("while emitting WebAssembly"), "{error}");
        assert!(!error.contains("checking bodies"), "{error}");
        assert!(!error.contains("precompiling WebAssembly"), "{error}");
    }

    #[test]
    fn hidden_worker_allocations_use_active_precompile_fallback() {
        let output = child("hidden-worker");
        assert_eq!(output.status.code(), Some(101));
        let error = String::from_utf8(output.stderr).unwrap();
        assert_eq!(error.lines().count(), 1, "{error}");
        assert!(error.contains("while precompiling WebAssembly"), "{error}");
        assert!(!error.contains("starting compiler"), "{error}");
    }

    #[test]
    fn reporter_wakes_when_its_unpark_token_was_already_spent() {
        // Models the reporter's rendezvous: its `send` parks and consumes the
        // single token that both the receiver's wakeup and the trip's wakeup
        // set. The report must still be seen, without a further unpark.
        let ready = std::sync::Arc::new(AtomicBool::new(false));
        let (go, wait) = std::sync::mpsc::channel::<()>();
        let (done, finished) = std::sync::mpsc::channel();
        let flag = std::sync::Arc::clone(&ready);
        let reporter = std::thread::spawn(move || {
            wait.recv().unwrap();
            // Spend the token if `recv` left one. A plain `park` would block
            // forever when `recv` already spent it, which is not the race
            // under test.
            std::thread::park_timeout(std::time::Duration::ZERO);
            await_report(&flag);
            done.send(()).unwrap();
        });
        ready.store(true, Ordering::Release);
        reporter.thread().unpark();
        reporter.thread().unpark();
        go.send(()).unwrap();
        let woke = finished.recv_timeout(std::time::Duration::from_secs(10));
        if woke.is_err() {
            // Release the thread so the test process can still finish.
            reporter.thread().unpark();
        }
        assert!(woke.is_ok(), "reporter slept through a ready report");
        reporter.join().unwrap();
    }

    #[test]
    fn real_allocator_trip_while_arming_reports_from_main_thread() {
        // `--max-memory=0` trips inside `arm` on the main thread, right
        // after the reporter rendezvous, as `hd check --max-memory=0` does.
        for _ in 0..20 {
            let output = child("arm");
            assert_eq!(output.status.code(), Some(101), "{output:?}");
            let error = String::from_utf8(output.stderr).unwrap();
            assert_eq!(error.lines().count(), 2, "{error}");
            assert!(error.contains("while starting compiler"), "{error}");
            assert!(error.contains("\"status\":101"), "{error}");
        }
    }

    #[test]
    fn live_bytes_count_frees_and_reallocation_overlap() {
        let count = AtomicU64::new(0);
        add_bytes(&count, 64);
        add_bytes(&count, 128);
        assert_eq!(count.load(Ordering::SeqCst), 192);
        remove_bytes(&count, 64);
        assert_eq!(count.load(Ordering::SeqCst), 128);
        remove_bytes(&count, 128);
        assert_eq!(count.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn nested_stage_scopes_restore_and_are_thread_local() {
        let before = ACTIVE_STAGE.with(Cell::get);
        let outer = stage("Body");
        let body = ACTIVE_STAGE.with(Cell::get);
        assert_eq!(body, stage_index(Stage::Body));
        {
            let _inner = stage("Precompile");
            assert_eq!(ACTIVE_STAGE.with(Cell::get), stage_index(Stage::Precompile));
        }
        assert_eq!(ACTIVE_STAGE.with(Cell::get), body);
        let worker = std::thread::spawn(|| {
            assert_eq!(ACTIVE_STAGE.with(Cell::get), STARTUP);
            let token = Observer.enter(Stage::Emit);
            assert_eq!(ACTIVE_STAGE.with(Cell::get), stage_index(Stage::Emit));
            Observer.exit(token);
            assert_eq!(ACTIVE_STAGE.with(Cell::get), STARTUP);
        });
        worker.join().unwrap();
        assert_eq!(ACTIVE_STAGE.with(Cell::get), body);
        drop(outer);
        assert_eq!(ACTIVE_STAGE.with(Cell::get), before);
    }

    #[test]
    fn emergency_report_matches_diagnostic_protocol() {
        let text = String::from_utf8(diagnostic(123, "checking bodies", false)).unwrap();
        assert_eq!(
            text,
            "error: memory-limit: compiler memory limit of 123 bytes exceeded while checking bodies\n"
        );
        let json = String::from_utf8(diagnostic(123, "checking bodies", true)).unwrap();
        let lines: Vec<_> = json.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"code\":\"memory-limit\""));
        assert!(lines[0].contains("\"file\":null"));
        assert!(lines[1].contains("\"errors\":1"));
        assert!(lines[1].contains("\"status\":101"));
    }
}
