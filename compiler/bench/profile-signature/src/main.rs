use std::time::Instant;

use hd_cache::MemoryStore;
use hd_driver::{Clock, Counters, Executor, Goal, Host, Output, build};
use hd_project::MemorySources;
use hd_sched::SerialOrder;

struct Wall(Instant);

impl Clock for Wall {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

fn files(app: &str, geo: &str) -> MemorySources {
    let mut s = MemorySources::default();
    s.insert("app/main.hd", app);
    s.insert("geo/shapes.hd", geo);
    s
}

fn print_report(label: &str, total: std::time::Duration, counters: &Counters) {
    let mut stages: Vec<_> = counters.stage_ns.iter().collect();
    stages.sort_by_key(|(_, ns)| std::cmp::Reverse(**ns));
    println!("== {label}: total {total:?}");
    println!("   stage ns {stages:?}");
    println!("   hits {:?} misses {:?}", counters.hits, counters.misses);
    println!(
        "   checked {:?}, interfaces {:?}, emitted {}",
        counters.modules_checked, counters.ifaces_built, counters.emitted
    );
}

fn compile(
    store: &MemoryStore,
    clock: &Wall,
    sources: &MemorySources,
    executor: Executor,
) -> Output {
    let host = Host {
        sources,
        store,
        clock,
        executor,
    };
    build(
        &host,
        "bench",
        &Goal::Program {
            entry: "app.main".into(),
        },
    )
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let n = args
        .first()
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let signature_only = args.iter().any(|a| a == "signature");
    let threads = args.get(1).and_then(|value| value.parse().ok());
    let executor = threads.map_or(Executor::Serial(SerialOrder::Priority), Executor::Pool);
    let (mut app, geo) = hd_driver::bench::sources(n);
    // `f32` and `f64` became invalid benchmark item names when M3 installed
    // the real prelude. Keep the input shape while avoiding those names.
    app = app.replace("f32(", "f_32(").replace("f64(", "f_64(");
    let store = MemoryStore::default();
    let clock = Wall(Instant::now());

    let start = Instant::now();
    let cold = compile(&store, &clock, &files(&app, &geo), executor);
    print_report("cold seed", start.elapsed(), &cold.counters);
    if !cold.diags.is_empty() {
        eprintln!("cold diagnostics:\n{}", cold.render());
        std::process::exit(1);
    }
    if let Some(wasm) = &cold.wasm {
        println!("   wasm {} bytes", wasm.len());
    }

    if !signature_only {
        let start = Instant::now();
        let warm = compile(&store, &clock, &files(&app, &geo), executor);
        print_report("warm, no edit", start.elapsed(), &warm.counters);

        let body = geo.replace("x = x * 3 + 1", "x = x * 3 + 2");
        let start = Instant::now();
        let edited = compile(&store, &clock, &files(&app, &body), executor);
        print_report(
            "private body edit in geo (all bodies)",
            start.elapsed(),
            &edited.counters,
        );

        let comment = geo.replacen("    return x\n", "    # note\n    return x\n", 1);
        let start = Instant::now();
        let edited = compile(&store, &clock, &files(&app, &comment), executor);
        print_report("comment edit in geo", start.elapsed(), &edited.counters);
        return;
    }

    let signature = geo.replacen(
        "pub fn g0(a: i32, b: i32) -> i32:",
        "pub fn g0(a: i32, b: i32, unused: i32) -> i32:",
        1,
    );
    let caller = app.replacen("g0(a, 0)", "g0(a, 0, 0)", 1);
    let start = Instant::now();
    let edited = compile(&store, &clock, &files(&caller, &signature), executor);
    print_report(
        "public signature and caller edit",
        start.elapsed(),
        &edited.counters,
    );
    if !edited.diags.is_empty() {
        eprintln!("signature diagnostics:\n{}", edited.render());
        std::process::exit(1);
    }
}
