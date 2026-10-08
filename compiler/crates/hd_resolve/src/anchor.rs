//! Declaration-relative source positions for interface items
//! (data-structures.md §3.7 "Token anchors", resolution-and-interfaces.md
//! §4.10 `anchors`). A folder interface keeps no byte offsets: an anchor
//! names the top-level declaration by its place among the file's
//! declarations, an optional member of its block, and a token range inside
//! that node. Whitespace and comments are not tokens, and a function body
//! can change without moving a header token, so an edit that leaves the
//! API text alone leaves every anchor valid. Anchors sit outside every API
//! hash; they only place diagnostics.
//!
//! Slots: 0 is the item's header; for functions, methods, data and enum
//! items slot `1 + i` is the `i`-th written type position, in the order
//! stage B checks them (parameters then result, or fields in order); a
//! newtype's slot 1 is its base type.

use std::collections::HashMap;

use hd_base::{DefId, Span};
use hd_intern::PathKind;
use hd_syntax::{NodeRef, SyntaxKind, TokenKind};

use crate::iface::{HeadKind, ImplKind, Item, ItemData, Names};
use crate::lower::Head;
use crate::view::Src;

/// A token range inside a declaration or one of its block members.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Anchor {
    /// Index among the root's children.
    pub decl: u32,
    /// 0 for the declaration itself, else 1 + the index among its block's children.
    pub member: u32,
    /// First token, counted from the first token of the declaration or member.
    pub tok: u32,
    /// Number of tokens.
    pub count: u32,
}

/// `(item, slot, anchor)` rows of one module.
pub type Anchors = Vec<(DefId, u32, Anchor)>;

struct Ctx<'a, 't> {
    src: &'a Src<'t>,
    out: Anchors,
    decl: u32,
}

impl<'t> Ctx<'_, 't> {
    /// An anchor for `node`, counted from `base`; `header` stops before the block.
    fn at(
        &self,
        base: NodeRef<'t>,
        member: u32,
        node: NodeRef<'t>,
        header: bool,
    ) -> Option<Anchor> {
        let lo = self.src.first(node);
        let mut hi = self.src.last(node);
        self.src.tkind(lo)?;
        self.src.tkind(hi)?;
        if header && let Some(b) = Src::child(node, SyntaxKind::Block) {
            let first = self.src.first(b).raw();
            if first > lo.raw() {
                hi = hd_base::TokenIdx::from_raw(first - 1);
            }
        }
        let base_lo = self.src.first(base).raw();
        Some(Anchor {
            decl: self.decl,
            member,
            tok: lo.raw().checked_sub(base_lo)?,
            count: hi.raw().checked_sub(lo.raw())? + 1,
        })
    }

    fn push(&mut self, def: DefId, slot: u32, a: Option<Anchor>) {
        if let Some(a) = a {
            self.out.push((def, slot, a));
        }
    }

    /// Parameters then the result of a function node.
    fn sig(&mut self, def: DefId, f: NodeRef<'t>, base: NodeRef<'t>, member: u32) {
        let mut slot = 1;
        if let Some(pl) = Src::child(f, SyntaxKind::ParameterList) {
            for p in pl.children().filter(|c| c.kind() == SyntaxKind::Parameter) {
                let node = Src::type_child(p).unwrap_or(p);
                let a = self.at(base, member, node, false);
                self.push(def, slot, a);
                slot += 1;
            }
        }
        if let Some(r) = Src::type_child(f) {
            let a = self.at(base, member, r, false);
            self.push(def, slot, a);
        }
    }

    /// The fields held by a block or parameter list, in `fields` order.
    fn fields(
        &mut self,
        def: DefId,
        holder: Option<NodeRef<'t>>,
        slot: &mut u32,
        base: NodeRef<'t>,
    ) {
        for f in holder.iter().flat_map(|h| h.children()) {
            if matches!(
                f.kind(),
                SyntaxKind::DataField | SyntaxKind::Parameter | SyntaxKind::EmbeddedField
            ) {
                let node = Src::type_child(f).unwrap_or(f);
                let a = self.at(base, 0, node, false);
                self.push(def, *slot, a);
                *slot += 1;
            }
        }
    }
}

/// The anchors of every item one module's heads produce.
#[must_use]
pub fn collect(names: &Names<'_>, src: &Src<'_>, heads: &[Head<'_>], items: &[Item]) -> Anchors {
    let order: HashMap<usize, u32> = src
        .root()
        .children()
        .enumerate()
        .map(|(i, c)| (c.index().idx(), u32::try_from(i).unwrap_or(u32::MAX)))
        .collect();
    let mut cx = Ctx {
        src,
        out: Vec::new(),
        decl: 0,
    };
    for h in heads {
        let Some(&decl) = order.get(&h.node.index().idx()) else {
            continue;
        };
        cx.decl = decl;
        let n = h.node;
        // This head's own rows start here.
        let start = cx.out.len();
        let a = cx.at(n, 0, n, true);
        cx.push(h.def, 0, a);
        match h.kind {
            HeadKind::Fn => cx.sig(h.def, n, n, 0),
            HeadKind::Data => {
                let mut slot = 1;
                cx.fields(h.def, Src::child(n, SyntaxKind::Block), &mut slot, n);
            }
            HeadKind::Newtype => {
                if let Some(base) = n.children().find(|c| c.kind().is_type()) {
                    let a = cx.at(n, 0, base, false);
                    cx.push(h.def, 1, a);
                }
            }
            HeadKind::Enum => {
                let mut slot = 1;
                cx.fields(
                    h.def,
                    Src::child(n, SyntaxKind::ParameterList),
                    &mut slot,
                    n,
                );
                let block = Src::child(n, SyntaxKind::Block);
                for v in block
                    .iter()
                    .flat_map(|b| b.children())
                    .filter(|c| c.kind() == SyntaxKind::EnumVariant)
                {
                    cx.fields(
                        h.def,
                        Src::child(v, SyntaxKind::ParameterList),
                        &mut slot,
                        n,
                    );
                }
            }
            HeadKind::Impl | HeadKind::Trait => {
                let block = Src::child(n, SyntaxKind::Block);
                for (i, f) in block.iter().flat_map(|b| b.children()).enumerate() {
                    if f.kind() != SyntaxKind::FnDecl {
                        continue;
                    }
                    let Some(t) = f
                        .name(&src.parse.tokens)
                        .or_else(|| src.name_after(f, TokenKind::KwFn))
                    else {
                        continue;
                    };
                    let def = names.member(h.def, PathKind::Member, src.text(t));
                    let member = u32::try_from(i + 1).unwrap_or(u32::MAX);
                    let a = cx.at(f, member, f, true);
                    cx.push(def, 0, a);
                    cx.sig(def, f, f, member);
                }
            }
            HeadKind::Alias => {}
        }
        if matches!(h.kind, HeadKind::Data | HeadKind::Enum | HeadKind::Newtype) {
            derived(&mut cx, names, h, items, start);
        }
    }
    cx.out
}

/// A derived implementation sits at the `@derive` argument list that names
/// it; its slots repeat those of the type it derives for, the rows of
/// `cx.out` from `start`.
fn derived(cx: &mut Ctx<'_, '_>, names: &Names<'_>, h: &Head<'_>, items: &[Item], start: usize) {
    let src = cx.src;
    for it in items {
        let ItemData::Impl { kind, self_ty, .. } = &it.data else {
            continue;
        };
        if *kind != ImplKind::Derived
            || !matches!(names.pool.get(*self_ty), hd_types::TyData::Adt { def, .. } if def == h.def)
        {
            continue;
        }
        let seg = names.text(it.name);
        let Some(tr) = seg
            .strip_prefix("derive ")
            .and_then(|s| s.split(" for ").next())
        else {
            continue;
        };
        let node = h
            .node
            .children()
            .filter(|c| c.kind() == SyntaxKind::Decorator)
            .find(|d| {
                src.tokens(*d).any(|t| src.text(t) == "derive")
                    && src.tokens(*d).any(|t| src.text(t) == tr)
            });
        if let Some(d) = node {
            let a = cx.at(h.node, 0, d, false);
            cx.push(it.def, 0, a);
            // The derived implementation also carries the positions of the
            // type's fields (or a newtype's base type), where a derived
            // trait's missing field implementation is reported even
            // when the private type itself has no interface item.
            let fields: Vec<(u32, Anchor)> = cx.out[start..]
                .iter()
                .filter(|(def, slot, _)| *def == h.def && *slot > 0)
                .map(|(_, slot, a)| (*slot, *a))
                .collect();
            for (slot, a) in fields {
                cx.push(it.def, slot, Some(a));
            }
        }
    }
}

/// `Anchor::decl` of a raw byte range: `tok` is its start and `count` its end.
/// For spans outside every declaration, such as a whole-file span.
pub const RAW: u32 = u32::MAX;

/// The anchor of a byte range inside one declaration (or one member of
/// its block); a range that spans several declarations stays a raw range.
#[must_use]
pub fn of_span(src: &Src<'_>, lo: u32, hi: u32) -> Anchor {
    let raw = Anchor {
        decl: RAW,
        member: 0,
        tok: lo,
        count: hi,
    };
    let toks = &src.parse.tokens;
    let covers = |n: NodeRef<'_>| {
        let (a, b) = (src.first(n), src.last(n));
        src.tkind(a).is_some()
            && src.tkind(b).is_some()
            && toks.span(a).0 <= lo
            && hi <= toks.span(b).1
            && lo < hi
    };
    let Some((decl, node)) = src.root().children().enumerate().find(|(_, c)| covers(*c)) else {
        return raw;
    };
    let (member, base) = Src::child(node, SyntaxKind::Block)
        .and_then(|b| b.children().enumerate().find(|(_, c)| covers(*c)))
        .map_or((0, node), |(i, c)| (i + 1, c));
    let first = src.first(base).raw();
    let last = src.last(base).raw();
    let Some(t_lo) = (first..=last).find(|t| toks.span(hd_base::TokenIdx::from_raw(*t)).1 > lo)
    else {
        return raw;
    };
    let Some(t_hi) = (t_lo..=last)
        .take_while(|t| toks.span(hd_base::TokenIdx::from_raw(*t)).0 < hi)
        .last()
    else {
        return raw;
    };
    Anchor {
        decl: u32::try_from(decl).unwrap_or(RAW),
        member: u32::try_from(member).unwrap_or(0),
        tok: t_lo - first,
        count: t_hi - t_lo + 1,
    }
}

/// The span an anchor names in the file it was taken from.
#[must_use]
pub fn resolve(src: &Src<'_>, a: &Anchor) -> Option<Span> {
    if a.decl == RAW {
        return Some(Span {
            file: src.file,
            lo: a.tok,
            hi: a.count,
        });
    }
    let decl = src.root().children().nth(a.decl as usize)?;
    let base = match a.member {
        0 => decl,
        m => Src::child(decl, SyntaxKind::Block)?
            .children()
            .nth(m as usize - 1)?,
    };
    let lo = hd_base::TokenIdx::from_raw(src.first(base).raw().checked_add(a.tok)?);
    let hi = hd_base::TokenIdx::from_raw(lo.raw().checked_add(a.count.checked_sub(1)?)?);
    src.tkind(lo)?;
    src.tkind(hi)?;
    Some(Span {
        file: src.file,
        lo: src.parse.tokens.span(lo).0,
        hi: src.parse.tokens.span(hi).1,
    })
}

const WORDS: usize = 7;

/// One module's anchors for the interface items, as `(item index, module
/// index, slot, anchor)` rows in a little-endian section.
#[must_use]
pub fn encode(rows: &[(u32, u32, u32, Anchor)]) -> Vec<u8> {
    let mut out = Vec::with_capacity(4 + rows.len() * WORDS * 4);
    out.extend_from_slice(&u32::try_from(rows.len()).unwrap_or(u32::MAX).to_le_bytes());
    for (item, module, slot, a) in rows {
        for w in [*item, *module, *slot, a.decl, a.member, a.tok, a.count] {
            out.extend_from_slice(&w.to_le_bytes());
        }
    }
    out
}

/// Decodes a section; `None` when it is malformed (a cache miss).
#[must_use]
pub fn decode(bytes: &[u8]) -> Option<Vec<(u32, u32, u32, Anchor)>> {
    let word = |i: usize| -> Option<u32> {
        let b = bytes.get(i * 4..i * 4 + 4)?;
        Some(u32::from_le_bytes(b.try_into().ok()?))
    };
    let n = word(0)? as usize;
    if bytes.len() != 4 + n * WORDS * 4 {
        return None;
    }
    (0..n)
        .map(|r| {
            let w = |k: usize| word(1 + r * WORDS + k);
            Some((
                w(0)?,
                w(1)?,
                w(2)?,
                Anchor {
                    decl: w(3)?,
                    member: w(4)?,
                    tok: w(5)?,
                    count: w(6)?,
                },
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Anchor, decode, encode};

    #[test]
    fn section_round_trips_and_rejects_truncation() {
        let a = Anchor {
            decl: 3,
            member: 2,
            tok: 5,
            count: 4,
        };
        let rows = vec![(1, 0, 2, a), (7, 1, 0, a)];
        let bytes = encode(&rows);
        assert_eq!(decode(&bytes), Some(rows));
        assert_eq!(decode(&bytes[..bytes.len() - 1]), None);
        assert_eq!(decode(&[]), None);
    }
}
