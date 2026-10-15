//! `cargo run -p hd_driver --example rebuild [N] [optimized]`: the fan-out
//! package of `hd_driver::bench::fanout_sources`, cold, warm, and after a
//! leaf body edit, a widely imported body edit and a widely imported
//! signature edit, with the counters and wall time of each run. Dev
//! tooling, not part of the `hd` command.

use std::time::Instant;

use hd_driver::{Clock, Executor, OptLevel};
use hd_sched::SerialOrder;

struct Wall(Instant);

impl Clock for Wall {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n = args.next().and_then(|n| n.parse().ok()).unwrap_or(20);
    let level = match args.next().as_deref() {
        Some("optimized") => OptLevel::Optimized,
        _ => OptLevel::Dev,
    };
    let executor = Executor::Serial(SerialOrder::Priority);
    match hd_driver::bench::rebuild_scope(n, level, &Wall(Instant::now()), executor) {
        Ok(report) => eprint!("{report}"),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
