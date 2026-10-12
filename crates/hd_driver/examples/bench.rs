//! `cargo run --release -p hd_driver --example bench [N] [THREADS]`: the
//! synthetic two-folder package, cold, warm and edited runs, timed. Dev
//! tooling, not part of the `hd` command.

use std::time::Instant;

use hd_driver::{Clock, Executor};
use hd_sched::SerialOrder;

struct Wall(Instant);

impl Clock for Wall {
    fn now_ns(&self) -> u64 {
        u64::try_from(self.0.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let n = args.next().and_then(|n| n.parse().ok()).unwrap_or(200);
    let executor = match args.next().and_then(|t| t.parse().ok()) {
        Some(t) => Executor::Pool(t),
        None => Executor::Serial(SerialOrder::Priority),
    };
    match hd_driver::bench::bench(n, &Wall(Instant::now()), executor) {
        Ok((report, _)) => eprint!("{report}"),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
