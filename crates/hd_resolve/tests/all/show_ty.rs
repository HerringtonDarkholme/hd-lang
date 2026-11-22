//! A type a message names is cut at `SHOWN_TYPE_BYTES` and
//! `SHOWN_TYPE_DEPTH` (#1861); `show_ty_full` prints it whole.

use hd_intern::{PathKind, PathTable, ShardedInterner};
use hd_resolve::iface::{ELIDED, SHOWN_TYPE_BYTES, SHOWN_TYPE_DEPTH};
use hd_resolve::{KnownItems, Names, show_ty, show_ty_full};
use hd_types::{ConstValue, InternPool, Prim, Ty, TyData};

fn with_names(test: impl FnOnce(&Names<'_>, &InternPool, &PathTable)) {
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    test(&names, &pool, &paths);
}

fn tuple(pool: &InternPool, elems: &[Ty], rest: Option<Ty>) -> Ty {
    pool.intern_ty(&TyData::Tuple {
        elems: pool.list(elems),
        rest,
    })
}

/// A cut type is at most the limit, one separator, the elision mark and
/// the closing brackets of the constructors open at the cut.
fn assert_short(shown: &str) {
    let bound = SHOWN_TYPE_BYTES + " | ".len() + ELIDED.len() + SHOWN_TYPE_DEPTH as usize;
    assert!(shown.len() <= bound, "{} bytes: {shown}", shown.len());
}

#[test]
fn a_wide_tuple_is_cut_at_the_byte_limit_and_stays_balanced() {
    with_names(|names, pool, _| {
        let wide = tuple(pool, &[Ty::I32; 40], Some(Ty::prim(Prim::F64)));
        let shown = show_ty(names, wide);
        assert_short(&shown);
        assert!(shown.starts_with("(i32, i32, "), "{shown}");
        assert!(shown.ends_with(", ...)"), "{shown}");
        let full = show_ty_full(names, wide);
        assert_eq!(full, format!("({}f64...)", "i32, ".repeat(40)));
    });
}

#[test]
fn a_deep_type_shows_its_outer_levels() {
    with_names(|names, pool, _| {
        let mut t = Ty::I32;
        for _ in 0..12 {
            t = tuple(pool, &[t], None);
        }
        let open = "(".repeat(SHOWN_TYPE_DEPTH as usize);
        let close = ",)".repeat(SHOWN_TYPE_DEPTH as usize);
        assert_eq!(show_ty(names, t), format!("{open}...{close}"));
        // A name at the depth limit is still shown: it has no part below.
        let mut t = Ty::I32;
        for _ in 0..SHOWN_TYPE_DEPTH {
            t = tuple(pool, &[t], None);
        }
        let open = "(".repeat(SHOWN_TYPE_DEPTH as usize);
        assert_eq!(show_ty(names, t), format!("{open}i32{close}"));
    });
}

#[test]
fn a_long_union_of_unions_is_cut_between_members() {
    with_names(|names, pool, _| {
        let inner = pool.intern_ty(&TyData::sum(pool.list(&[Ty::I32, Ty::STRING])));
        let members: Vec<Ty> = (0..20)
            .map(|i| {
                tuple(
                    pool,
                    &[
                        inner,
                        if i % 2 == 0 {
                            Ty::prim(Prim::F64)
                        } else {
                            Ty::BOOL
                        },
                    ],
                    None,
                )
            })
            .chain([inner])
            .collect();
        let union = pool.intern_ty(&TyData::sum(pool.list(&members)));
        let shown = show_ty(names, union);
        assert_short(&shown);
        assert!(shown.starts_with("(i32 | string, f64) | "), "{shown}");
        assert!(
            shown.ends_with(" | ...)") || shown.ends_with(", ...)"),
            "{shown}"
        );
        let full = show_ty_full(names, union);
        assert!(full.ends_with(" | (i32 | string)"), "{full}");
    });
}

#[test]
fn a_name_too_long_for_the_room_left_is_elided_whole() {
    with_names(|names, pool, paths| {
        let text = "x".repeat(2 * SHOWN_TYPE_BYTES);
        let long = pool.intern_ty(&TyData::Const(ConstValue::Str(paths.intern(
            hd_base::PathId::NONE,
            PathKind::Text,
            &text,
        ))));
        assert_eq!(show_ty(names, long), ELIDED);
        assert_eq!(
            show_ty(names, tuple(pool, &[Ty::I32, long], None)),
            "(i32, ...)"
        );
        assert_eq!(show_ty_full(names, long), format!("\"{text}\""));
    });
}
