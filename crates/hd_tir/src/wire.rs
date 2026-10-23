//! The wire form of a body (`docs/compiler/data-structures.md` §3.18, §3.20.2;
//! `crates/hd_tir/DESIGN.md` §4.13.11): entry-local tables, then the columns,
//! with every run ID (item, type, type list, symbol) replaced by a table
//! row. The tables are per body, so the TIR hash is a function of the body
//! alone (reconciliation, SK-N9).
//!
//! Remapping knows the ID-carrying words of each tag. A tag whose words
//! are not mapped yet is refused with a structured error, never written
//! with run IDs.

use hd_base::wire::{Reader, Writer};
use hd_base::{
    DefId, Hash128, LocalId, NodeIdx, NotImplemented, Range32, Stage, StageResult, SubId, Symbol,
    hash128,
};
use hd_intern::{PathTable, ShardedInterner};
use hd_types::wire::{TableWriter, Tables};
use hd_types::{InternPool, Ty, TyList};

use crate::ir::{Body, BodyKind, CaptureMode, ChoiceKind, Op, Tag, place};

const KINDS: [BodyKind; 7] = [
    BodyKind::Fn,
    BodyKind::Init,
    BodyKind::TestCase,
    BodyKind::Fact,
    BodyKind::Default,
    BodyKind::DeriveInstance,
    BodyKind::Handle,
];

/// Where a word of `extra` holds an ID.
#[derive(Clone, Copy)]
pub(crate) enum IdWord {
    Def(usize),
    Ty(usize),
    List(usize),
}

impl IdWord {
    fn at(self) -> usize {
        match self {
            IdWord::Def(at) | IdWord::Ty(at) | IdWord::List(at) => at,
        }
    }
}

/// The ID words of one instruction's records, as `extra` positions: those
/// of its place operands, then its tag's.
pub(crate) fn id_words(b: &Body, i: usize) -> StageResult<Vec<IdWord>> {
    let mut out = Vec::new();
    append_place_id_words(b, i, &mut out);
    append_tag_id_words(b, i, &mut out)?;
    Ok(out)
}

/// Appends the IDs of instruction `i`'s place operands ([`Op::Place`]): a
/// global root's `DefId` and `Ty`, `[len, 1, def, ty, ...]`.
fn append_place_id_words(b: &Body, i: usize, out: &mut Vec<IdWord>) {
    let (oa, ob) = b.tags[i].operands();
    for (word, op) in [(b.data[i][0], oa), (b.data[i][1], ob)] {
        if op == Op::Place
            && word & place::BARE_LOCAL == 0
            && b.extra.get(word as usize + 1) == Some(&place::ROOT_GLOBAL)
        {
            out.extend([
                IdWord::Def(word as usize + 2),
                IdWord::Ty(word as usize + 3),
            ]);
        }
    }
}

/// Every ID word of a body, in instruction order, each position once:
/// instructions may share a place record, whose IDs are remapped once.
fn body_id_words(b: &Body) -> StageResult<Vec<IdWord>> {
    let mut seen = vec![false; b.extra.len()];
    let mut out = Vec::new();
    for i in 0..b.len() {
        let start = out.len();
        append_place_id_words(b, i, &mut out);
        append_tag_id_words(b, i, &mut out)?;
        // Compact just the appended words, retaining their first-seen
        // order without allocating a temporary vector for the instruction.
        let mut write = start;
        for read in start..out.len() {
            let word = out[read];
            let keep = match seen.get_mut(word.at()) {
                Some(s) if *s => false,
                Some(s) => {
                    *s = true;
                    true
                }
                // Out of range: the caller's lookup refuses it.
                None => true,
            };
            if keep {
                out[write] = word;
                write += 1;
            }
        }
        out.truncate(write);
    }
    Ok(out)
}

/// Appends the ID words of one instruction's tag-specific records.
fn append_tag_id_words(b: &Body, i: usize, out: &mut Vec<IdWord>) -> StageResult<()> {
    let [a, _] = b.data[i];
    match b.tags[i] {
        Tag::Call => return append_id_words_call(b, a as usize + 1, out),
        Tag::Await => return append_id_words_call(b, a as usize + 1, out),
        // `ProviderGet` and `ItemRef` keep their IDs in one-word records.
        Tag::ProviderGet => out.push(IdWord::Ty(a as usize + 1)),
        // `With` and `ContextNew`: (key type, provider) pairs.
        Tag::With | Tag::ContextNew => {
            let n = b.extra.get(a as usize).copied().unwrap_or(0) as usize;
            out.extend((0..n / 2).map(|k| IdWord::Ty(a as usize + 1 + 2 * k)));
        }
        Tag::DefaultCall => out.extend([IdWord::Def(a as usize + 1), IdWord::List(a as usize + 2)]),
        // A generic local function's instantiation: its type arguments
        // are the record's fourth word (`sub_flags::GENERIC`).
        Tag::Closure if b.is_instantiation(i) => out.push(IdWord::List(b.data[i][1] as usize + 4)),
        Tag::ItemRef => out.extend([
            IdWord::Def(a as usize + 1),
            IdWord::List(b.data[i][1] as usize + 1),
        ]),
        Tag::RefDyn | Tag::RefIdentity => {}
        // A global root's IDs are the place's ([`append_place_id_words`]).
        Tag::Read | Tag::Store | Tag::Ref => {}
        // The ownership passes run after decoding, so their statements
        // never reach the wire (`docs/compiler/tir-v2.md`, "Encoding, wire
        // format and caching").
        Tag::Drop | Tag::IsUnique | Tag::Pin | Tag::FlagSet | Tag::FlagClear => {
            return Err(NotImplemented::bug(
                Stage::ModuleFinish,
                format!("wire form of the ownership statement {}", b.tags[i].name()),
            ));
        }
        Tag::Prim
        | Tag::CallHost
        | Tag::CallValue
        | Tag::AwaitValue
        | Tag::AwaitAll
        | Tag::AwaitRace
        | Tag::Intrinsic
        | Tag::NewStruct
        | Tag::CopyStruct
        | Tag::NewVariant
        | Tag::NewTuple
        | Tag::NewList
        | Tag::NewMap
        | Tag::Closure
        | Tag::Interp
        | Tag::Coerce
        | Tag::And
        | Tag::Or
        | Tag::Is
        | Tag::Block
        | Tag::Scope
        | Tag::Defer
        | Tag::If
        | Tag::Loop
        | Tag::Break
        | Tag::Continue
        | Tag::Return
        | Tag::Match
        | Tag::SwitchTag
        | Tag::SwitchInt
        | Tag::SwitchChar
        | Tag::SwitchStr
        | Tag::Guard
        | Tag::ToArm
        | Tag::Unreachable
        | Tag::Poison => {}
        other => {
            return Err(NotImplemented::new(
                Stage::ModuleFinish,
                format!("wire form of TIR tag {}", other.name()),
            ));
        }
    }
    Ok(())
}

/// Appends the ID words of a callee record starting at `at`.
fn append_id_words_call(b: &Body, at: usize, out: &mut Vec<IdWord>) -> StageResult<()> {
    match b.extra.get(at) {
        Some(0) => out.extend([IdWord::Def(at + 1), IdWord::List(at + 2)]),
        Some(1) => {
            out.extend([
                IdWord::Def(at + 1),
                IdWord::Def(at + 2),
                IdWord::Ty(at + 3),
                IdWord::List(at + 4),
            ]);
            // An impl choice and a trait-value choice carry the item's ID.
            if matches!(
                b.extra.get(at + 5),
                Some(&k) if k == ChoiceKind::Impl as u32 || k == ChoiceKind::TraitValue as u32
            ) {
                out.push(IdWord::Def(at + 6));
            }
        }
        _ => {
            return Err(NotImplemented::bug(
                Stage::ModuleFinish,
                "wire form of a malformed callee record",
            ));
        }
    }
    Ok(())
}

fn u32s(w: &mut Writer, v: impl ExactSizeIterator<Item = u32>) {
    w.u32(u32::try_from(v.len()).expect("column"));
    for x in v {
        w.u32(x);
    }
}

/// Encodes a body: tables, then columns.
pub fn write_body(
    b: &Body,
    pool: &InternPool,
    paths: &PathTable,
    syms: &ShardedInterner,
) -> StageResult<Vec<u8>> {
    let mut t = TableWriter::new(pool, paths, syms);
    let mut extra = b.extra.clone();
    for i in 0..b.len() {
        if b.tags[i] == Tag::Coerce {
            // The evidence word would be an ID; none is written today, and a
            // future one must be remapped like `Impl` before it can leak.
            if b.record(b.data[i][1]).get(1).copied() != Some(crate::ir::NONE) {
                return Err(NotImplemented::new(
                    Stage::ModuleFinish,
                    "wire form of coercion evidence",
                ));
            }
        }
    }
    for w in body_id_words(b)? {
        match w {
            IdWord::Def(at) => extra[at] = t.def(DefId::from_raw(extra[at])),
            IdWord::Ty(at) => extra[at] = t.ty(Ty(extra[at]))?,
            IdWord::List(at) => extra[at] = t.list(TyList(extra[at]))?,
        }
    }
    let item = t.def(b.item);
    let ty: Vec<u32> = b.ty.iter().map(|x| t.ty(*x)).collect::<StageResult<_>>()?;
    let local_ty: Vec<u32> = b
        .local_ty
        .iter()
        .map(|x| t.ty(*x))
        .collect::<StageResult<_>>()?;
    let local_name: Vec<u32> = b.local_name.iter().map(|s| t.sym(*s)).collect();
    let consts: Vec<(u32, u64)> = b
        .consts
        .iter()
        .map(|(ct, bits)| Ok((t.ty(*ct)?, *bits)))
        .collect::<StageResult<_>>()?;
    if !b.susp.is_empty() {
        return Err(NotImplemented::new(
            Stage::ModuleFinish,
            "wire form of suspension rows",
        ));
    }
    // The exit and unwind sets are the ownership passes' output, which
    // runs after decoding; lifetime regions are tier 2's.
    if !b.exit_drops.is_empty()
        || !b.unwind_drops.is_empty()
        || !b.full_cells.is_empty()
        || !b.stable_lists.is_empty()
        || !b.regions.is_empty()
        || !b.anchors.is_empty()
    {
        return Err(NotImplemented::bug(
            Stage::ModuleFinish,
            "wire form of exit drops, unwind drops, stable lists, lifetime regions or anchors",
        ));
    }
    let mut w = Writer::default();
    t.write(&mut w);
    w.u32(item);
    w.u8(b.kind as u8);
    u32s(&mut w, b.tags.iter().map(|x| *x as u32));
    u32s(
        &mut w,
        b.data
            .iter()
            .flat_map(|d| d.iter().copied())
            .collect::<Vec<_>>()
            .into_iter(),
    );
    u32s(&mut w, ty.into_iter());
    u32s(&mut w, extra.into_iter());
    u32s(&mut w, local_ty.into_iter());
    u32s(&mut w, local_name.into_iter());
    u32s(&mut w, b.local_flags.iter().map(|f| u32::from(*f)));
    u32s(&mut w, b.sub_root.iter().copied());
    u32s(
        &mut w,
        b.sub_params
            .iter()
            .flat_map(|r| [r.start, r.len])
            .collect::<Vec<_>>()
            .into_iter(),
    );
    u32s(&mut w, b.sub_parent.iter().map(|s| s.raw()));
    u32s(&mut w, b.sub_flags.iter().map(|f| u32::from(*f)));
    u32s(&mut w, b.label_inst.iter().copied());
    u32s(&mut w, b.cap_local.iter().map(|l| l.raw()));
    u32s(&mut w, b.cap_mode.iter().map(|m| *m as u32));
    w.len_of(&consts);
    for (ct, bits) in consts {
        w.u32(ct);
        w.u64(bits);
    }
    w.len_of(&b.strings);
    for st in &b.strings {
        w.str(st);
    }
    w.str(&b.label);
    // Locations last, outside the hashed prefix: moving a body in its file
    // changes node indices, not the body (`crates/hd_cache/DESIGN.md` §5.3).
    let hashed = u32::try_from(w.bytes.len()).expect("body over 4 GiB");
    u32s(&mut w, b.syn.iter().map(|n| n.raw()));
    u32s(&mut w, b.local_syn.iter().map(|n| n.raw()));
    u32s(
        &mut w,
        b.fallback
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>()
            .into_iter(),
    );
    let mut out = hashed.to_le_bytes().to_vec();
    out.extend_from_slice(&w.bytes);
    Ok(out)
}

fn col(r: &mut Reader<'_>) -> Vec<u32> {
    let n = r.count();
    (0..n).map(|_| r.u32()).collect()
}

/// Decodes a body into this run's tables; `None` on a malformed entry.
#[must_use]
pub fn read_body(
    bytes: &[u8],
    pool: &InternPool,
    paths: &PathTable,
    syms: &ShardedInterner,
) -> Option<Body> {
    let mut r = Reader::new(bytes.get(4..)?);
    let t = Tables::read(&mut r, pool, paths, syms)?;
    let item = t.def(r.u32())?;
    let kind = *KINDS.get(r.u8() as usize)?;
    let mut b = Body::new(item, kind);
    b.tags = col(&mut r)
        .into_iter()
        .map(|x| Tag::ALL.get(x as usize).copied())
        .collect::<Option<_>>()?;
    let data = col(&mut r);
    b.data = data
        .chunks(2)
        .map(|c| [c[0], c.get(1).copied().unwrap_or(0)])
        .collect();
    b.ty = col(&mut r)
        .into_iter()
        .map(|x| t.ty(x))
        .collect::<Option<_>>()?;
    b.extra = col(&mut r);
    b.local_ty = col(&mut r)
        .into_iter()
        .map(|x| t.ty(x))
        .collect::<Option<_>>()?;
    b.local_name = col(&mut r)
        .into_iter()
        .map(|x| t.sym(x))
        .collect::<Option<Vec<Symbol>>>()?;
    b.local_flags = col(&mut r)
        .into_iter()
        .map(|x| u8::try_from(x).ok())
        .collect::<Option<_>>()?;
    b.sub_root = col(&mut r);
    b.sub_params = col(&mut r)
        .chunks(2)
        .map(|c| Range32::new(c[0], c.get(1).copied().unwrap_or(0)))
        .collect();
    b.sub_parent = col(&mut r).into_iter().map(SubId::from_raw).collect();
    b.sub_flags = col(&mut r)
        .into_iter()
        .map(|x| u8::try_from(x).ok())
        .collect::<Option<_>>()?;
    b.label_inst = col(&mut r);
    b.cap_local = col(&mut r).into_iter().map(LocalId::from_raw).collect();
    b.cap_mode = col(&mut r)
        .into_iter()
        .map(|m| {
            [CaptureMode::Copy, CaptureMode::Move, CaptureMode::Shared]
                .get(m as usize)
                .copied()
        })
        .collect::<Option<_>>()?;
    for _ in 0..r.count() {
        let ct = t.ty(r.u32())?;
        b.consts.push((ct, r.u64()));
    }
    for _ in 0..r.count() {
        b.strings.push(r.str().into());
    }
    b.label = r.str().into();
    b.syn = col(&mut r).into_iter().map(NodeIdx::from_raw).collect();
    b.local_syn = col(&mut r).into_iter().map(NodeIdx::from_raw).collect();
    b.fallback = col(&mut r).chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    if !r.ok() || b.data.len() != b.tags.len() || b.ty.len() != b.tags.len() {
        return None;
    }
    for w in body_id_words(&b).ok()? {
        match w {
            IdWord::Def(at) => b.extra[at] = t.def(*b.extra.get(at)?)?.raw(),
            IdWord::Ty(at) => b.extra[at] = t.ty(*b.extra.get(at)?)?.0,
            IdWord::List(at) => b.extra[at] = t.list(*b.extra.get(at)?)?.0,
        }
    }
    Some(b)
}

/// Rewrites every run ID of a body through `def` and `ty`: the item, the
/// type columns, the constants' types, and the ID words of each record,
/// the ones the wire form remaps. A derive instance is its template's
/// checked body with the template's parameters replaced this way
/// (`crates/hd_tir/DESIGN.md` §4.13.9).
///
/// # Errors
/// A tag whose ID words are not mapped yet, as for the wire form.
pub fn map_ids(
    b: &mut Body,
    def: &dyn Fn(DefId) -> DefId,
    ty: &dyn Fn(Ty) -> Ty,
    list: &dyn Fn(TyList) -> TyList,
) -> StageResult<()> {
    for w in body_id_words(b)? {
        match w {
            IdWord::Def(at) => b.extra[at] = def(DefId::from_raw(b.extra[at])).raw(),
            IdWord::Ty(at) => b.extra[at] = ty(Ty(b.extra[at])).0,
            IdWord::List(at) => b.extra[at] = list(TyList(b.extra[at])).0,
        }
    }
    b.item = def(b.item);
    for t in b.ty.iter_mut().chain(b.local_ty.iter_mut()) {
        *t = ty(*t);
    }
    for c in &mut b.consts {
        c.0 = ty(c.0);
    }
    Ok(())
}

/// The TIR hash (§4.13.11): the hash of the body's wire bytes before the
/// location columns, so it is a function of the body's content alone.
#[must_use]
pub fn tir_hash(bytes: &[u8]) -> Hash128 {
    let n = bytes
        .get(..4)
        .and_then(|b| b.try_into().ok())
        .map_or(0, u32::from_le_bytes) as usize;
    hash128(bytes.get(4..4 + n).unwrap_or(bytes))
}

#[cfg(test)]
mod tests {
    use super::{map_ids, read_body, tir_hash, write_body};
    use crate::ir::{
        BodyKind, Callee, Place, PlaceRoot, Providers, ReadMode, Tag, TirBuilder, TirSink,
        local_flags, map_body_types,
    };
    use hd_base::NodeIdx;
    use hd_intern::{PathTable, ShardedInterner};
    use hd_types::{InternPool, Ty, TyData};

    #[test]
    fn a_body_round_trips_into_a_fresh_run_with_an_equal_hash() {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let f = paths.item("app", "main", "f");
        let g = paths.item("app", "main", "g");
        let point = pool.intern_ty(&TyData::Adt {
            def: paths.item("app", "geo", "Point"),
            args: pool.list(&[]),
        });
        let mut b = TirBuilder::new(f, BodyKind::Fn);
        let n = NodeIdx::from_raw(0);
        let blk = b.open_block();
        let x = b.local(point, syms.intern("p"), local_flags::PARAM, n);
        let one = b.const_value(Ty::I32, 1);
        let get = b.get(x, point, n);
        let call = b.call(
            &Callee::Item {
                def: g,
                targs: pool.list(&[point]),
            },
            &[get, one],
            Providers::None,
            Ty::I32,
            n,
        );
        b.emit(Tag::Return, call.0, crate::ir::NONE, Ty::NEVER, n);
        let root = b.close_block(blk, None, Ty::NEVER, n);
        let body = b.finish(root, &[]).expect("verifies");
        let bytes = write_body(&body, &pool, &paths, &syms).expect("wire");
        let (pool2, paths2, syms2) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        paths2.item("zzz", "a", "b");
        let back = read_body(&bytes, &pool2, &paths2, &syms2).expect("read");
        let again = write_body(&back, &pool2, &paths2, &syms2).expect("wire");
        assert_eq!(tir_hash(&bytes), tir_hash(&again));
        assert_eq!(back.item, paths2.item("app", "main", "f"));
    }

    /// `counter.count += 1` on a top-level binding: a `Read` and a `Store`
    /// sharing one place record whose root's `DefId` and `Ty` are IDs.
    #[test]
    fn a_shared_global_place_is_remapped_once() {
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let counter_ty = |pool: &InternPool, paths: &PathTable| {
            pool.intern_ty(&TyData::Adt {
                def: paths.item("app", "main", "Counter"),
                args: pool.list(&[]),
            })
        };
        let f = paths.item("app", "main", "f");
        let counter = paths.item("app", "main", "counter");
        let ty = counter_ty(&pool, &paths);
        let mut b = TirBuilder::new(f, BodyKind::Fn);
        let n = NodeIdx::from_raw(0);
        let blk = b.open_block();
        let at = b.place(&Place {
            root: PlaceRoot::Global(counter, ty),
            proj: vec![crate::ir::Proj::Field(0)],
        });
        let read = b.emit(Tag::Read, at, ReadMode::Use as u32, Ty::I32, n);
        let one = b.const_value(Ty::I32, 1);
        let sum = b.prim(crate::ir::PrimOp::Add as u32, &[read, one], Ty::I32, n);
        b.emit(Tag::Store, at, sum.0, Ty::VOID, n);
        let root = b.close_block(blk, None, Ty::VOID, n);
        let body = b.finish(root, &[]).expect("verifies");
        let bytes = write_body(&body, &pool, &paths, &syms).expect("wire");
        let (pool2, paths2, syms2) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        paths2.item("zzz", "a", "b");
        pool2.intern_ty(&TyData::Tuple {
            elems: pool2.list(&[Ty::I32, Ty::BOOL]),
            rest: None,
        });
        let back = read_body(&bytes, &pool2, &paths2, &syms2).expect("read");
        let want = Place {
            root: PlaceRoot::Global(
                paths2.item("app", "main", "counter"),
                counter_ty(&pool2, &paths2),
            ),
            proj: vec![crate::ir::Proj::Field(0)],
        };
        for i in [0, 2] {
            assert_eq!(back.decode_place(back.data[i][0]).as_ref(), Some(&want));
        }
        let again = write_body(&back, &pool2, &paths2, &syms2).expect("wire");
        assert_eq!(tir_hash(&bytes), tir_hash(&again));
        // The other two mappers, once per shared word too.
        let mut moved = body.clone();
        map_body_types(&mut moved, &|t| Ty(t.0 + 100), &|l| l);
        let PlaceRoot::Global(_, t) = moved.decode_place(at).expect("place").root else {
            panic!("a global root");
        };
        assert_eq!(t, Ty(ty.0 + 100));
        let mut moved = body;
        map_ids(
            &mut moved,
            &|d| hd_base::DefId::from_raw(d.raw() + 100),
            &|t| t,
            &|l| l,
        )
        .expect("mapped");
        let PlaceRoot::Global(d, _) = moved.decode_place(at).expect("place").root else {
            panic!("a global root");
        };
        assert_eq!(d.raw(), counter.raw() + 100);
    }
}
