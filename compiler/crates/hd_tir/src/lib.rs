//! `hd_tir`: the run's type and item tables (`world`) and TIR
//! (checking-and-tir.md §4.13.11): one typed IR per body, in columns, with
//! the tags the subset needs and the catalog's operand shapes; the wire form
//! with remapping and the TIR hash; a printer and a verifier stub.

use std::collections::HashMap;

use hd_base::{Hash128, Symbol};
use hd_iface::{CTy, KeyHasher, Reader, put_str, put_u32};

pub mod ir;
pub mod print;
pub mod verify;
pub mod world;

use crate::world::{DefId, Ty, World};

pub const NONE: u32 = u32::MAX;
pub const CONST_BIT: u32 = 1 << 31;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TirTag {
    LocalGet,
    LocalSet,
    Prim,
    Call,
    Intrinsic,
    NewData,
    Field,
    Block,
    If,
    Loop,
    Break,
    Return,
}
const _: () = assert!(core::mem::size_of::<TirTag>() == 1);

impl TirTag {
    #[must_use]
    pub fn from_u8(v: u8) -> Self {
        use TirTag::{
            Block, Break, Call, Field, If, Intrinsic, LocalGet, LocalSet, Loop, NewData, Prim,
            Return,
        };
        [
            LocalGet, LocalSet, Prim, Call, Intrinsic, NewData, Field, Block, If, Loop, Break,
            Return,
        ][v as usize]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub enum PrimOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    Neg,
    And,
    Or,
}
impl PrimOp {
    #[must_use]
    pub fn from_u32(v: u32) -> Self {
        use PrimOp::{Add, And, Div, Eq, Ge, Gt, Le, Lt, Mul, Ne, Neg, Or, Rem, Sub};
        [
            Add, Sub, Mul, Div, Rem, Eq, Ne, Lt, Le, Gt, Ge, Neg, And, Or,
        ][v as usize]
    }
}

/// Intrinsics: `println` of an `i32`, a host import.
pub const INTRINSIC_PRINTLN_I32: u32 = 0;

/// Callee record kinds and choices (the callee record of §4.13.11).
pub const CALLEE_ITEM: u32 = 0;
pub const CALLEE_TRAIT_METHOD: u32 = 1;
pub const CHOICE_IMPL: u32 = 0;
pub const CHOICE_BOUND: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Inst(pub u32);

#[derive(Clone, Debug, Default)]
pub struct TirBody {
    pub item: Option<DefId>,
    pub tags: Vec<TirTag>,
    pub data: Vec<[u32; 2]>,
    pub ty: Vec<Ty>,
    pub syn: Vec<(u32, u32)>, // span bytes; the design keeps a NodeIdx here
    pub extra: Vec<u32>,
    pub local_ty: Vec<Ty>,
    pub local_name: Vec<Symbol>,
    pub local_flags: Vec<u8>,
    pub sub_root: Vec<Inst>,
    pub label_inst: Vec<Inst>,
    /// Representation summary per type parameter (codegen.md §13.2, A1):
    /// 1 when the body needs the exact representation, 0 when it only moves.
    pub rep_exact: Vec<u8>,
}

pub const LOCAL_PARAM: u8 = 1;

impl TirBody {
    pub fn push(&mut self, tag: TirTag, a: u32, b: u32, ty: Ty, syn: (u32, u32)) -> Inst {
        let i = Inst(u32::try_from(self.tags.len()).expect("insts"));
        self.tags.push(tag);
        self.data.push([a, b]);
        self.ty.push(ty);
        self.syn.push(syn);
        i
    }
    /// A `[len, items...]` record in `extra`; returns its start.
    pub fn list(&mut self, items: &[u32]) -> u32 {
        let start = u32::try_from(self.extra.len()).expect("extra");
        self.extra.push(u32::try_from(items.len()).expect("len"));
        self.extra.extend_from_slice(items);
        start
    }
    #[must_use]
    pub fn get_list(&self, at: u32) -> &[u32] {
        let n = self.extra[at as usize] as usize;
        &self.extra[at as usize + 1..at as usize + 1 + n]
    }
    pub fn local(&mut self, name: Symbol, ty: Ty, flags: u8) -> u32 {
        let id = u32::try_from(self.local_ty.len()).expect("locals");
        self.local_ty.push(ty);
        self.local_name.push(name);
        self.local_flags.push(flags);
        id
    }
    #[must_use]
    pub fn tag(&self, i: u32) -> TirTag {
        self.tags[i as usize]
    }
}

// ----------------------------------------------------------- the wire form

/// Entry-local tables: every `DefId`, `Ty` and constant is a row here, so the
/// bytes hold no run ID (§4.13.11, "Remap, not copy").
#[derive(Default)]
pub struct Tables {
    pub paths: Vec<String>,
    pub types: Vec<CTy>,
    pub consts: Vec<(u32, u64)>, // (type row, bits)
    path_rows: HashMap<DefId, u32>,
    type_rows: HashMap<Ty, u32>,
    const_rows: HashMap<(u32, u64), u32>,
}
impl Tables {
    fn path(&mut self, w: &World, d: DefId) -> u32 {
        if let Some(&r) = self.path_rows.get(&d) {
            return r;
        }
        let r = u32::try_from(self.paths.len()).expect("row");
        self.paths.push(w.path(d).to_owned());
        self.path_rows.insert(d, r);
        r
    }
    fn ty(&mut self, w: &World, t: Ty) -> u32 {
        if let Some(&r) = self.type_rows.get(&t) {
            return r;
        }
        let r = u32::try_from(self.types.len()).expect("row");
        self.types.push(w.canon(t));
        self.type_rows.insert(t, r);
        r
    }
    fn konst(&mut self, w: &World, c: u32) -> u32 {
        let (t, bits) = w.const_value(c);
        let row = (self.ty(w, t), bits);
        if let Some(&r) = self.const_rows.get(&row) {
            return r;
        }
        let r = u32::try_from(self.consts.len()).expect("row");
        self.consts.push(row);
        self.const_rows.insert(row, r);
        r
    }
}

/// Which word kinds an operand holds; the generated codec of the design is
/// this table written by hand.
fn remap_ref(w: &World, t: &mut Tables, r: u32) -> u32 {
    if r != NONE && r & CONST_BIT != 0 {
        t.konst(w, r & !CONST_BIT) | CONST_BIT
    } else {
        r
    }
}

/// Writes one body's columns with ID words remapped to entry rows. Returns
/// the bytes that the TIR hash covers; spans go to a separate, unhashed list.
pub fn write_body(w: &World, t: &mut Tables, b: &TirBody, out: &mut Vec<u8>, spans: &mut Vec<u8>) {
    put_u32(out, u32::try_from(b.tags.len()).expect("n"));
    // `extra` is rebuilt record by record, remapped.
    let mut extra: Vec<u32> = Vec::new();
    let list = |extra: &mut Vec<u32>, items: Vec<u32>| -> u32 {
        let s = u32::try_from(extra.len()).expect("extra");
        extra.push(u32::try_from(items.len()).expect("n"));
        extra.extend(items);
        s
    };
    for i in 0..b.tags.len() {
        let [a, bb] = b.data[i];
        let (na, nb) = match b.tags[i] {
            TirTag::LocalGet | TirTag::Break => (a, bb),
            TirTag::LocalSet => (a, remap_ref(w, t, bb)),
            TirTag::Prim | TirTag::Intrinsic | TirTag::NewData => {
                let items = b.get_list(bb).iter().map(|&r| remap_ref(w, t, r)).collect();
                (a, list(&mut extra, items))
            }
            TirTag::Call => {
                let rec = b.get_list(a).to_vec();
                let mut nrec = vec![rec[0]];
                if rec[0] == CALLEE_ITEM {
                    nrec.push(t.path(w, DefId(rec[1])));
                    nrec.extend(rec[2..].iter().map(|&x| t.ty(w, Ty(x))));
                } else {
                    // [kind, trait, method index, self ty, choice kind, choice value, type args...]
                    nrec.push(t.path(w, DefId(rec[1])));
                    nrec.push(rec[2]);
                    nrec.push(t.ty(w, Ty(rec[3])));
                    nrec.push(rec[4]);
                    nrec.push(if rec[4] == CHOICE_IMPL {
                        t.path(w, DefId(rec[5]))
                    } else {
                        rec[5]
                    });
                }
                let ra = list(&mut extra, nrec);
                let args = b.get_list(bb).iter().map(|&r| remap_ref(w, t, r)).collect();
                (ra, list(&mut extra, args))
            }
            TirTag::Field => (remap_ref(w, t, a), bb),
            TirTag::Block => {
                let items = b.get_list(a).to_vec();
                (list(&mut extra, items), remap_ref(w, t, bb))
            }
            TirTag::If => {
                let items = b.get_list(bb).to_vec();
                (remap_ref(w, t, a), list(&mut extra, items))
            }
            TirTag::Loop => (a, bb),
            TirTag::Return => (remap_ref(w, t, a), bb),
        };
        out.push(b.tags[i] as u8);
        put_u32(out, na);
        put_u32(out, nb);
        put_u32(out, t.ty(w, b.ty[i]));
        put_u32(spans, b.syn[i].0);
        put_u32(spans, b.syn[i].1);
    }
    put_u32(out, u32::try_from(extra.len()).expect("n"));
    for x in extra {
        put_u32(out, x);
    }
    put_u32(out, u32::try_from(b.local_ty.len()).expect("n"));
    for l in 0..b.local_ty.len() {
        put_u32(out, t.ty(w, b.local_ty[l]));
        put_str(out, w.text(b.local_name[l]));
        out.push(b.local_flags[l]);
    }
    put_u32(out, u32::try_from(b.sub_root.len()).expect("n"));
    for s in &b.sub_root {
        put_u32(out, s.0);
    }
    put_u32(out, u32::try_from(b.label_inst.len()).expect("n"));
    for s in &b.label_inst {
        put_u32(out, s.0);
    }
    put_u32(out, u32::try_from(b.rep_exact.len()).expect("n"));
    out.extend_from_slice(&b.rep_exact);
}

pub fn write_tables(t: &Tables, out: &mut Vec<u8>) {
    put_u32(out, u32::try_from(t.paths.len()).expect("n"));
    for p in &t.paths {
        put_str(out, p);
    }
    put_u32(out, u32::try_from(t.types.len()).expect("n"));
    for c in &t.types {
        c.encode(out);
    }
    put_u32(out, u32::try_from(t.consts.len()).expect("n"));
    for (ty, bits) in &t.consts {
        put_u32(out, *ty);
        out.extend_from_slice(&bits.to_le_bytes());
    }
}

/// Reads the entry tables, interning every row into this run's pools.
pub struct RunRows {
    pub paths: Vec<DefId>,
    pub types: Vec<Ty>,
    pub consts: Vec<u32>,
}
pub fn read_tables(w: &mut World, r: &mut Reader<'_>) -> RunRows {
    let n = r.u32();
    let paths = (0..n).map(|_| r.str()).map(|p| w.def(&p)).collect();
    let n = r.u32();
    let cts: Vec<CTy> = (0..n).map(|_| CTy::decode(r)).collect();
    let types: Vec<Ty> = cts.iter().map(|c| w.intern_canon(c)).collect();
    let n = r.u32();
    let consts = (0..n)
        .map(|_| {
            let ty = types[r.u32() as usize];
            let bits = r.u64();
            w.konst(ty, bits)
        })
        .collect();
    RunRows {
        paths,
        types,
        consts,
    }
}

fn unmap_ref(rows: &RunRows, r: u32) -> u32 {
    if r != NONE && r & CONST_BIT != 0 {
        rows.consts[(r & !CONST_BIT) as usize] | CONST_BIT
    } else {
        r
    }
}

pub fn read_body(w: &mut World, rows: &RunRows, r: &mut Reader<'_>) -> TirBody {
    let mut b = TirBody::default();
    let n = r.u32();
    let mut raw = Vec::new();
    for _ in 0..n {
        let tag = TirTag::from_u8(r.u8());
        let a = r.u32();
        let bb = r.u32();
        let ty = rows.types[r.u32() as usize];
        raw.push((tag, a, bb, ty));
    }
    let ne = r.u32();
    let wire_extra: Vec<u32> = (0..ne).map(|_| r.u32()).collect();
    let get = |at: u32| -> Vec<u32> {
        let n = wire_extra[at as usize] as usize;
        wire_extra[at as usize + 1..at as usize + 1 + n].to_vec()
    };
    for (tag, a, bb, ty) in raw {
        let (na, nb) = match tag {
            TirTag::LocalGet | TirTag::Break | TirTag::Loop => (a, bb),
            TirTag::LocalSet => (a, unmap_ref(rows, bb)),
            TirTag::Prim | TirTag::Intrinsic | TirTag::NewData => {
                let items: Vec<u32> = get(bb).iter().map(|&x| unmap_ref(rows, x)).collect();
                (a, b.list(&items))
            }
            TirTag::Call => {
                let rec = get(a);
                let mut nrec = vec![rec[0], rows.paths[rec[1] as usize].0];
                if rec[0] == CALLEE_ITEM {
                    nrec.extend(rec[2..].iter().map(|&x| rows.types[x as usize].0));
                } else {
                    nrec.push(rec[2]);
                    nrec.push(rows.types[rec[3] as usize].0);
                    nrec.push(rec[4]);
                    nrec.push(if rec[4] == CHOICE_IMPL {
                        rows.paths[rec[5] as usize].0
                    } else {
                        rec[5]
                    });
                }
                let ra = b.list(&nrec);
                let args: Vec<u32> = get(bb).iter().map(|&x| unmap_ref(rows, x)).collect();
                (ra, b.list(&args))
            }
            TirTag::Field | TirTag::Return => (unmap_ref(rows, a), bb),
            TirTag::Block => {
                let items = get(a);
                (b.list(&items), unmap_ref(rows, bb))
            }
            TirTag::If => {
                let items = get(bb);
                (unmap_ref(rows, a), b.list(&items))
            }
        };
        b.push(tag, na, nb, ty, (0, 0));
    }
    let nl = r.u32();
    for _ in 0..nl {
        let ty = rows.types[r.u32() as usize];
        let name = r.str();
        let flags = r.u8();
        let s = w.sym(&name);
        b.local(s, ty, flags);
    }
    let ns = r.u32();
    b.sub_root = (0..ns).map(|_| Inst(r.u32())).collect();
    let nl = r.u32();
    b.label_inst = (0..nl).map(|_| Inst(r.u32())).collect();
    let nr = r.u32();
    b.rep_exact = (0..nr).map(|_| r.u8()).collect();
    b
}

/// The TIR hash: the remapped columns with types and paths by content, and
/// no spans.
#[must_use]
pub fn tir_hash(body_bytes: &[u8], tables: &Tables) -> Hash128 {
    let mut t = Vec::new();
    write_tables(tables, &mut t);
    KeyHasher::new("tir").bytes(&t).bytes(body_bytes).finish()
}
