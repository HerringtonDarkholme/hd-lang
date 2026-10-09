//! The bodies the compiler supplies for `dbg` (spec/lang/10-modules.md,
//! "Debug Printing", "Debug Values", "Large Values"). Collection asks for
//! them at concrete types, where the declarations are known:
//!
//! - [`show_body`]: `std.format.dbg_show[T](value, out)`, which writes a
//!   part of a `dbg` line. A type that implements `Debug` prints through it
//!   (`module.dbg.value.debug`); any other prints its structure, each part
//!   by `dbg_show` again (`module.dbg.value.structural`), so a recursive
//!   type recurses through its own instance.
//! - [`values_body`]: `std.format.dbg[Args](values)`, the body of a `dbg`
//!   that has no call site, which prints each element of its one tuple
//!   argument on a line of its own (`module.dbg.body.value`).
//!
//! The writer calls are the layout `std.format.DebugWriter` offers its
//! builders, so a structural part is laid out as a builder lays it out.

use hd_base::{DefId, LocalId, StageResult};
use hd_intern::PathKind;
use hd_resolve::ItemData;
use hd_tir::Body;
use hd_tir::ir::{BodyKind, Callee, Coercion, NONE, Providers, Ref, Tag, TirBuilder, TirSink};
use hd_types::{ParamRef, Prim, Ty, TyData, TyList};

use super::{Cx, Gen, Handles, Mem, SYN, Shape, Var, strip_mut};

const FORMAT: &str = "std.format";

/// How a printed part is introduced inside its parent.
enum Label {
    /// A positional argument or tuple element.
    Tuple,
    /// A named argument, `name=value`.
    Named(String),
    /// A named field, `name: value`.
    Field(String),
}

/// What a declared type is, for its printed name.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Data,
    Newtype,
    Enum,
}

/// The body of `dbg_show[ty]`. `debug`: `ty` implements `Debug`.
pub fn show_body(cx: &Cx<'_, '_>, item: DefId, ty: Ty, debug: bool) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    let mut g = Gen::new(cx, ty, item, Handles::Inline);
    g.b = TirBuilder::new(item, BodyKind::Fn);
    let out_ty = pool.intern_ty(&TyData::Mut(g.format_ty("DebugWriter", &[])));
    let value = g.b.local(
        ty,
        names.syms.intern("value"),
        hd_tir::ir::local_flags::PARAM,
        SYN,
    );
    let out = g.b.local(
        out_ty,
        names.syms.intern("out"),
        hd_tir::ir::local_flags::PARAM,
        SYN,
    );
    let blk = g.b.open_block();
    g.print(value, out, out_ty, ty, debug)?;
    let root = g.b.close_block(blk, None, Ty::VOID, SYN);
    g.finish(root).map(|(body, _)| body)
}

/// The body of `dbg[args]`, whose one parameter is the tuple `args`.
pub fn values_body(cx: &Cx<'_, '_>, item: DefId, args: Ty) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    let TyData::Tuple { elems, rest: None } = pool.get(args) else {
        return cx.unsupported("a `dbg` whose arguments are not a plain tuple");
    };
    let elems = pool.list_items(elems).to_vec();
    let mut g = Gen::new(cx, args, item, Handles::Inline);
    g.b = TirBuilder::new(item, BodyKind::Fn);
    let values = g.b.local(
        args,
        names.syms.intern("values"),
        hd_tir::ir::local_flags::PARAM,
        SYN,
    );
    let blk = g.b.open_block();
    let bare = names.item(FORMAT, "dbg_bare_one");
    for (k, e) in elems.iter().enumerate() {
        let e = strip_mut(names, *e);
        let tuple = g.b.get(values, args, SYN);
        let v = g.b.emit(
            Tag::TupleGet,
            tuple.0,
            u32::try_from(k).unwrap_or(0),
            e,
            SYN,
        );
        let prefix = g.b.const_str("");
        let show = g.show_ref(e);
        g.b.call(
            &Callee::Item {
                def: bare,
                targs: pool.list(&[e]),
            },
            &[prefix, v, show],
            Providers::None,
            Ty::VOID,
            SYN,
        );
    }
    let root = g.b.close_block(blk, None, Ty::VOID, SYN);
    g.finish(root).map(|(body, _)| body)
}

impl Gen<'_, '_> {
    /// A `std.format` type.
    fn format_ty(&self, name: &str, args: &[Ty]) -> Ty {
        self.adt(self.names.item(FORMAT, name), args)
    }

    fn usize_const(&mut self, n: usize) -> Ref {
        self.b
            .const_value(Ty::prim(Prim::Usize), u64::try_from(n).unwrap_or(0))
    }

    /// A call of `DebugWriter`'s method `name` on `out`.
    fn writer_call(&mut self, out: (LocalId, Ty), name: &str, args: &[Ref], ret: Ty) -> Ref {
        let names = self.names;
        let impl_ = DefId::from_raw(
            names
                .paths
                .intern(names.module(FORMAT), PathKind::Impl, "impl DebugWriter")
                .raw(),
        );
        let def = names.member(impl_, PathKind::Member, name);
        let recv = self.b.get(out.0, out.1, SYN);
        let mut all = vec![recv];
        all.extend_from_slice(args);
        self.b.call(
            &Callee::Item {
                def,
                targs: TyList::EMPTY,
            },
            &all,
            Providers::None,
            ret,
            SYN,
        )
    }

    fn write(&mut self, out: (LocalId, Ty), text: &str) {
        let t = self.b.const_str(text);
        self.writer_call(out, "write", &[t], Ty::VOID);
    }

    /// `if cond: body`.
    fn when(
        &mut self,
        cond: Ref,
        body: impl FnOnce(&mut Self) -> StageResult<()>,
    ) -> StageResult<()> {
        let tb = self.b.open_block();
        body(self)?;
        let then = self.b.close_block(tb, None, Ty::VOID, SYN);
        let rec = self.b.refs_record(&[then, Ref(NONE)]);
        self.b.emit(Tag::If, cond.0, rec, Ty::VOID, SYN);
        Ok(())
    }

    /// A reference to `dbg_show` at `ty`: the printer of a part, as a value.
    fn show_ref(&mut self, ty: Ty) -> Ref {
        let pool = self.names.pool;
        let show = self.names.item(FORMAT, "dbg_show");
        let writer = pool.intern_ty(&TyData::Mut(self.format_ty("DebugWriter", &[])));
        let ft = self.fn_ty(&[ty, writer], Ty::VOID);
        let a = self.b.refs_record(&[Ref(show.raw())]);
        let l = pool.list(&[ty]);
        let bw = self.b.refs_record(&[Ref(l.0)]);
        self.b.emit(Tag::ItemRef, a, bw, ft, SYN)
    }

    /// Writes `v`, a part of type `ty`, through its own `dbg_show`.
    fn part(&mut self, out: (LocalId, Ty), ty: Ty, v: Ref) {
        let ty = strip_mut(self.names, ty);
        let show = self.names.item(FORMAT, "dbg_show");
        let recv = self.b.get(out.0, out.1, SYN);
        self.b.call(
            &Callee::Item {
                def: show,
                targs: self.names.pool.list(&[ty]),
            },
            &[v, recv],
            Providers::None,
            Ty::VOID,
            SYN,
        );
    }

    /// A part opened one level deeper, as a builder opens one: its name,
    /// then each item, then its close. `read` makes the value of item `k`.
    fn opened(
        &mut self,
        out: (LocalId, Ty),
        name: Option<&str>,
        items: &[(Label, Ty)],
        read: &dyn Fn(&mut Self, usize) -> Ref,
        close: (&str, &[Ref]),
    ) -> StageResult<()> {
        let shown = self.writer_call(out, "open", &[], Ty::BOOL);
        self.when(shown, |g| {
            if let Some(n) = name {
                g.write(out, n);
            }
            for (k, (label, ty)) in items.iter().enumerate() {
                let index = g.usize_const(k);
                match label {
                    Label::Tuple => {
                        g.writer_call(out, "tuple_item", &[index], Ty::VOID);
                    }
                    Label::Named(n) => {
                        let n = g.b.const_str(n);
                        g.writer_call(out, "named_item", &[n, index], Ty::VOID);
                    }
                    Label::Field(n) => {
                        let n = g.b.const_str(n);
                        g.writer_call(out, "struct_item", &[n, index], Ty::VOID);
                    }
                }
                let v = read(g, k);
                g.part(out, *ty, v);
                g.writer_call(out, "end_item", &[], Ty::VOID);
            }
            g.writer_call(out, close.0, close.1, Ty::VOID);
            Ok(())
        })
    }

    /// Writes the value of `value`, of type `ty`.
    fn print(
        &mut self,
        value: LocalId,
        out: LocalId,
        out_ty: Ty,
        ty: Ty,
        debug: bool,
    ) -> StageResult<()> {
        let names = self.names;
        let pool = names.pool;
        let out = (out, out_ty);
        let v = self.b.get(value, ty, SYN);
        if debug {
            let nested = names.item(FORMAT, "dbg_nested");
            let recv = self.b.get(out.0, out.1, SYN);
            self.b.call(
                &Callee::Item {
                    def: nested,
                    targs: pool.list(&[ty]),
                },
                &[v, recv],
                Providers::None,
                Ty::VOID,
                SYN,
            );
            return Ok(());
        }
        let opaque = |g: &mut Self, text: &str| g.write(out, text);
        match pool.get(ty) {
            TyData::Prim(Prim::Void) => opaque(self, "()"),
            TyData::Never => opaque(self, "never"),
            TyData::Fn { .. } | TyData::TraitValue { .. } => {
                opaque(self, &format!("<{}>", pool.display(ty)));
            }
            TyData::Option(inner) => {
                let inner = strip_mut(names, inner);
                let helper = names.item(FORMAT, "dbg_option");
                let show = self.show_ref(inner);
                let recv = self.b.get(out.0, out.1, SYN);
                self.b.call(
                    &Callee::Item {
                        def: helper,
                        targs: pool.list(&[inner]),
                    },
                    &[v, recv, show],
                    Providers::None,
                    Ty::VOID,
                    SYN,
                );
            }
            TyData::Tuple { elems, rest: None } => {
                let elems: Vec<Ty> = pool.list_items(elems).to_vec();
                let items: Vec<(Label, Ty)> = elems.iter().map(|e| (Label::Tuple, *e)).collect();
                let count = [self.usize_const(items.len()), self.bool_(true)];
                let close = ("tuple_close", &count[..]);
                let read = |g: &mut Self, k: usize| {
                    let t = strip_mut(g.names, elems[k]);
                    g.b.emit(Tag::TupleGet, v.0, u32::try_from(k).unwrap_or(0), t, SYN)
                };
                self.opened(out, None, &items, &read, close)?;
            }
            TyData::Adt { def, args } => {
                let args: Vec<Ty> = pool
                    .list_items(args)
                    .iter()
                    .map(|a| strip_mut(names, *a))
                    .collect();
                let known = names.known;
                if def == known.list || def == known.map || def == known.result {
                    self.composite(def, &args, v, out)?;
                } else if def == known.suspend {
                    opaque(self, "<handle>");
                } else {
                    self.declared(def, &args, ty, v, out)?;
                }
            }
            _ => opaque(self, &format!("<{}>", pool.display(ty))),
        }
        Ok(())
    }

    /// A list, a map or a `Result` whose parts do not all implement `Debug`.
    fn composite(
        &mut self,
        def: DefId,
        args: &[Ty],
        v: Ref,
        out: (LocalId, Ty),
    ) -> StageResult<()> {
        let names = self.names;
        let pool = names.pool;
        let known = names.known;
        let (helper, nargs) = if def == known.list {
            ("dbg_list", 1)
        } else if def == known.map {
            ("dbg_map", 2)
        } else {
            ("dbg_result", 2)
        };
        if args.len() != nargs {
            return self
                .cx
                .unsupported("a built-in composite without its arguments");
        }
        let helper = names.item(FORMAT, helper);
        let recv = self.b.get(out.0, out.1, SYN);
        let mut all = vec![v, recv];
        for a in args {
            all.push(self.show_ref(*a));
        }
        self.b.call(
            &Callee::Item {
                def: helper,
                targs: pool.list(args),
            },
            &all,
            Providers::None,
            Ty::VOID,
            SYN,
        );
        Ok(())
    }

    /// A `data` type, a newtype or an enum, from its declaration.
    fn declared(
        &mut self,
        def: DefId,
        args: &[Ty],
        ty: Ty,
        v: Ref,
        out: (LocalId, Ty),
    ) -> StageResult<()> {
        let names = self.names;
        let pool = names.pool;
        let Some(item) = self.cx.items.item(def) else {
            self.write(out, &format!("<{}>", pool.display(ty)));
            return Ok(());
        };
        let shown = names.text(item.name).to_owned();
        let sub = |t: Ty| {
            strip_mut(
                names,
                pool.subst(t, &|p: ParamRef| {
                    (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
                }),
            )
        };
        let member = |i: usize, f: &hd_resolve::Field| {
            let declared = names.text(f.name).to_owned();
            Mem {
                name: declared.clone(),
                positional: declared.starts_with(|c: char| c.is_ascii_digit()),
                declared,
                position: u32::try_from(i).unwrap_or(0),
                field: u32::try_from(i).unwrap_or(0),
                ty: sub(f.ty),
                embedded: f.embedded,
                has_default: false,
                omitted: false,
                rest: false,
                self_ref: 0,
            }
        };
        let (kind, vars) = match &item.data {
            ItemData::Data(fields) => (
                Kind::Data,
                vec![Var {
                    name: shown.clone(),
                    index: 0,
                    def,
                    shape: Shape::Data,
                    members: fields
                        .iter()
                        .enumerate()
                        .map(|(i, f)| member(i, f))
                        .collect(),
                    self_ref: 0,
                }],
            ),
            ItemData::Newtype(base) => (
                Kind::Newtype,
                vec![Var {
                    name: shown.clone(),
                    index: 0,
                    def,
                    shape: Shape::Data,
                    members: vec![Mem {
                        name: "0".to_owned(),
                        declared: "0".to_owned(),
                        position: 0,
                        field: 0,
                        ty: sub(*base),
                        embedded: false,
                        positional: true,
                        has_default: false,
                        omitted: false,
                        rest: false,
                        self_ref: 0,
                    }],
                    self_ref: 0,
                }],
            ),
            ItemData::Enum { shared, variants } if shared.is_empty() => (
                Kind::Enum,
                variants
                    .iter()
                    .enumerate()
                    .map(|(k, vr)| Var {
                        name: names.text(vr.name).to_owned(),
                        index: u32::try_from(k).unwrap_or(0),
                        def: vr.def,
                        shape: Shape::Enum,
                        members: vr
                            .fields
                            .iter()
                            .enumerate()
                            .map(|(i, f)| member(i, f))
                            .collect(),
                        self_ref: 0,
                    })
                    .collect(),
            ),
            _ => {
                self.write(out, &format!("<{}>", pool.display(ty)));
                return Ok(());
            }
        };
        // A local data type is not inspectable, so it converts to no `Any`
        // and takes no part in finding a cycle; an enum has no identity
        // (r-module.dbg.cycle.tracked).
        let tracked = kind == Kind::Data && !names.is_local(def);
        let guard = if tracked {
            let any = pool.intern_ty(&TyData::TraitValue {
                def: names.known.any,
                args: TyList::EMPTY,
                bindings: vec![],
            });
            let as_any = self.b.coerce(Coercion::ToTraitValue, NONE, v, any, SYN);
            Some(self.writer_call(out, "enter", &[as_any], Ty::BOOL))
        } else {
            None
        };
        let body = |g: &mut Self| -> StageResult<()> {
            g.dispatch(&vars, v, &mut |g, var| {
                let members: Vec<&Mem> = var.members.iter().collect();
                let qualified = if kind == Kind::Enum {
                    format!("{shown}.{}", var.name)
                } else {
                    shown.clone()
                };
                let read = |g: &mut Self, k: usize| g.read(v, var, members[k], members[k].ty);
                if kind == Kind::Enum && members.is_empty() {
                    g.write(out, &qualified);
                    return Ok(());
                }
                let n = members.len();
                if kind == Kind::Data {
                    let items: Vec<(Label, Ty)> = members
                        .iter()
                        .map(|m| (Label::Field(m.name.clone()), m.ty))
                        .collect();
                    let count = [g.usize_const(n)];
                    return g.opened(
                        out,
                        Some(&qualified),
                        &items,
                        &read,
                        ("struct_close", &count),
                    );
                }
                let items: Vec<(Label, Ty)> = members
                    .iter()
                    .map(|m| {
                        let label = if m.positional {
                            Label::Tuple
                        } else {
                            Label::Named(m.name.clone())
                        };
                        (label, m.ty)
                    })
                    .collect();
                let count = [g.usize_const(n), g.bool_(false)];
                g.opened(
                    out,
                    Some(&qualified),
                    &items,
                    &read,
                    ("tuple_close", &count),
                )
            })
        };
        match guard {
            Some(entered) => self.when(entered, |g| {
                body(g)?;
                g.writer_call(out, "leave", &[], Ty::VOID);
                Ok(())
            }),
            None => body(self),
        }
    }
}
