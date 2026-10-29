#![forbid(unsafe_code)]
//! `hd_check`: body checking that emits TIR (`crates/hd_tir/DESIGN.md` §4.13)
//! over `hd_types` (`InternPool`, `InferTable`, the solver) and
//! `hd_tir::ir`, plus the checking stages beyond bodies (the header checks
//! that need goals, test overlay, init order). It reads syntax and
//! interfaces and writes TIR; nothing downstream reads syntax.

mod access;
#[cfg(feature = "analysis")]
pub mod analysis;
pub mod body;
mod call;
mod callable;
mod coll;
mod collide;
pub mod comparison;
pub mod conflicts;
mod conform;
mod dbg;
pub use dbg::DbgRef;
pub mod delegate;
mod dep_results;
pub mod derive;
mod dupfacts;
mod dynref;
mod elem_refs;
pub mod error;
mod expr;
mod facts;
pub mod fork;
pub mod newtype;
pub use dupfacts::{SlotTypes, check_duplicate_facts, declaration_types, layer_types};
pub use facts::{
    FactSource, FactTarget, check_decl_facts, check_enum_tagging_decls, check_fact_list,
    check_facts,
};

mod fnref;
pub mod header;
pub mod init;
pub mod inst_cycles;
mod join;
mod literals;
mod local;
mod local_refs;
mod opaque;
mod pat;
mod privacy;
mod promote;
mod receiver;
mod refs;
mod render;
pub mod results;
mod rows;
pub mod stages;
pub mod structure;
pub mod tests;
mod trial;
mod ty;
mod update;
mod using;
mod value;

pub use body::{
    BodyCx, TestsView, check_default, check_fn, check_fn_with_inferred, default_body_def,
};
pub use call::MethodIndex;
pub use conform::{AssocFaults, assoc_faults, omitted_trait_methods};
pub use local_refs::declarations as local_ref_declarations;
pub use render::{FieldNames, render};
