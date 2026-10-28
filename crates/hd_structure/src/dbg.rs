//! The private printer-selection bridge and ordinary Structure metadata for dbg.
//! Rendering policy and traversal are in std.format; this module only selects
//! a function, or supplies type-name metadata to an hd printer factory.

use hd_base::{DefId, StageResult};
use hd_resolve::ItemData;
use hd_tir::Body;
use hd_tir::ir::{BodyKind, Callee, Providers, Ref, Tag, TirBuilder, TirSink};
use hd_types::{ParamRef, Ty, TyData};

use super::{Cx, Gen, Handles, Mem, SYN, Shape, Target, Var, strip_mut};

const FORMAT: &str = "std.format";

/// A raw declaration's traversal metadata, without any public trait opt-in.
/// This is used only by the private dbg Structure template.
#[must_use]
pub fn target(cx: &Cx<'_, '_>, ty: Ty) -> Option<Target> {
    if let Some(target) = super::tuple_target(cx.names.pool, ty) {
        return Some(target);
    }
    let names = cx.names;
    let pool = names.pool;
    let TyData::Adt { def, args } = pool.get(ty) else {
        return None;
    };
    let args = pool.list_items(args);
    let item = cx.items.item(def)?;
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
            type_default: None,
            rest: false,
            self_ref: 0,
            facts: None,
        }
    };
    let vars = match &item.data {
        ItemData::Data(fields) => vec![Var {
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
            facts: None,
            shared: Vec::new(),
        }],
        ItemData::Newtype(base) => vec![Var {
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
                type_default: None,
                rest: false,
                self_ref: 0,
                facts: None,
            }],
            self_ref: 0,
            facts: None,
            shared: Vec::new(),
        }],
        ItemData::Enum { shared, variants } if shared.is_empty() => variants
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
                facts: None,
                shared: Vec::new(),
            })
            .collect(),
        _ => {
            return None;
        }
    };
    Some(Target {
        ty,
        name: shown,
        vars,
        facts: None,
    })
}

/// Select a printer; no value is inspected or rendered by the compiler.
pub fn printer_body(
    cx: &Cx<'_, '_>,
    item: DefId,
    ty: Ty,
    debug: bool,
    structural: DefId,
) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    // Mutability belongs to the place, not the value being printed.
    let ty = strip_mut(names, ty);
    let mut g = Gen::new(cx, ty, item, Handles::Supplied);
    g.b = TirBuilder::new(item, BodyKind::Fn);
    let writer = pool.intern_ty(&TyData::Ref {
        inner: g.format_ty("DebugWriter", &[]),
        mutable: true,
        counted: true,
    });
    let ft = g.fn_ty(&[ty, writer], Ty::VOID);
    let block = g.b.open_block();
    let (name, args) = if debug {
        ("dbg_nested", vec![ty])
    } else {
        match pool.get(ty) {
            TyData::Never => ("dbg_never", vec![ty]),
            TyData::Option(t) => ("dbg_option_printer", vec![t]),
            TyData::Ref { inner, .. } => ("dbg_ref_printer", vec![inner]),
            TyData::Adt { def, args } if def == names.known.ref_cell => {
                ("dbg_ref_printer", pool.list_items(args).to_vec())
            }
            TyData::Adt { def, args } if def == names.known.list => {
                ("dbg_list_printer", pool.list_items(args).to_vec())
            }
            TyData::Adt { def, args } if def == names.known.map => {
                ("dbg_map_printer", pool.list_items(args).to_vec())
            }
            TyData::Adt { def, args } if def == names.known.result => {
                ("dbg_result_printer", pool.list_items(args).to_vec())
            }
            TyData::Adt { def, .. } if def == names.known.suspend => ("dbg_handle", vec![ty]),
            _ if target(cx, ty).is_some() => ("", vec![ty]),
            _ => ("dbg_opaque_printer", vec![ty]),
        }
    };
    let def = if name.is_empty() {
        structural
    } else {
        names.item(FORMAT, name)
    };
    let value = if name == "dbg_opaque_printer" {
        let text = g.b.const_str(&pool.display(ty));
        g.b.call(
            &Callee::Item {
                def,
                targs: pool.list(&args),
            },
            &[text],
            Providers::None,
            ft,
            SYN,
        )
    } else {
        let a = g.b.refs_record(&[Ref(def.raw())]);
        let bw = g.b.refs_record(&[Ref(pool.list(&args).0)]);
        g.b.emit(Tag::ItemRef, a, bw, ft, SYN)
    };
    let body = g.b.close_block(block, Some(value), ft, SYN);
    g.finish(body).map(|(body, _)| body)
}

/// The body of `dbg[args]`, whose one parameter is the tuple `args`.
pub fn values_body(cx: &Cx<'_, '_>, item: DefId, args: Ty) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    let TyData::Tuple { elems, rest: None } = pool.get(args) else {
        return cx.unsupported("a `dbg` whose arguments are not a plain tuple");
    };
    let elems = pool.list_items(elems).to_vec();
    let mut g = Gen::new(cx, args, item, Handles::Supplied);
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
        let v = g.b.field(tuple, u32::try_from(k).unwrap_or(0), e, SYN);
        let prefix = g.b.const_str("");
        let show = g.printer(e);
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
    fn format_ty(&self, name: &str, args: &[Ty]) -> Ty {
        self.adt(self.names.item(FORMAT, name), args)
    }

    fn printer(&mut self, ty: Ty) -> Ref {
        let writer = self.names.pool.intern_ty(&TyData::Ref {
            inner: self.format_ty("DebugWriter", &[]),
            mutable: true,
            counted: true,
        });
        let ft = self.fn_ty(&[ty, writer], Ty::VOID);
        self.b.call(
            &Callee::Item {
                def: self.names.item(FORMAT, "dbg_printer"),
                targs: self.names.pool.list(&[ty]),
            },
            &[],
            Providers::None,
            ft,
            SYN,
        )
    }
}
