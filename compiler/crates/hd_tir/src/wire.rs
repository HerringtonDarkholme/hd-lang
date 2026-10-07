//! The wire form of a body (data-structures.md §3.18, §3.20.2;
//! checking-and-tir.md §4.13.11): entry-local tables, then the columns,
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

use crate::ir::{Body, BodyKind, CaptureMode, ChoiceKind, Tag};

const KINDS: [BodyKind; 6] = [
    BodyKind::Fn,
    BodyKind::Init,
    BodyKind::TestCase,
    BodyKind::Fact,
    BodyKind::Default,
    BodyKind::DeriveInstance,
];

/// Where a word of `extra` holds an ID.
#[derive(Clone, Copy)]
enum IdWord {
    Def(usize),
    Ty(usize),
    List(usize),
}

/// The ID words of one instruction's records, as `extra` positions.
fn id_words(b: &Body, i: usize) -> StageResult<Vec<IdWord>> {
    let [a, _] = b.data[i];
    Ok(match b.tags[i] {
        Tag::Call => return id_words_call(b, a as usize + 1),
        Tag::Await => return id_words_call(b, a as usize + 1),
        // `ProviderGet` and `ItemRef` keep their IDs in one-word records.
        Tag::ProviderGet => vec![IdWord::Ty(a as usize + 1)],
        Tag::DefaultCall => vec![IdWord::Def(a as usize + 1), IdWord::List(a as usize + 2)],
        Tag::ItemRef => vec![
            IdWord::Def(a as usize + 1),
            IdWord::List(b.data[i][1] as usize + 1),
        ],
        Tag::LocalGet
        | Tag::LocalSet
        | Tag::Prim
        | Tag::CallHost
        | Tag::CallValue
        | Tag::AwaitValue
        | Tag::AwaitAll
        | Tag::AwaitRace
        | Tag::Intrinsic
        | Tag::NewData
        | Tag::NewVariant
        | Tag::NewTuple
        | Tag::NewList
        | Tag::NewMap
        | Tag::Field
        | Tag::FieldSet
        | Tag::TupleGet
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
        | Tag::Payload
        | Tag::Unwrap
        | Tag::Guard
        | Tag::ToArm
        | Tag::Unreachable
        | Tag::Poison => vec![],
        other => {
            return Err(NotImplemented::new(
                Stage::ModuleFinish,
                format!("wire form of TIR tag {}", other.name()),
            ));
        }
    })
}

/// The ID words of a callee record starting at `at`.
fn id_words_call(b: &Body, at: usize) -> StageResult<Vec<IdWord>> {
    Ok(match b.extra.get(at) {
        Some(0) => vec![IdWord::Def(at + 1), IdWord::List(at + 2)],
        Some(1) => {
            let mut v = vec![
                IdWord::Def(at + 1),
                IdWord::Def(at + 2),
                IdWord::Ty(at + 3),
                IdWord::List(at + 4),
            ];
            if b.extra.get(at + 5) == Some(&(ChoiceKind::Impl as u32)) {
                v.push(IdWord::Def(at + 6));
            }
            v
        }
        _ => {
            return Err(NotImplemented::new(
                Stage::ModuleFinish,
                "wire form of a malformed callee record",
            ));
        }
    })
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
        for w in id_words(b, i)? {
            match w {
                IdWord::Def(at) => extra[at] = t.def(DefId::from_raw(extra[at])),
                IdWord::Ty(at) => extra[at] = t.ty(Ty(extra[at]))?,
                IdWord::List(at) => extra[at] = t.list(TyList(extra[at]))?,
            }
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
    // Locations last, outside the hashed prefix: moving a body in its file
    // changes node indices, not the body (cache.md §5.3).
    let hashed = u32::try_from(w.bytes.len()).expect("body over 4 GiB");
    u32s(&mut w, b.syn.iter().map(|n| n.raw()));
    u32s(&mut w, b.local_syn.iter().map(|n| n.raw()));
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
    b.syn = col(&mut r).into_iter().map(NodeIdx::from_raw).collect();
    b.local_syn = col(&mut r).into_iter().map(NodeIdx::from_raw).collect();
    if !r.ok() || b.data.len() != b.tags.len() || b.ty.len() != b.tags.len() {
        return None;
    }
    for i in 0..b.len() {
        for w in id_words(&b, i).ok()? {
            match w {
                IdWord::Def(at) => b.extra[at] = t.def(*b.extra.get(at)?)?.raw(),
                IdWord::Ty(at) => b.extra[at] = t.ty(*b.extra.get(at)?)?.0,
                IdWord::List(at) => b.extra[at] = t.list(*b.extra.get(at)?)?.0,
            }
        }
    }
    Some(b)
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
    use super::{read_body, tir_hash, write_body};
    use crate::ir::{BodyKind, Callee, Providers, Tag, TirBuilder, TirSink, local_flags};
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
}
