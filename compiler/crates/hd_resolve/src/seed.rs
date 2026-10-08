//! Compiler-supplied std declarations (std-bootstrap.md, "Prelude"):
//! `std.core`'s primitives, collections, `Option`, `Result`, the `Any`
//! traits and `panic`; `std.task`'s suspension protocol; `std.function`'s
//! function type constructors; `std.testing.it`. Each is an ordinary
//! interface item of its module, so uses, the prelude and re-exports reach
//! it like any declaration. A source declaration of the same name wins.

use hd_base::Symbol;
use hd_intern::PathKind;
use hd_types::{ParamRef, Prim, RowId, Ty, TyData, TyList};

use crate::iface::{Field, FnSig, Generic, Item, ItemData, Names, TraitData, Variant};

/// The text of the virtual `std.core` module: it declares nothing itself.
pub const CORE_SOURCE: &str = "# std.core: compiler-supplied declarations (hd_resolve::seed).\n";

/// The modules that have compiler-supplied items.
pub const SEEDED: &[&str] = &[
    "std.core",
    "std.task",
    "std.function",
    "std.inspect",
    "std.structure",
    "std.testing",
];

fn param(names: &Names<'_>, owner: hd_base::DefId, index: u16) -> Ty {
    names
        .pool
        .intern_ty(&TyData::Param(ParamRef { owner, index }))
}

fn gens(names: &Names<'_>, list: &[&str]) -> Vec<Generic> {
    list.iter()
        .map(|g| Generic::plain(names.syms.intern(g)))
        .collect()
}

fn field(names: &Names<'_>, name: &str, ty: Ty) -> Field {
    Field {
        name: names.syms.intern(name),
        ty,
        public: true,
        has_default: false,
        embedded: false,
    }
}

fn sym(names: &Names<'_>, s: &str) -> Symbol {
    names.syms.intern(s)
}

/// The compiler-supplied items of `module` (empty for any other module).
#[must_use]
pub fn items(names: &Names<'_>, module: &str) -> Vec<Item> {
    let pool = names.pool;
    let mut out = Vec::new();
    let mut item = |name: &str, generics: Vec<Generic>, data: ItemData| {
        let mut it = Item::new(names.item(module, name), sym(names, name), true, data);
        it.generics = generics;
        out.push(it);
    };
    match module {
        "std.core" => {
            for p in Prim::ALL {
                item(p.name(), vec![], ItemData::Alias(Ty::prim(p)));
            }
            item("never", vec![], ItemData::Alias(Ty::NEVER));
            item("List", gens(names, &["T"]), ItemData::Data(vec![]));
            item("Map", gens(names, &["K", "V"]), ItemData::Data(vec![]));
            let opt = names.item(module, "Option");
            let t = param(names, opt, 0);
            item(
                "Option",
                gens(names, &["T"]),
                ItemData::Alias(pool.intern_ty(&TyData::Option(t))),
            );
            let res = names.item(module, "Result");
            let variants = vec![
                Variant {
                    name: sym(names, "Ok"),
                    def: names.member(res, PathKind::Variant, "Ok"),
                    fields: vec![field(names, "0", param(names, res, 0))],
                },
                Variant {
                    name: sym(names, "Err"),
                    def: names.member(res, PathKind::Variant, "Err"),
                    fields: vec![field(names, "0", param(names, res, 1))],
                },
            ];
            item(
                "Result",
                gens(names, &["T", "E"]),
                ItemData::Enum {
                    shared: vec![],
                    variants,
                },
            );
            let any = names.item(module, "Any");
            let any_tv = pool.intern_ty(&TyData::TraitValue {
                def: any,
                args: TyList::EMPTY,
                bindings: vec![],
            });
            item("Any", vec![], ItemData::Trait(TraitData::default()));
            for n in ["AnyVal", "AnyRef"] {
                item(
                    n,
                    vec![],
                    ItemData::Trait(TraitData {
                        supers: vec![any_tv],
                        ..TraitData::default()
                    }),
                );
            }
            let sig = FnSig::simple(vec![], vec![(sym(names, "message"), Ty::STRING)], Ty::NEVER);
            item("panic", vec![], ItemData::Fn(sig));
            if let Some(p) = out.last_mut() {
                p.intrinsic = Some(sym(names, "panic_message"));
            }
        }
        "std.task" => {
            let suspend = names.item(module, "Suspend");
            item("Suspend", gens(names, &["T"]), ItemData::Data(vec![]));
            let poll_def = names.item(module, "Poll");
            item(
                "Poll",
                gens(names, &["T"]),
                ItemData::Enum {
                    shared: vec![],
                    variants: vec![
                        Variant {
                            name: sym(names, "Ready"),
                            def: names.member(poll_def, PathKind::Variant, "Ready"),
                            fields: vec![field(names, "0", param(names, poll_def, 0))],
                        },
                        Variant {
                            name: sym(names, "Pending"),
                            def: names.member(poll_def, PathKind::Variant, "Pending"),
                            fields: vec![],
                        },
                    ],
                },
            );
            item("PollContext", vec![], ItemData::Data(vec![]));
            item("Waker", vec![], ItemData::Data(vec![]));
            let block_on = names.item(module, "block_on");
            let t = param(names, block_on, 0);
            let s = pool.intern_ty(&TyData::Mut(pool.intern_ty(&TyData::Adt {
                def: suspend,
                args: pool.list(&[t]),
            })));
            item(
                "block_on",
                vec![],
                ItemData::Fn(FnSig::simple(
                    gens(names, &["T"]),
                    vec![(sym(names, "s"), s)],
                    t,
                )),
            );
            // `all!` has no written signature (suspension.md §14.5): its
            // checker rule is the compiler's.
            let mut all = FnSig::simple(vec![], vec![], Ty::VOID);
            all.suspends = true;
            all.variadic = true;
            item("all", vec![], ItemData::Fn(all));
            if let Some(p) = out.last_mut() {
                p.intrinsic = Some(sym(names, "task_all"));
            }
        }
        "std.function" => {
            let mut g = gens(names, &["Params", "Result", "R"]);
            g[2].row = true;
            item("Fn", g.clone(), ItemData::Data(vec![]));
            item("SuspendFn", g, ItemData::Data(vec![]));
        }
        "std.inspect" => {
            // `downcast_val[T < Inspectable](value: dyn Inspectable) -> T?`
            let f = names.item(module, "downcast_val");
            let insp = pool.intern_ty(&TyData::TraitValue {
                def: names.item(module, "Inspectable"),
                args: TyList::EMPTY,
                bindings: vec![],
            });
            let t = param(names, f, 0);
            let mut g = gens(names, &["T"]);
            g[0].bound = Some(names.item(module, "Inspectable"));
            g[0].bounds = vec![insp];
            item(
                "downcast_val",
                vec![],
                ItemData::Fn(FnSig::simple(
                    g,
                    vec![(sym(names, "value"), insp)],
                    pool.intern_ty(&TyData::Option(t)),
                )),
            );
            if let Some(p) = out.last_mut() {
                p.intrinsic = Some(sym(names, "downcast_val"));
            }
        }
        "std.structure" => {
            // `Structure` (annotations, "The Structure Trait"): sealed; the
            // compiler supplies it to a target while a template runs.
            let tr = names.item(module, "Structure");
            let self_ty = param(names, tr, 0);
            let facts = pool.intern_ty(&TyData::Adt {
                def: names.item(module, "Facts"),
                args: TyList::EMPTY,
            });
            let result = |ok: Ty, err: Ty| {
                pool.intern_ty(&TyData::Adt {
                    def: names.known.result,
                    args: pool.list(&[ok, err]),
                })
            };
            let mut methods = Vec::new();
            let mut members = Vec::new();
            let mut method = |name: &str, sig: FnSig| {
                let def = names.member(tr, PathKind::Member, name);
                methods.push((sym(names, name), def));
                members.push(Item::new(
                    def,
                    sym(names, name),
                    true,
                    ItemData::Method {
                        owner: tr,
                        sig,
                        has_body: false,
                    },
                ));
            };
            method("facts", FnSig::simple(vec![], vec![], facts));
            method("name", FnSig::simple(vec![], vec![], Ty::STRING));
            for (name, proto, by_value, ret_self) in [
                ("walk", "Walker", true, false),
                ("describe", "Describer", false, false),
                ("build", "Source", false, true),
            ] {
                let m = names.member(tr, PathKind::Member, name);
                let p = param(names, m, 0);
                let ptr = names.item(module, proto);
                let bound = pool.intern_ty(&TyData::TraitValue {
                    def: ptr,
                    args: pool.list(&[self_ty]),
                    bindings: vec![],
                });
                let err = pool.intern_ty(&TyData::Assoc {
                    assoc: names.member(ptr, PathKind::Member, "Error"),
                    trait_: ptr,
                    self_ty: p,
                    args: TyList::EMPTY,
                });
                let mut g = gens(names, &["P"]);
                g[0].bound = Some(ptr);
                g[0].bounds = vec![bound];
                let mut params = Vec::new();
                if by_value {
                    params.push((sym(names, "self"), self_ty));
                }
                params.push((sym(names, "p"), pool.intern_ty(&TyData::Mut(p))));
                let ok = if ret_self {
                    pool.intern_ty(&TyData::Mut(self_ty))
                } else {
                    Ty::VOID
                };
                method(name, FnSig::simple(g, params, result(ok, err)));
            }
            item(
                "Structure",
                vec![],
                ItemData::Trait(TraitData {
                    methods,
                    ..TraitData::default()
                }),
            );
            out.extend(members);
        }
        "std.testing" => {
            let body = pool.intern_ty(&TyData::Fn {
                params: TyList::EMPTY,
                result: Ty::VOID,
                row: RowId::EMPTY,
                suspends: false,
            });
            item(
                "it",
                vec![],
                ItemData::Fn(FnSig::simple(
                    vec![],
                    vec![(sym(names, "name"), Ty::STRING), (sym(names, "body"), body)],
                    Ty::VOID,
                )),
            );
            // `assert_equal[T < Eq & Debug](actual: T, expected: T, reason:
            // string)` (module.testing.exports): the checker checks the
            // call and runs std's `check_equal` (lib/std/testing.hd).
            let ae = names.item(module, "assert_equal");
            let t = param(names, ae, 0);
            let tv = |d| {
                pool.intern_ty(&TyData::TraitValue {
                    def: d,
                    args: TyList::EMPTY,
                    bindings: vec![],
                })
            };
            let eq = names.known.eq;
            let g = Generic {
                bound: Some(eq),
                bounds: vec![tv(eq), tv(names.known.debug)],
                ..Generic::plain(sym(names, "T"))
            };
            item(
                "assert_equal",
                vec![],
                ItemData::Fn(FnSig::simple(
                    vec![g],
                    vec![
                        (sym(names, "actual"), t),
                        (sym(names, "expected"), t),
                        (sym(names, "reason"), Ty::STRING),
                    ],
                    Ty::VOID,
                )),
            );
        }
        _ => {}
    }
    for (name, key) in [("it", "test_case"), ("assert_equal", "assert_equal")] {
        if module == "std.testing"
            && let Some(p) = out.iter_mut().find(|i| i.def == names.item(module, name))
        {
            p.intrinsic = Some(sym(names, key));
        }
    }
    out
}
