//! The wire form of a body (`docs/compiler/data-structures.md` §3.18, §3.20.2;
//! `crates/hd_tir/DESIGN.md` §4.13.11): entry-local tables, then the columns,
//! with every run ID (item, type, type list, symbol) replaced by a table
//! row. The tables are per body, so the TIR hash is a function of the body
//! alone (reconciliation, SK-N9). An owned body, after the ownership
//! passes, adds their columns after the locations ([`write_owned`]).
//!
//! Remapping knows the ID-carrying words of each tag. A tag whose words
//! are not mapped yet is refused with a structured error, never written
//! with run IDs. Only the `extra` records a body reaches are written
//! ([`compact`]), so a record a rebuild orphaned carries none either.

use hd_base::SliceSort as _;
use hd_base::wire::{Reader, Writer};
use hd_base::{
    DefId, Hash128, LocalId, NodeIdx, NotImplemented, Range32, Stage, StageResult, SubId, Symbol,
    hash128,
};
use hd_intern::{PathTable, ShardedInterner};
use hd_types::wire::{TableWriter, Tables};
use hd_types::{InternPool, Ty, TyList};

use crate::ir::{Body, BodyKind, CaptureMode, ChoiceKind, Op, Storage, SuspRow, Tag, place};

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
#[derive(Clone, Copy, Debug)]
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

/// Where a word that points into `extra` sits ([`Reach::Span`]).
#[derive(Clone, Copy, Debug)]
enum Holder {
    /// Data word `k` of instruction `i`.
    Data(usize, usize),
    /// A sub-body's parameter record (`Body::sub_params`).
    SubParams(usize),
    /// A suspension row's scope list (`SuspRow::scopes`).
    Susp(usize),
    /// A stable list's place word (`Body::stable_lists`).
    StableList(usize),
}

/// One reference of a body into `extra` ([`references`]).
#[derive(Clone, Copy, Debug)]
enum Reach {
    /// The word at `at` points at `start`, and the words `start..end` are
    /// what it reaches: a record with its length word, or a range.
    Span { at: Holder, start: u32, end: u32 },
    /// A word of a reached record that holds an ID.
    Id(IdWord),
}

/// The ID words of one instruction's records, as `extra` positions: those
/// of its place operands, then its tag's.
pub(crate) fn id_words(b: &Body, i: usize) -> StageResult<Vec<IdWord>> {
    let mut out = Vec::new();
    inst_refs(b, i, &mut |r| {
        if let Reach::Id(w) = r {
            out.push(w);
        }
    })?;
    Ok(out)
}

/// Every reference of `b` into `extra`, in first-reach order: each
/// instruction's place operands, then its records and their ID words,
/// then the columns that point into `extra`. The one enumeration of how a
/// word reaches a record: the ID remapper ([`body_id_words`]) and the
/// compaction ([`compact`]) both read it. A tag or record whose
/// references are not known is refused.
fn references(b: &Body, f: &mut dyn FnMut(Reach)) -> StageResult<()> {
    for i in 0..b.len() {
        inst_refs(b, i, f)?;
    }
    for (s, r) in b.sub_params.iter().enumerate() {
        // The range length, not the prefix word, defines its payload.
        // Empty ranges reach nothing, including a copied body's root.
        if r.len > 0 {
            let payload = bounded(b, r.start, 1)?;
            f(Reach::Span {
                at: Holder::SubParams(s),
                start: r.start,
                end: bounded(b, payload, r.len)?,
            });
        }
    }
    for (s, row) in b.susp.iter().enumerate() {
        // A range of scope instructions, with no length word.
        if row.scopes.len > 0 {
            f(Reach::Span {
                at: Holder::Susp(s),
                start: row.scopes.start,
                end: bounded(b, row.scopes.start, row.scopes.len)?,
            });
        }
    }
    for (r, &(_, word)) in b.stable_lists.iter().enumerate() {
        if word & place::BARE_LOCAL == 0 {
            place_refs(b, Holder::StableList(r), word, f)?;
        }
    }
    Ok(())
}

/// `start + len`, when the words `start..start + len` are in `extra`.
fn bounded(b: &Body, start: u32, len: u32) -> StageResult<u32> {
    start
        .checked_add(len)
        .filter(|&end| end as usize <= b.extra.len())
        .ok_or_else(|| NotImplemented::bug(Stage::ModuleFinish, "a TIR record past `extra`"))
}

/// The record at `at`, `[len, words...]`, held by `holder`.
fn record_span(b: &Body, holder: Holder, at: u32, f: &mut dyn FnMut(Reach)) -> StageResult<()> {
    let len = *b
        .extra
        .get(at as usize)
        .ok_or_else(|| NotImplemented::bug(Stage::ModuleFinish, "a TIR record past `extra`"))?;
    f(Reach::Span {
        at: holder,
        start: at,
        end: bounded(b, at, len.saturating_add(1))?,
    });
    Ok(())
}

/// A place record: its span, and a global root's `DefId` and `Ty`,
/// `[len, 1, def, ty, ...]`.
fn place_refs(b: &Body, holder: Holder, word: u32, f: &mut dyn FnMut(Reach)) -> StageResult<()> {
    record_span(b, holder, word, f)?;
    if b.extra.get(word as usize + 1) == Some(&place::ROOT_GLOBAL) {
        f(Reach::Id(IdWord::Def(word as usize + 2)));
        f(Reach::Id(IdWord::Ty(word as usize + 3)));
    }
    Ok(())
}

/// Instruction `i`'s references: its place operands' records and IDs,
/// then its other records, then their ID words.
fn inst_refs(b: &Body, i: usize, f: &mut dyn FnMut(Reach)) -> StageResult<()> {
    let tag = b.tags[i];
    let (oa, ob) = tag.operands();
    let sides = [(0, b.data[i][0], oa), (1, b.data[i][1], ob)];
    for (k, word, op) in sides {
        if op == Op::Place && word & place::BARE_LOCAL == 0 {
            place_refs(b, Holder::Data(i, k), word, f)?;
        }
    }
    for (k, word, op) in sides {
        // `ProviderGet` and `ItemRef` keep their IDs in one-word records
        // their meta word points at.
        let record = matches!(op, Op::Record | Op::Values | Op::Blocks)
            || (k == 0 && matches!(tag, Tag::ProviderGet | Tag::ItemRef));
        if record && word != crate::ir::NONE {
            record_span(b, Holder::Data(i, k), word, f)?;
        }
    }
    tag_ids(b, i, f)
}

/// The ID words of instruction `i`'s tag-specific records.
fn tag_ids(b: &Body, i: usize, f: &mut dyn FnMut(Reach)) -> StageResult<()> {
    let [a, _] = b.data[i];
    let mut id = |w| f(Reach::Id(w));
    match b.tags[i] {
        Tag::Call | Tag::Await => {
            call_ids(b, a as usize + 1, &mut id)?;
            // `Providers::Pairs` points at a range of its own; nothing
            // builds one, so its words are not mapped yet.
            let args = b.data[i][1] as usize;
            let n = b.extra.get(args).copied().unwrap_or(0) as usize;
            if n >= 3 && b.extra.get(args + 1 + n - 3) == Some(&1) {
                return Err(NotImplemented::bug(
                    Stage::ModuleFinish,
                    "wire form of a call's provider pairs",
                ));
            }
        }
        Tag::ProviderGet => id(IdWord::Ty(a as usize + 1)),
        // `With` and `ContextNew`: (key type, provider) pairs.
        Tag::With | Tag::ContextNew => {
            let n = b.extra.get(a as usize).copied().unwrap_or(0) as usize;
            for k in 0..n / 2 {
                id(IdWord::Ty(a as usize + 1 + 2 * k));
            }
        }
        Tag::DefaultCall => {
            id(IdWord::Def(a as usize + 1));
            id(IdWord::List(a as usize + 2));
        }
        // A generic local function's instantiation: its type arguments
        // are the record's fourth word (`sub_flags::GENERIC`).
        Tag::Closure if b.is_instantiation(i) => id(IdWord::List(b.data[i][1] as usize + 4)),
        Tag::ItemRef => {
            id(IdWord::Def(a as usize + 1));
            id(IdWord::List(b.data[i][1] as usize + 1));
        }
        Tag::RefDyn | Tag::RefIdentity => {}
        // A global root's IDs are the place's ([`place_refs`]).
        Tag::Read | Tag::Store | Tag::Ref => {}
        // The ownership statements of an owned body (`write_body`): a
        // place's IDs are its own, and a count's record names a part of
        // the instruction it counts, no ID.
        Tag::Drop
        | Tag::LoanEnd
        | Tag::Retain
        | Tag::Release
        | Tag::MakeUnique
        | Tag::IsUnique
        | Tag::FlagSet
        | Tag::FlagClear => {}
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

/// The ID words of a callee record starting at `at`.
fn call_ids(b: &Body, at: usize, id: &mut dyn FnMut(IdWord)) -> StageResult<()> {
    match b.extra.get(at) {
        Some(0) => {
            id(IdWord::Def(at + 1));
            id(IdWord::List(at + 2));
        }
        Some(1) => {
            id(IdWord::Def(at + 1));
            id(IdWord::Def(at + 2));
            id(IdWord::Ty(at + 3));
            id(IdWord::List(at + 4));
            // An impl choice and a trait-value choice carry the item's ID.
            if matches!(
                b.extra.get(at + 5),
                Some(&k) if k == ChoiceKind::Impl as u32 || k == ChoiceKind::TraitValue as u32
            ) {
                id(IdWord::Def(at + 6));
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

/// Every ID word of a body, in first-reach order, each position once:
/// instructions may share a place record, whose IDs are remapped once.
fn body_id_words(b: &Body) -> StageResult<Vec<IdWord>> {
    let mut seen = vec![false; b.extra.len()];
    let mut out = Vec::new();
    references(b, &mut |r| {
        // A word out of range is kept: the caller's lookup refuses it.
        if let Reach::Id(w) = r
            && !seen.get(w.at()).is_some_and(|&s| s)
        {
            if let Some(s) = seen.get_mut(w.at()) {
                *s = true;
            }
            out.push(w);
        }
    })?;
    Ok(out)
}

/// `b` with only the `extra` words its references reach ([`references`]),
/// laid out in first-reach order and every pointer renumbered. A rebuild
/// leaves records nothing reaches, such as an inlined call's callee
/// record, whose IDs no remapping sees; dropping them makes the wire form
/// a function of the body's content (#1423). Overlapping spans, a record
/// shared or a range inside one, move together.
fn compact(b: &Body) -> StageResult<std::borrow::Cow<'_, Body>> {
    let mut spans: Vec<(Holder, u32, u32)> = Vec::new();
    references(b, &mut |r| {
        if let Reach::Span { at, start, end } = r {
            spans.push((at, start, end));
        }
    })?;
    // The reached intervals, by start: (start, end, first reach).
    let mut by_start: Vec<usize> = (0..spans.len()).collect();
    by_start.sort_stable_by_key(|&k| spans[k].1);
    let mut runs: Vec<(u32, u32, usize)> = Vec::new();
    for k in by_start {
        let (_, start, end) = spans[k];
        match runs.last_mut() {
            Some(run) if start < run.1 => {
                run.1 = run.1.max(end);
                run.2 = run.2.min(k);
            }
            _ => runs.push((start, end, k)),
        }
    }
    // Each interval's new start, in first-reach order.
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_stable_by_key(|&r| runs[r].2);
    let mut base = vec![0; runs.len()];
    let mut extra = Vec::with_capacity(b.extra.len());
    let mut next = 0;
    for &r in &order {
        let (start, end, _) = runs[r];
        base[r] = next;
        next += end - start;
        extra.extend_from_slice(&b.extra[start as usize..end as usize]);
    }
    if extra == b.extra {
        return Ok(std::borrow::Cow::Borrowed(b));
    }
    let moved = |p: u32| {
        let r = runs.partition_point(|run| run.0 <= p) - 1;
        base[r] + (p - runs[r].0)
    };
    let mut out = b.clone();
    out.extra = extra;
    for (at, start, _) in spans {
        let to = moved(start);
        match at {
            Holder::Data(i, k) => out.data[i][k] = to,
            Holder::SubParams(s) => out.sub_params[s].start = to,
            Holder::Susp(s) => out.susp[s].scopes.start = to,
            Holder::StableList(r) => out.stable_lists[r].1 = to,
        }
    }
    Ok(std::borrow::Cow::Owned(out))
}

fn u32s(w: &mut Writer, v: impl ExactSizeIterator<Item = u32>) {
    w.u32(u32::try_from(v.len()).expect("column"));
    for x in v {
        w.u32(x);
    }
}

/// Encodes a body: tables, then columns, with only the `extra` records
/// it reaches ([`compact`]). A debug build decodes the bytes and checks
/// that they encode back the same.
pub fn write_body(
    b: &Body,
    pool: &InternPool,
    paths: &PathTable,
    syms: &ShardedInterner,
) -> StageResult<Vec<u8>> {
    let out = encode(compact(b)?.as_ref(), pool, paths, syms)?;
    if cfg!(debug_assertions) {
        let again = read_body(&out, pool, paths, syms).map(|back| encode(&back, pool, paths, syms));
        assert!(
            matches!(&again, Some(Ok(again)) if *again == out),
            "item {}: a decoded body encodes differently",
            b.item.raw()
        );
    }
    Ok(out)
}

/// [`write_body`] of a compacted body.
fn encode(
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
    // The inliner's frames name their callees: IDs, in the tables.
    let inlined: Vec<[u32; 4]> = b
        .inlined
        .iter()
        .map(|&[first, len, callee, call]| [first, len, t.def(DefId::from_raw(callee)), call])
        .collect();
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
            .flat_map(|r| [if r.len == 0 { 0 } else { r.start }, r.len])
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
    let held: Vec<u32> = b.held_args.iter().flat_map(|&(c, a)| [c, a]).collect();
    u32s(&mut w, held.into_iter());
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
    if owned(b) {
        write_owned(&mut w, b, &inlined);
    }
    let mut out = hashed.to_le_bytes().to_vec();
    out.extend_from_slice(&w.bytes);
    Ok(out)
}

/// Whether `b` holds the ownership passes' output, which only an owned
/// body's wire form carries: a checker's body encodes as it always has.
fn owned(b: &Body) -> bool {
    !(b.susp.is_empty()
        && b.exit_drops.is_empty()
        && b.storage.is_empty()
        && b.unwind_drops.is_empty()
        && b.live.is_empty()
        && b.full_cells.is_empty()
        && b.stable_lists.is_empty()
        && b.regions.is_empty()
        && b.anchors.is_empty()
        && b.inlined.is_empty()
        && b.inlined_origins.is_empty()
        && b.inlined_shared.is_empty()
        && b.inlined_capture_origins.is_empty())
}

/// The owned section, after the locations: the passes' columns
/// (`Body::susp` to `Body::inlined_capture_origins`), each a flat `u32`
/// column of its rows; `inlined` has its callees as table rows. Outside
/// the hashed prefix: a cache of owned bodies keys one by its checked
/// body, never by this encoding's hash.
fn write_owned(w: &mut Writer, b: &Body, inlined: &[[u32; 4]]) {
    let pairs = |w: &mut Writer, v: &[(u32, u32)]| {
        u32s(
            w,
            v.iter()
                .flat_map(|&(x, y)| [x, y])
                .collect::<Vec<_>>()
                .into_iter(),
        );
    };
    u32s(
        w,
        b.susp
            .iter()
            .flat_map(|s| [s.inst, s.scopes.start, s.scopes.len, s.hook_site])
            .collect::<Vec<_>>()
            .into_iter(),
    );
    u32s(
        w,
        b.exit_drops
            .iter()
            .flat_map(|&(s, l)| [s.raw(), l.raw()])
            .collect::<Vec<_>>()
            .into_iter(),
    );
    u32s(
        w,
        b.storage
            .iter()
            .flat_map(|&(s, l, k)| [s.raw(), l.raw(), k as u32])
            .collect::<Vec<_>>()
            .into_iter(),
    );
    pairs(w, &b.unwind_drops);
    w.len_of(&b.live);
    for (point, held) in &b.live {
        w.u32(*point);
        u32s(w, held.iter().copied());
    }
    pairs(w, &b.full_cells);
    pairs(w, &b.stable_lists);
    pairs(w, &b.regions);
    u32s(w, b.anchors.iter().copied());
    u32s(
        w,
        inlined
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>()
            .into_iter(),
    );
    u32s(
        w,
        b.inlined_origins
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>()
            .into_iter(),
    );
    u32s(
        w,
        b.inlined_shared
            .iter()
            .flat_map(|&(s, row)| [s.raw(), row])
            .collect::<Vec<_>>()
            .into_iter(),
    );
    u32s(
        w,
        b.inlined_capture_origins
            .iter()
            .flat_map(|&(s, a, c)| [s.raw(), a.raw(), c.raw()])
            .collect::<Vec<_>>()
            .into_iter(),
    );
}

/// Reads the owned section into `b` ([`write_owned`]); `None` on a
/// malformed row.
fn read_owned(r: &mut Reader<'_>, b: &mut Body, t: &Tables) -> Option<()> {
    let pairs = |v: Vec<u32>| v.chunks_exact(2).map(|c| (c[0], c[1])).collect();
    b.susp = col(r)
        .chunks_exact(4)
        .map(|c| SuspRow {
            inst: c[0],
            scopes: Range32::new(c[1], c[2]),
            hook_site: c[3],
        })
        .collect();
    b.exit_drops = col(r)
        .chunks_exact(2)
        .map(|c| (SubId::from_raw(c[0]), LocalId::from_raw(c[1])))
        .collect();
    b.storage = col(r)
        .chunks_exact(3)
        .map(|c| {
            let k = [Storage::Slot, Storage::Cell].get(c[2] as usize).copied()?;
            Some((SubId::from_raw(c[0]), LocalId::from_raw(c[1]), k))
        })
        .collect::<Option<_>>()?;
    b.unwind_drops = pairs(col(r));
    for _ in 0..r.count() {
        let point = r.u32();
        b.live.push((point, col(r)));
    }
    b.full_cells = pairs(col(r));
    b.stable_lists = pairs(col(r));
    b.regions = pairs(col(r));
    b.anchors = col(r);
    b.inlined = col(r)
        .chunks_exact(4)
        .map(|c| Some([c[0], c[1], t.def(c[2])?.raw(), c[3]]))
        .collect::<Option<_>>()?;
    b.inlined_origins = col(r).chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    b.inlined_shared = col(r)
        .chunks_exact(2)
        .map(|c| (SubId::from_raw(c[0]), c[1]))
        .collect();
    b.inlined_capture_origins = col(r)
        .chunks_exact(3)
        .map(|c| {
            (
                SubId::from_raw(c[0]),
                LocalId::from_raw(c[1]),
                LocalId::from_raw(c[2]),
            )
        })
        .collect();
    Some(())
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
            [
                CaptureMode::Copy,
                CaptureMode::Move,
                CaptureMode::Shared,
                CaptureMode::Frame,
            ]
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
    b.held_args = col(&mut r).chunks_exact(2).map(|c| (c[0], c[1])).collect();
    b.syn = col(&mut r).into_iter().map(NodeIdx::from_raw).collect();
    b.local_syn = col(&mut r).into_iter().map(NodeIdx::from_raw).collect();
    b.fallback = col(&mut r).chunks_exact(2).map(|c| [c[0], c[1]]).collect();
    if !r.at_end() {
        read_owned(&mut r, &mut b, &t)?;
    }
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
    for frame in &mut b.inlined {
        frame[2] = def(DefId::from_raw(frame[2])).raw();
    }
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

    /// An owned body's pass columns ride in the owned section, the
    /// inliner's callee remapped like any ID; a checker's body writes no
    /// such section, so its bytes and hash do not change.
    #[test]
    fn an_owned_body_round_trips_its_pass_columns_into_a_fresh_run() {
        use crate::ir::{Storage, SuspRow};
        use hd_base::{LocalId, Range32, SubId};
        let (pool, paths, syms) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        let f = paths.item("app", "main", "f");
        let g = paths.item("app", "main", "g");
        let mut b = TirBuilder::new(f, BodyKind::Fn);
        let n = NodeIdx::from_raw(0);
        let blk = b.open_block();
        let x = b.local(Ty::I32, syms.intern("x"), local_flags::PARAM, n);
        let get = b.get(x, Ty::I32, n);
        b.emit(Tag::Return, get.0, crate::ir::NONE, Ty::NEVER, n);
        let root = b.close_block(blk, None, Ty::NEVER, n);
        let mut body = b.finish(root, &[]).expect("verifies");
        let checked = write_body(&body, &pool, &paths, &syms).expect("wire");
        body.susp = vec![SuspRow {
            inst: 1,
            scopes: Range32::new(0, 0),
            hook_site: crate::ir::NONE,
        }];
        body.exit_drops = vec![(SubId::from_raw(0), LocalId::from_raw(0))];
        body.storage = vec![(SubId::from_raw(0), LocalId::from_raw(0), Storage::Cell)];
        body.unwind_drops = vec![(1, 2)];
        body.live = vec![(1, vec![3, 4]), (2, vec![])];
        body.full_cells = vec![(0, 0)];
        body.stable_lists = vec![(2, crate::ir::place::BARE_LOCAL)];
        body.anchors = vec![0, crate::ir::NONE, 2];
        body.inlined = vec![[3, 2, g.raw(), 1]];
        body.inlined_origins = vec![[4, 1]];
        body.inlined_shared = vec![(SubId::from_raw(0), 0)];
        body.inlined_capture_origins = vec![(
            SubId::from_raw(0),
            LocalId::from_raw(1),
            LocalId::from_raw(0),
        )];
        let bytes = write_body(&body, &pool, &paths, &syms).expect("wire");
        assert_ne!(
            tir_hash(&bytes),
            tir_hash(&checked),
            "the callee's path row"
        );
        let (pool2, paths2, syms2) = (
            InternPool::new(),
            PathTable::new(),
            ShardedInterner::default(),
        );
        paths2.item("zzz", "a", "b");
        let back = read_body(&bytes, &pool2, &paths2, &syms2).expect("read");
        assert_eq!(
            back.inlined,
            vec![[3, 2, paths2.item("app", "main", "g").raw(), 1]]
        );
        let mut want = body.clone();
        want.item = back.item;
        want.inlined = back.inlined.clone();
        want.local_name = back.local_name.clone();
        assert_eq!(back, want);
        let again = write_body(&back, &pool2, &paths2, &syms2).expect("wire");
        assert_eq!(bytes, again);
        // A checker's body reads back with no pass column.
        let plain = read_body(&checked, &pool2, &paths2, &syms2).expect("read");
        assert!(plain.anchors.is_empty() && plain.inlined.is_empty());
    }

    /// A record nothing reaches, such as an inlined call's callee record,
    /// is not written: two runs that intern paths in another order write
    /// the same bytes, and the reached records move with their pointers.
    #[test]
    fn an_unreached_record_is_not_written() {
        let encode = |earlier: usize| {
            let (pool, paths, syms) = (
                InternPool::new(),
                PathTable::new(),
                ShardedInterner::default(),
            );
            for k in 0..earlier {
                paths.item("app", "other", &format!("x{k}"));
            }
            let f = paths.item("app", "main", "f");
            let g = paths.item("app", "main", "g");
            let mut b = TirBuilder::new(f, BodyKind::Fn);
            let n = NodeIdx::from_raw(0);
            let blk = b.open_block();
            let x = b.local(Ty::I32, syms.intern("x"), local_flags::PARAM, n);
            let get = b.get(x, Ty::I32, n);
            let call = b.call(
                &Callee::Item {
                    def: g,
                    targs: pool.list(&[]),
                },
                &[get],
                Providers::None,
                Ty::I32,
                n,
            );
            b.emit(Tag::Return, call.0, crate::ir::NONE, Ty::NEVER, n);
            let root = b.close_block(blk, None, Ty::NEVER, n);
            let mut body = b.finish(root, &[]).expect("verifies");
            assert!(body.sub_param_locals(0).is_empty());
            // A dead callee record in front of every live one.
            let dead = paths.item("app", "main", "dead").raw();
            let shift = 4;
            for i in 0..body.len() {
                let (oa, ob) = body.tags[i].operands();
                for (k, op) in [oa, ob].into_iter().enumerate() {
                    let w = body.data[i][k];
                    let pointer = matches!(
                        op,
                        crate::ir::Op::Record | crate::ir::Op::Values | crate::ir::Op::Blocks
                    ) || (op == crate::ir::Op::Place
                        && w & crate::ir::place::BARE_LOCAL == 0);
                    if pointer && w != crate::ir::NONE {
                        body.data[i][k] = w + shift;
                    }
                }
            }
            // A suspension row's scope range and a root with a parameter
            // record point into `extra` too.
            let list = u32::try_from(body.extra.len()).expect("extra");
            body.extra.extend([5, 1, 0]);
            body.susp = vec![crate::ir::SuspRow {
                inst: 0,
                scopes: hd_base::Range32::new(list, 1),
                hook_site: crate::ir::NONE,
            }];
            body.sub_params[0] = hd_base::Range32::new(list + 1, 1);
            body.extra.splice(0..0, [3, 0, dead, 0]);
            for r in &mut body.sub_params {
                r.start += shift;
            }
            body.susp[0].scopes.start += shift;
            // A nested range is authoritative even when its prefix names
            // more words. Empty nested ranges may have unrelated or stale
            // offsets, just like a copied body's default root.
            let child = list + 3 + shift;
            body.extra.extend([7, 0]);
            for range in [
                hd_base::Range32::new(child, 1),
                hd_base::Range32::default(),
                hd_base::Range32::new(u32::MAX, 0),
            ] {
                body.sub_root.push(root.0);
                body.sub_params.push(range);
                body.sub_parent.push(hd_base::SubId::from_raw(0));
                body.sub_flags.push(0);
            }
            assert_eq!(body.sub_param_locals(1), &[0]);
            assert!(body.sub_param_locals(2).is_empty());
            assert!(body.sub_param_locals(3).is_empty());
            let bytes = write_body(&body, &pool, &paths, &syms).expect("wire");
            let back = read_body(&bytes, &pool, &paths, &syms).expect("read");
            assert_eq!(back.extra.len() + 4, body.extra.len());
            assert_eq!(back.extra[back.susp[0].scopes.start as usize], 5);
            assert_eq!(back.sub_param_locals(0), &[0]);
            assert_eq!(back.sub_param_locals(1), &[0]);
            for sub in [2, 3] {
                assert!(back.sub_param_locals(sub).is_empty());
                assert_eq!(back.sub_params[sub], hd_base::Range32::default());
            }
            let words = back.record(back.data[call.0 as usize][0]);
            assert_eq!(
                words[1],
                g.raw(),
                "the live callee record moved with its call"
            );
            bytes
        };
        assert_eq!(encode(0), encode(3));
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
