//! The TIR printer: one line per instruction, for tests and debugging
//! (checking-and-tir.md §4.13.11). Types print by content, constants by
//! value, so the text holds no run ID that a reader would need.

use std::fmt::Write as _;

use crate::world::{DefId, Ty, World};
use crate::{CALLEE_ITEM, CHOICE_IMPL, CONST_BIT, NONE, PrimOp, TirBody, TirTag};

fn operand(w: &World, r: u32) -> String {
    if r == NONE {
        "_".to_owned()
    } else if r & CONST_BIT != 0 {
        let (t, bits) = w.const_value(r & !CONST_BIT);
        format!("{bits}:{}", w.display(t))
    } else {
        format!("%{r}")
    }
}

fn operands(w: &World, b: &TirBody, at: u32) -> String {
    b.get_list(at)
        .iter()
        .map(|&r| operand(w, r))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Prints a body as text.
#[must_use]
pub fn print_body(w: &World, b: &TirBody) -> String {
    let mut out = String::new();
    if let Some(item) = b.item {
        let _ = writeln!(out, "body {}", w.path(item));
    }
    for (l, ty) in b.local_ty.iter().enumerate() {
        let _ = writeln!(
            out,
            "  local {l} {}: {}{}",
            w.text(b.local_name[l]),
            w.display(*ty),
            if b.local_flags[l] & crate::LOCAL_PARAM != 0 {
                " param"
            } else {
                ""
            }
        );
    }
    for i in 0..b.tags.len() {
        let [a, bb] = b.data[i];
        let ty = w.display(b.ty[i]);
        let text = match b.tags[i] {
            TirTag::LocalGet => format!("LocalGet l{a}"),
            TirTag::LocalSet => format!("LocalSet l{a} = {}", operand(w, bb)),
            TirTag::Prim => format!("Prim {:?}({})", PrimOp::from_u32(a), operands(w, b, bb)),
            TirTag::Intrinsic => format!("Intrinsic #{a}({})", operands(w, b, bb)),
            TirTag::NewData => format!("NewData({})", operands(w, b, bb)),
            TirTag::Field => format!("Field {}.{bb}", operand(w, a)),
            TirTag::Call => {
                let rec = b.get_list(a);
                let callee = if rec[0] == CALLEE_ITEM {
                    let args: Vec<String> = rec[2..].iter().map(|&t| w.display(Ty(t))).collect();
                    format!("{}[{}]", w.path(DefId(rec[1])), args.join(", "))
                } else {
                    let choice = if rec[4] == CHOICE_IMPL {
                        w.path(DefId(rec[5])).to_owned()
                    } else {
                        format!("bound T{}", rec[5])
                    };
                    format!(
                        "{}#{} at {} by {choice}",
                        w.path(DefId(rec[1])),
                        rec[2],
                        w.display(Ty(rec[3]))
                    )
                };
                format!("Call {callee}({})", operands(w, b, bb))
            }
            TirTag::Block => format!("Block [{}] tail {}", operands(w, b, a), operand(w, bb)),
            TirTag::If => {
                let arms = b.get_list(bb);
                format!(
                    "If {} then %{} else {}",
                    operand(w, a),
                    arms[0],
                    operand(w, arms[1])
                )
            }
            TirTag::Loop => format!("Loop %{a}"),
            TirTag::Break => format!("Break label {a}"),
            TirTag::Return => format!("Return {}", operand(w, a)),
        };
        let _ = writeln!(out, "  %{i} = {text} : {ty}");
    }
    let roots: Vec<String> = b.sub_root.iter().map(|r| format!("%{}", r.0)).collect();
    let _ = writeln!(out, "  roots {} rep {:?}", roots.join(", "), b.rep_exact);
    out
}
