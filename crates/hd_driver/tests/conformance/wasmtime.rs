//! The wasmtime half of the conformance engine modes, built with the
//! `wasmtime` feature. `no_wasmtime.rs` stands in for it without. The
//! engine's options follow the selected level (`EngineConfig::pipeline`).

use std::sync::OnceLock;
use std::time::Instant;

use super::{Case, RUNTIME_STATS, Ran, Verdict, WASMTIME_DIFFS, level, tier, workers};

/// Runs `wasm` on wasmtime and, when Node ran it too (`node`), compares
/// the two. A wasmtime failure is an error, never a Node fallback.
pub fn run_program(
    case: &Case,
    wasm: &[u8],
    profile: Option<&str>,
    node: Option<Ran>,
) -> std::io::Result<Ran> {
    let setup = hd_run::HostSetup {
        providers: Vec::new(),
        grants: hd_run::Grants::default(),
        limits: hd_run::Limits::default(),
        args: Vec::new(),
        program: String::new(),
    };
    let config = hd_run_wasmtime::EngineConfig::pipeline(level());
    let profile = hd_run_wasmtime::RuntimeProfile::from_name(profile.unwrap_or("default"))
        .map_err(std::io::Error::other)?;
    let started = Instant::now();
    let result = hd_run_wasmtime::run_captured_with_profile(config, profile, wasm, &setup);
    {
        let mut stats = RUNTIME_STATS.lock().expect("runtime stats lock");
        let stats = stats.entry(tier(case)).or_default();
        stats.wasmtime_elapsed += started.elapsed();
        stats.wasmtime_programs.insert(case.path.clone());
    }
    let w = result.map_err(|error| std::io::Error::other(format!("wasmtime: {error}")))?;
    let w = Ran {
        stdout: w.stdout,
        stderr: w.stderr,
        status: Some(i32::from(w.status)),
    };
    let Some(node) = node else { return Ok(w) };
    if (&w.stdout, &w.stderr, w.status) == (&node.stdout, &node.stderr, node.status) {
        RUNTIME_STATS
            .lock()
            .expect("runtime stats lock")
            .entry(tier(case))
            .or_default()
            .matches += 1;
    } else {
        let line = |r: &Ran| {
            format!(
                "{:?} out {:?} err {:?}",
                r.status,
                String::from_utf8_lossy(&r.stdout),
                String::from_utf8_lossy(&r.stderr)
            )
        };
        WASMTIME_DIFFS.lock().expect("diffs lock").push(format!(
            "{}\n  node     {}\n  wasmtime {}",
            case.path,
            line(&node),
            line(&w)
        ));
    }
    Ok(w)
}

/// Drives a runtime scenario on wasmtime and, when Node drove it too
/// (`node`), compares the two. A wasmtime failure is an error, never a
/// Node fallback.
pub fn drive_scenario(
    case: &Case,
    wasm: &[u8],
    profile: Option<&str>,
    scenario: &str,
    node: Option<Ran>,
) -> std::io::Result<Ran> {
    use hd_run_wasmtime::scenario::{Scenario, drive_scenario};

    let setup = hd_run::HostSetup {
        providers: Vec::new(),
        grants: hd_run::Grants::default(),
        limits: hd_run::Limits::default(),
        args: Vec::new(),
        program: String::new(),
    };
    let config = hd_run_wasmtime::EngineConfig::pipeline(level());
    let profile = hd_run_wasmtime::RuntimeProfile::from_name(profile.unwrap_or("default"))
        .map_err(std::io::Error::other)?;
    let scenario = match scenario {
        "cancellation-cleanup" => Scenario::CancellationCleanup,
        "competing-drivers" => Scenario::CompetingDrivers,
        "reentrant-poll" => Scenario::ReentrantPoll,
        name => {
            return Err(std::io::Error::other(format!(
                "wasmtime drives no runtime scenario '{name}'"
            )));
        }
    };
    let started = Instant::now();
    let result = drive_scenario(config, profile, wasm, &setup, scenario);
    {
        let mut stats = RUNTIME_STATS.lock().expect("runtime stats lock");
        let stats = stats.entry(tier(case)).or_default();
        stats.wasmtime_elapsed += started.elapsed();
        stats.wasmtime_programs.insert(case.path.clone());
    }
    let w = result.map_err(|error| std::io::Error::other(format!("wasmtime: {error}")))?;
    let w = Ran {
        stdout: w.stdout,
        stderr: w.stderr,
        status: Some(w.status),
    };
    let Some(node) = node else { return Ok(w) };
    if (&w.stdout, &w.stderr, w.status) == (&node.stdout, &node.stderr, node.status) {
        RUNTIME_STATS
            .lock()
            .expect("runtime stats lock")
            .entry(tier(case))
            .or_default()
            .matches += 1;
    } else {
        let line = |r: &Ran| {
            format!(
                "{:?} out {:?} err {:?}",
                r.status,
                String::from_utf8_lossy(&r.stdout),
                String::from_utf8_lossy(&r.stderr)
            )
        };
        WASMTIME_DIFFS.lock().expect("diffs lock").push(format!(
            "{}\n  node     {}\n  wasmtime {}",
            case.path,
            line(&node),
            line(&w)
        ));
    }
    Ok(w)
}

/// Runs the test cases `runs` on wasmtime and returns their result lines
/// as the Node host prints them. When Node ran them too (`node`, its
/// output), the outcomes are compared, timing excluded.
pub fn test_stdout(
    case: &Case,
    wasm: &[u8],
    runs: &[(&hd_driver::TestCase, (u32, u32))],
    node: Option<&str>,
) -> Result<String, Verdict> {
    let started = Instant::now();
    let result = native_test_results(wasm, runs);
    {
        let mut stats = RUNTIME_STATS.lock().expect("runtime stats lock");
        let stats = stats.entry(tier(case)).or_default();
        stats.wasmtime_elapsed += started.elapsed();
        *stats.wasmtime_harness.entry(case.path.clone()).or_default() += 1;
    }
    let results = result.map_err(|e| Verdict::Crash(format!("wasmtime tests: {e}")))?;
    if let Some(node) = node {
        let parsed = node
            .lines()
            .map(hd_run_wasmtime::testing::CaseRun::from_json_line)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| Verdict::Crash(format!("Node test protocol: {e}")))?;
        compare_test_results(case, &parsed, &results);
    }
    Ok(results
        .iter()
        .map(hd_run_wasmtime::testing::CaseRun::to_json_line)
        .collect::<Vec<_>>()
        .join("\n"))
}

/// The code cache over `$HD_CACHE/obj` when `HD_CACHE` is set
/// (`docs/compiler/engines-and-test-runner.md` §18.2): a later run with
/// the same cache reuses this run's machine code.
pub fn code_cache() -> Option<hd_run_wasmtime::CodeCache> {
    let dir = std::env::var_os("HD_CACHE")?;
    hd_run_wasmtime::install_code_cache(hd_cache::DiskStore::new(
        std::path::PathBuf::from(dir).join("obj"),
    ))
}

/// What the code cache did, for the runtime summary.
pub fn code_cache_line() -> String {
    let stats = hd_run_wasmtime::code_cache_stats();
    format!(
        "code cache: {} functions reused, {} compiled",
        stats.hits, stats.compiled
    )
}

/// The stack a conformance worker thread needs to host a guest.
pub fn worker_stack() -> usize {
    hd_run_wasmtime::thread_stack(&hd_run_wasmtime::EngineConfig::pipeline(level()))
}

fn native_test_results(
    wasm: &[u8],
    runs: &[(&hd_driver::TestCase, (u32, u32))],
) -> Result<Vec<hd_run_wasmtime::testing::CaseRun>, String> {
    use hd_run_wasmtime::{
        NativeBodyRunner, RuntimeProfile, TestEngine, TestEnvironment, TestHostKind,
        TestPoolConfig,
        testing::{TestCase, execute_case},
    };
    static ENGINE: OnceLock<Result<TestEngine, String>> = OnceLock::new();
    let engine = ENGINE
        .get_or_init(|| {
            let workers =
                u32::try_from(workers()).map_err(|_| "too many test workers".to_owned())?;
            TestEngine::new(
                hd_run_wasmtime::EngineConfig::pipeline(level()),
                TestPoolConfig {
                    workers,
                    shrink_spares: workers,
                },
            )
        })
        .as_ref()
        .map_err(Clone::clone)?;
    let module = engine.compile(wasm)?;
    let host_kind = |test: &hd_driver::TestCase| {
        if test.file.starts_with("tests/$doc") {
            TestHostKind::Doc(RuntimeProfile::Default)
        } else if test.file.starts_with("tests/") {
            TestHostKind::Integration
        } else {
            TestHostKind::Unit
        }
    };
    let kind = host_kind(runs[0].0);
    if runs.iter().any(|(test, _)| host_kind(test) != kind) {
        return Err("a conformance test program mixes host kinds".into());
    }
    let prepared = engine.prepare(&module, kind)?;
    let environment = TestEnvironment {
        cwd: std::env::current_dir().map_err(|e| e.to_string())?,
        program: String::new(),
        children: Vec::new(),
    };
    let mut results = Vec::new();
    for (test, (export, init)) in runs {
        let case = TestCase {
            test: *export,
            init: *init,
            kind: test.kind.clone(),
            ..TestCase::default()
        };
        let mut runner = NativeBodyRunner::new(
            prepared.clone(),
            case.clone(),
            environment.clone(),
            hd_run::HostSetup {
                providers: Vec::new(),
                grants: hd_run::Grants::default(),
                limits: hd_run::Limits::default(),
                args: Vec::new(),
                program: String::new(),
            },
        );
        execute_case(&case, &mut runner, &mut |result| results.push(result))?;
    }
    Ok(results)
}

fn compare_test_results(
    case: &Case,
    node: &[hd_run_wasmtime::testing::CaseRun],
    native: &[hd_run_wasmtime::testing::CaseRun],
) {
    let matches = node
        .iter()
        .zip(native)
        .filter(|(n, w)| n.equivalent(w))
        .count();
    let comparisons = node.len().max(native.len());
    {
        let mut stats = RUNTIME_STATS.lock().expect("runtime stats lock");
        let stats = stats.entry(tier(case)).or_default();
        stats.test_matches += matches;
        stats.test_comparisons += comparisons;
    }
    if matches != comparisons {
        WASMTIME_DIFFS.lock().expect("diffs lock").push(format!(
            "{} test outcomes (timing excluded)\n  node     {node:?}\n  wasmtime {native:?}",
            case.path,
        ));
    }
}
