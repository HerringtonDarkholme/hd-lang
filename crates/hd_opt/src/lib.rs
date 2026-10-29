//! `hd_opt`: the optimization pass manager. It holds the pass declarations,
//! the body-unit passes and their facts, the pipelines as data with their
//! knobs and the table that picks one per (engine target, level), the
//! startup validator that hashes a pipeline's content, and each pipeline's
//! planner, which
//! implements `hd_mono::plan::Planner`; and the optimizations that rewrite
//! TIR before the ownership passes (`inline`); and the whole-program
//! analyses whose result a backend consumes (`whole`). It depends on `hd_mono`, never the
//! reverse, and never on `hd_wasm`.

pub mod fact_specialize;
pub mod inline;
pub mod passes;
pub mod planner;
pub mod structure_specialize;
pub mod value_flow;
pub mod whole;
