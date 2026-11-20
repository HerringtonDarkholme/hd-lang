//! The `hd` that `cargo install hd-lang` builds: [`hd_command::main`] with the
//! `dist` feature, which this crate's manifest turns on.

fn main() -> std::process::ExitCode {
    hd_command::main()
}
