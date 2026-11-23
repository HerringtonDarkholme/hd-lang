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
    pool.product(pool.list(elems), rest)
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
        // Unit remains tuple syntax, rather than a declaration with no args.
        let mut unit = Ty::VOID;
        for _ in 0..SHOWN_TYPE_DEPTH {
            unit = tuple(pool, &[unit], None);
        }
        assert_eq!(show_ty(names, unit), format!("{open}...{close}"));
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

#[test]
fn opaque_bounds_keep_their_names_and_references_keep_dyn() {
    with_names(|names, pool, _| {
        let owner = names.item("app.make", "create");
        let trait_ = names.item("app.api", "Shown");
        for (args, short, qualified) in [
            (pool.list(&[]), "Shown", "app.api.Shown"),
            (pool.list(&[Ty::I32]), "Shown[i32]", "app.api.Shown[i32]"),
        ] {
            let bound = pool.intern_ty(&TyData::TraitValue {
                def: trait_,
                args,
                bindings: Vec::new(),
            });
            let opaque = pool.intern_ty(&TyData::Opaque {
                owner,
                site: 0,
                borrows: false,
                args: pool.list(&[Ty::STRING]),
                bound,
            });
            assert_eq!(show_ty(names, opaque), format!("impl {short}"));
            assert_eq!(show_ty_full(names, opaque), format!("impl {short}"));
            assert_eq!(
                hd_resolve::iface::show_ty_qualified_in(names, pool.types(), opaque, false),
                format!("impl {qualified} returned by app.make.create[string]")
            );
            assert_eq!(
                hd_resolve::iface::show_ty_diagnostic_in(names, pool.types(), opaque, false),
                format!("impl {short} returned by create[string]")
            );
            for (counted, prefix) in [(false, "&"), (true, "*")] {
                let reference = pool.intern_ty(&TyData::Ref {
                    inner: bound,
                    mutable: false,
                    counted,
                });
                assert_eq!(show_ty(names, reference), format!("{prefix}dyn {short}"));
            }
        }
        // An unknown bound still needs a printable type in error recovery.
        let erroneous = pool.intern_ty(&TyData::Opaque {
            owner,
            site: 0,
            borrows: false,
            args: pool.list(&[]),
            bound: Ty::POISON,
        });
        assert_eq!(show_ty(names, erroneous), "impl _");
    });
}
