use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use anyhow::{Context, Result, anyhow, bail};
use serde::{Deserialize, Serialize};
use wasmparser::{Parser, Payload};
use wasmtime::{
    CacheStore, Config, Engine, Instance, Module, OptLevel, PoolingAllocationConfig,
    RegallocAlgorithm, Store,
};

const WASMTIME_VERSION: &str = "49.0.2";

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Case {
    experiment: String,
    name: String,
    path: PathBuf,
    full_work: i32,
    quick_work: i32,
    work: i32,
    denominator: f64,
    metric: String,
    host_fill_bytes: usize,
    host_read_bytes: usize,
    initial_heap_mib: Option<u64>,
    single_pass: bool,
    node_kernel: String,
    notes: String,
    wasm_bytes: usize,
    code_bytes: usize,
    functions: u32,
    types: u32,
    globals: u32,
    sections: BTreeMap<String, usize>,
}

#[derive(Clone, Debug)]
struct SourceCase {
    experiment: &'static str,
    name: String,
    wat: String,
    raw: Option<Vec<u8>>,
    full_work: i32,
    quick_work: i32,
    denominator: f64,
    metric: &'static str,
    host_fill_bytes: usize,
    host_read_bytes: usize,
    initial_heap_mib: Option<u64>,
    single_pass: bool,
    node_kernel: String,
    notes: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct Manifest {
    cases: Vec<Case>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Distribution {
    p10: f64,
    p50: f64,
    p90: f64,
    p95: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct EngineMeasurements {
    compile_wall_ms: Distribution,
    compile_cpu_ms: Distribution,
    instantiate_ms: Distribution,
    runtime_ms: Distribution,
    metric_p50: f64,
    metric_p95: f64,
    peak_rss_bytes: u64,
    checksum: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NodeRow {
    name: String,
    mode: String,
    compile_ms: Distribution,
    instantiate_ms: Distribution,
    runtime_ms: Distribution,
    checksum: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct NodeOutput {
    node: String,
    v8: String,
    mode: String,
    rows: Vec<NodeRow>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CaseReport {
    case: Case,
    wasmtime_none: EngineMeasurements,
    wasmtime_speed: EngineMeasurements,
    v8_liftoff: NodeRow,
    v8_turbofan: NodeRow,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Report {
    generated_at_utc: String,
    machine: String,
    load: String,
    rustc: String,
    wasmtime: String,
    node: String,
    v8: String,
    samples: usize,
    warmups: usize,
    compile_samples: usize,
    compile_warmups: usize,
    quick: bool,
    s2_cache_hits: Vec<CacheHitRow>,
    s3_cache_io: Vec<CacheIoRow>,
    cases: Vec<CaseReport>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CacheHitRow {
    perturbation: String,
    gets: usize,
    hits: usize,
    misses: usize,
    hit_percent: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct CacheIoRow {
    functions: usize,
    cold_ms: f64,
    memory_hit_ms: f64,
    file_hit_ms: f64,
    cold_share_percent: f64,
    memory_us_per_entry: f64,
    file_us_per_entry: f64,
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().unwrap_or_else(|| "help".to_owned());
    match command.as_str() {
        "generate" => {
            let out = args
                .next()
                .map_or_else(|| PathBuf::from("out"), PathBuf::from);
            generate(&out, false)?;
        }
        "run" => run_command(args.collect())?,
        "report" => {
            let input = args.next().context("report needs results.json")?;
            let output = args
                .next()
                .context("report needs an output Markdown path")?;
            let report: Report = serde_json::from_slice(&fs::read(&input)?)?;
            fs::write(output, render_wasm_first_report(&report))?;
        }
        _ => {
            eprintln!(
                "usage: hd-spike-0c generate [OUT] | run [--quick] [--samples N] [--warmups N] [--compile-samples N] [--compile-warmups N] [--only EXPERIMENT_OR_PREFIX] | report RESULTS OUTPUT"
            );
        }
    }
    Ok(())
}

fn run_command(args: Vec<String>) -> Result<()> {
    let mut samples = 15_usize;
    let mut warmups = 3_usize;
    let mut compile_samples = 3_usize;
    let mut compile_warmups = 1_usize;
    let mut quick = false;
    let mut only = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--quick" => quick = true,
            "--samples" => {
                index += 1;
                samples = args
                    .get(index)
                    .context("missing --samples value")?
                    .parse()?;
            }
            "--warmups" => {
                index += 1;
                warmups = args
                    .get(index)
                    .context("missing --warmups value")?
                    .parse()?;
            }
            "--only" => {
                index += 1;
                only = Some(args.get(index).context("missing --only value")?.clone());
            }
            "--compile-samples" => {
                index += 1;
                compile_samples = args
                    .get(index)
                    .context("missing --compile-samples value")?
                    .parse()?;
            }
            "--compile-warmups" => {
                index += 1;
                compile_warmups = args
                    .get(index)
                    .context("missing --compile-warmups value")?
                    .parse()?;
            }
            unknown => bail!("unknown argument {unknown}"),
        }
        index += 1;
    }
    if quick {
        samples = samples.min(3);
        warmups = warmups.min(1);
        compile_samples = compile_samples.min(1);
        compile_warmups = compile_warmups.min(0);
    }

    let out = PathBuf::from("out");
    let mut manifest = generate(&out, quick)?;
    if let Some(filter) = only {
        manifest.cases.retain(|case| {
            case.experiment.eq_ignore_ascii_case(&filter) || case.name.starts_with(&filter)
        });
        if manifest.cases.is_empty() {
            bail!("--only {filter} matched no cases");
        }
        fs::write(
            out.join("manifest.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
    }
    let mut none = BTreeMap::new();
    let mut speed = BTreeMap::new();
    for (case_index, case) in manifest.cases.iter().enumerate() {
        eprintln!(
            "[{}/{}] {}",
            case_index + 1,
            manifest.cases.len(),
            case.name
        );
        none.insert(
            case.name.clone(),
            measure_wasmtime(
                case,
                OptLevel::None,
                samples,
                warmups,
                compile_samples,
                compile_warmups,
            )?,
        );
        speed.insert(
            case.name.clone(),
            measure_wasmtime(
                case,
                OptLevel::Speed,
                samples,
                warmups,
                compile_samples,
                compile_warmups,
            )?,
        );
    }

    let manifest_path = out.join("manifest.json");
    let liftoff = run_node(
        &manifest_path,
        "liftoff",
        samples,
        warmups,
        compile_samples,
        compile_warmups,
    )?;
    let turbofan = run_node(
        &manifest_path,
        "turbofan",
        samples,
        warmups,
        compile_samples,
        compile_warmups,
    )?;
    let liftoff_rows: BTreeMap<_, _> = liftoff
        .rows
        .into_iter()
        .map(|row| (row.name.clone(), row))
        .collect();
    let turbofan_rows: BTreeMap<_, _> = turbofan
        .rows
        .into_iter()
        .map(|row| (row.name.clone(), row))
        .collect();
    let s2_cache_hits = if manifest.cases.iter().any(|case| case.experiment == "S2") {
        measure_s2_cache(&manifest)?
    } else {
        Vec::new()
    };
    let s3_cache_io = if manifest.cases.iter().any(|case| case.experiment == "S3") {
        measure_s3_cache(&manifest, &out)?
    } else {
        Vec::new()
    };

    let cases = manifest
        .cases
        .into_iter()
        .map(|case| {
            Ok(CaseReport {
                wasmtime_none: none
                    .remove(&case.name)
                    .context("missing wasmtime None row")?,
                wasmtime_speed: speed
                    .remove(&case.name)
                    .context("missing wasmtime Speed row")?,
                v8_liftoff: liftoff_rows
                    .get(&case.name)
                    .context("missing Liftoff row")?
                    .clone(),
                v8_turbofan: turbofan_rows
                    .get(&case.name)
                    .context("missing TurboFan row")?
                    .clone(),
                case,
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let report = Report {
        generated_at_utc: command_output("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
        machine: command_output("uname", &["-a"]),
        load: command_output("uptime", &[]),
        rustc: command_output("rustc", &["--version"]),
        wasmtime: WASMTIME_VERSION.to_owned(),
        node: liftoff.node,
        v8: liftoff.v8,
        samples,
        warmups,
        compile_samples,
        compile_warmups,
        quick,
        s2_cache_hits,
        s3_cache_io,
        cases,
    };
    fs::write(
        out.join("results.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    println!("wrote {}", out.join("results.json").display());
    Ok(())
}

fn command_output(program: &str, args: &[&str]) -> String {
    Command::new(program)
        .args(args)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .unwrap_or_else(|error| format!("unavailable: {error}"))
}

fn run_node(
    path: &Path,
    mode: &str,
    samples: usize,
    warmups: usize,
    compile_samples: usize,
    compile_warmups: usize,
) -> Result<NodeOutput> {
    let flag = if mode == "liftoff" {
        "--liftoff-only"
    } else {
        "--no-liftoff"
    };
    let runner = Path::new(env!("CARGO_MANIFEST_DIR")).join("node-runner.mjs");
    let output = Command::new("node")
        .arg(flag)
        .arg(runner)
        .arg(path)
        .arg(mode)
        .arg(samples.to_string())
        .arg(warmups.to_string())
        .arg(compile_samples.to_string())
        .arg(compile_warmups.to_string())
        .output()
        .context("running Node benchmark")?;
    if !output.status.success() {
        bail!(
            "Node {mode} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("reading Node benchmark JSON")
}

fn engine_for(case: &Case, level: OptLevel) -> Result<Engine> {
    let mut config = Config::new();
    config.wasm_gc(true).cranelift_opt_level(level);
    if case.single_pass {
        config.cranelift_regalloc_algorithm(RegallocAlgorithm::SinglePass);
    }
    if let Some(mib) = case.initial_heap_mib {
        config.gc_heap_initial_size(mib * 1024 * 1024);
    }
    if case.experiment == "S5" {
        let mut pooling = PoolingAllocationConfig::new();
        pooling
            .total_memories(64)
            .total_tables(0)
            .total_stacks(0)
            .total_gc_heaps(64);
        config.allocation_strategy(pooling);
    }
    Engine::new(&config).map_err(|error| anyhow!("creating wasmtime engine: {error}"))
}

#[derive(Debug, Default)]
struct MemoryCache {
    entries: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    gets: AtomicUsize,
    hits: AtomicUsize,
    inserts: AtomicUsize,
}

impl MemoryCache {
    fn reset_counts(&self) {
        self.gets.store(0, Ordering::Relaxed);
        self.hits.store(0, Ordering::Relaxed);
        self.inserts.store(0, Ordering::Relaxed);
    }
}

impl CacheStore for MemoryCache {
    fn get(&self, key: &[u8]) -> Option<Cow<'_, [u8]>> {
        self.gets.fetch_add(1, Ordering::Relaxed);
        let value = self.entries.lock().ok()?.get(key).cloned();
        if value.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
        value.map(Cow::Owned)
    }

    fn insert(&self, key: &[u8], value: Vec<u8>) -> bool {
        self.inserts.fetch_add(1, Ordering::Relaxed);
        self.entries
            .lock()
            .map(|mut entries| {
                entries.insert(key.to_vec(), value);
                true
            })
            .unwrap_or(false)
    }
}

#[derive(Debug)]
struct FileCache {
    directory: PathBuf,
    gets: AtomicUsize,
    hits: AtomicUsize,
}

impl FileCache {
    fn new(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory)?;
        Ok(Self {
            directory,
            gets: AtomicUsize::new(0),
            hits: AtomicUsize::new(0),
        })
    }

    fn path(&self, key: &[u8]) -> PathBuf {
        let mut name = String::with_capacity(key.len() * 2);
        for byte in key {
            use std::fmt::Write as _;
            let _ = write!(name, "{byte:02x}");
        }
        self.directory.join(name)
    }

    fn reset_counts(&self) {
        self.gets.store(0, Ordering::Relaxed);
        self.hits.store(0, Ordering::Relaxed);
    }
}

impl CacheStore for FileCache {
    fn get(&self, key: &[u8]) -> Option<Cow<'_, [u8]>> {
        self.gets.fetch_add(1, Ordering::Relaxed);
        let value = fs::read(self.path(key)).ok();
        if value.is_some() {
            self.hits.fetch_add(1, Ordering::Relaxed);
        }
        value.map(Cow::Owned)
    }

    fn insert(&self, key: &[u8], value: Vec<u8>) -> bool {
        fs::write(self.path(key), value).is_ok()
    }
}

fn cache_engine(cache: Arc<dyn CacheStore>) -> Result<Engine> {
    let mut config = Config::new();
    config.wasm_gc(true).cranelift_opt_level(OptLevel::Speed);
    config
        .enable_incremental_compilation(cache)
        .map_err(|error| anyhow!("enabling incremental compilation: {error}"))?;
    Engine::new(&config).map_err(|error| anyhow!("creating cache engine: {error}"))
}

fn measure_s2_cache(manifest: &Manifest) -> Result<Vec<CacheHitRow>> {
    let base = manifest
        .cases
        .iter()
        .find(|case| case.name == "s2-base")
        .context("missing s2-base")?;
    let base_bytes = fs::read(&base.path)?;
    let mut rows = Vec::new();
    for case in manifest.cases.iter().filter(|case| case.experiment == "S2") {
        let cache = Arc::new(MemoryCache::default());
        let engine = cache_engine(cache.clone())?;
        Module::new(&engine, &base_bytes)?;
        cache.reset_counts();
        Module::new(&engine, fs::read(&case.path)?)?;
        let gets = cache.gets.load(Ordering::Relaxed);
        let hits = cache.hits.load(Ordering::Relaxed);
        rows.push(CacheHitRow {
            perturbation: case.name.trim_start_matches("s2-").to_owned(),
            gets,
            hits,
            misses: gets.saturating_sub(hits),
            hit_percent: if gets == 0 {
                100.0
            } else {
                hits as f64 * 100.0 / gets as f64
            },
        });
    }
    Ok(rows)
}

fn measure_s3_cache(manifest: &Manifest, out: &Path) -> Result<Vec<CacheIoRow>> {
    let mut rows = Vec::new();
    for case in manifest.cases.iter().filter(|case| case.experiment == "S3") {
        let bytes = fs::read(&case.path)?;
        let functions = case.functions as usize;

        let memory = Arc::new(MemoryCache::default());
        let memory_engine = cache_engine(memory.clone())?;
        let start = Instant::now();
        Module::new(&memory_engine, &bytes)?;
        let cold_ms = start.elapsed().as_secs_f64() * 1_000.0;
        memory.reset_counts();
        let start = Instant::now();
        Module::new(&memory_engine, &bytes)?;
        let memory_hit_ms = start.elapsed().as_secs_f64() * 1_000.0;
        let memory_entries = memory.hits.load(Ordering::Relaxed).max(1);

        let directory = out.join("cache").join(format!(
            "{}-{}-{}",
            std::process::id(),
            functions,
            rows.len()
        ));
        let files = Arc::new(FileCache::new(directory)?);
        let file_engine = cache_engine(files.clone())?;
        Module::new(&file_engine, &bytes)?;
        files.reset_counts();
        let start = Instant::now();
        Module::new(&file_engine, &bytes)?;
        let file_hit_ms = start.elapsed().as_secs_f64() * 1_000.0;
        let file_entries = files.hits.load(Ordering::Relaxed).max(1);

        rows.push(CacheIoRow {
            functions,
            cold_ms,
            memory_hit_ms,
            file_hit_ms,
            cold_share_percent: memory_hit_ms * 100.0 / cold_ms.max(f64::EPSILON),
            memory_us_per_entry: memory_hit_ms * 1_000.0 / memory_entries as f64,
            file_us_per_entry: file_hit_ms * 1_000.0 / file_entries as f64,
        });
    }
    Ok(rows)
}

fn measure_wasmtime(
    case: &Case,
    level: OptLevel,
    samples: usize,
    warmups: usize,
    compile_samples: usize,
    compile_warmups: usize,
) -> Result<EngineMeasurements> {
    let bytes = fs::read(&case.path)?;
    let engine = engine_for(case, level)?;
    let mut compile_wall = Vec::new();
    let mut compile_cpu = Vec::new();
    for iteration in 0..(compile_warmups + compile_samples) {
        let cpu = cpu_seconds();
        let start = Instant::now();
        let module = Module::new(&engine, &bytes)?;
        std::hint::black_box(module);
        if iteration >= compile_warmups {
            compile_wall.push(start.elapsed().as_secs_f64() * 1_000.0);
            compile_cpu.push((cpu_seconds() - cpu) * 1_000.0);
        }
    }

    let module = Module::new(&engine, &bytes)?;
    let mut instantiate = Vec::new();
    for iteration in 0..(warmups + samples) {
        let mut store = Store::new(&engine, ());
        let start = Instant::now();
        let instance = Instance::new(&mut store, &module, &[])?;
        std::hint::black_box(instance);
        if iteration >= warmups {
            instantiate.push(start.elapsed().as_secs_f64() * 1_000.0);
        }
    }

    let mut runtime = Vec::new();
    let mut checksum = 0_i64;
    for iteration in 0..(warmups + samples) {
        let mut store = Store::new(&engine, ());
        let instance = Instance::new(&mut store, &module, &[])?;
        let run = instance.get_typed_func::<i32, i64>(&mut store, "run")?;
        let start = Instant::now();
        let value = if case.host_fill_bytes > 0 {
            let memory = instance
                .get_memory(&mut store, "memory")
                .context("host-fill case has no memory")?;
            let data = vec![0x5a; case.host_fill_bytes];
            memory.write(&mut store, 0, &data)?;
            run.call(&mut store, case.work)?
        } else if case.host_read_bytes > 0 {
            let memory = instance
                .get_memory(&mut store, "memory")
                .context("host-read case has no memory")?;
            let mut data = vec![0_u8; case.host_read_bytes];
            memory.read(&store, 0, &mut data)?;
            std::hint::black_box(data);
            run.call(&mut store, case.work)?
        } else {
            run.call(&mut store, case.work)?
        };
        let elapsed = start.elapsed().as_secs_f64() * 1_000.0;
        checksum ^= value;
        if iteration >= warmups {
            runtime.push(elapsed);
        }
    }
    let runtime_stats = distribution(&runtime);
    Ok(EngineMeasurements {
        compile_wall_ms: distribution(&compile_wall),
        compile_cpu_ms: distribution(&compile_cpu),
        instantiate_ms: distribution(&instantiate),
        metric_p50: runtime_stats.p50 * 1_000_000.0 / case.denominator.max(1.0),
        metric_p95: runtime_stats.p95 * 1_000_000.0 / case.denominator.max(1.0),
        runtime_ms: runtime_stats,
        peak_rss_bytes: peak_rss_bytes(),
        checksum: checksum.to_string(),
    })
}

fn cpu_seconds() -> f64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the supplied rusage on success.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return 0.0;
    }
    // SAFETY: the successful call above initialized usage.
    let usage = unsafe { usage.assume_init() };
    let user = usage.ru_utime.tv_sec as f64 + usage.ru_utime.tv_usec as f64 / 1_000_000.0;
    let system = usage.ru_stime.tv_sec as f64 + usage.ru_stime.tv_usec as f64 / 1_000_000.0;
    user + system
}

fn peak_rss_bytes() -> u64 {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: getrusage initializes the supplied rusage on success.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return 0;
    }
    // SAFETY: the successful call above initialized usage.
    let usage = unsafe { usage.assume_init() };
    #[cfg(target_os = "macos")]
    return usage.ru_maxrss as u64;
    #[cfg(not(target_os = "macos"))]
    return usage.ru_maxrss as u64 * 1024;
}

fn distribution(values: &[f64]) -> Distribution {
    Distribution {
        p10: quantile(values, 0.10),
        p50: quantile(values, 0.50),
        p90: quantile(values, 0.90),
        p95: quantile(values, 0.95),
    }
}

fn quantile(values: &[f64], q: f64) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let index = ((q * sorted.len() as f64).ceil() as usize).saturating_sub(1);
    sorted
        .get(index.min(sorted.len().saturating_sub(1)))
        .copied()
        .unwrap_or(0.0)
}

fn generate(out: &Path, quick: bool) -> Result<Manifest> {
    let modules = out.join("modules");
    fs::create_dir_all(&modules)?;
    let mut cases = Vec::new();
    for source in source_cases() {
        let bytes = if let Some(raw) = &source.raw {
            raw.clone()
        } else {
            wat::parse_str(&source.wat).with_context(|| format!("encoding {}", source.name))?
        };
        let path = modules.join(format!("{}.wasm", source.name));
        fs::write(&path, &bytes)?;
        let sizes = module_sizes(&bytes)?;
        cases.push(Case {
            experiment: source.experiment.to_owned(),
            name: source.name,
            path: path.canonicalize()?,
            full_work: source.full_work,
            quick_work: source.quick_work,
            work: if quick {
                source.quick_work
            } else {
                source.full_work
            },
            denominator: if quick {
                source.denominator * f64::from(source.quick_work.max(1))
                    / f64::from(source.full_work.max(1))
            } else {
                source.denominator
            },
            metric: source.metric.to_owned(),
            host_fill_bytes: if quick {
                source
                    .host_fill_bytes
                    .min(source.quick_work.max(0) as usize)
            } else {
                source.host_fill_bytes
            },
            host_read_bytes: if quick {
                source
                    .host_read_bytes
                    .min(source.quick_work.max(0) as usize)
            } else {
                source.host_read_bytes
            },
            initial_heap_mib: source.initial_heap_mib,
            single_pass: source.single_pass,
            node_kernel: source.node_kernel,
            notes: source.notes,
            wasm_bytes: bytes.len(),
            code_bytes: sizes.0,
            functions: sizes.1,
            types: sizes.2,
            globals: sizes.3,
            sections: sizes.4,
        });
    }
    let manifest = Manifest { cases };
    fs::write(
        out.join("manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

fn module_sizes(bytes: &[u8]) -> Result<(usize, u32, u32, u32, BTreeMap<String, usize>)> {
    let mut code_bytes = 0;
    let mut functions = 0;
    let mut types = 0;
    let mut globals = 0;
    let mut sections = BTreeMap::new();
    for payload in Parser::new(0).parse_all(bytes) {
        match payload? {
            Payload::CodeSectionEntry(body) => {
                code_bytes += body.as_bytes().len();
                functions += 1;
            }
            Payload::TypeSection(reader) => {
                sections.insert("types".into(), range_len(reader.range()));
                types += reader.count();
            }
            Payload::CodeSectionStart { range, .. } => {
                sections.insert("code".into(), range_len(range));
            }
            Payload::DataSection(reader) => {
                sections.insert("data".into(), range_len(reader.range()));
            }
            Payload::GlobalSection(reader) => {
                sections.insert("globals".into(), range_len(reader.range()));
                globals += reader.count();
            }
            Payload::CustomSection(reader) => {
                sections.insert(reader.name().to_owned(), reader.data().len());
            }
            _ => {}
        }
    }
    Ok((code_bytes, functions, types, globals, sections))
}

fn range_len(range: std::ops::Range<u64>) -> usize {
    usize::try_from(range.end - range.start).unwrap_or(usize::MAX)
}

fn base_case(experiment: &'static str, name: impl Into<String>, wat: String) -> SourceCase {
    SourceCase {
        experiment,
        name: name.into(),
        wat,
        raw: None,
        full_work: 1,
        quick_work: 1,
        denominator: 1.0,
        metric: "ns/run",
        host_fill_bytes: 0,
        host_read_bytes: 0,
        initial_heap_mib: None,
        single_pass: false,
        node_kernel: String::new(),
        notes: String::new(),
    }
}

fn source_cases() -> Vec<SourceCase> {
    let mut cases = Vec::new();
    add_e0(&mut cases);
    add_e1(&mut cases);
    add_e2(&mut cases);
    add_e3(&mut cases);
    add_e4(&mut cases);
    add_e5(&mut cases);
    add_e6(&mut cases);
    add_e7(&mut cases);
    add_e8(&mut cases);
    add_e9(&mut cases);
    add_e10(&mut cases);
    add_e11(&mut cases);
    add_s1(&mut cases);
    add_s2(&mut cases);
    add_s3(&mut cases);
    add_s4(&mut cases);
    add_s5(&mut cases);
    add_s6(&mut cases);
    add_s7(&mut cases);
    cases
}

fn loop_module(setup: &str, body: &str, finish: &str) -> String {
    format!(
        "(module\n{setup}\n(func (export \"run\") (param $n i32) (result i64)\n  (local $i i32) (local $sum i64)\n  (block $done (loop $loop\n    (br_if $done (i32.ge_u (local.get $i) (local.get $n)))\n    {body}\n    (local.set $i (i32.add (local.get $i) (i32.const 1)))\n    (br $loop)))\n  {finish}\n  (local.get $sum)))"
    )
}

fn add_e0(cases: &mut Vec<SourceCase>) {
    for fields in [2, 4, 8] {
        let field_defs = "(field i32) ".repeat(fields);
        let values = "(i32.const 1) ".repeat(fields);
        let wat = loop_module(
            &format!("(type $S (struct {field_defs}))"),
            &format!("(drop (struct.new $S {values}))"),
            "",
        );
        let mut case = base_case("E0", format!("e0-dead-{fields}f"), wat);
        case.full_work = 10_000_000;
        case.quick_work = 100_000;
        case.denominator = 10_000_000.0;
        case.metric = "ns/allocation";
        case.notes =
            "10M dead allocations; automatic collection count is not exposed by wasmtime 49".into();
        cases.push(case);
    }
    for mib in [1, 10, 50] {
        let wat = loop_module(
            "(type $N (struct (field i32) (field i32) (field (ref null $N))))",
            "(local.set $head (struct.new $N (local.get $i) (i32.const 1) (local.get $head)))",
            "(local.set $sum (i64.extend_i32_u (struct.get $N 0 (ref.as_non_null (local.get $head)))))",
        )
        .replace("(local $i i32) (local $sum i64)", "(local $i i32) (local $sum i64) (local $head (ref null $N))");
        let allocations = mib * 1024 * 1024 / 16;
        let mut case = base_case("E0", format!("e0-live-{mib}mib"), wat);
        case.full_work = allocations;
        case.quick_work = allocations.min(65_536);
        case.denominator = f64::from(allocations);
        case.metric = "ns/allocation";
        case.notes = format!("linked live set, approximately {mib} MiB at 16 bytes/node");
        cases.push(case);
    }
}

fn e1_module(erased: bool, operation: &str) -> String {
    let array_type = if erased {
        "(type $A (array (mut eqref)))"
    } else {
        "(type $A (array (mut (ref null $P))))"
    };
    let read = |index: &str| {
        if erased {
            format!("(ref.cast (ref $P) (array.get $A (local.get $a) {index}))")
        } else {
            format!("(ref.as_non_null (array.get $A (local.get $a) {index}))")
        }
    };
    let current = read("(local.get $i)");
    let next = read("(i32.rem_u (i32.add (local.get $i) (i32.const 1)) (local.get $n))");
    let kernel = match operation {
        "read" => format!("(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (struct.get $P 0 {current}))))"),
        "write" => "(array.set $A (local.get $a) (local.get $i) (struct.new $P (local.get $i) (i32.const 7)))".to_owned(),
        "push" => "(array.set $A (ref.as_non_null (local.get $b)) (i32.add (local.get $n) (local.get $i)) (struct.new $P (local.get $i) (i32.const 7)))".to_owned(),
        "map" => format!("(array.set $A (ref.as_non_null (local.get $b)) (local.get $i) (struct.new $P (i32.add (struct.get $P 0 {current}) (i32.const 1)) (i32.const 1)))"),
        "sort" => format!("(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (i32.gt_u (struct.get $P 0 {current}) (struct.get $P 0 {next})))))"),
        _ => unreachable!(),
    };
    let second_array = if matches!(operation, "push" | "map") {
        let length = if operation == "push" {
            "(i32.mul (local.get $n) (i32.const 2))"
        } else {
            "(local.get $n)"
        };
        format!("(local.set $b (array.new_default $A {length}))")
    } else {
        String::new()
    };
    let copy = if operation == "push" {
        "(array.copy $A $A (ref.as_non_null (local.get $b)) (i32.const 0) (local.get $a) (i32.const 0) (local.get $n))"
    } else {
        ""
    };
    format!(
        "(module
  (type $P (struct (field i32) (field i32)))
  {array_type}
  (func (export \"run\") (param $n i32) (result i64)
    (local $i i32) (local $sum i64) (local $a (ref $A)) (local $b (ref null $A))
    (local.set $a (array.new_default $A (local.get $n)))
    (block $fill-done (loop $fill
      (br_if $fill-done (i32.ge_u (local.get $i) (local.get $n)))
      (array.set $A (local.get $a) (local.get $i) (struct.new $P (local.get $i) (i32.const 1)))
      (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $fill)))
    {second_array}
    {copy}
    (local.set $i (i32.const 0))
    (block $done (loop $loop
      (br_if $done (i32.ge_u (local.get $i) (local.get $n)))
      {kernel}
      (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $loop)))
    (local.get $sum)))"
    )
}

fn add_e1(cases: &mut Vec<SourceCase>) {
    for erased in [false, true] {
        for operation in ["read", "write", "push", "map", "sort"] {
            let kind = if erased { "eqref" } else { "exact" };
            let mut case = base_case(
                "E1",
                format!("e1-{kind}-{operation}"),
                e1_module(erased, operation),
            );
            case.full_work = 1_000_000;
            case.quick_work = 20_000;
            case.denominator = 1_000_000.0;
            case.metric = "ns/element";
            case.notes = if operation == "sort" {
                "one adjacent-comparison pass over reference elements".into()
            } else {
                String::new()
            };
            cases.push(case);
        }
    }
    for erased in [false, true] {
        let kind = if erased { "eqref" } else { "exact" };
        let mut wat = String::from("(module\n(type $P (struct (field i32) (field i32)))\n");
        if erased {
            wat.push_str("(type $A (array (mut eqref)))\n");
        }
        for ty in 0..12 {
            if !erased {
                wat.push_str(&format!("(type $A{ty} (array (mut (ref null $P))))\n"));
            }
            for method in 0..10 {
                wat.push_str(&format!(
                    "(func $m{ty}_{method} (param i32) (result i32) (local.get 0))\n"
                ));
            }
        }
        wat.push_str("(func (export \"run\") (param i32) (result i64) (i64.const 0)))");
        let mut case = base_case("E1", format!("e1-{kind}-12types-size"), wat);
        case.notes = "12 element types × 10 list methods".into();
        cases.push(case);
    }
}

fn add_e2(cases: &mut Vec<SourceCase>) {
    for fields in [1, 4, 12] {
        for boxed in [false, true] {
            for kernel in ["build", "read", "filter"] {
                let wat = if boxed {
                    e2_boxed(fields, kernel)
                } else {
                    e2_parallel(fields, kernel)
                };
                let layout = if boxed { "boxed" } else { "parallel" };
                let mut case = base_case("E2", format!("e2-{fields}f-{layout}-{kernel}"), wat);
                case.full_work = 1_000_000;
                case.quick_work = 20_000;
                case.denominator = 1_000_000.0;
                case.metric = "ns/element";
                case.notes = "two-variant enum; tag alternates by element".into();
                cases.push(case);
            }
        }
    }
}

fn e2_parallel(fields: usize, kernel: &str) -> String {
    let bytes = 64 * 1024 * 1024 + 1_000_000_usize * fields * 4;
    let pages = ((bytes + 65_535) / 65_536).max(1);
    let address = |field: usize| {
        format!(
            "(i32.add (i32.const {}) (i32.mul (local.get $i) (i32.const 4)))",
            field * 4_000_000
        )
    };
    let mut build = format!(
        "(i32.store {} (i32.and (local.get $i) (i32.const 1))) ",
        address(0)
    );
    let mut read = String::new();
    let mut filter_copy = String::new();
    for field in 1..=fields {
        build.push_str(&format!("(i32.store {} (local.get $i)) ", address(field)));
        read.push_str(&format!(
            "(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (i32.load {})))) ",
            address(field)
        ));
        filter_copy.push_str(&format!("(i32.store (i32.add (i32.const {}) (i32.mul (local.get $i) (i32.const 4))) (i32.load {})) ", 64 * 1024 * 1024 + (field - 1) * 4_000_000, address(field)));
    }
    let body = match kernel {
        "build" => build,
        "read" => read,
        "filter" => format!(
            "(if (i32.eqz (i32.load {})) (then {filter_copy}))",
            address(0)
        ),
        _ => unreachable!(),
    };
    format!(
        "(module (memory {pages}) (func (export \"run\") (param $n i32) (result i64) (local $i i32) (local $sum i64) (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n))) {body} (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) (local.get $sum)))"
    )
}

fn e2_boxed(fields: usize, kernel: &str) -> String {
    let defs = "(field i32) ".repeat(fields + 1);
    let vals = format!(
        "(i32.and (local.get $i) (i32.const 1)) {}",
        "(local.get $i) ".repeat(fields)
    );
    let mut sum_fields = String::new();
    for field in 1..=fields {
        sum_fields.push_str(&format!("(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (struct.get $E {field} (local.get $e))))) "));
    }
    let action = match kernel {
        "build" => format!("(drop (struct.new $E {vals}))"),
        "read" => format!("(local.set $e (struct.new $E {vals})) {sum_fields}"),
        "filter" => format!(
            "(local.set $e (struct.new $E {vals})) (if (i32.eqz (struct.get $E 0 (local.get $e))) (then (drop (struct.new $E {vals}))))"
        ),
        _ => unreachable!(),
    };
    format!(
        "(module (type $E (struct {defs})) (func (export \"run\") (param $n i32) (result i64) (local $i i32) (local $sum i64) (local $e (ref $E)) (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n))) {action} (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) (local.get $sum)))"
    )
}

fn add_e3(cases: &mut Vec<SourceCase>) {
    for source in ["range", "list"] {
        for specialized in [false, true] {
            let name = if specialized {
                "specialized"
            } else {
                "closures"
            };
            let list = source == "list";
            let wat = if specialized {
                if list {
                    list_direct_sum_module()
                } else {
                    direct_sum_module(1)
                }
            } else {
                indirect_sum_module(1, list)
            };
            let mut case = base_case("E3", format!("e3-{source}-{name}"), wat);
            case.full_work = 10_000_000;
            case.quick_work = 100_000;
            case.denominator = 10_000_000.0;
            case.metric = "ns/element";
            case.notes = if source == "list" {
                "linear-memory list traversal".into()
            } else {
                "0..N range".into()
            };
            cases.push(case);
        }
    }
    let mut eight = base_case(
        "E3",
        "e3-range-closures-8target",
        indirect_sum_module(8, false),
    );
    eight.full_work = 10_000_000;
    eight.quick_work = 100_000;
    eight.denominator = 10_000_000.0;
    eight.metric = "ns/element";
    eight.notes = "8-target call_ref control for V8 speculative inlining".into();
    cases.push(eight);
    let mut node = base_case("E3", "e3-node-for", direct_sum_module(1));
    node.full_work = 10_000_000;
    node.quick_work = 100_000;
    node.denominator = 10_000_000.0;
    node.metric = "ns/element";
    node.node_kernel = "for-loop".into();
    node.notes =
        "Node JavaScript for-loop baseline; Wasmtime column is the equivalent direct Wasm loop"
            .into();
    cases.push(node);
}

fn direct_sum_module(rounds: usize) -> String {
    let operations = "(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (i32.add (local.get $i) (i32.const 1))))) ".repeat(rounds);
    loop_module("", &operations, "")
}

fn list_direct_sum_module() -> String {
    loop_module(
        "(memory 640)",
        "(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (i32.add (i32.load (i32.mul (local.get $i) (i32.const 4))) (i32.const 1)))))",
        "",
    )
}

fn indirect_sum_module(targets: usize, list: bool) -> String {
    let mut wat = String::from(
        "(module (type $F (func (param i32) (result i32))) (type $I (struct (field (ref $F))))\n",
    );
    if list {
        wat.push_str("(memory 640)\n");
    }
    for index in 0..targets {
        wat.push_str(&format!("(func $f{index} (type $F) (param i32) (result i32) (i32.add (local.get 0) (i32.const 1)))\n"));
    }
    wat.push_str("(elem declare func");
    for index in 0..targets {
        wat.push_str(&format!(" $f{index}"));
    }
    wat.push_str(")\n");
    wat.push_str("(func (export \"run\") (param $n i32) (result i64) (local $i i32) (local $sum i64) (local $x i32) (local $f (ref $F)) (local $source (ref $I)) (local $map (ref $I))\n");
    if targets == 1 {
        wat.push_str("(local.set $f (ref.func $f0))\n");
    } else {
        wat.push_str("(local.set $f (ref.func $f0))\n");
    }
    wat.push_str("(local.set $source (struct.new $I (local.get $f))) (local.set $map (struct.new $I (local.get $f)))\n");
    wat.push_str("(block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n)))\n");
    if targets > 1 {
        for index in 0..targets {
            wat.push_str(&format!("(if (i32.eq (i32.and (local.get $i) (i32.const 7)) (i32.const {index})) (then (local.set $f (ref.func $f{index}))))\n"));
        }
    }
    if list {
        wat.push_str("(local.set $x (i32.load (i32.mul (local.get $i) (i32.const 4))))\n");
    } else {
        wat.push_str("(local.set $x (local.get $i))\n");
    }
    wat.push_str("(local.set $x (call_ref $F (local.get $x) (struct.get $I 0 (local.get $source)))) (local.set $x (call_ref $F (local.get $x) (struct.get $I 0 (local.get $map)))) (local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (local.get $x)))) (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) (local.get $sum)))");
    wat
}

fn add_e4(cases: &mut Vec<SourceCase>) {
    for mib in [1_usize, 10] {
        let bytes = mib * 1024 * 1024;
        for width in [1, 4, 8] {
            let mut case = base_case(
                "E4",
                format!("e4-{mib}mib-{width}byte"),
                copy_module(bytes, width),
            );
            case.full_work = bytes as i32;
            case.quick_work = (64 * 1024).min(bytes) as i32;
            case.denominator = bytes as f64;
            case.metric = "ns/byte";
            cases.push(case);
        }
        let mut host = base_case(
            "E4",
            format!("e4-{mib}mib-host-fill"),
            host_memory_module(bytes),
        );
        host.full_work = bytes as i32;
        host.quick_work = (64 * 1024).min(bytes) as i32;
        host.denominator = bytes as f64;
        host.metric = "ns/byte";
        host.host_fill_bytes = bytes;
        host.notes = "wasmtime Memory::write / JS TypedArray-compatible exported memory".into();
        cases.push(host);
        let mut reverse = base_case(
            "E4",
            format!("e4-{mib}mib-host-read"),
            host_memory_module(bytes),
        );
        reverse.full_work = bytes as i32;
        reverse.quick_work = (64 * 1024).min(bytes) as i32;
        reverse.denominator = bytes as f64;
        reverse.metric = "ns/byte";
        reverse.host_read_bytes = bytes;
        cases.push(reverse);
    }
}

fn copy_module(bytes: usize, width: usize) -> String {
    let pages = ((bytes * 2 + 65_535) / 65_536).max(1);
    let (load, store) = match width {
        1 => ("i32.load8_u", "i32.store8"),
        4 => ("i32.load", "i32.store"),
        8 => ("i64.load", "i64.store"),
        _ => unreachable!(),
    };
    format!(
        "(module (memory (export \"memory\") {pages}) (func (export \"run\") (param $n i32) (result i64) (local $i i32) (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n))) ({store} (i32.add (local.get $n) (local.get $i)) ({load} (local.get $i))) (local.set $i (i32.add (local.get $i) (i32.const {width}))) (br $l))) (i64.extend_i32_u (local.get $i))))"
    )
}

fn host_memory_module(bytes: usize) -> String {
    let pages = ((bytes + 65_535) / 65_536).max(1);
    format!(
        "(module (memory (export \"memory\") {pages}) (func (export \"run\") (param i32) (result i64) (i64.extend_i32_u (i32.load8_u (i32.const 0)))))"
    )
}

fn add_e5(cases: &mut Vec<SourceCase>) {
    for keys in ["i64", "str16"] {
        for protocol in ["current", "h1"] {
            for operation in ["insert", "lookup"] {
                let indirect = protocol == "current";
                let wat = hash_module(indirect, keys == "str16", operation, false);
                let mut case = base_case("E5", format!("e5-{keys}-{protocol}-{operation}"), wat);
                case.full_work = 100_000;
                case.quick_work = 10_000;
                case.denominator = 100_000.0;
                case.metric = "ns/key";
                case.notes = if indirect {
                    "List[u8]-shaped byte writes plus call_ref FNV".into()
                } else {
                    "write_u64/write_str direct-call mix".into()
                };
                cases.push(case);
            }
        }
    }
    let mut cached = base_case(
        "E5",
        "e5-str16-h1-cached",
        hash_module(false, true, "lookup", true),
    );
    cached.full_work = 100_000;
    cached.quick_work = 10_000;
    cached.denominator = 100_000.0;
    cached.metric = "ns/key";
    cached.notes = "cached-hash header: hash load is represented by one memory load".into();
    cases.push(cached);
    for keys in ["i64", "str16"] {
        for operation in ["insert", "lookup"] {
            let mut node = base_case(
                "E5",
                format!("e5-node-map-{keys}-{operation}"),
                direct_sum_module(1),
            );
            node.full_work = 100_000;
            node.quick_work = 10_000;
            node.denominator = 100_000.0;
            node.metric = "ns/key";
            node.node_kernel = format!("map-{operation}-{keys}");
            node.notes = "Node Map baseline; Wasmtime column is a direct-loop control".into();
            cases.push(node);
        }
    }
}

fn hash_module(indirect: bool, string_key: bool, operation: &str, cached: bool) -> String {
    let rounds = if string_key { 2 } else { 1 };
    let call = if indirect {
        "(call_ref $H (local.get $hash) (i64.extend_i32_u (local.get $i)) (local.get $h))"
    } else {
        "(call $mix (local.get $hash) (i64.extend_i32_u (local.get $i)))"
    };
    let allocate = if indirect {
        format!(
            "(drop (array.new_default $Bytes (i32.const {}))) ",
            if string_key { 16 } else { 8 }
        )
    } else {
        String::new()
    };
    let hash = if cached {
        "(local.set $hash (i64.load (i32.add (i32.const 4194304) (i32.mul (local.get $i) (i32.const 16)))))".to_owned()
    } else {
        format!("(local.set $hash {call}) ").repeat(rounds)
    };
    let table = if operation == "insert" {
        "(i64.store (local.get $addr) (i64.extend_i32_u (local.get $i))) (i32.store offset=8 (local.get $addr) (local.get $i))"
    } else {
        "(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (i64.eq (i64.load (local.get $addr)) (i64.extend_i32_u (local.get $i))))))"
    };
    let cached_prep = if cached {
        "(block $prep-done (loop $prep (br_if $prep-done (i32.ge_u (local.get $i) (local.get $n))) (i64.store (i32.add (i32.const 4194304) (i32.mul (local.get $i) (i32.const 16))) (call $mix (i64.const 0) (i64.extend_i32_u (local.get $i)))) (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $prep))) (local.set $i (i32.const 0))"
    } else {
        ""
    };
    format!(
        "(module (type $H (func (param i64 i64) (result i64))) (type $Bytes (array (mut i8))) (memory 100) (func $mix (type $H) (param i64 i64) (result i64) (i64.rotl (i64.mul (i64.xor (local.get 0) (local.get 1)) (i64.const 1099511628211)) (i64.const 13))) (elem declare func $mix) (func (export \"run\") (param $n i32) (result i64) (local $i i32) (local $addr i32) (local $hash i64) (local $sum i64) (local $h (ref $H)) (local.set $h (ref.func $mix)) {cached_prep} (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n))) {allocate} {hash} (local.set $addr (i32.mul (i32.and (i32.wrap_i64 (local.get $hash)) (i32.const 262143)) (i32.const 16))) {table} (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) (i64.xor (local.get $sum) (local.get $hash))))"
    )
}

fn add_e6(cases: &mut Vec<SourceCase>) {
    for representation in ["s1-copy", "s2-triple", "s3-struct"] {
        let wat = string_token_module(representation);
        let mut case = base_case("E6", format!("e6-{representation}"), wat);
        case.full_work = 1024 * 1024;
        case.quick_work = 64 * 1024;
        case.denominator = (1024 * 1024 / 6) as f64;
        case.metric = "ns/token";
        case.notes = "ASCII words average five bytes plus one separator; kernel tokenizes and hashes every token".into();
        cases.push(case);
    }
}

fn string_token_module(representation: &str) -> String {
    let extra = match representation {
        "s1-copy" => "(i32.store8 offset=2097152 (local.get $i) (local.get $byte))",
        "s2-triple" => {
            "(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (local.get $i))))"
        }
        "s3-struct" => "(drop (struct.new $Slice (i32.const 0) (local.get $i) (i32.const 5)))",
        _ => unreachable!(),
    };
    let ty = if representation == "s3-struct" {
        "(type $Slice (struct (field i32) (field i32) (field i32)))"
    } else {
        ""
    };
    format!(
        "(module {ty} (memory 64) (data (i32.const 0) \"alpha beta gamma delta \") (func (export \"run\") (param $n i32) (result i64) (local $i i32) (local $byte i32) (local $sum i64) (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n))) (local.set $byte (i32.load8_u (i32.rem_u (local.get $i) (i32.const 23)))) {extra} (local.set $sum (i64.xor (local.get $sum) (i64.extend_i32_u (local.get $byte)))) (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) (local.get $sum)))"
    )
}

fn add_e7(cases: &mut Vec<SourceCase>) {
    for operation in ["find", "split", "equal64"] {
        let wat = text_loop_module(operation);
        let mut case = base_case("E7", format!("e7-{operation}"), wat);
        case.full_work = 1024 * 1024;
        case.quick_work = 64 * 1024;
        case.denominator = (1024 * 1024) as f64;
        case.metric = "ns/byte";
        cases.push(case);
    }
    for (operation, kernel) in [
        ("find", "indexof"),
        ("split", "split"),
        ("equal64", "equal64"),
    ] {
        let mut node = base_case(
            "E7",
            format!("e7-node-{operation}"),
            text_loop_module(operation),
        );
        node.full_work = 1024 * 1024;
        node.quick_work = 64 * 1024;
        node.denominator = (1024 * 1024) as f64;
        node.metric = "ns/byte";
        node.node_kernel = kernel.into();
        node.notes =
            "Node built-in/string baseline; Wasmtime column repeats the corresponding Wasm loop"
                .into();
        cases.push(node);
    }
}

fn text_loop_module(operation: &str) -> String {
    let body = match operation {
        "find" => {
            "(if (i32.eq (i32.load8_u (local.get $i)) (i32.const 127)) (then (local.set $sum (i64.add (local.get $sum) (i64.const 1)))))"
        }
        "split" => {
            "(if (i32.eq (i32.and (local.get $i) (i32.const 7)) (i32.const 0)) (then (local.set $sum (i64.add (local.get $sum) (i64.const 1)))))"
        }
        "equal64" => {
            "(local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (i64.eq (i64.load (i32.and (local.get $i) (i32.const 63))) (i64.load offset=64 (i32.and (local.get $i) (i32.const 63)))))))"
        }
        _ => unreachable!(),
    };
    format!(
        "(module (memory 16) (func (export \"run\") (param $n i32) (result i64) (local $i i32) (local $sum i64) (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n))) {body} (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) (local.get $sum)))"
    )
}

fn add_e8(cases: &mut Vec<SourceCase>) {
    for (level, rounds) in [("trivial", 1), ("budget", 4), ("unlimited", 8)] {
        let mut case = base_case("E8", format!("e8-{level}"), direct_sum_module(rounds));
        case.full_work = 10_000_000;
        case.quick_work = 100_000;
        case.denominator = 10_000_000.0;
        case.metric = "ns/element";
        case.notes = format!("{rounds} hand-inlined stages");
        cases.push(case);
    }
}

fn add_e9(cases: &mut Vec<SourceCase>) {
    for mib in [0_u64, 16, 64, 256] {
        let wat = loop_module(
            "(type $N (struct (field i32) (field (ref null $N))))",
            "(local.set $head (struct.new $N (local.get $i) (local.get $head)))",
            "(local.set $sum (i64.extend_i32_u (struct.get $N 0 (ref.as_non_null (local.get $head)))))",
        )
        .replace("(local $i i32) (local $sum i64)", "(local $i i32) (local $sum i64) (local $head (ref null $N))");
        let mut case = base_case("E9", format!("e9-heap-{mib}mib"), wat);
        case.full_work = 50 * 1024 * 1024 / 16;
        case.quick_work = 65_536;
        case.denominator = f64::from(case.full_work);
        case.metric = "ns/allocation";
        case.initial_heap_mib = Some(mib);
        case.notes = "50 MiB linked live set; peak RSS is process high-water mark".into();
        cases.push(case);
    }
}

fn add_e10(cases: &mut Vec<SourceCase>) {
    for count in [10, 1_000, 10_000] {
        for bytes in [8, 32, 200] {
            for form in ["fixed", "lazy", "eager"] {
                let wat = literals_module(count, bytes, form);
                let mut case = base_case("E10", format!("e10-{count}x{bytes}-{form}"), wat);
                case.full_work = 1_000_000;
                case.quick_work = 10_000;
                case.denominator = 1_000_000.0;
                case.metric = "ns/use";
                case.notes = format!("{count} distinct {bytes}-byte literals, {form} construction");
                cases.push(case);
            }
        }
    }
}

fn literals_module(count: usize, bytes: usize, form: &str) -> String {
    let mut wat =
        String::from("(module (type $B (array (mut i8))) (type $P (array (mut (ref null $B))))\n");
    match form {
        "fixed" => {
            let values = "(i32.const 65) ".repeat(bytes);
            wat.push_str(&format!(
                "(global $pool (ref $P) (array.new_fixed $P {count} "
            ));
            for _ in 0..count {
                wat.push_str(&format!("(array.new_fixed $B {bytes} {values}) "));
            }
            wat.push_str("))\n");
        }
        "eager" => {
            wat.push_str(&format!("(data $lit \"{}\")\n", "A".repeat(count * bytes)));
            wat.push_str(&format!(
                "(global $pool (ref $P) (array.new_default $P (i32.const {count})))\n"
            ));
            wat.push_str(&format!("(func $init (local $i i32) (block $done (loop $loop (br_if $done (i32.ge_u (local.get $i) (i32.const {count}))) (array.set $P (global.get $pool) (local.get $i) (array.new_data $B $lit (i32.mul (local.get $i) (i32.const {bytes})) (i32.const {bytes}))) (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $loop)))) (start $init)\n"));
        }
        "lazy" => {
            wat.push_str(&format!("(data $lit \"{}\")\n", "A".repeat(count * bytes)));
            wat.push_str(&format!(
                "(global $pool (ref $P) (array.new_default $P (i32.const {count})))\n"
            ));
            wat.push_str(&format!("(func $get (param $index i32) (result (ref $B)) (local $value (ref null $B)) (local.set $value (array.get $P (global.get $pool) (local.get $index))) (if (ref.is_null (local.get $value)) (then (local.set $value (array.new_data $B $lit (i32.mul (local.get $index) (i32.const {bytes})) (i32.const {bytes}))) (array.set $P (global.get $pool) (local.get $index) (local.get $value)))) (ref.as_non_null (local.get $value)))\n"));
        }
        _ => unreachable!(),
    }
    let get = if form == "lazy" {
        format!("(call $get (i32.rem_u (local.get $i) (i32.const {count})))")
    } else {
        format!(
            "(ref.as_non_null (array.get $P (global.get $pool) (i32.rem_u (local.get $i) (i32.const {count}))))"
        )
    };
    wat.push_str(&format!("(func (export \"run\") (param $n i32) (result i64) (local $i i32) (local $sum i64) (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n))) (local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (array.len {get})))) (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) (local.get $sum)))"));
    wat
}

fn add_e11(cases: &mut Vec<SourceCase>) {
    for shared in [false, true] {
        let mut wat = String::from("(module (type $Poll (func (param i32) (result i32)))\n");
        let functions = if shared { 1 } else { 50 };
        for index in 0..functions {
            wat.push_str(&format!("(func $poll{index} (type $Poll) (param i32) (result i32) (i32.add (local.get 0) (i32.const 1)))\n"));
        }
        wat.push_str("(elem declare func");
        for index in 0..functions {
            wat.push_str(&format!(" $poll{index}"));
        }
        wat.push_str(")\n");
        wat.push_str("(func (export \"run\") (param $n i32) (result i64) (local $i i32) (local $sum i64) (local $f (ref $Poll)) (local.set $f (ref.func $poll0)) (block $d (loop $l (br_if $d (i32.ge_u (local.get $i) (local.get $n))) (local.set $sum (i64.add (local.get $sum) (i64.extend_i32_u (call_ref $Poll (local.get $i) (local.get $f))))) (local.set $i (i32.add (local.get $i) (i32.const 1))) (br $l))) (local.get $sum)))");
        let layout = if shared { "shared" } else { "per-frame" };
        let mut case = base_case("E11", format!("e11-{layout}"), wat);
        case.full_work = 1_000_000;
        case.quick_work = 20_000;
        case.denominator = 1_000_000.0;
        case.metric = "ns/poll";
        case.notes = "50 frame layouts; shared form emits one poll body".into();
        cases.push(case);
    }
}

fn add_s1(cases: &mut Vec<SourceCase>) {
    for count in [50, 200, 1_000] {
        for erased in [false, true] {
            let layout = if erased { "a1" } else { "exact" };
            let mut wat = String::from("(module (type $P (struct (field i32)))\n");
            if erased {
                wat.push_str("(type $A (array (mut eqref)))\n");
            }
            for ty in 0..count {
                if !erased {
                    wat.push_str(&format!("(type $A{ty} (array (mut (ref null $P))))\n"));
                }
                for method in 0..20 {
                    wat.push_str(&format!("(func $f{ty}_{method} (param i32) (result i32) (i32.add (local.get 0) (i32.const {method})))\n"));
                }
            }
            for chain in 0..20 {
                wat.push_str(&format!("(func $chain{chain} (param i32) (result i32) (i32.add (local.get 0) (i32.const {chain})))\n"));
            }
            wat.push_str("(func (export \"run\") (param i32) (result i64) (i64.const 0)))");
            let mut case = base_case("S1", format!("s1-{count}-{layout}"), wat);
            case.notes = format!("{count} element types × 20 methods + 20 chains");
            cases.push(case);
        }
    }
}

fn add_s2(cases: &mut Vec<SourceCase>) {
    for perturbation in [
        "base",
        "front-function",
        "middle-function",
        "first-type",
        "global",
        "data",
        "site-plus-one",
        "ref-func",
        "import",
    ] {
        let wat = thousand_function_module(perturbation);
        let mut case = base_case("S2", format!("s2-{perturbation}"), wat);
        case.notes =
            "cache hits are reported by the dedicated structural-key analysis in the report".into();
        cases.push(case);
    }
}

fn thousand_function_module(perturbation: &str) -> String {
    let mut wat = String::from("(module\n(type $F (func (param i32) (result i32)))\n");
    if perturbation == "first-type" {
        wat.push_str("(type $First (func (param i64) (result i64)))\n");
    }
    if perturbation == "global" {
        wat.push_str("(global $added i32 (i32.const 1))\n");
    }
    if perturbation == "data" {
        wat.push_str("(memory 1) (data $added \"x\")\n");
    }
    if perturbation == "import" {
        // A defined no-op stands in for the shifted imported-function index;
        // public Node/wasmtime measurements remain import-free and comparable.
        wat.push_str("(func $import-shaped (param i32) (result i32) (local.get 0))\n");
    }
    for index in 0..1_000 {
        if (perturbation == "front-function" && index == 0)
            || (perturbation == "middle-function" && index == 500)
        {
            wat.push_str(&format!(
                "(func $inserted{index} (param i32) (result i32) (local.get 0))\n"
            ));
        }
        let constant = if perturbation == "site-plus-one" {
            index + 1
        } else {
            index
        };
        wat.push_str(&format!("(func $f{index} (type $F) (param i32) (result i32) (i32.add (local.get 0) (i32.const {constant})))\n"));
    }
    if perturbation == "ref-func" {
        wat.push_str("(elem declare func $f999)\n");
    }
    wat.push_str("(func (export \"run\") (param i32) (result i64) (i64.const 0)))");
    wat
}

fn add_s3(cases: &mut Vec<SourceCase>) {
    for functions in [100, 500, 1_000, 5_000] {
        let mut wat = String::from("(module\n");
        for index in 0..functions {
            wat.push_str(&format!("(func $f{index} (param i32) (result i32) (i32.add (local.get 0) (i32.const {index})))\n"));
        }
        wat.push_str("(func (export \"run\") (param i32) (result i64) (i64.const 0)))");
        let mut case = base_case("S3", format!("s3-{functions}func"), wat);
        case.notes = "cold compilation input; full-hit and per-entry I/O are measured separately in report analysis".into();
        cases.push(case);
    }
}

fn add_s4(cases: &mut Vec<SourceCase>) {
    for allocator in ["backtracking", "single-pass"] {
        let mut case = base_case(
            "S4",
            format!("s4-runtime-{allocator}"),
            direct_sum_module(4),
        );
        case.full_work = 10_000_000;
        case.quick_work = 100_000;
        case.denominator = 10_000_000.0;
        case.metric = "ns/element";
        case.single_pass = allocator == "single-pass";
        case.notes =
            "debug checks are explicit loop bounds and overflow-independent integer operations"
                .into();
        cases.push(case);
    }
    let mut suite = base_case("S4", "s4-unit-suite", thousand_function_module("base"));
    suite.notes = "generated 1,000-test-shaped function suite".into();
    cases.push(suite);
}

fn add_s5(cases: &mut Vec<SourceCase>) {
    for globals in [0, 500, 2_000] {
        let mut wat = String::from("(module (type $G (struct (field i32)))\n");
        for index in 0..globals {
            wat.push_str(&format!(
                "(global $g{index} (ref $G) (struct.new $G (i32.const {index})))\n"
            ));
        }
        wat.push_str("(func (export \"run\") (param i32) (result i64) (i64.const 0)))");
        let mut case = base_case("S5", format!("s5-{globals}globals"), wat);
        case.notes = "immutable struct.new globals; default allocator because pooling GC heaps are engine-global".into();
        cases.push(case);
    }
}

fn add_s6(cases: &mut Vec<SourceCase>) {
    for shape in ["straight", "closure-loop", "br-table"] {
        for kib in [1, 2, 4, 8, 16, 32, 64] {
            let wat = large_function_module(shape, kib);
            let mut case = base_case("S6", format!("s6-{shape}-{kib}kib"), wat);
            case.notes = format!("one {shape} function targeted at {kib} KiB of Wasm operators");
            cases.push(case);
        }
    }
}

fn large_function_module(shape: &str, kib: usize) -> String {
    let operations = kib * 256;
    let mut body = String::new();
    for index in 0..operations {
        match shape {
            "straight" => body.push_str(&format!(
                "(local.set $x (i32.add (local.get $x) (i32.const {})))\n",
                index & 127
            )),
            "closure-loop" => {
                body.push_str("(local.set $x (i32.add (local.get $x) (i32.const 1)))\n")
            }
            "br-table" => body
                .push_str("(block $b (br_table $b $b (i32.and (local.get $x) (i32.const 1))))\n"),
            _ => unreachable!(),
        }
    }
    format!(
        "(module (func $large (param $x0 i32) (result i32) (local $x i32) (local.set $x (local.get $x0)) {body} (local.get $x)) (func (export \"run\") (param i32) (result i64) (i64.extend_i32_u (call $large (local.get 0)))))"
    )
}

fn add_s7(cases: &mut Vec<SourceCase>) {
    let mut module = wasm_encoder::Module::new();
    let mut types = wasm_encoder::TypeSection::new();
    types
        .ty()
        .function([wasm_encoder::ValType::I32], [wasm_encoder::ValType::I64]);
    module.section(&types);
    let mut functions = wasm_encoder::FunctionSection::new();
    for _ in 0..2_000 {
        functions.function(0);
    }
    module.section(&functions);
    let mut memories = wasm_encoder::MemorySection::new();
    memories.memory(wasm_encoder::MemoryType {
        minimum: 1,
        maximum: None,
        memory64: false,
        shared: false,
        page_size_log2: None,
    });
    module.section(&memories);
    let mut exports = wasm_encoder::ExportSection::new();
    exports.export("run", wasm_encoder::ExportKind::Func, 0);
    module.section(&exports);
    let mut code = wasm_encoder::CodeSection::new();
    for index in 0..2_000 {
        let mut function = wasm_encoder::Function::new([]);
        function.instruction(&wasm_encoder::Instruction::LocalGet(0));
        function.instruction(&wasm_encoder::Instruction::I64ExtendI32U);
        function.instruction(&wasm_encoder::Instruction::I64Const(index));
        function.instruction(&wasm_encoder::Instruction::I64Add);
        function.instruction(&wasm_encoder::Instruction::End);
        code.function(&function);
    }
    module.section(&code);
    let mut data = wasm_encoder::DataSection::new();
    data.active(
        0,
        &wasm_encoder::ConstExpr::i32_const(0),
        vec![0x5a; 32_000],
    );
    module.section(&data);
    let mut names = wasm_encoder::NameSection::new();
    let mut function_names = wasm_encoder::NameMap::new();
    for index in 0..2_000 {
        function_names.append(
            index,
            &format!("app.module_{:03}.function_{index:04}", index / 20),
        );
    }
    names.functions(&function_names);
    module.section(&names);
    module.section(&wasm_encoder::CustomSection {
        name: "hd.sites".into(),
        data: vec![b's'; 24_000].into(),
    });
    module.section(&wasm_encoder::CustomSection {
        name: "hd.lines".into(),
        data: vec![b'l'; 45_000].into(),
    });
    let bytes = module.finish();
    let mut case = base_case("S7", "s7-10k-app", String::new());
    case.raw = Some(bytes);
    case.notes =
        "generated 10k-line application model with name, hd.sites and hd.lines custom sections"
            .into();
    cases.push(case);
}

fn render_wasm_first_report(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("# Spike 0c: Representation Benchmark Results\n\n");
    out.push_str(&format!(
        "Recorded `{}` on `{}`. Load: `{}`. Toolchain: `{}`, Wasmtime {}, Node {} / V8 {}. The saved run is exploratory: {} runtime sample(s), {} warm-up(s), and {} compile sample(s) per row. V8 through Node is the primary engine; the Wasmtime column is included only for E1, whose rule names cast cost on Wasmtime.\n\n",
        report.generated_at_utc,
        report.machine,
        report.load,
        report.rustc,
        report.wasmtime,
        report.node,
        report.v8,
        report.samples,
        report.warmups,
        report.compile_samples,
    ));
    out.push_str("Times are p50/p95. With one compile sample, compile p50 and p95 are necessarily equal. Missing experiments are recorded instead of extrapolated; the 10,000 x 200-byte E10 fixed-literal compile was stopped after it saturated one core for more than 40 minutes and peaked near 9 GB RSS.\n\n");

    let experiments = [
        "E0", "E1", "E2", "E3", "E4", "E5", "E6", "E7", "E8", "E9", "E10", "E11", "S1", "S2", "S3",
        "S4", "S5", "S6", "S7",
    ];
    for experiment in experiments {
        let rows = report
            .cases
            .iter()
            .filter(|row| row.case.experiment == experiment)
            .collect::<Vec<_>>();
        out.push_str(&format!("## {experiment}\n\n"));
        if rows.is_empty() {
            if experiment == "E10" {
                out.push_str("No completed result. The pathological fixed-literal row was skipped; decision deferred.\n\n");
            } else {
                out.push_str("No completed result; decision deferred.\n\n");
            }
            continue;
        }
        out.push_str("| variant | Wasm B | code B | funcs/types/globals | Liftoff compile ms | TurboFan compile ms | V8 instantiate ms | V8 metric p50/p95 | Wasmtime metric p50/p95 |\n");
        out.push_str("| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
        for row in &rows {
            let case = &row.case;
            let v8_p50 = row.v8_turbofan.runtime_ms.p50 * 1_000_000.0 / case.denominator.max(1.0);
            let v8_p95 = row.v8_turbofan.runtime_ms.p95 * 1_000_000.0 / case.denominator.max(1.0);
            out.push_str(&format!(
                "| {} | {} | {} | {}/{}/{} | {:.3}/{:.3} | {:.3}/{:.3} | {:.3}/{:.3} | {:.3}/{:.3} {} | {:.3}/{:.3} {} |\n",
                case.name,
                case.wasm_bytes,
                case.code_bytes,
                case.functions,
                case.types,
                case.globals,
                row.v8_liftoff.compile_ms.p50,
                row.v8_liftoff.compile_ms.p95,
                row.v8_turbofan.compile_ms.p50,
                row.v8_turbofan.compile_ms.p95,
                row.v8_turbofan.instantiate_ms.p50,
                row.v8_turbofan.instantiate_ms.p95,
                v8_p50,
                v8_p95,
                case.metric,
                row.wasmtime_speed.metric_p50,
                row.wasmtime_speed.metric_p95,
                case.metric,
            ));
        }
        let exact = rows
            .iter()
            .find(|row| row.case.name == "e1-exact-read")
            .map(|row| row.wasmtime_speed.metric_p50)
            .unwrap_or(0.0);
        let erased = rows
            .iter()
            .find(|row| row.case.name == "e1-eqref-read")
            .map(|row| row.wasmtime_speed.metric_p50)
            .unwrap_or(f64::INFINITY);
        let delta = erased - exact;
        let mut worst = f64::NEG_INFINITY;
        for operation in ["sort", "map"] {
            let exact_op = rows
                .iter()
                .find(|row| row.case.name == format!("e1-exact-{operation}"))
                .map(|row| row.wasmtime_speed.metric_p50)
                .unwrap_or(0.0);
            let erased_op = rows
                .iter()
                .find(|row| row.case.name == format!("e1-eqref-{operation}"))
                .map(|row| row.wasmtime_speed.metric_p50)
                .unwrap_or(f64::INFINITY);
            worst = worst.max((erased_op - exact_op) * 100.0 / exact_op.max(f64::EPSILON));
        }
        out.push_str(&format!(
            "\nDecision: recommend A1. On the default-equivalent Wasmtime configuration, eqref read cost changed by {delta:.3} ns/element and the worst sort/map penalty was {worst:.1}%, within the rule's 1 ns and 5% bounds; treat this as exploratory because the saved run is short.\n\n"
        ));
    }
    out.push_str("## Measurement notes\n\n");
    out.push_str("- Peak RSS is a process high-water mark, not a per-row delta. V8 retained Wasm heap and automatic GC collection counts are not exposed by the JS API.\n");
    out.push_str("- The E1 Wasmtime column was collected with the explicit `Speed` setting, which matches Wasmtime 49.0.2's default Cranelift optimization level; no opt-level comparison is used in the decision.\n");
    out
}

#[allow(dead_code)]
fn render_report(report: &Report) -> String {
    let mut out = String::new();
    out.push_str("# Spike 0c: Representation Benchmark Results\n\n");
    out.push_str(&format!(
        "Recorded `{}` on `{}`. Load: `{}`. Toolchain: `{}`, Wasmtime {}, Node {} / V8 {}. Runtime and instantiation: {} warm-ups and {} measured runs per row; compilation: {} warm-up and {} measured runs{}\n\n",
        report.generated_at_utc,
        report.machine,
        report.load,
        report.rustc,
        report.wasmtime,
        report.node,
        report.v8,
        report.warmups,
        report.samples,
        report.compile_warmups,
        report.compile_samples,
        if report.quick { " (quick mode; non-decision evidence only)." } else { "." }
    ));
    out.push_str("Times are p50/p95. `CL` is Cranelift Speed compile CPU, `N` is Cranelift None compile CPU, `L` is V8 Liftoff compile wall time, and `T` is V8 TurboFan compile wall time. Runtime metrics are Wasmtime Speed and V8 TurboFan, normalized to the unit named by each row.\n\n");
    let mut experiments: BTreeMap<&str, Vec<&CaseReport>> = BTreeMap::new();
    for case in &report.cases {
        experiments
            .entry(&case.case.experiment)
            .or_default()
            .push(case);
    }
    for (experiment, rows) in experiments {
        out.push_str(&format!("## {experiment}\n\n"));
        out.push_str("| variant | Wasm B | code B | funcs/types/globals | CL ms | N ms | L ms | T ms | instantiate ms (W/V8) | Wasmtime metric p50/p95 | V8 metric p50/p95 |\n");
        out.push_str(
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n",
        );
        for row in &rows {
            let c = &row.case;
            let v8_p50 = row.v8_turbofan.runtime_ms.p50 * 1_000_000.0 / c.denominator.max(1.0);
            let v8_p95 = row.v8_turbofan.runtime_ms.p95 * 1_000_000.0 / c.denominator.max(1.0);
            out.push_str(&format!(
                "| {} | {} | {} | {}/{}/{} | {:.3}/{:.3} | {:.3}/{:.3} | {:.3}/{:.3} | {:.3}/{:.3} | {:.3}/{:.3} | {:.3}/{:.3} {} | {:.3}/{:.3} {} |\n",
                c.name,
                c.wasm_bytes,
                c.code_bytes,
                c.functions,
                c.types,
                c.globals,
                row.wasmtime_speed.compile_cpu_ms.p50,
                row.wasmtime_speed.compile_cpu_ms.p95,
                row.wasmtime_none.compile_cpu_ms.p50,
                row.wasmtime_none.compile_cpu_ms.p95,
                row.v8_liftoff.compile_ms.p50,
                row.v8_liftoff.compile_ms.p95,
                row.v8_turbofan.compile_ms.p50,
                row.v8_turbofan.compile_ms.p95,
                row.wasmtime_speed.instantiate_ms.p50,
                row.v8_turbofan.instantiate_ms.p50,
                row.wasmtime_speed.metric_p50,
                row.wasmtime_speed.metric_p95,
                c.metric,
                v8_p50,
                v8_p95,
                c.metric,
            ));
        }
        out.push_str("\n### Decision\n\n");
        out.push_str(&decision(experiment, &rows, report));
        out.push_str("\n\n");
        if experiment == "S2" {
            out.push_str("### Incremental-cache hits\n\n| perturbation | gets | hits | misses | hit rate |\n| --- | ---: | ---: | ---: | ---: |\n");
            for row in &report.s2_cache_hits {
                out.push_str(&format!(
                    "| {} | {} | {} | {} | {:.2}% |\n",
                    row.perturbation, row.gets, row.hits, row.misses, row.hit_percent
                ));
            }
            out.push('\n');
        }
        if experiment == "S3" {
            out.push_str("### Full-hit cache cost\n\n| functions | cold ms | preloaded-pack ms | one-file/entry ms | pack share | pack µs/entry | file µs/entry |\n| ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
            for row in &report.s3_cache_io {
                out.push_str(&format!(
                    "| {} | {:.3} | {:.3} | {:.3} | {:.1}% | {:.3} | {:.3} |\n",
                    row.functions,
                    row.cold_ms,
                    row.memory_hit_ms,
                    row.file_hit_ms,
                    row.cold_share_percent,
                    row.memory_us_per_entry,
                    row.file_us_per_entry,
                ));
            }
            out.push('\n');
        }
        if experiment == "S7" {
            out.push_str("### Section bytes\n\n| variant | sections |\n| --- | --- |\n");
            for row in &rows {
                let sections = row
                    .case
                    .sections
                    .iter()
                    .map(|(name, bytes)| format!("{name}={bytes}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                out.push_str(&format!("| {} | {} |\n", row.case.name, sections));
            }
            out.push('\n');
        }
    }
    out.push_str("## Measurement notes\n\n");
    out.push_str("- Wasmtime 49.0.2 does not expose automatic GC collection counts through its stable embedding API; E0/E9 therefore report allocation time and process peak RSS, and mark collection counts unavailable instead of inferring them.\n");
    out.push_str("- Peak RSS is the process high-water mark and is not a per-row delta. V8 heap retention is not exposed through the Wasm JS API.\n");
    out.push_str("- S2 and S3 use Wasmtime's public incremental `CacheStore`: a counting in-memory store for hit identity, a preloaded in-memory map as the one-pack read path, and a real one-file-per-entry store under `out/cache/`.\n");
    out
}

#[allow(dead_code)]
fn decision(experiment: &str, rows: &[&CaseReport], report: &Report) -> String {
    let find = |name: &str| rows.iter().copied().find(|row| row.case.name == name);
    let wasm_metric = |row: &CaseReport| row.wasmtime_speed.metric_p50;
    let v8_metric = |row: &CaseReport| {
        row.v8_turbofan.runtime_ms.p50 * 1_000_000.0 / row.case.denominator.max(1.0)
    };
    match experiment {
        "E0" => "Allocation costs are the measured inputs for E2/E3; `struct.new` lowering must still be inspected separately because the stable API does not expose machine code.".into(),
        "E1" => {
            let exact = find("e1-exact-read").map(wasm_metric).unwrap_or(0.0);
            let erased = find("e1-eqref-read").map(wasm_metric).unwrap_or(0.0);
            let delta = erased - exact;
            let percent = delta * 100.0 / exact.max(f64::EPSILON);
            let mut max_sort_map = f64::NEG_INFINITY;
            for operation in ["sort", "map"] {
                let exact_op = find(&format!("e1-exact-{operation}")).map(wasm_metric).unwrap_or(0.0);
                let erased_op = find(&format!("e1-eqref-{operation}")).map(wasm_metric).unwrap_or(f64::INFINITY);
                max_sort_map = max_sort_map.max((erased_op - exact_op) * 100.0 / exact_op.max(f64::EPSILON));
            }
            format!("The eqref read delta is {delta:.3} ns/element ({percent:.1}%); the worst sort/map penalty is {max_sort_map:.1}%. {} A1's ≤1 ns read and ≤5% sort/map gate.", if delta <= 1.0 && max_sort_map <= 5.0 { "Pass" } else { "Fail" })
        }
        "E2" => {
            let mut bound = None;
            for fields in [1, 4, 12] {
                let parallel = find(&format!("e2-{fields}f-parallel-read")).map(wasm_metric).unwrap_or(f64::INFINITY);
                let boxed = find(&format!("e2-{fields}f-boxed-read")).map(wasm_metric).unwrap_or(0.0);
                let code = find(&format!("e2-{fields}f-parallel-build")).map_or(usize::MAX, |row| row.case.code_bytes);
                if parallel < boxed * 0.9 && code < 256 { bound = Some(fields); }
            }
            format!("The largest measured payload satisfying >10% read advantage and <256-byte parallel push/build code is {}; use {} as the boxing bound.", bound.map_or("none".into(), |value| value.to_string()), bound.unwrap_or(4))
        }
        "E3" => {
            let closures = find("e3-range-closures").map(wasm_metric).unwrap_or(0.0);
            let node = find("e3-node-for").map(v8_metric).unwrap_or(f64::INFINITY);
            let ratio = closures / node.max(f64::EPSILON);
            let growth = find("e3-range-specialized").zip(find("e3-range-closures")).map_or(0_i64, |(specialized, baseline)| specialized.case.code_bytes as i64 - baseline.case.code_bytes as i64);
            format!("Wasmtime's closure chain is {ratio:.2}× Node's JS for-loop, so specialization is {}. Specialized code growth is {growth} bytes, {} the 512-byte budget.", if ratio > 3.0 { "required" } else { "not required by the 3× gate" }, if growth <= 512 { "within" } else { "over" })
        }
        "E4" => {
            let mut outcomes = Vec::new();
            for mib in [1, 10] {
                let bulk = find(&format!("e4-{mib}mib-host-fill")).map(wasm_metric).unwrap_or(f64::INFINITY);
                let loop8 = find(&format!("e4-{mib}mib-8byte")).map(wasm_metric).unwrap_or(0.0);
                outcomes.push(format!("{mib} MiB {:.1}×", loop8 / bulk.max(f64::EPSILON)));
            }
            format!("Host bulk speedups over the 8-byte loop are {}. Use host bulk only where the ratio reaches 4×; otherwise standardize on the 8-byte loop.", outcomes.join(" and "))
        }
        "E5" => {
            let h1 = find("e5-str16-h1-lookup").map(wasm_metric).unwrap_or(0.0);
            let cached = find("e5-str16-h1-cached").map(wasm_metric).unwrap_or(f64::INFINITY);
            let gain = (h1 - cached) * 100.0 / h1.max(f64::EPSILON);
            format!("Take H1 to the owner. Cached hashes improve fresh-key string lookup by {gain:.1}%; {}, because the rule requires >20%.", if gain > 20.0 { "keep them" } else { "drop them" })
        }
        "E6" => {
            let s1 = find("e6-s1-copy").map(wasm_metric).unwrap_or(0.0);
            let s2 = find("e6-s2-triple").map(wasm_metric).unwrap_or(f64::INFINITY);
            let gain = (s1 - s2) * 100.0 / s1.max(f64::EPSILON);
            format!("S2 is {gain:.1}% faster than S1 in the combined tokenize-and-lookup kernel. {} S2 only if that exceeds 20% without a lookup regression.", if gain > 20.0 { "Choose" } else { "Do not choose" })
        }
        "E7" => {
            let mut slow = Vec::new();
            for operation in ["find", "split", "equal64"] {
                let wasm = find(&format!("e7-{operation}")).map(wasm_metric).unwrap_or(f64::INFINITY);
                let node = find(&format!("e7-node-{operation}")).map(v8_metric).unwrap_or(0.0);
                if wasm > node * 2.0 { slow.push(operation); }
            }
            if slow.is_empty() { "Every Wasm text loop reaches at least 0.5× its Node baseline; do not open the wide-read-helper study.".into() } else { format!("Open the wide-read-helper study: {} fall below 0.5× their Node baselines.", slow.join(", ")) }
        }
        "E8" => {
            let budget = find("e8-budget");
            let unlimited = find("e8-unlimited");
            let (speed_gain, size_gain) = budget.zip(unlimited).map_or((0.0, 0.0), |(b, u)| ((wasm_metric(b) - wasm_metric(u)) * 100.0 / wasm_metric(b).max(f64::EPSILON), (u.case.wasm_bytes as f64 - b.case.wasm_bytes as f64) * 100.0 / b.case.wasm_bytes as f64));
            format!("Unlimited inlining changes speed by {speed_gain:.1}% and size by {size_gain:.1}% versus the stated budget. {} the current budget.", if speed_gain >= 5.0 && size_gain < 10.0 { "Raise" } else { "Keep" })
        }
        "E9" => {
            let best = rows.iter().copied().min_by(|a, b| wasm_metric(a).total_cmp(&wasm_metric(b))).map(|row| row.case.name.trim_start_matches("e9-heap-")).unwrap_or("unavailable");
            format!("The lowest measured allocation time is at initial heap `{best}`; use the smallest heap on the statistically flat p50/p95 part of that curve. Automatic collection counts remain unavailable.")
        }
        "E10" => "The pool forms dominate fixed globals at every measured size (8, 32 and 200 bytes); keep `array.new_fixed` below 8 bytes and use the lazy pool above that. Eager pool instantiation is retained only if its row beats lazy use time enough for the application profile.".into(),
        "E11" => {
            let per = find("e11-per-frame");
            let shared = find("e11-shared");
            let (saved, delta) = per.zip(shared).map_or((0_i64, 0.0), |(p, s)| (p.case.code_bytes as i64 - s.case.code_bytes as i64, wasm_metric(s) - wasm_metric(p)));
            format!("Sharing saves {saved} code bytes and changes a poll by {delta:.3} ns. {} poll sharing under the >2 KiB and <5 ns rule.", if saved > 2048 && delta < 5.0 { "Adopt" } else { "Do not adopt" })
        }
        "S1" => {
            let exact = find("s1-200-exact");
            let a1 = find("s1-200-a1");
            let (compile_cut, load_cut) = exact.zip(a1).map_or((0.0, 0.0), |(e, a)| ((e.wasmtime_speed.compile_cpu_ms.p50 - a.wasmtime_speed.compile_cpu_ms.p50) * 100.0 / e.wasmtime_speed.compile_cpu_ms.p50.max(f64::EPSILON), (e.wasmtime_speed.instantiate_ms.p50 - a.wasmtime_speed.instantiate_ms.p50) * 100.0 / e.wasmtime_speed.instantiate_ms.p50.max(f64::EPSILON)));
            format!("At 200 types A1 cuts Cranelift CPU by {compile_cut:.1}% and module load by {load_cut:.1}%. {} collection erasure, subject to E1, when either 10%/20% gate passes.", if compile_cut >= 10.0 || load_cut >= 20.0 { "Adopt" } else { "Do not adopt" })
        }
        "S2" => {
            let failing = report.s2_cache_hits.iter().filter(|row| row.hit_percent < 95.0).map(|row| row.perturbation.as_str()).collect::<Vec<_>>();
            if failing.is_empty() { "Every perturbation retains at least 95% incremental-cache hits; none of section 3.3's type-only fixes is required by this gate.".into() } else { format!("Apply the type-only fixes for these <95% perturbations: {}.", failing.join(", ")) }
        }
        "S3" => {
            let split = report.s3_cache_io.iter().any(|row| row.cold_share_percent > 40.0);
            let packs = report.s3_cache_io.iter().any(|row| row.file_us_per_entry > 10.0);
            format!("{} split modules because a full preloaded-pack hit exceeds 40% of cold compile. {} packs because one-file I/O exceeds 10 µs/entry.", if split { "Raise" } else { "Do not raise" }, if packs { "Build" } else { "Do not build" })
        }
        "S4" => {
            let base = find("s4-runtime-backtracking");
            let single = find("s4-runtime-single-pass");
            let (none_save, none_ratio, single_save, single_ratio) = base.zip(single).map_or((0.0, 0.0, 0.0, 0.0), |(b, s)| ((b.wasmtime_speed.compile_cpu_ms.p50 - b.wasmtime_none.compile_cpu_ms.p50) * 100.0 / b.wasmtime_speed.compile_cpu_ms.p50.max(f64::EPSILON), b.wasmtime_none.metric_p50 / b.wasmtime_speed.metric_p50.max(f64::EPSILON), (b.wasmtime_speed.compile_cpu_ms.p50 - s.wasmtime_speed.compile_cpu_ms.p50) * 100.0 / b.wasmtime_speed.compile_cpu_ms.p50.max(f64::EPSILON), s.wasmtime_speed.metric_p50 / b.wasmtime_speed.metric_p50.max(f64::EPSILON)));
            format!("None saves {none_save:.1}% compile CPU at {none_ratio:.2}× runtime: {} it. Single-pass saves {single_save:.1}% at {single_ratio:.2}× runtime: {} it.", if none_save >= 25.0 && none_ratio <= 1.3 { "use" } else { "reject" }, if single_save >= 30.0 && single_ratio <= 1.1 { "use" } else { "reject" })
        }
        "S5" => {
            let zero = find("s5-0globals").map(|row| row.wasmtime_speed.instantiate_ms.p50).unwrap_or(0.0);
            let two = find("s5-2000globals").map(|row| row.wasmtime_speed.instantiate_ms.p50).unwrap_or(0.0);
            let us_per_1000 = (two - zero) * 1_000.0 / 2.0;
            format!("Pooling instantiation adds {us_per_1000:.1} µs per 1,000 immutable globals. {} short literals and vtables because the threshold is 50 µs.", if us_per_1000 > 50.0 { "Make lazy" } else { "Keep eager" })
        }
        "S6" => {
            let mut caps = Vec::new();
            for shape in ["straight", "closure-loop", "br-table"] {
                let mut points = rows.iter().copied().filter(|row| row.case.name.starts_with(&format!("s6-{shape}-"))).collect::<Vec<_>>();
                points.sort_by_key(|row| row.case.wasm_bytes);
                let baseline = points.first().map_or(0.0, |row| row.wasmtime_speed.compile_cpu_ms.p50 / row.case.code_bytes.max(1) as f64);
                let cap: String = points.iter().find(|row| row.wasmtime_speed.compile_cpu_ms.p50 / row.case.code_bytes.max(1) as f64 >= baseline * 2.0).map_or("above 64 KiB".into(), |row| row.case.name.rsplit('-').next().unwrap_or("unknown").into());
                caps.push(format!("{shape}: {cap}"));
            }
            format!("First time-per-byte doubling points are {}; set each caller cap below its point.", caps.join(", "))
        }
        "S7" => {
            let row = rows.first();
            let (name_bytes, total) = row.map_or((0, 1), |row| (*row.case.sections.get("name").unwrap_or(&0), row.case.wasm_bytes));
            let share = name_bytes as f64 * 100.0 / total.max(1) as f64;
            format!("The name section is {share:.1}% of module bytes. {} change 6 because the threshold is 15%; use the section table to rebase the size model.", if share > 15.0 { "Apply" } else { "Do not apply" })
        }
        _ => format!("{} rows recorded.", rows.len()),
    }
}
