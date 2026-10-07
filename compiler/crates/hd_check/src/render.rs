//! A readable rendering of a checked body, for golden tests: items and
//! types by their stable names, so the text does not depend on run IDs.

use std::fmt::Write as _;

use hd_resolve::{Names, show_ty};
use hd_tir::Body;
use hd_tir::ir::{Callee, NONE, Op, Ref, Tag};
use hd_types::{Ty, TyList};

fn ty(names: &Names<'_>, t: Ty) -> String {
    show_ty(names, t)
}

fn list(names: &Names<'_>, l: TyList) -> String {
    names
        .pool
        .list_items(l)
        .into_iter()
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
    format!("{bits}:{}", ty(names, t))
}

/// One line per local, sub-body and instruction.
#[must_use]
pub fn render(names: &Names<'_>, b: &Body) -> String {
    let mut o = String::new();
    let _ = writeln!(o, "{} ({:?})", names.path(b.item), b.kind);
    for i in 0..b.local_ty.len() {
        let _ = writeln!(
            o,
            "  local ${i} {}: {} flags={}",
            names.text(b.local_name[i]),
            ty(names, b.local_ty[i]),
            b.local_flags[i]
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
    for i in 0..b.len() {
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
        } else {
            for (word, op) in [(a, tag.operands().0), (w, tag.operands().1)] {
                match op {
                    Op::None => {}
                    Op::Value | Op::Block => ops.push(value(names, b, word)),
                    Op::Label => ops.push(format!("L{word}")),
                    Op::Local => ops.push(format!("${word}")),
                    Op::Meta => ops.push(format!("#{word}")),
                    Op::Values | Op::Blocks | Op::Record => {
                        if word == NONE {
                            ops.push("-".into());
                        } else if tag == Tag::ProviderGet || tag == Tag::ItemRef {
                            ops.push("[..]".into());
                        } else if matches!(tag, Tag::SwitchTag | Tag::Payload | Tag::FieldSet) {
                            // Variant and field numbers stay numbers.
                            let r = b.record(word);
                            let vals: Vec<String> = r
                                .iter()
                                .enumerate()
                                .map(|(k, x)| {
                                    let number = match tag {
                                        Tag::SwitchTag => k % 2 == 0 && k + 1 < r.len(),
                                        Tag::Payload => true,
                                        _ => k == 0,
                                    };
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
    o
}
