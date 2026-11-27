//! What `tests/unit` reaches inside the backend: the feature `testing`,
//! which only the crate's own dev-dependency turns on, so a normal build
//! neither compiles nor exports it. The unit tests live outside `src` so
//! that a test build compiles this library once, not again in test mode
//! (`[lib] test = false`).
//!
//! One module per source module the tests reach, at the same path,
//! re-exporting what they call there; wrappers of crate-root items, since
//! a `pub` item at the root would be public API; and the assembler
//! harness's runtime, which `Module::default` links.
//!
//! Cost: nothing in a normal build; with the feature, the work counters
//! the cost tests read (`cycle_graph`, `emit::borrow`, `module`,
//! `opt::live_set`), one add per unit of work they count.

use hd_base::Hash128;

use crate::module::{Module, Sym, Val};
use crate::rt::Helper;

/// Symbolic runtime identities for assembler-only tests. Program behaviour is
/// exercised through driver builds with the actual std runtime definitions.
#[must_use]
pub fn runtime() -> crate::Runtime {
    let keys: Vec<_> = crate::runtime_items(false, false)
        .iter()
        .enumerate()
        .map(|(i, _)| Some(Hash128((1u128 << 127) + i as u128)))
        .collect();
    // ruled out: the test supplies one key for every runtime_items entry.
    crate::emit::required(
        crate::Runtime::from_keys(&keys),
        "the assembler harness defines every runtime key",
    )
}

/// Assembler harness definitions, not a runtime implementation: these tests
/// validate linking and generated helper control flow, never ownership counts.
/// Keep their historical bump allocation while making every runtime symbol
/// explicit. Count/drop behaviour is covered by driver tests with std linked.
pub fn define_runtime(module: &mut Module) {
    let runtime = runtime();
    let keys = crate::runtime_items(false, false);
    for (i, (_, name)) in keys.into_iter().enumerate() {
        let (args, result) = match name {
            "collect" => (6, false),
            "due" | "marking" | "metadata" => (0, true),
            "alloc_class" | "lookup" | "type_of" => {
                (if name == "alloc_class" { 2 } else { 1 }, true)
            }
            "alloc"
            | "block_of"
            | "block_bytes"
            | "visit"
            | "release"
            | "release_known"
            | "release_acyclic"
            | "release_known_acyclic" => (1, true),
            "bind" | "forget" | "type_bind" => (2, false),
            _ => (1, false),
        };
        let mut f = module.func(
            format!("test-runtime:{name}"),
            vec![Val::I32; args],
            if result { vec![Val::I32] } else { vec![] },
        );
        if name == "alloc" {
            f.get(0);
            f.helper(Helper::Alloc);
        } else if result {
            f.i32(0);
        }
        module.define(Sym::Inst(Hash128((1u128 << 127) + i as u128)), f);
    }
    assert_eq!(module.runtime, runtime);
}

pub mod boundary {
    pub use crate::boundary::{
        CallRes, LCodec, LEnum, LList, LNamed, LTy, discard_strings, dynamic_method,
        package_module, record_package_interface, tags_only,
    };
}
pub mod bulk {
    pub use crate::bulk::{copy, fill, lower};
}
pub mod count_tests {
    pub use crate::count_tests::{Counts, lower, lower_with_work};
}
pub mod cycle_graph {
    pub use crate::cycle_graph::{Graph, Ranks, Shape};
}
pub mod emit {
    use hd_tir::ir::Body;

    type BodyTransform = fn(&str, &Body) -> Option<Body>;
    thread_local! {
        static BODY_TRANSFORM: std::cell::Cell<Option<BodyTransform>> = const { std::cell::Cell::new(None) };
    }

    /// Run a backend stage over hand-built TIR with real program metadata.
    /// Test-only: one TLS read per link when unarmed; transforms only
    /// selected bodies when armed. No shipped build contains the hook.
    pub fn with_body_transform<R>(transform: BodyTransform, run: impl FnOnce() -> R) -> R {
        struct Restore(Option<BodyTransform>);
        impl Drop for Restore {
            fn drop(&mut self) {
                BODY_TRANSFORM.set(self.0);
            }
        }
        let _restore = Restore(BODY_TRANSFORM.replace(Some(transform)));
        run()
    }

    pub(crate) fn transformed_bodies(
        input: &crate::Input<'_>,
    ) -> Option<hd_base::HashMap<hd_base::InstId, Body>> {
        let transform = BODY_TRANSFORM.get()?;
        Some(
            input
                .order
                .iter()
                .filter_map(|&id| {
                    let original = (input.body)(id)?;
                    transform(&(input.path)(original.item), original).map(|body| (id, body))
                })
                .collect(),
        )
    }
    pub use crate::emit::{Ctl, FrameRefs, OnceCell, int_range, operand_range};
    pub mod borrow {
        pub use crate::emit::borrow::{ANY_FRAME, Borrows, CaptureOrigins};
    }
    pub mod control {
        pub use crate::emit::control::Stack;
    }
    pub mod refs {
        pub use crate::emit::refs::{bare_local, project_reference};
    }
    pub mod shape {
        pub use crate::emit::shape::{local_store, ref_local};
    }
    pub mod susp {
        pub use crate::emit::susp::{FRAME, save_area};
    }
    pub mod susp_spill {
        pub use crate::emit::susp_spill::{Plan, resident};
    }
}
pub mod frame_referents {
    pub use crate::frame_referents::{Guard, Plan, Referent};
}
pub mod layout {
    pub use crate::layout::{Discr, Enc, Leaf, Mem, flat_of, variant_steps};
}
pub mod map {
    pub use crate::map::{Entry, KeyOps, SEED};
}
pub mod module {
    pub use crate::module::{
        Address, BUF, DescriptorKey, Func, Global, HEADER, HEAP, Import, Ins, Module, Numbers,
        Reached, STATIC_BASE, Sym, SymId, Symbols, Val, WasmVal, descriptor_slot, mem,
    };
}
pub mod op {
    pub use crate::op::{LabelListId, Op};
}
pub mod opt {
    pub use crate::opt::{
        Arity, Live, OPERAND_REACH, Rule, Settled, basic_blocks, blocks, blocks_with, coalesce,
        copy_sources, drops, exits, fold, fold_constant_branches, forward, forward_with, ifs,
        inline_counts, live_once, live_rules, locals, locals_with_work, schedule, schedule_with,
        shapes, static_counts, static_values, zero_edges, zero_releases,
    };
    pub mod bounds {
        pub use crate::opt::bounds::{rewrite, run};
    }
    pub mod branch_shapes {
        pub use crate::opt::branch_shapes::run;
    }
    pub mod constant {
        pub use crate::opt::constant::{compact_tees, evaluate, run, run_with};
    }
    pub mod empty_loops {
        pub use crate::opt::empty_loops::{rewrite, run};
    }
    pub mod live_set {
        pub use crate::opt::live_set::{Set, Sets, hash_probes};
    }
}
pub mod rt {
    pub use crate::rt::{
        Counted, Elem, Helper, HostCounts, LIST_ELEMS, LIST_LEN, PrimArg, RefLayout, RefRead,
        ShimSyms, arity, helper,
    };
    pub mod row {
        pub use crate::rt::row::{RowAdapter, RowEntry, build};
    }
}
pub mod static_data {
    pub use crate::static_data::{Chunk, active, bytes, mark, section};
}
pub mod task {
    pub use crate::task::{
        BORROWED_DROP, CANCELLED, DONE, F_CANCEL, F_POLL, F_SAVED, HostArg, HostLeaf, HostRes,
        LEAF_ARGS, W_READY, W_SLOT, all_field, all_for, flags_or, guard_frame, host_cold,
        host_drive, leaf_drop, new_zeroed, pending_cold,
    };
}
pub mod wide {
    pub use crate::wide::{WIDTH, callee, calls, storage};
}

/// [`crate::locate_layout_error`].
pub fn locate_layout_error(
    error: hd_base::NotImplemented,
    span: impl FnOnce() -> Option<hd_base::Span>,
) -> hd_base::NotImplemented {
    crate::locate_layout_error(error, span)
}

/// Round-trip every emitted-unit record and reject truncated forms.
pub use crate::module::wire::tests::round_trip_records as unit_wire_records;

pub use crate::units::place_root;
