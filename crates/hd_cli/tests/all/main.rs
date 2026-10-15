//! Every `hd` command test except the CLI conformance runner, linked once. They
//! spawn `hd` as a child process, so none shares process state with another.
//! `cli_conformance.rs` stays a separate target: it judges the whole CLI tier
//! against `KNOWN_FAILURES.tsv` and runs its own worker pool.

mod affected;
mod cache_gc;
mod check_cmd;
mod dbg;
mod deps;
mod doc_links;
mod doc_name;
mod doc_tests;
mod host_traits;
mod node_host_exit;
mod panic_frames;
mod property_tests;
mod resource_limits;
mod run;
mod same_size_edit;
mod spread_position;
mod test_bodies;
mod test_cmd;
mod test_env;
mod tests_block_items;
#[cfg(feature = "wasmtime")]
mod wasmtime_engine;
