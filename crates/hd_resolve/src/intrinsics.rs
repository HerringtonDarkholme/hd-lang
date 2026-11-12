//! The intrinsic table (`trait.impl.intrinsic.shape`): every key a std
//! declaration may name in `@intrinsic("key")`, with the signature shapes
//! the compiler implements for it. Lowering checks each std declaration
//! against its row once, when it lowers the declaration's header; every
//! later stage, the backends included, reads an intrinsic call's arguments
//! by position under that guarantee.
//!
//! A shape fixes the parameter count (the receiver first), each
//! parameter's kind, the number of the function's own type parameters
//! (lifetimes aside) and the result's kind. A kind is coarse where the
//! lowering reads a value whole (a type parameter, a container of any
//! element) and exact where it reads the value's representation (a scalar
//! width, a string's two words).
//!
//! The compiler also gives four seeded std functions an intrinsic body
//! that no declaration names (`seed.rs`): `panic_message`, `box`,
//! `task_all` and `test_case`. Seeding builds their signatures, so they
//! have no row here.

use hd_types::{Prim, Ty, TyData};

use crate::iface::{FnSig, Names};

/// The kind of one parameter or of the result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    /// Any type: a type parameter, or a value the lowering moves whole.
    Any,
    /// Exactly this type: a primitive, `void` or `never`.
    Is(Ty),
    /// The named type (a struct, enum or newtype), with any arguments.
    Named(&'static str),
    /// A reference of this form: `&`, `&mut`, `*` or `*mut`.
    Ref { mutable: bool, counted: bool },
    /// `T?`, of any `T`.
    Opt,
    /// A tuple of this many elements.
    Tuple(u8),
    /// A function type.
    Fn,
}

/// One signature an intrinsic accepts.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// The function's own type parameters, lifetimes aside.
    pub generics: u8,
    /// The parameters in order, the receiver first.
    pub params: &'static [Kind],
    pub ret: Kind,
}

use Kind::{Any, Fn, Is, Named, Opt, Tuple};

const STR: Kind = Is(Ty::STRING);
const BOOL: Kind = Is(Ty::BOOL);
const VOID: Kind = Is(Ty::VOID);
const NEVER: Kind = Is(Ty::NEVER);
const CHAR: Kind = Is(Ty::prim(Prim::Char));
const U8: Kind = Is(Ty::prim(Prim::U8));
const U16: Kind = Is(Ty::prim(Prim::U16));
const U32: Kind = Is(Ty::prim(Prim::U32));
const U64: Kind = Is(Ty::prim(Prim::U64));
const I64: Kind = Is(Ty::prim(Prim::I64));
const USIZE: Kind = Is(Ty::prim(Prim::Usize));
const F32: Kind = Is(Ty::prim(Prim::F32));
const F64: Kind = Is(Ty::prim(Prim::F64));
const LIST: Kind = Named("List");
const SUSPEND: Kind = Named("Suspend");
const PTR: Kind = Named("Ptr");
const REF: Kind = Kind::Ref {
    mutable: false,
    counted: false,
};
const REF_MUT: Kind = Kind::Ref {
    mutable: true,
    counted: false,
};
const COUNTED_MUT: Kind = Kind::Ref {
    mutable: true,
    counted: true,
};

const fn s(generics: u8, params: &'static [Kind], ret: Kind) -> Shape {
    Shape {
        generics,
        params,
        ret,
    }
}

/// Every intrinsic key with its shapes, sorted by key for a binary search.
/// A static table: nothing builds it at run time.
pub static INTRINSICS: &[(&str, &[Shape])] = &[
    ("block_on", &[s(1, &[SUSPEND], Named("BlockOn"))]),
    ("bytes_at", &[s(0, &[STR, USIZE], U8)]),
    ("bytes_index", &[s(0, &[STR, USIZE, U8], USIZE)]),
    (
        "bytes_index_pair",
        &[s(0, &[STR, USIZE, U8, U8, USIZE], USIZE)],
    ),
    (
        "bytes_index_set",
        &[s(0, &[STR, USIZE, U64, U64, U64, U64], USIZE)],
    ),
    ("bytes_len", &[s(0, &[STR], USIZE)]),
    ("bytes_mismatch", &[s(0, &[STR, STR], USIZE)]),
    ("bytes_slice", &[s(0, &[STR, USIZE, USIZE], STR)]),
    ("caller_args", &[s(0, &[], LIST)]),
    ("caller_location", &[s(0, &[], Named("Location"))]),
    ("char_from_scalar", &[s(0, &[U32], Opt)]),
    ("char_scalar", &[s(0, &[CHAR], U32)]),
    ("dbg_printer", &[s(1, &[], Fn)]),
    ("dbg_write", &[s(0, &[STR], VOID)]),
    ("debug_texts", &[s(1, &[Any, LIST], LIST)]),
    ("descriptor_glue_mask", &[s(0, &[], USIZE)]),
    ("downcast_mut", &[s(1, &[REF_MUT], Opt)]),
    ("downcast_ref", &[s(1, &[REF], Opt)]),
    ("entry_write", &[s(0, &[STR], VOID)]),
    ("f32_from_bits", &[s(0, &[U32], F32)]),
    ("f32_to_bits", &[s(0, &[F32], U32)]),
    ("f64_from_bits", &[s(0, &[U64], F64)]),
    ("f64_to_bits", &[s(0, &[F64], U64)]),
    ("facts_of", &[s(1, &[Any], Named("Facts"))]),
    ("fn_call", &[s(0, &[REF, Any], Any)]),
    ("fn_mut_call", &[s(0, &[REF_MUT, Any], Any)]),
    ("format_f32", &[s(0, &[F32], STR)]),
    ("format_f64", &[s(0, &[F64], STR)]),
    ("format_f64_fixed", &[s(0, &[F64, USIZE], STR)]),
    (
        "format_int",
        &[
            s(0, &[I64, PTR], Tuple(2)),
            s(0, &[U64, PTR], Tuple(2)),
            s(1, &[Any, PTR], Tuple(2)),
        ],
    ),
    ("int_count_ones", &[s(0, &[Any], U32)]),
    ("int_leading_zeros", &[s(0, &[Any], U32)]),
    ("int_max", &[s(1, &[], Any)]),
    ("int_min", &[s(1, &[], Any)]),
    ("int_trailing_zeros", &[s(0, &[Any], U32)]),
    ("int_wrapping_add", &[s(0, &[Any, Any], Any)]),
    ("int_wrapping_mul", &[s(0, &[Any, Any], Any)]),
    ("int_wrapping_sub", &[s(0, &[Any, Any], Any)]),
    ("list_extend", &[s(1, &[REF_MUT, LIST], VOID)]),
    ("list_reserve", &[s(1, &[REF_MUT, USIZE], VOID)]),
    ("list_truncate", &[s(1, &[REF_MUT, USIZE], VOID)]),
    ("map_hash_seed", &[s(0, &[], U64)]),
    ("map_probe", &[s(0, &[REF_MUT, Any, Any], REF_MUT)]),
    ("map_steps", &[s(2, &[Named("Map")], Fn)]),
    ("mem_copy", &[s(0, &[USIZE, USIZE, USIZE], VOID)]),
    ("mem_fill", &[s(0, &[USIZE, U8, USIZE], VOID)]),
    ("mem_grow", &[s(0, &[USIZE], Opt)]),
    ("mem_load_u16", &[s(0, &[USIZE], U16)]),
    ("mem_load_u32", &[s(0, &[USIZE], U32)]),
    ("mem_load_u64", &[s(0, &[USIZE], U64)]),
    ("mem_load_u8", &[s(0, &[USIZE], U8)]),
    ("mem_new_string", &[s(0, &[USIZE], STR)]),
    ("mem_pages", &[s(0, &[], USIZE)]),
    ("mem_store_u16", &[s(0, &[USIZE, U16], VOID)]),
    ("mem_store_u32", &[s(0, &[USIZE, U32], VOID)]),
    ("mem_store_u64", &[s(0, &[USIZE, U64], VOID)]),
    ("mem_store_u8", &[s(0, &[USIZE, U8], VOID)]),
    ("mem_string_bytes", &[s(0, &[STR], PTR)]),
    (
        "panic",
        &[s(0, &[STR, STR], VOID), s(0, &[STR, STR], NEVER)],
    ),
    ("parse_f64", &[s(0, &[STR], F64)]),
    ("ptr_mut", &[s(0, &[PTR], REF_MUT)]),
    ("ptr_offset", &[s(0, &[PTR, USIZE], PTR)]),
    ("ptr_read", &[s(0, &[PTR], Any)]),
    ("ptr_ref", &[s(0, &[PTR], REF)]),
    ("ptr_size_of", &[s(1, &[], USIZE)]),
    ("ptr_write", &[s(0, &[PTR, Any], VOID)]),
    ("ref_identity", &[s(1, &[REF], Named("RefIdentity"))]),
    ("repl_write", &[s(0, &[STR], VOID)]),
    ("snapshot_update", &[s(0, &[STR], BOOL)]),
    (
        "string_from_bytes",
        &[
            s(0, &[LIST], STR),
            s(0, &[Named("Buffer"), USIZE], STR),
            s(0, &[STR, USIZE, USIZE], STR),
        ],
    ),
    ("string_lower", &[s(0, &[STR], STR)]),
    ("string_to_bytes", &[s(0, &[STR], LIST)]),
    ("string_upper", &[s(0, &[STR], STR)]),
    ("task_abandon", &[s(1, &[SUSPEND], VOID)]),
    ("task_all_frame", &[s(1, &[LIST], SUSPEND)]),
    ("task_cleanup_frame", &[s(1, &[SUSPEND], SUSPEND)]),
    ("task_idle_hook", &[s(0, &[Fn], VOID)]),
    ("task_pending_frame", &[s(1, &[], SUSPEND)]),
    ("task_race_frame", &[s(1, &[LIST], SUSPEND)]),
    ("task_wait_cell", &[s(0, &[], COUNTED_MUT)]),
    ("task_wait_frame", &[s(0, &[COUNTED_MUT], SUSPEND)]),
    ("task_wait_signal", &[s(0, &[COUNTED_MUT], VOID)]),
    ("type_id_of", &[s(1, &[], Named("TypeId"))]),
    ("utf8_valid_len", &[s(0, &[LIST], USIZE)]),
    ("weak_new", &[s(1, &[REF], Opt)]),
    ("weak_new_mut", &[s(1, &[REF_MUT], Opt)]),
    ("weak_upgrade", &[s(1, &[Named("Weak")], Opt)]),
    ("weak_upgrade_mut", &[s(1, &[Named("WeakMut")], Opt)]),
    ("weak_weaken", &[s(1, &[Named("WeakMut")], Named("Weak"))]),
];

/// The shapes of `key`, or `None` when no intrinsic has that key.
#[must_use]
pub fn shapes(key: &str) -> Option<&'static [Shape]> {
    INTRINSICS
        .binary_search_by(|(k, _)| (*k).cmp(key))
        .ok()
        .map(|i| INTRINSICS[i].1)
}

/// Whether `t` has kind `k`.
fn has_kind(names: &Names<'_>, t: Ty, k: Kind) -> bool {
    let is_named = |def: hd_base::DefId, name: &str| {
        names.paths.segment(hd_base::PathId::from_raw(def.raw())) == name
    };
    match (k, names.pool.get(t)) {
        (Any, _) => true,
        (Is(want), _) => t == want,
        (Named(name), TyData::Adt { def, .. }) => is_named(def, name),
        (
            Kind::Ref { mutable, counted },
            TyData::Ref {
                mutable: m,
                counted: c,
                ..
            },
        ) => mutable == m && counted == c,
        (Opt, TyData::Adt { def, .. }) => def == names.known.option,
        (Tuple(n), TyData::Tuple { elems, rest: None }) => {
            names.pool.list_items(elems).len() == usize::from(n)
        }
        (Fn, TyData::Fn { .. }) => true,
        _ => false,
    }
}

/// Whether `sig` has `shape`.
fn fits(names: &Names<'_>, sig: &FnSig, shape: &Shape) -> bool {
    let own = sig.generics.iter().filter(|g| !g.lifetime).count();
    own == usize::from(shape.generics)
        && sig.params.len() == shape.params.len()
        && sig
            .params
            .iter()
            .zip(shape.params)
            .all(|((_, t), k)| has_kind(names, *t, *k))
        && has_kind(names, sig.ret, shape.ret)
}

/// Why a declaration does not fit the table.
#[derive(Debug, PartialEq, Eq)]
pub enum Misfit {
    /// No intrinsic has the key.
    Unknown,
    /// The signature has none of the key's shapes: their text.
    Shape(String),
}

/// Checks a declaration `@intrinsic(key)` with signature `sig` against
/// the table (`trait.impl.intrinsic.shape`).
pub fn check(names: &Names<'_>, key: &str, sig: &FnSig) -> Result<(), Misfit> {
    let shapes = shapes(key).ok_or(Misfit::Unknown)?;
    if shapes.iter().any(|shape| fits(names, sig, shape)) {
        return Ok(());
    }
    let text: Vec<String> = shapes.iter().map(|sh| show(names, sh)).collect();
    Err(Misfit::Shape(text.join("` or `")))
}

/// A shape as text, as in `fn[1](List) -> Suspend`.
fn show(names: &Names<'_>, shape: &Shape) -> String {
    let kind = |k: &Kind| match *k {
        Any => "_".to_owned(),
        Is(t) => crate::iface::show_ty(names, t),
        Named(n) => n.to_owned(),
        Kind::Ref { mutable, counted } => {
            let sigil = if counted { "*" } else { "&" };
            format!("{sigil}{}_", if mutable { "mut " } else { "" })
        }
        Opt => "_?".to_owned(),
        Tuple(n) => format!("({})", vec!["_"; usize::from(n)].join(", ")),
        Fn => "fn(..)".to_owned(),
    };
    let params: Vec<String> = shape.params.iter().map(kind).collect();
    let generics = if shape.generics == 0 {
        String::new()
    } else {
        format!("[{}]", shape.generics)
    };
    format!(
        "fn{generics}({}) -> {}",
        params.join(", "),
        kind(&shape.ret)
    )
}

#[cfg(test)]
mod tests {
    use super::INTRINSICS;

    #[test]
    fn the_table_is_sorted_with_one_row_per_key() {
        assert!(INTRINSICS.windows(2).all(|w| w[0].0 < w[1].0));
        assert!(INTRINSICS.iter().all(|(_, shapes)| !shapes.is_empty()));
    }
}
