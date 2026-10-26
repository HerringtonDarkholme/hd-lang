//! The operand-slot schema: what each word of an instruction's operands
//! names, read from the tag table ([`Tag::operands`]) and the layout of
//! each record. The passes' uses ([`crate::own::operands`]), the splice's
//! renumbering, the blocks an instruction runs ([`super::for_blocks`]),
//! [`Body::value_operands`], the verifier's constant rows and the
//! emitter's alias check all read operands through [`slots`].

use super::{Body, IntrinsicOp, NONE, Op, Tag, place};
use Layout as L;

/// How an instruction uses one of its value operands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// The use reads the value and keeps nothing: an operand of `Prim`, a
    /// place's root or element index, a borrowed argument.
    Lend,
    /// The use takes the value's count: a stored value (a list push's and
    /// a map store's too), a construction part, a `Return` or `Break`
    /// value.
    Consume,
    /// Argument `k` of a call, which consumes it when its parameter is a
    /// value parameter ([`crate::own::OwnCx::pass`]).
    Arg(u32),
    /// A block's tail, which its block's value takes when the block's
    /// owner uses that value.
    Tail,
    /// The place of a `Drop`: no use, the value's end.
    Dropped,
}

/// Where an operand word sits, so a pass can rewrite it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Loc {
    /// Data word `k` of the instruction.
    Data(u8),
    /// A word of `extra`.
    Extra(u32),
}

/// What an operand word names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Word {
    /// A value, an instruction's or a constant, and how it is used.
    Value(Role),
    /// A provider's value: a call's, or one a `With` or `ContextNew` binds.
    /// Lent.
    Provider,
    /// A block the instruction runs.
    Block,
    /// An instruction that is no operand value: a block's statement, the
    /// one a `Hook` observes.
    Inst,
}

/// The slots of one data word.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// None: a `None`, `Meta` or `Label` word, or a record of IDs.
    Opaque,
    /// The word itself.
    One(Word),
    /// A place word's value root and element indices.
    Place(Role),
    /// A record, each word the same: values, blocks or statements.
    Each(Word),
    /// A record of arguments.
    Args,
    /// A call's record: its arguments, then three provider words: none
    /// (0), `(key, value)` pairs in an `extra` range (1), or a context
    /// value (2).
    Call,
    /// A record of `(key, value)` pairs.
    Pairs(Word),
    /// A switch's record: `(key, Block)` cases, then a default or `NONE`.
    Cases,
    /// An intrinsic's operands, lent but for the value a list push or a
    /// map store keeps.
    Stored,
}

/// The layout of each data word of `tag`: its [`Op`]'s, but where the
/// tag's role or record says more.
fn layout(tag: Tag) -> [Layout; 2] {
    let plain = |op| match op {
        Op::Value => L::One(Word::Value(Role::Lend)),
        Op::Block => L::One(Word::Block),
        Op::Place => L::Place(Role::Lend),
        Op::Values => L::Each(Word::Value(Role::Lend)),
        Op::Blocks => L::Each(Word::Block),
        _ => L::Opaque,
    };
    let (a, b) = (plain(tag.operands().0), plain(tag.operands().1));
    let (consume, lend) = (Word::Value(Role::Consume), Word::Value(Role::Lend));
    match tag {
        Tag::Block => [L::Each(Word::Inst), L::One(Word::Value(Role::Tail))],
        Tag::Drop => [L::Place(Role::Dropped), b],
        Tag::Store | Tag::Break => [a, L::One(consume)],
        Tag::Return => [L::One(consume), b],
        Tag::Call | Tag::Await => [a, L::Call],
        Tag::CallValue => [a, L::Args],
        Tag::NewStruct | Tag::NewVariant | Tag::NewMap | Tag::NewList => [a, L::Each(consume)],
        Tag::NewTuple => [L::One(consume), L::Each(consume)],
        Tag::CopyStruct => [a, L::Pairs(consume)],
        Tag::With | Tag::ContextNew => [L::Pairs(Word::Provider), b],
        Tag::SwitchTag | Tag::SwitchInt | Tag::SwitchChar | Tag::SwitchStr => [a, L::Cases],
        Tag::Intrinsic => [a, L::Stored],
        Tag::Interp => [a, L::Each(lend)],
        // A `defer`'s suite and a `Hook`'s observed instruction sit in
        // meta words.
        Tag::Defer => [L::One(Word::Block), b],
        Tag::Hook => [a, L::One(Word::Inst)],
        _ => [a, b],
    }
}

/// Calls `f` with each slot of instruction `i`: its position, its word and
/// what the word names, data word `a`'s slots before `b`'s (a tuple's
/// elements before its first operand), each in emission order. A `NONE`
/// data word has none.
pub fn slots(b: &Body, i: usize, mut f: impl FnMut(Loc, u32, Word)) {
    let tag = b.tags[i];
    let mut layouts = layout(tag);
    // A coercion that keeps its operand's values takes its count.
    if b.same_value_coercion(i) {
        layouts[0] = L::One(Word::Value(Role::Consume));
    }
    for k in if tag == Tag::NewTuple { [1, 0] } else { [0, 1] } {
        let word = b.data[i][k];
        if word == NONE {
            continue;
        }
        let at = |x: u32| b.extra.get(x as usize).copied().unwrap_or(0);
        let (rec, n) = (word + 1, at(word));
        let mut put = |x: u32, slot| {
            if let Some(&v) = b.extra.get(x as usize) {
                f(Loc::Extra(x), v, slot);
            }
        };
        match layouts[k] {
            L::Opaque => {}
            L::One(w) => f(Loc::Data(u8::from(k == 1)), word, w),
            L::Place(r) if word & place::BARE_LOCAL == 0 => {
                place_words(b, word, |v, x| f(x, v, Word::Value(r)));
            }
            L::Place(_) => {}
            L::Each(w) => (0..n).for_each(|x| put(rec + x, w)),
            L::Pairs(w) => (1..n).step_by(2).for_each(|x| put(rec + x, w)),
            // The odd words, and the last.
            L::Cases => (1..n)
                .step_by(2)
                .chain(n.checked_sub(1).filter(|_| n % 2 == 1))
                .for_each(|x| put(rec + x, Word::Block)),
            L::Stored => {
                let kept = match IntrinsicOp::from_u32(b.data[i][0]) {
                    Some(IntrinsicOp::ListPush) => 1,
                    Some(IntrinsicOp::MapSet) => 2,
                    _ => NONE,
                };
                for x in 0..n {
                    let role = if x == kept { Role::Consume } else { Role::Lend };
                    put(rec + x, Word::Value(role));
                }
            }
            L::Args | L::Call => {
                let args = if layouts[k] == L::Call {
                    n.saturating_sub(3)
                } else {
                    n
                };
                (0..args).for_each(|x| put(rec + x, Word::Value(Role::Arg(x))));
                let p = rec + args;
                match at(p) {
                    _ if layouts[k] == L::Args || n < 3 => {}
                    1 => (1..at(p + 2))
                        .step_by(2)
                        .for_each(|y| put(at(p + 1) + y, Word::Provider)),
                    2 => put(p + 1, Word::Provider),
                    _ => {}
                }
            }
        }
    }
}

/// The words of instruction `i`'s slots that `keep` keeps, in [`slots`]
/// order.
pub fn slot_words(b: &Body, i: usize, keep: impl Fn(Word) -> bool) -> Vec<u32> {
    let mut out = Vec::new();
    slots(b, i, |_, v, s| {
        if keep(s) {
            out.push(v);
        }
    });
    out
}

/// The value words of the place record at `at`: a value or pointer root,
/// then each element index, with their positions.
fn place_words(b: &Body, at: u32, mut f: impl FnMut(u32, Loc)) {
    let Some(&n) = b.extra.get(at as usize) else {
        return;
    };
    let rec = at + 1;
    let end = rec + n;
    let word = |x: u32| b.extra.get(x as usize).copied().unwrap_or(0);
    let mut x = match word(rec) {
        place::ROOT_VALUE | place::ROOT_PTR => {
            f(word(rec + 1), Loc::Extra(rec + 1));
            rec + 2
        }
        place::ROOT_GLOBAL => rec + 3,
        _ => rec + 2,
    };
    while x < end {
        if word(x) & place::PROJ_KIND == place::PROJ_ELEM {
            f(word(x + 1), Loc::Extra(x + 1));
            x += 2;
        } else {
            x += 1;
        }
    }
}
