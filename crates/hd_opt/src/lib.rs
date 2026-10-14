//! `hd_opt`: the optimization pass manager. It holds the pass declarations,
//! the body-unit passes and their facts, the pipelines as data, the startup
//! validator that hashes a pipeline, and each pipeline's planner, which
//! implements `hd_mono::plan::Planner`. It depends on `hd_mono`, never the
//! reverse, and never on `hd_wasm`.

pub mod passes;
pub mod planner;
pub mod value_flow;
