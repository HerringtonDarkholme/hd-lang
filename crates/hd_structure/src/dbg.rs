//! The private printer-selection bridge and ordinary Structure metadata for dbg.
//! Rendering policy and traversal are in std.format; this module only selects
//! a function, or supplies type-name metadata to an hd printer factory.

use hd_base::{DefId, NotImplemented, StageResult};
use hd_resolve::ItemData;
use hd_tir::Body;
use hd_tir::ir::{BlockMark, BodyKind, Callee, Providers, Ref, Tag, TirBuilder, TirSink};
use hd_types::{ParamRef, Prim, Ty, TyData};

use super::{Cx, Gen, Handles, Mem, SYN, Shape, Target, Var};

use hd_resolve::known::{FORMAT_MODULE as FORMAT, REMOTE_MODULE as REMOTE};

/// A raw declaration's traversal metadata, without any public trait opt-in.
/// This is used only by the private dbg Structure template.
#[must_use]
pub fn target(cx: &Cx<'_, '_>, ty: Ty) -> Option<Target> {
    if let Some(target) = super::tuple_target(cx.names.pool, ty) {
        return Some(target);
    }
    if let Some(target) = super::union_target(cx.names.pool, ty) {
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
        pool.subst(t, &|p: ParamRef| {
            (p.owner == def).then(|| args.get(usize::from(p.index)).copied())?
        })
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
        ItemData::Struct(fields, _) => vec![Var {
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
        ItemData::Newtype(base, _) => vec![Var {
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
        ItemData::Enum {
            shared, variants, ..
        } if shared.is_empty() => variants
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
pub fn printer_body(cx: &Cx<'_, '_>, item: DefId, ty: Ty, debug: bool) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    // Mutability belongs to the place, not the value being printed.
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
            TyData::Ref { inner, .. } => ("dbg_ref_printer", vec![inner]),
            TyData::Adt { def, args } if def == names.known.option => {
                ("dbg_option_printer", pool.list_items(args).to_vec())
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
            _ if target(cx, ty).is_some() => ("dbg_structural", vec![ty]),
            _ => ("dbg_opaque_printer", vec![ty]),
        }
    };
    let def = names.item(FORMAT, name);
    if name != "dbg_opaque_printer" {
        return selected(g, block, def, &args, ft);
    }
    let value = {
        // A trait value, printed behind its reference, shows the trait it
        // implements and not its concrete type (`module.dbg.value.trait-value`).
        let shown = hd_resolve::show_ty_full(names, ty);
        let shown = match pool.get(ty) {
            TyData::TraitValue { .. } => format!("dyn {shown}"),
            _ => shown,
        };
        let text = g.b.const_str(&shown);
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
    };
    let body = g.b.close_block(block, Some(value), ft, SYN);
    g.finish(body).map(|(body, _)| body)
}

/// The body that returns `def[args]` as a function value of type `ft`, in
/// `g`'s open `block`.
fn selected(
    mut g: Gen<'_, '_>,
    block: BlockMark,
    def: DefId,
    args: &[Ty],
    ft: Ty,
) -> StageResult<Body> {
    let a = g.b.refs_record(&[Ref(def.raw())]);
    let bw = g.b.refs_record(&[Ref(g.names.pool.list(args).0)]);
    let value = g.b.emit(Tag::ItemRef, a, bw, ft, SYN);
    let body = g.b.close_block(block, Some(value), ft, SYN);
    g.finish(body).map(|(body, _)| body)
}

/// Select `ty`'s `Wire` encoder (`wire_put`) or, with `take`, its decoder
/// (`wire_take`) among `std.remote`'s codec functions
/// (`module.remote.codec`); no value is encoded or decoded here. A struct,
/// enum, tuple, union or newtype selects the private `WireStructural`
/// template, whose members select their own.
pub fn codec_body(cx: &Cx<'_, '_>, item: DefId, ty: Ty, take: bool) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    let mut g = Gen::new(cx, ty, item, Handles::Supplied);
    g.b = TirBuilder::new(item, BodyKind::Fn);
    let local = |inner, mutable| {
        pool.intern_ty(&TyData::Ref {
            inner,
            mutable,
            counted: false,
        })
    };
    let ft = if take {
        let input = local(g.adt(names.item(REMOTE, "WireIn"), &[]), true);
        g.fn_ty(&[input], g.adt(names.known.option, &[ty]))
    } else {
        let out = local(g.adt(names.item(REMOTE, "WireOut"), &[]), true);
        g.fn_ty(&[local(ty, false), out], Ty::VOID)
    };
    let block = g.b.open_block();
    let (name, args) = match pool.get(ty) {
        TyData::Prim(Prim::Bool) => ("bool", vec![]),
        TyData::Prim(Prim::Char) => ("char", vec![]),
        TyData::Prim(Prim::F32) => ("f32", vec![]),
        TyData::Prim(Prim::F64) => ("f64", vec![]),
        TyData::Prim(Prim::String) => ("string", vec![]),
        TyData::Prim(_) => ("int", vec![ty]),
        TyData::Never => ("never", vec![]),
        TyData::Adt { def, args } if def == names.known.list => {
            let items = pool.list_items(args);
            if items == [Ty::prim(Prim::U8)] {
                ("bytes", vec![])
            } else {
                ("list", items.to_vec())
            }
        }
        TyData::Adt { def, args } if def == names.known.map => {
            ("map", pool.list_items(args).to_vec())
        }
        _ if target(cx, ty).is_some() => ("structural", vec![ty]),
        // gap: #2288 — a remote reference crosses as its home, interface,
        // object and token (R3), and a counted reference only at the host
        // boundary, which keeps its own codecs until the boundary move;
        // module.marker.table rules out every other type for `Wire`.
        _ => {
            return Err(NotImplemented::new(
                cx.stage,
                format!(
                    "no `Wire` codec for `{}` yet (#2288)",
                    hd_resolve::show_ty_full(names, ty)
                ),
            ));
        }
    };
    let side = if take { "take" } else { "put" };
    let def = names.item(REMOTE, &format!("wire_{side}_{name}"));
    selected(g, block, def, &args, ft)
}

/// The body of `debug_texts[args]` (`module.dbg.texts`): its parameters are
/// the tuple `values`, the `columns` each element starts at, and the hidden
/// call-site record `$site` (`fn.caller.site-argument`). Each element's text is
/// `dbg_layout` at its column with the printer selected for its type; a
/// function's names the function its argument names
/// (`module.dbg.value.function`); a runtime rest's items follow through
/// `dbg_rest_texts`. Cost: one `dbg_layout` call per element, and one
/// record split per element of a function type.
pub fn texts_body(cx: &Cx<'_, '_>, item: DefId, args: Ty) -> StageResult<Body> {
    let names = cx.names;
    let pool = names.pool;
    // ruled out: debug_texts's sealed Args < Tuple bound excludes non-tuples.
    let (elems, rest) = pool.tuple_parts(args);
    let elems = pool.list_items(elems).to_vec();
    let mut g = Gen::new(cx, args, item, Handles::Supplied);
    g.b = TirBuilder::new(item, BodyKind::Fn);
    let usize = Ty::prim(hd_types::Prim::Usize);
    let columns_ty = g.adt(names.known.list, &[usize]);
    let texts_ty = g.adt(names.known.list, &[Ty::STRING]);
    let param = hd_tir::ir::local_flags::PARAM;
    let values = g.b.local(args, names.syms.intern("values"), param, SYN);
    let columns =
        g.b.local(columns_ty, names.syms.intern("columns"), param, SYN);
    let site =
        g.b.local(Ty::STRING, names.syms.intern("$site"), param, SYN);
    let blk = g.b.open_block();
    let call = |g: &mut Gen<'_, '_>, def: DefId, targs: &[Ty], refs: &[Ref], ret: Ty| {
        let c = Callee::Item {
            def,
            targs: pool.list(targs),
        };
        g.b.call(&c, refs, Providers::None, ret, SYN)
    };
    let mut texts = Vec::with_capacity(elems.len());
    // ruled out: interned type-list lengths and TIR field indices are u32.
    debug_assert!(u32::try_from(elems.len()).is_ok());
    let mut position = 0u32;
    for e in &elems {
        let e = *e;
        let tuple = g.b.get(values, args, SYN);
        let v = g.b.field(tuple, position, e, SYN);
        let at = g.b.const_value(usize, u64::from(position));
        position += 1;
        let cols = g.b.get(columns, columns_ty, SYN);
        let column = call(
            &mut g,
            names.item(FORMAT, "dbg_column"),
            &[],
            &[cols, at],
            usize,
        );
        let show = g.printer(e);
        let layout = names.item(FORMAT, "dbg_layout");
        let mut text = call(&mut g, layout, &[e], &[column, v, show], Ty::STRING);
        if matches!(pool.get(e), TyData::Fn { .. }) {
            let record = g.b.get(site, Ty::STRING, SYN);
            let site_text = names.known.site_function_text;
            text = call(&mut g, site_text, &[], &[record, at, text], Ty::STRING);
        }
        texts.push(text);
    }
    let rec = g.b.refs_record(&texts);
    let mut list = g.b.emit(Tag::NewList, hd_tir::ir::NONE, rec, texts_ty, SYN);
    if let Some(rest_type) = rest {
        let (def, args) = pool.adt_parts(rest_type);
        // ruled out: types.tuple.rest.list fixes the rest to List[T].
        debug_assert_eq!(def, names.known.list);
        let item = pool.list_items(args)[0];
        let tuple = g.b.get(values, g.target, SYN);
        let tail = g.b.field(tuple, position, rest_type, SYN);
        let cols = g.b.get(columns, columns_ty, SYN);
        let show = g.printer(item);
        let rest_texts = names.item(FORMAT, "dbg_rest_texts");
        list = call(
            &mut g,
            rest_texts,
            &[item],
            &[list, cols, tail, show],
            texts_ty,
        );
    }
    let root = g.b.close_block(blk, Some(list), texts_ty, SYN);
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
