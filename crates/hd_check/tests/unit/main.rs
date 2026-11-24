//! `hd_check`'s unit tests: internal-error paths, metadata shapes and work
//! counts, over the `testing` facade (`src/testing.rs`). One module per
//! source file it tests.

mod body;
mod call;
mod collide;
mod conform;
mod dep_results;
mod derive;
mod error;
mod expr;
mod facts;
mod fnref;
mod frames;
mod join;
mod local;
mod local_refs;
mod newtype;
mod opaque;
mod pat;
mod regions;
mod results;
mod rows;
mod stages;
mod structure;
mod trial;
mod value;
mod work;
