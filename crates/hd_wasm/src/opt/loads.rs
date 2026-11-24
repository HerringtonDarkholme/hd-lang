//! Redundant-load elimination and store-to-load forwarding (#2343): a load
//! of an address that a dominating load or store already read or wrote
//! reads that value from a local, when no instruction that may write memory
//! and no write of a local the address reads runs between them.
//!
//! This is LLVM `EarlyCSE`'s load rule (`llvm/lib/Transforms/Scalar/EarlyCSE.cpp`:
//! `AvailableLoads` in scoped tables over the dominator tree, a store
//! recorded as the value of its address, and any instruction that
//! `mayWriteToMemory` ending every recorded value) on structured Wasm, with
//! Binaryen `LocalCSE`'s alias rule (`src/passes/LocalCSE.cpp`: any memory
//! write invalidates every load) and its request counting, so a reused
//! parent takes the place of its reused children. Where hd differs:
//!
//! - Structured control flow is the dominator tree: a fact recorded in an
//!   open block, loop or `if` arm dominates everything after it in that
//!   arm, and dies when the arm closes.
//! - `EarlyCSE` forgets every load at a join; here a fact crosses a nested
//!   construct whose instructions write neither memory nor the locals it
//!   reads. One backward walk gives each position the next instruction that
//!   ends a fact made there, so the check is one comparison. A loop entered
//!   after the fact reaches its uses again through its back edge, so its
//!   whole body must leave the fact alone.
//! - No type-based alias facts: a store, `memory.copy` or `memory.fill`, an
//!   atomic access and any call end every fact, as Binaryen's
//!   `EffectAnalyzer` orders them. Wasm keeps no types, and TIR has no rule
//!   yet that a shared loan freezes the bytes behind it.
//!
//! Addresses are value numbers: a local (until its next write), an `i32`
//! constant, or another fact's loaded value, so a chain of loads through
//! one pointer is reused link by link. A store of a local or a constant
//! forwards that operand; another stored value is saved by a `local.tee`
//! before the store, a reused load's value by one after it.
//!
//! Cost: O(N + L) time and memory for N instructions and L locals, plus
//! O(log D) per reused load for D open loops; one backward walk, one forward
//! walk with a hash table of at most one entry per load or store, and one
//! rewrite walk. Nothing iterates or recurses.

use hd_base::HashMap;
use wasm_encoder::{BlockType, MemArg};

use super::{Arity, I, Ins, Remap, effect, is_load};
use crate::module::{Func, Val};

/// No position: a fact that nothing ends, or a slot whose value is not one
/// run of pure instructions.
const NONE: u32 = u32::MAX;

/// A value an address is known to hold.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Vn {
    Local(u32),
    Const(i32),
    Fact(u32),
}

/// An access: its address, its width and extension, its offset and memory.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    address: Vn,
    kind: u8,
    offset: u64,
    memory: u32,
}

/// Where a fact's value can be read again.
#[derive(Clone, Copy)]
enum Value {
    /// In a temporary that a `local.tee` after the load sets.
    Loaded,
    /// In a temporary that a `local.tee` before the store sets.
    Stored,
    /// In the local the store's operand read.
    Local(u32),
    /// The store's constant operand.
    Const(Ins),
}

struct Fact {
    /// The control frame it was made in; it dies when that frame closes.
    frame: u32,
    /// The load or store that made it.
    at: u32,
    /// The first position that may change it.
    until: u32,
    value: Value,
    ty: Val,
    /// Loads that read it in place of their own instructions.
    uses: u32,
}

/// An operand on the walked stack.
#[derive(Clone, Copy)]
struct Slot {
    /// Its value number: a fact's value or a constant is the value itself,
    /// a local's is the local's value until `until`.
    vn: Option<Vn>,
    /// The first instruction of the pure run that pushes it, or `NONE`.
    start: u32,
    /// The instruction that pushed it.
    end: u32,
    /// The first position that may change what `vn` names.
    until: u32,
}

impl Slot {
    const OPAQUE: Self = Self {
        vn: None,
        start: NONE,
        end: NONE,
        until: NONE,
    };

    /// What a write at `at` of frame `frame` leaves its local holding: a
    /// value number that stays the value itself, never another local's.
    fn copy(self, at: u32, frame: u32) -> Option<(Vn, u32, u32)> {
        match self.vn? {
            Vn::Local(_) => None,
            vn => Some((vn, at, frame)),
        }
    }
}

struct Frame {
    serial: u32,
    base: usize,
    results: usize,
    dead: bool,
    looped: bool,
}

pub fn run(f: &mut Func, arity: &Arity) -> bool {
    run_with(f, arity, &mut || {})
}

/// [`run`], calling `visit` once per instruction each walk examines.
pub fn run_with(f: &mut Func, arity: &Arity, visit: &mut impl FnMut()) -> bool {
    let Some((next, loop_ends)) = next_changes(f, visit) else {
        return false;
    };
    let Some(walk) = Walk::new(f, next, loop_ends).run(f, arity, visit) else {
        return false;
    };
    if walk.replaced.is_empty() {
        return false;
    }
    rewrite(f, &walk, visit);
    true
}

/// For each position, the next instruction after it that may change a fact
/// made there: for a local access, the next write of its local; for any
/// other instruction, the next one that may write memory. Also each loop
/// opener's `end`. `None` when a frame expansion is left, which writes
/// both.
fn next_changes(f: &Func, visit: &mut impl FnMut()) -> Option<(Vec<u32>, HashMap<u32, u32>)> {
    let n = f.body.len();
    u32::try_from(n).ok().filter(|&n| n < NONE)?;
    let mut next = vec![NONE; n];
    let mut write = vec![NONE; f.params.len() + f.locals.len()];
    let mut kill = NONE;
    let mut ends = Vec::new();
    let mut loop_ends = HashMap::default();
    for at in (0..n).rev() {
        visit();
        let position = u32::try_from(at).ok()?;
        match f.body[at] {
            Ins::Op(I::End) => ends.push(position),
            Ins::Op(I::Loop(_)) => {
                loop_ends.insert(position, ends.pop()?);
            }
            Ins::Op(I::Block(_) | I::If(_)) => {
                ends.pop()?;
            }
            Ins::Op(I::LocalGet(l)) => next[at] = *write.get(l as usize)?,
            Ins::Op(I::LocalSet(l) | I::LocalTee(l)) => {
                let write = write.get_mut(l as usize)?;
                next[at] = *write;
                *write = position;
            }
            // ruled out: emission expands every frame save and load before
            // the module optimizes (the link refuses one it finds).
            Ins::LoadAll | Ins::SpillBase => return None,
            ref ins => {
                next[at] = kill;
                if writes_memory(ins) {
                    kill = position;
                }
            }
        }
    }
    Some((next, loop_ends))
}

/// Whether `ins` may write memory or order other accesses: a store, a bulk
/// operation, an atomic access, a growth, or a call.
fn writes_memory(ins: &Ins) -> bool {
    match ins {
        Ins::Call(..) | Ins::CallIndirect(..) => true,
        Ins::Op(op) => {
            matches!(
                op,
                I::MemoryCopy { .. }
                    | I::MemoryFill(_)
                    | I::MemoryGrow(_)
                    | I::AtomicFence
                    | I::I32AtomicLoad(_)
                    | I::I32AtomicStore(_)
                    | I::I32AtomicRmwAdd(_)
                    | I::I32AtomicRmwSub(_)
                    | I::I32AtomicRmwCmpxchg(_)
                    | I::MemoryAtomicNotify(_)
                    | I::MemoryAtomicWait32(_)
            ) || store_kind(op).is_some()
        }
        _ => false,
    }
}

/// A load's kind, value type and memory argument.
fn load_kind(op: &I) -> Option<(u8, Val, MemArg)> {
    Some(match *op {
        I::I32Load(m) => (0, Val::I32, m),
        I::I64Load(m) => (1, Val::I64, m),
        I::F32Load(m) => (2, Val::F32, m),
        I::F64Load(m) => (3, Val::F64, m),
        I::I32Load8S(m) => (4, Val::I32, m),
        I::I32Load8U(m) => (5, Val::I32, m),
        I::I32Load16S(m) => (6, Val::I32, m),
        I::I32Load16U(m) => (7, Val::I32, m),
        _ => return None,
    })
}

/// A store's memory argument and, when a load reads back exactly its
/// operand, that load's kind and value type.
fn store_kind(op: &I) -> Option<(Option<(u8, Val)>, MemArg)> {
    Some(match *op {
        I::I32Store(m) => (Some((0, Val::I32)), m),
        I::I64Store(m) => (Some((1, Val::I64)), m),
        I::F32Store(m) => (Some((2, Val::F32)), m),
        I::F64Store(m) => (Some((3, Val::F64)), m),
        I::I32Store8(m) | I::I32Store16(m) => (None, m),
        _ => return None,
    })
}

/// The stack effect of an instruction [`effect`] leaves out.
fn shape(ins: &Ins) -> Option<(usize, usize)> {
    Some(match ins {
        Ins::Static(_) => (0, 1),
        Ins::DrainGlue(_) | Ins::Op(I::AtomicFence) => (0, 0),
        Ins::Op(I::I32AtomicLoad(_)) => (1, 1),
        Ins::Op(I::I32AtomicStore(_)) => (2, 0),
        Ins::Op(I::I32AtomicRmwAdd(_) | I::I32AtomicRmwSub(_) | I::MemoryAtomicNotify(_)) => (2, 1),
        Ins::Op(I::I32AtomicRmwCmpxchg(_) | I::MemoryAtomicWait32(_)) => (3, 1),
        _ => return None,
    })
}

struct Walk {
    next: Vec<u32>,
    frames: Vec<Frame>,
    /// Whether each frame serial is still open.
    open: Vec<bool>,
    /// The open loops, outermost first: their first and `end` positions.
    loops: Vec<(u32, u32)>,
    /// Each loop opener's `end`.
    loop_ends: HashMap<u32, u32>,
    slots: Vec<Slot>,
    /// Each local's last write of a known value: the value, the write, and
    /// the frame it ran in.
    copies: Vec<Option<(Vn, u32, u32)>>,
    table: HashMap<Key, u32>,
    facts: Vec<Fact>,
    /// Reused runs, ascending and disjoint: first, last, fact, and whether
    /// the run is one load whose address is dropped instead.
    replaced: Vec<(u32, u32, u32, bool)>,
}

impl Walk {
    fn new(f: &Func, next: Vec<u32>, loop_ends: HashMap<u32, u32>) -> Self {
        Self {
            next,
            frames: vec![Frame {
                serial: 0,
                base: 0,
                results: 0,
                dead: false,
                looped: false,
            }],
            open: vec![true],
            loops: Vec::new(),
            loop_ends,
            slots: Vec::new(),
            copies: vec![None; f.params.len() + f.locals.len()],
            table: HashMap::default(),
            facts: Vec::new(),
            replaced: Vec::new(),
        }
    }

    /// Whether what position `at` of frame `frame` made, which nothing
    /// changes before position `until`, still holds at a later position
    /// `q`. An open frame dominates `q`; a loop entered after `at` reaches
    /// `q` again through its back edge, so its whole body must leave the
    /// value alone.
    fn holds(&self, frame: u32, at: u32, until: u32, q: u32) -> bool {
        if !self.open[frame as usize] {
            return false;
        }
        let entered = self.loops.partition_point(|&(start, _)| start <= at);
        let reach = self.loops.get(entered).map_or(q, |&(_, end)| end.max(q));
        until > reach
    }

    fn fact_holds(&self, id: u32, q: u32) -> bool {
        let fact = &self.facts[id as usize];
        self.holds(fact.frame, fact.at, fact.until, q)
    }

    fn pop(&mut self) -> Option<Slot> {
        let frame = self.frames.last()?;
        if self.slots.len() <= frame.base {
            return None;
        }
        self.slots.pop()
    }

    fn open_frame(&mut self, results: usize, dead: bool, looped: bool) {
        let serial = u32::try_from(self.open.len()).expect("frames fit positions");
        self.open.push(true);
        self.frames.push(Frame {
            serial,
            base: self.slots.len(),
            results,
            dead,
            looped,
        });
    }

    fn run(mut self, f: &Func, arity: &Arity, visit: &mut impl FnMut()) -> Option<Self> {
        for (at, ins) in (0u32..).zip(&f.body) {
            visit();
            let dead = self.frames.last()?.dead;
            match ins {
                Ins::Op(op @ (I::Block(bt) | I::Loop(bt) | I::If(bt))) => {
                    let results = match bt {
                        BlockType::Empty => 0,
                        BlockType::Result(_) => 1,
                        // ruled out: the emitter builds no block with a
                        // function type, so no block takes parameters.
                        BlockType::FunctionType(_) => return None,
                    };
                    if !dead && matches!(op, I::If(_)) {
                        self.pop()?;
                    }
                    let looped = matches!(op, I::Loop(_));
                    if looped {
                        self.loops.push((at, *self.loop_ends.get(&at)?));
                    }
                    self.open_frame(results, dead, looped);
                }
                Ins::Op(I::Else) => {
                    let frame = self.frames.pop()?;
                    self.open[frame.serial as usize] = false;
                    self.slots.truncate(frame.base);
                    let dead = self.frames.last()?.dead;
                    self.open_frame(frame.results, dead, false);
                }
                Ins::Op(I::End) => {
                    let frame = self.frames.pop()?;
                    self.frames.last()?;
                    self.open[frame.serial as usize] = false;
                    self.slots.truncate(frame.base);
                    if frame.looped {
                        self.loops.pop();
                    }
                    self.slots
                        .extend(std::iter::repeat_n(Slot::OPAQUE, frame.results));
                }
                _ if dead => {}
                Ins::Op(I::Br(_) | I::BrTable(..) | I::Return | I::Unreachable) => {
                    let frame = self.frames.last_mut()?;
                    frame.dead = true;
                    self.slots.truncate(frame.base);
                }
                Ins::Op(I::BrIf(_)) => {
                    self.pop()?;
                }
                Ins::Op(I::LocalGet(l)) => {
                    // A local holds a value number of its own while the
                    // dominating write that gave it is its latest one, as
                    // SSA's copy of a value.
                    let copy = self.copies.get(*l as usize)?.filter(|&(_, set, frame)| {
                        self.holds(frame, set, self.next[set as usize], at)
                    });
                    self.slots.push(Slot {
                        vn: Some(copy.map_or(Vn::Local(*l), |(vn, _, _)| vn)),
                        start: at,
                        end: at,
                        until: if copy.is_some() {
                            NONE
                        } else {
                            self.next[at as usize]
                        },
                    });
                }
                Ins::Op(I::LocalSet(l)) => {
                    let slot = self.pop()?;
                    let frame = self.frames.last()?.serial;
                    *self.copies.get_mut(*l as usize)? = slot.copy(at, frame);
                }
                Ins::Op(I::I32Const(c)) => self.slots.push(Slot {
                    vn: Some(Vn::Const(*c)),
                    start: at,
                    end: at,
                    until: NONE,
                }),
                Ins::Op(I::I64Const(_) | I::F32Const(_) | I::F64Const(_)) => {
                    self.slots.push(Slot {
                        vn: None,
                        start: at,
                        end: at,
                        until: NONE,
                    });
                }
                Ins::Op(I::LocalTee(l)) => {
                    let slot = self.pop()?;
                    let frame = self.frames.last()?.serial;
                    *self.copies.get_mut(*l as usize)? = slot.copy(at, frame);
                    self.slots.push(Slot {
                        start: NONE,
                        end: at,
                        ..slot
                    });
                }
                Ins::Op(op) if load_kind(op).is_some() => {
                    let (kind, ty, m) = load_kind(op)?;
                    let address = self.pop()?;
                    let slot = self.load(at, address, kind, ty, m);
                    self.slots.push(slot);
                }
                Ins::Op(op) if store_kind(op).is_some() => {
                    let (kind, m) = store_kind(op)?;
                    let value = self.pop()?;
                    let address = self.pop()?;
                    if let (Some(address_vn), Some((kind, ty))) = (address.vn, kind) {
                        self.store(f, at, address_vn, address.until, value, kind, ty, m);
                    }
                }
                _ => {
                    let (pops, pushes) = effect(ins, arity, &f.symbols).or_else(|| shape(ins))?;
                    for _ in 0..pops {
                        self.pop()?;
                    }
                    self.slots.extend(std::iter::repeat_n(Slot::OPAQUE, pushes));
                }
            }
        }
        Some(self)
    }

    /// A load at `at` of `address`: the value of a fact that still holds,
    /// or a new fact.
    fn load(&mut self, at: u32, address: Slot, kind: u8, ty: Val, m: MemArg) -> Slot {
        let Some(vn) = address.vn else {
            return Slot::OPAQUE;
        };
        let key = Key {
            address: vn,
            kind,
            offset: m.offset,
            memory: m.memory_index,
        };
        // A run of `local.get`, constants and loads that only feeds the
        // load goes with it; another address is computed and dropped.
        let pure = address.start != NONE && address.end + 1 == at;
        if let Some(&id) = self.table.get(&key)
            && self.fact_holds(id, at)
        {
            if pure {
                while let Some(&(first, _, inner, _)) = self.replaced.last()
                    && first >= address.start
                {
                    self.replaced.pop();
                    self.facts[inner as usize].uses -= 1;
                }
            }
            self.replaced
                .push((if pure { address.start } else { at }, at, id, !pure));
            self.facts[id as usize].uses += 1;
            return Slot {
                vn: Some(Vn::Fact(id)),
                start: if pure { address.start } else { NONE },
                end: at,
                until: NONE,
            };
        }
        let until = self.next[at as usize].min(address.until);
        let id = self.record(key, at, until, Value::Loaded, ty);
        Slot {
            vn: Some(Vn::Fact(id)),
            start: if pure { address.start } else { NONE },
            end: at,
            until: NONE,
        }
    }

    /// A store at `at` makes its operand the value of its address, until
    /// the next write of memory or of a local either reads.
    #[expect(
        clippy::too_many_arguments,
        reason = "one store's operands, each a separate stack slot or immediate"
    )]
    fn store(
        &mut self,
        f: &Func,
        at: u32,
        address: Vn,
        address_until: u32,
        value: Slot,
        kind: u8,
        ty: Val,
        m: MemArg,
    ) {
        let single = value.start != NONE && value.start == value.end;
        let (stored, value_until) = match f.body.get(value.start as usize) {
            Some(&Ins::Op(I::LocalGet(l))) if single => {
                (Value::Local(l), self.next[value.start as usize])
            }
            Some(
                &ins @ Ins::Op(I::I32Const(_) | I::I64Const(_) | I::F32Const(_) | I::F64Const(_)),
            ) if single => (Value::Const(ins), NONE),
            _ => (Value::Stored, NONE),
        };
        let key = Key {
            address,
            kind,
            offset: m.offset,
            memory: m.memory_index,
        };
        let until = self.next[at as usize].min(address_until).min(value_until);
        self.record(key, at, until, stored, ty);
    }

    fn record(&mut self, key: Key, at: u32, until: u32, value: Value, ty: Val) -> u32 {
        let id = u32::try_from(self.facts.len()).expect("facts fit positions");
        let frame = self.frames.last().map_or(0, |f| f.serial);
        self.facts.push(Fact {
            frame,
            at,
            until,
            value,
            ty,
            uses: 0,
        });
        self.table.insert(key, id);
        id
    }
}

/// Replaces each reused run with its fact's value and saves each used
/// value to a new local where its load or store runs.
fn rewrite(f: &mut Func, walk: &Walk, visit: &mut impl FnMut()) {
    let mut temps = vec![NONE; walk.facts.len()];
    for (id, fact) in walk.facts.iter().enumerate() {
        if fact.uses > 0 && matches!(fact.value, Value::Loaded | Value::Stored) {
            temps[id] = u32::try_from(f.params.len() + f.locals.len()).expect("locals");
            f.locals.push(fact.ty);
        }
    }
    // Saves, by position: after a load, before a store.
    let mut saves = walk
        .facts
        .iter()
        .zip(&temps)
        .filter(|(_, t)| **t != NONE)
        .map(|(fact, t)| (fact.at, *t))
        .peekable();
    let mut replaced = walk.replaced.iter().peekable();
    let body = std::mem::take(&mut f.body);
    let mut origins = Remap::new(f.origins.as_deref());
    let mut out = Vec::with_capacity(body.len() + walk.facts.len());
    let mut at = 0usize;
    while at < body.len() {
        visit();
        let position = u32::try_from(at).expect("positions");
        if let Some(&&(first, last, id, dropped)) = replaced.peek()
            && first == position
        {
            replaced.next();
            if dropped {
                out.push(Ins::Op(I::Drop));
            }
            let fact = &walk.facts[id as usize];
            out.push(match fact.value {
                Value::Loaded | Value::Stored => Ins::Op(I::LocalGet(temps[id as usize])),
                Value::Local(l) => Ins::Op(I::LocalGet(l)),
                Value::Const(ins) => ins,
            });
            origins.merged(at..last as usize + 1, 1 + usize::from(dropped));
            at = last as usize + 1;
            // A reused run holds only reused loads, never a fact's origin.
            debug_assert!(saves.peek().is_none_or(|&(p, _)| p as usize >= at));
            continue;
        }
        let save = saves.next_if(|&(p, _)| p == position).map(|(_, t)| t);
        let store = !matches!(body[at], Ins::Op(ref op) if is_load(op));
        if let Some(t) = save.filter(|_| store) {
            out.push(Ins::Op(I::LocalTee(t)));
        }
        out.push(body[at]);
        if let Some(t) = save.filter(|_| !store) {
            out.push(Ins::Op(I::LocalTee(t)));
        }
        origins.merged(at..at + 1, 1 + usize::from(save.is_some()));
        at += 1;
    }
    f.body = out;
    origins.finish(f);
}
