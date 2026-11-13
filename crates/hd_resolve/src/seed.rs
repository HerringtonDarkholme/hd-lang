//! Compiler-supplied std declarations (`crates/hd_stdpack/DESIGN.md`, "Prelude"):
//! `std.core`'s primitives, collections, `Option`, `Result`, the `Any`
//! traits, `panic` and `box`; `std.task`'s suspension protocol; `std.function`'s
//! function type constructors; `std.testing.it`. Each is an ordinary
//! interface item of its module, so uses, the prelude and re-exports reach
//! it like any declaration. A source declaration of the same name wins.

use hd_base::Symbol;
use hd_intern::PathKind;
use hd_types::{ParamRef, Prim, RowData, RowId, RowParamRef, Ty, TyData, TyList};

use crate::iface::{
    CallAttrs, Field, FnAttrs, FnSig, Generic, Item, ItemData, Names, TraitData, TypeAttrs, Variant,
};

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
        lifetimes: Vec::new(),
        fn_lifetimes: Vec::new(),
    }
}

fn sym(names: &Names<'_>, s: &str) -> Symbol {
    names.syms.intern(s)
}

/// The compiler-supplied items of `module` (empty for any other module).
#[must_use]
/// A seeded function whose body is the compiler's `@intrinsic(key)`.
fn intrinsic(names: &Names<'_>, key: &str) -> FnAttrs {
    FnAttrs {
        call: CallAttrs {
            intrinsic: Some(sym(names, key)),
            ..CallAttrs::default()
        },
        ..FnAttrs::default()
    }
}

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
                item(
                    p.name(),
                    vec![],
                    ItemData::Alias {
                        body: Ty::prim(p),
                        lifetimes: vec![],
                    },
                );
            }
            item(
                "never",
                vec![],
                ItemData::Alias {
                    body: Ty::NEVER,
                    lifetimes: vec![],
                },
            );
            item(
                "void",
                vec![],
                ItemData::Alias {
                    body: Ty::VOID,
                    lifetimes: vec![],
                },
            );
            let bound = |def| {
                pool.intern_ty(&TyData::TraitValue {
                    def,
                    args: TyList::EMPTY,
                    bindings: vec![],
                })
            };
            // A collection's items copy (`ref.copy.collections`).
            let copy = bound(names.known.copy);
            let mut list_gens = gens(names, &["T"]);
            list_gens[0].bounds = vec![copy];
            item("List", list_gens, ItemData::Builtin);
            // `Map[K < Eq & Hash, V]` (types.map-key.declared-bound).
            let mut map_gens = gens(names, &["K", "V"]);
            map_gens[0].bound = Some(names.known.eq);
            map_gens[0].bounds = [names.known.eq, names.known.hash, names.known.copy]
                .into_iter()
                .map(bound)
                .collect();
            map_gens[1].bounds = vec![copy];
            item("Map", map_gens, ItemData::Builtin);
            // `enum Option[T] { None, Some(T) }` (`types.option.decl`).
            let opt = names.known.option;
            let variants = vec![
                Variant {
                    name: sym(names, "None"),
                    def: names.member(opt, PathKind::Variant, "None"),
                    fields: vec![],
                },
                Variant {
                    name: sym(names, "Some"),
                    def: names.member(opt, PathKind::Variant, "Some"),
                    fields: vec![field(names, "0", param(names, opt, 0))],
                },
            ];
            item(
                "Option",
                gens(names, &["T"]),
                ItemData::Enum {
                    shared: vec![],
                    variants,
                    attrs: TypeAttrs::default(),
                },
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
                    attrs: TypeAttrs::default(),
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
            item(
                "panic",
                vec![],
                ItemData::Fn(sig, intrinsic(names, "panic_message")),
            );
            // `'static` (ref.generic.static): a sealed trait that no
            // path names, which a type parameter's bound `'static` writes.
            let st = names.item(module, "'static");
            out.push(Item::new(
                st,
                sym(names, "'static"),
                true,
                ItemData::Trait(TraitData::default()),
            ));
            // `box[T](value: T) -> *mut T` (ref.box.declaration): the
            // checker supplies the body, a new object holding a copy of
            // `value`. A `T` that holds a local reference makes the result
            // hold one (ref.box.local).
            let bx = names.item(module, "box");
            let t = param(names, bx, 0);
            let boxed = pool.intern_ty(&TyData::Ref {
                inner: t,
                mutable: true,
                counted: true,
            });
            let sig = FnSig::simple(gens(names, &["T"]), vec![(sym(names, "value"), t)], boxed);
            out.push(Item::new(
                bx,
                sym(names, "box"),
                true,
                ItemData::Fn(sig, intrinsic(names, "box")),
            ));
        }
        "std.task" => {
            item("Suspend", gens(names, &["T"]), ItemData::Builtin);
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
                    attrs: TypeAttrs::default(),
                },
            );
            // Only the host runtime and `std.task` construct one
            // (req.poll-context.construction): the private field makes a
            // struct literal elsewhere the ordinary private-member error alone
            // (the default keeps `missing-required-field` out).
            let mut private = field(names, "waker", Ty::VOID);
            private.public = false;
            private.has_default = true;
            item(
                "PollContext",
                vec![],
                ItemData::Struct(vec![private], TypeAttrs::default()),
            );
            item(
                "Waker",
                vec![],
                ItemData::Struct(vec![], TypeAttrs::default()),
            );
            // `join!` has no written signature (`crates/hd_wasm/DESIGN.md` §14.5): its
            // checker rule is the compiler's.
            let mut join = FnSig::simple(vec![], vec![], Ty::VOID);
            join.suspends = true;
            join.variadic = true;
            item(
                "join",
                vec![],
                ItemData::Fn(join, intrinsic(names, "task_all")),
            );
        }
        "std.function" => {
            let tr = names.item(module, "Fn");
            let tuple = pool.intern_ty(&TyData::TraitValue {
                def: names.known.tuple,
                args: TyList::EMPTY,
                bindings: vec![],
            });
            let mut g = gens(names, &["I", "O", "R"]);
            g[0].bound = Some(names.known.tuple);
            g[0].bounds = vec![tuple];
            g[2].row = true;
            g[2].default = Some(pool.intern_ty(&TyData::Row(RowId::EMPTY)));

            let self_ty = param(names, tr, 0);
            let input = param(names, tr, 1);
            let output = param(names, tr, 2);
            let row = pool.row(&RowData {
                keys: vec![],
                params: vec![RowParamRef {
                    owner: tr,
                    index: 3,
                }],
            });
            let mut sig = FnSig::simple(
                vec![],
                vec![
                    (
                        sym(names, "self"),
                        pool.intern_ty(&TyData::Ref {
                            inner: self_ty,
                            mutable: false,
                            counted: false,
                        }),
                    ),
                    (sym(names, "args"), input),
                ],
                output,
            );
            sig.row = row;
            let method = names.member(tr, PathKind::Member, "call");
            item(
                "Fn",
                g.clone(),
                ItemData::Trait(TraitData {
                    methods: vec![(sym(names, "call"), method)],
                    ..TraitData::default()
                }),
            );
            let call = Item::new(
                method,
                sym(names, "call"),
                true,
                ItemData::Method {
                    owner: tr,
                    sig,
                    has_body: false,
                    call: intrinsic(names, "fn_call").call,
                },
            );
            item(
                "SuspendFn",
                g,
                ItemData::Struct(vec![], TypeAttrs::default()),
            );
            out.push(call);
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
                        call: CallAttrs::default(),
                    },
                ));
            };
            method("facts", FnSig::simple(vec![], vec![], facts));
            method("name", FnSig::simple(vec![], vec![], Ty::STRING));
            for (name, proto, has_receiver, ret_self) in [
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
                if has_receiver {
                    params.push((
                        sym(names, "self"),
                        pool.intern_ty(&TyData::Ref {
                            inner: self_ty,
                            mutable: false,
                            counted: false,
                        }),
                    ));
                }
                // The walker, describer or source sits behind `&mut`
                // (annot.structure.protocol-ref): its methods take
                // `&mut self`.
                params.push((
                    sym(names, "p"),
                    pool.intern_ty(&TyData::Ref {
                        inner: p,
                        mutable: true,
                        counted: false,
                    }),
                ));
                let ok = if ret_self { self_ty } else { Ty::VOID };
                method(name, FnSig::simple(g, params, result(ok, err)));
            }
            item(
                "Structure",
                vec![],
                ItemData::Trait(TraitData {
                    methods,
                    relaxed_copy: true,
                    ..TraitData::default()
                }),
            );
            item(
                "UnionType",
                vec![],
                ItemData::Trait(TraitData {
                    relaxed_copy: true,
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
                inputs: hd_types::Inputs::Fixed,
                deps: hd_types::FnDeps::NONE,
            });
            item(
                "it",
                vec![],
                ItemData::Fn(
                    FnSig::simple(
                        vec![],
                        vec![(sym(names, "name"), Ty::STRING), (sym(names, "body"), body)],
                        Ty::VOID,
                    ),
                    FnAttrs::default(),
                ),
            );
        }
        // ruled out: a match on a module path; the other modules have no seeded items.
        _ => {}
    }
    if module == "std.testing"
        && let Some(c) = (out.iter_mut())
            .find(|i| i.def == names.item(module, "it"))
            .and_then(Item::call_mut)
    {
        c.intrinsic = Some(sym(names, "test_case"));
    }
    for it in &out {
        names.record_params(it);
    }
    out
}
