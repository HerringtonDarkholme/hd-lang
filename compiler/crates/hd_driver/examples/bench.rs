//! `cargo run --release -p hd_driver --example bench [N]`: the synthetic
//! two-folder package, cold, warm and edited runs, timed. Dev tooling, not
//! part of the `hd` command.

fn main() {
    let n = std::env::args().nth(1).and_then(|n| n.parse().ok()).unwrap_or(200);
    match hd_driver::bench::bench(n) {
        Ok((report, _)) => eprint!("{report}"),
        Err(error) => {
            eprintln!("error: {error}");
            std::process::exit(1);
        }
    }
}
