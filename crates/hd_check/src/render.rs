//! A readable rendering of a checked body, for golden tests and the stage
//! fixtures' `tir` view: items, types and fields by their names, so the
//! text does not depend on run IDs or on the order fields are declared in;
//! a `Block` shows its result, not the list of its instructions.

use std::fmt::Write as _;

use hd_resolve::{Names, show_ty};
use hd_tir::Body;
use hd_tir::ir::{Callee, Coercion, NONE, Op, Ref, Tag};
use hd_types::{Ty, TyList};

fn ty(names: &Names<'_>, t: Ty) -> String {
    show_ty(names, t)
}

fn list(names: &Names<'_>, l: TyList) -> String {
    names
        .pool
        .list_items(l)
        .iter()
        .copied()
        .map(|t| ty(names, t))
        .collect::<Vec<_>>()
        .join(", ")
}

fn value(names: &Names<'_>, b: &Body, w: u32) -> String {
    if w == NONE {
        return "-".into();
    }
    let r = Ref(w);
    if r.as_inst().is_some() {
        return format!("%{w}");
    }
    let Some(&(t, bits)) = b.consts.get((w & !Ref::CONST_BIT) as usize) else {
        return "?const".into();
    };
    if t == Ty::STRING {
        let s = usize::try_from(bits)
            .ok()
            .and_then(|i| b.strings.get(i))
            .map_or("?", |s| s.as_ref());
        return format!("{s:?}");
    }
    // A float by its value, not its bits.
    match names.pool.get(t) {
        hd_types::TyData::Prim(hd_types::Prim::F64) => format!("{:?}:f64", f64::from_bits(bits)),
        hd_types::TyData::Prim(hd_types::Prim::F32) => {
            format!(
                "{:?}:f32",
                f32::from_bits(u32::try_from(bits).unwrap_or_default())
            )
        }
        _ => format!("{bits}:{}", ty(names, t)),
    }
}

/// A `Coerce` record: the kind by name, then the evidence word.
fn coercion(names: &Names<'_>, b: &Body, word: u32) -> String {
    let r = b.record(word);
    let kinds = [
        Coercion::Never,
        Coercion::Weaken,
        Coercion::Variance,
        Coercion::WrapSome,
        Coercion::RowSubsume,
        Coercion::ToTraitValue,
        Coercion::ToAny,
        Coercion::Supertrait,
        Coercion::SuspendFnToCtor,
        Coercion::Identity,
        Coercion::RefToTraitValue,
        Coercion::RefAsProvider,
        Coercion::ProviderAsRef,
    ];
    let kind = r
        .first()
        .and_then(|&k| kinds.iter().find(|c| **c as u32 == k))
        .map_or_else(|| "?kind".to_owned(), |c| format!("{c:?}"));
    let evidence = r.get(1).map_or("-".to_owned(), |&e| value(names, b, e));
    format!("[{kind} {evidence}]")
}

/// The name and type of field `k` of a value of type `t`, when `t` has
/// named fields: what [`render`] prints a field by.
pub type FieldNames<'a> = dyn Fn(Ty, u32) -> Option<(String, Ty)> + 'a;

/// Field `k` of a value of type `t`: `.name`, or `.k` when `t` names none
/// (a tuple).
fn field(fields: &FieldNames<'_>, t: Ty, k: u32) -> String {
    match fields(t, k) {
        Some((name, _)) => format!(".{name}"),
        None => format!(".{k}"),
    }
}

/// The type of value `w` of `b`: an instruction's, or a constant's.
fn type_of(b: &Body, w: u32) -> Option<Ty> {
    match Ref(w).as_inst() {
        Some(i) => b.ty.get(i.0 as usize).copied(),
        None => b.consts.get((w & !Ref::CONST_BIT) as usize).map(|c| c.0),
    }
}

/// The type a dereference of a value of type `t` reaches: a reference's
/// referent, before or after the checker lowers it to `std.core.Ref[T]`;
/// a compiler-supplied data object's payload is the object's own type.
fn deref_ty(names: &Names<'_>, t: Ty) -> Ty {
    use hd_types::TyData;
    let strip = |t: Ty| match names.pool.get(t) {
        TyData::Mut(inner) => inner,
        _ => t,
    };
    match names.pool.get(strip(t)) {
        TyData::Ref { inner, .. } => inner,
        TyData::Adt { def, args } if def == names.known.ref_cell => names
            .pool
            .list_items(args)
            .first()
            .map_or(t, |inner| strip(*inner)),
        _ => t,
    }
}

/// A place operand (`hd_tir::ir::place`), as `[*$0 .x [%5]]`: fields by
/// name under the type each step reaches, variants by number.
fn place(names: &Names<'_>, b: &Body, word: u32, fields: &FieldNames<'_>) -> String {
    use hd_tir::ir::{PlaceRoot, Proj};
    let Some(p) = b.decode_place(word) else {
        return "?place".into();
    };
    let (root, mut at) = match p.root {
        PlaceRoot::Local(l) => (format!("${}", l.raw()), b.local_ty.get(l.idx()).copied()),
        PlaceRoot::Global(d, t) => (format!("@{}", names.path(d)), Some(t)),
        PlaceRoot::Value(r) => (value(names, b, r.0), type_of(b, r.0)),
        PlaceRoot::Frame(s) => (format!("^{s}"), None),
    };
    let lead = p.proj.iter().take_while(|x| **x == Proj::Deref).count();
    let mut out = format!("[{}{root}", "*".repeat(lead));
    for (k, step) in p.proj.iter().enumerate() {
        match *step {
            Proj::Deref => {
                if k >= lead {
                    out.push_str(" *");
                }
                at = at.map(|t| deref_ty(names, t));
            }
            Proj::Field(f) => {
                out.push(' ');
                match at {
                    Some(t) => out.push_str(&field(fields, t, f)),
                    None => {
                        let _ = write!(out, ".{f}");
                    }
                }
                at = at.and_then(|t| fields(t, f)).map(|(_, ft)| ft);
            }
            Proj::Elem(r) => {
                let _ = write!(out, " [{}]", value(names, b, r.0));
                at = at.and_then(|t| match names.pool.get(t) {
                    hd_types::TyData::Adt { args, .. } => {
                        names.pool.list_items(args).first().copied()
                    }
                    _ => None,
                });
            }
            Proj::Variant(v) => {
                let _ = write!(out, " as #{v}");
                at = None;
            }
        }
    }
    out.push(']');
    out
}

/// A local's flags by name, as ` [param read]`; empty when none is set.
fn local_flag_names(flags: u8) -> String {
    use hd_tir::ir::local_flags as f;
    let names: Vec<&str> = [
        (f::PARAM, "param"),
        (f::SHORT, ":="),
        (f::READ, "read"),
        (f::ASSIGNED, "assigned"),
        (f::MUTATED, "mutated"),
        (f::CAPTURED, "captured"),
        (f::CAPTURED_ASSIGNED, "captured-assigned"),
        (f::NOT_YET, "not-yet"),
    ]
    .into_iter()
    .filter(|(bit, _)| flags & bit != 0)
    .map(|(_, name)| name)
    .collect();
    if names.is_empty() {
        String::new()
    } else {
        format!(" [{}]", names.join(" "))
    }
}

/// One line per local, sub-body and instruction, then the ownership
/// columns when the body has them. `fields` names the fields of a type.
#[must_use]
pub fn render(names: &Names<'_>, b: &Body, fields: &FieldNames<'_>) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "{} ({:?})", names.path(b.item), b.kind);
    for i in 0..b.local_ty.len() {
        let _ = writeln!(
            o,
            "  local ${i} {}: {}{}",
            names.text(b.local_name[i]),
            ty(names, b.local_ty[i]),
            local_flag_names(b.local_flags[i])
        );
    }
    for s in 1..b.sub_root.len() {
        let caps: Vec<String> = b
            .cap_local
            .iter()
            .zip(&b.cap_mode)
            .map(|(l, m)| format!("${} {m:?}", l.raw()))
            .collect();
        let _ = writeln!(
            o,
            "  sub {s} root %{} captures [{}]",
            b.sub_root[s],
            caps.join(" ")
        );
    }
    // An instruction that no block lists and no instruction reads is not
    // computed: it is not shown.
    let mut read = vec![false; b.len()];
    for &r in &b.sub_root {
        if let Some(x) = read.get_mut(r as usize) {
            *x = true;
        }
    }
    if let Some(x) = read.last_mut() {
        *x = true;
    }
    // Operands come before their readers, so one pass from the end finds
    // every instruction a shown one reads.
    for i in (0..b.len()).rev() {
        if !read[i] {
            continue;
        }
        let [a, w] = b.data[i];
        for (word, op) in [(a, b.tags[i].operands().0), (w, b.tags[i].operands().1)] {
            let words: Vec<u32> = match op {
                Op::Value | Op::Block => vec![word],
                // A `defer`'s suite and a `Hook`'s observed instruction
                // sit in meta words.
                Op::Meta if matches!(b.tags[i], Tag::Defer | Tag::Hook) => vec![word],
                Op::Values | Op::Blocks | Op::Record if word != NONE => b.record(word).to_vec(),
                Op::Place => b.place_values(word),
                _ => Vec::new(),
            };
            for v in words {
                if Ref(v).as_inst().is_some()
                    && let Some(x) = read.get_mut(v as usize)
                {
                    *x = true;
                }
            }
        }
    }
    for (i, &shown) in read.iter().enumerate() {
        if !shown {
            continue;
        }
        let tag = b.tags[i];
        let [a, w] = b.data[i];
        let mut ops = Vec::new();
        if tag == Tag::Call || tag == Tag::Await {
            let words = b.record(a);
            match Callee::from_words(words) {
                Some(Callee::Item { def, targs }) => {
                    ops.push(format!("{}[{}]", names.path(def), list(names, targs)));
                }
                Some(Callee::TraitMethod {
                    trait_,
                    method,
                    self_ty,
                    targs,
                    choice,
                }) => {
                    let _ = trait_;
                    ops.push(format!(
                        "{} for {} [{}] via {:?}",
                        names.path(method),
                        ty(names, self_ty),
                        list(names, targs),
                        choice.0
                    ));
                }
                None => ops.push("?callee".into()),
            }
            let args = b.record(w);
            let n = args.len().saturating_sub(3);
            let vals: Vec<String> = args[..n].iter().map(|x| value(names, b, *x)).collect();
            ops.push(format!("({})", vals.join(", ")));
        } else if tag == Tag::CopyStruct {
            // `base` with each replaced field `.name = value`.
            ops.push(value(names, b, a));
            let t = type_of(b, a);
            let pairs: Vec<String> = b
                .record(w)
                .chunks(2)
                .map(|p| match (p, t) {
                    ([k, v], Some(t)) => {
                        format!("{} = {}", field(fields, t, *k), value(names, b, *v))
                    }
                    _ => "?".to_owned(),
                })
                .collect();
            ops.push(format!("[{}]", pairs.join(", ")));
        } else if matches!(tag, Tag::Read | Tag::Ref) {
            // `Ref mut [$0]`, `Read [*$0 .x]`; a read mode other than the
            // checker's `use` before the place, as `Read copy [$1]`.
            let mode = match tag {
                Tag::Ref if w == 1 => Some("mut".to_owned()),
                Tag::Read => hd_tir::ir::ReadMode::from_u32(w)
                    .filter(|m| *m != hd_tir::ir::ReadMode::Use)
                    .map(|m| format!("{m:?}").to_lowercase()),
                _ => None,
            };
            ops.extend(mode);
            ops.push(place(names, b, a, fields));
        } else if tag == Tag::Block {
            // A block's result; its instructions are the lines above.
            ops.push(value(names, b, w));
        } else {
            for (side, (word, op)) in [(a, tag.operands().0), (w, tag.operands().1)]
                .into_iter()
                .enumerate()
            {
                match op {
                    Op::None => {}
                    Op::Value | Op::Block => ops.push(value(names, b, word)),
                    Op::Label => ops.push(format!("L{word}")),
                    Op::Place => ops.push(place(names, b, word, fields)),
                    // A `Prim`'s or an `Intrinsic`'s operation by name.
                    Op::Meta
                        if tag == Tag::Prim
                            && let Some(op) = hd_tir::ir::PrimOp::from_u32(word) =>
                    {
                        ops.push(format!("{op:?}"));
                    }
                    Op::Meta
                        if tag == Tag::Intrinsic
                            && let Some(op) = hd_tir::ir::IntrinsicOp::from_u32(word) =>
                    {
                        ops.push(format!("{op:?}"));
                    }
                    Op::Meta => ops.push(format!("#{word}")),
                    Op::Values | Op::Blocks | Op::Record => {
                        if word == NONE {
                            ops.push("-".into());
                        } else if tag == Tag::NewList && side == 0 {
                            // The positions of the spread elements.
                            let ks: Vec<String> =
                                b.record(word).iter().map(|x| format!("#{x}")).collect();
                            ops.push(format!("[{}]", ks.join(" ")));
                        } else if tag == Tag::ProviderGet || tag == Tag::ItemRef {
                            ops.push("[..]".into());
                        } else if tag == Tag::Coerce {
                            ops.push(coercion(names, b, word));
                        } else if tag == Tag::SwitchTag {
                            // Variant numbers stay numbers.
                            let r = b.record(word);
                            let vals: Vec<String> = r
                                .iter()
                                .enumerate()
                                .map(|(k, x)| {
                                    let number = k % 2 == 0 && k + 1 < r.len();
                                    if number {
                                        format!("#{x}")
                                    } else {
                                        value(names, b, *x)
                                    }
                                })
                                .collect();
                            ops.push(format!("[{}]", vals.join(" ")));
                        } else {
                            let vals: Vec<String> =
                                b.record(word).iter().map(|x| value(names, b, *x)).collect();
                            ops.push(format!("[{}]", vals.join(" ")));
                        }
                    }
                }
            }
        }
        let _ = writeln!(
            o,
            "  %{i} = {} {} : {}",
            tag.name(),
            ops.join(" "),
            ty(names, b.ty[i])
        );
    }
    // The ownership passes' exit set, per sub-body (`Body::exit_drops`).
    for s in 0..b.sub_root.len() {
        let locals: Vec<String> = b
            .exit_drops
            .iter()
            .filter(|(sub, _)| sub.idx() == s)
            .map(|(_, l)| format!("${}", l.raw()))
            .collect();
        if !locals.is_empty() {
            let _ = writeln!(o, "  exit sub {s} drops [{}]", locals.join(" "));
        }
    }
    o
}
