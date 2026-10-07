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

fn compile(store: &MemoryStore, clock: &Wall, sources: &MemorySources) -> Output {
    let host = Host {
        sources,
        store,
        clock,
        executor: Executor::Serial(SerialOrder::Priority),
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
    let n = std::env::args()
        .nth(1)
        .and_then(|value| value.parse().ok())
        .unwrap_or(200);
    let (app, geo) = hd_driver::bench::sources(n);
    let store = MemoryStore::default();
    let clock = Wall(Instant::now());

    let start = Instant::now();
    let cold = compile(&store, &clock, &files(&app, &geo));
    print_report("cold seed", start.elapsed(), &cold.counters);
    if !cold.diags.is_empty() {
        eprintln!("cold diagnostics:\n{}", cold.render());
        std::process::exit(1);
    }

    let signature = geo.replacen(
        "pub fn g0(a: i32, b: i32) -> i32:",
        "pub fn g0(a: i32, b: i32, unused: i32) -> i32:",
        1,
    );
    let caller = app.replacen("g0(a, 0)", "g0(a, 0, 0)", 1);
    let start = Instant::now();
    let edited = compile(&store, &clock, &files(&caller, &signature));
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
