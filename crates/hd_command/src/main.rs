//! The developer build of `hd`: [`hd_command::main`] with this crate's default
//! features, so without Wasmtime. The shipped one is `crates/hd-lang`.

fn main() -> std::process::ExitCode {
    hd_command::main()
}
