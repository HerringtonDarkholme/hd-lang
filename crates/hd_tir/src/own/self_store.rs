//! A store of a struct that keeps fields of the place it replaces
//! (`items[i] = Item { ...items[i], n: 1 }`) stores only the fields it
//! changes. A kept field takes no count and no write, so the old value stays
//! whole until the field stores, and its buffer gains no second owner.
//!
//! This is the fourth fact of a value, its source doesn't change while it
//! lives ([`super::derive`]), carried up to the store. A kept read's chain up
//! to the place is all borrows, which the derivation settled, and nothing
//! from the first kept read to the store can write. Past the last use of a
//! read the store itself would change it, so a value used after the store is
//! no borrow here and keeps its copies.
//!
//! A `data` value's copy is a new object, so it isn't one of these.

use super::values::{self, Local};
use super::{OwnCx, operands, splice};
use crate::ir::{
    Body, Callee, NONE, Proj, ReadMode, Ref, Root, Tag, Topology, place, ref_word, u32_of,
};

/// What a place's chain of borrowed reads ends at, and all its steps.
struct Resolved {
    root: Root,
    steps: Vec<Proj>,
}

struct Facts {
    uses: Vec<u32>,
    /// The one `Store` of all of each local, if it has just one store.
    stored: Vec<u32>,
    classes: Vec<Local>,
}

impl Facts {
    fn of(b: &Body) -> Self {
        let mut facts = Facts {
            uses: vec![0; b.len()],
            stored: vec![NONE; b.local_ty.len()],
            classes: Vec::new(),
        };
        values::classify(b, &mut facts.classes);
        let mut ops = Vec::new();
        for i in 0..b.len() {
            operands(b, i, &mut ops);
            for op in &ops {
                facts.uses[op.value as usize] += 1;
            }
            if b.tags[i] == Tag::Store
                && let Some(l) = b.view(b.data[i][0]).local()
            {
                facts.stored[l as usize] = u32_of(i);
            }
        }
        facts
    }

    /// The `Store` that makes local `l` a binding: `l` names one value, a
    /// borrowed read, in store `at`'s block.
    fn binding(&self, b: &Body, topo: &Topology, l: u32, at: usize) -> Option<u32> {
        let s = self.stored[l as usize];
        (self.classes[l as usize] == Local::Value(1)
            && s != NONE
            && b.view(b.data[s as usize][0]).whole()
            && topo.blk[s as usize] == topo.blk[at])
            .then(|| b.data[s as usize][1])
            .filter(|&v| borrowed_read(b, v))
    }

    /// Follows place `word` through borrowed reads and bindings to where it
    /// starts. Each read must stand in store `at`'s block, so all of them ran
    /// in the same pass through it.
    fn resolve(&self, b: &Body, topo: &Topology, word: u32, at: usize) -> Resolved {
        let mut steps = Vec::new();
        let mut word = word;
        loop {
            let p = b.view(word);
            let mut own: Vec<Proj> = p.steps(b).collect();
            own.append(&mut steps);
            steps = own;
            let next = match p.root {
                Root::Value(v) if borrowed_read(b, v) && topo.blk[v as usize] == topo.blk[at] => {
                    Some(v)
                }
                Root::Local(l) => self.binding(b, topo, l, at),
                _ => None,
            };
            let Some(next) = next else {
                return Resolved {
                    root: p.root,
                    steps,
                };
            };
            word = b.data[next as usize][0];
        }
    }

    /// Whether two index values are one value: the same value or constant,
    /// or reads of a local nothing else reaches, with no store between them.
    fn same_index(&self, b: &Body, x: Ref, y: Ref) -> bool {
        if x == y {
            return true;
        }
        let (Some(rx), Some(ry)) = (x.as_inst(), y.as_inst()) else {
            let constant = |r: Ref| b.consts.get((r.0 & !Ref::CONST_BIT) as usize);
            return x.as_inst().is_none() && y.as_inst().is_none() && constant(x) == constant(y);
        };
        let (rx, ry) = (rx.0 as usize, ry.0 as usize);
        if b.tags[rx] != Tag::Read || b.tags[ry] != Tag::Read {
            return false;
        }
        let (px, py) = (b.view(b.data[rx][0]), b.view(b.data[ry][0]));
        let (Some(l), Some(m)) = (px.local(), py.local()) else {
            return false;
        };
        l == m
            && px.whole()
            && py.whole()
            && matches!(self.classes[l as usize], Local::Value(_))
            && !(rx.min(ry)..rx.max(ry))
                .any(|i| b.tags[i] == Tag::Store && b.view(b.data[i][0]).local() == Some(l))
    }

    fn same_steps(&self, b: &Body, x: &[Proj], y: &[Proj]) -> bool {
        x.len() == y.len()
            && x.iter().zip(y).all(|(s, t)| match (*s, *t) {
                (Proj::Elem(i), Proj::Elem(j)) => self.same_index(b, i, j),
                (Proj::Field(i), Proj::Field(j)) => i == j,
                (Proj::Deref, Proj::Deref) => true,
                _ => false,
            })
    }
}

fn borrowed_read(b: &Body, v: u32) -> bool {
    b.tags.get(v as usize) == Some(&Tag::Read) && b.data[v as usize][1] == ReadMode::Borrow as u32
}

/// The (field, value) pairs of a struct's construction.
fn parts(b: &Body, a: usize) -> Option<Vec<(u32, u32)>> {
    match b.tags[a] {
        Tag::CopyStruct => Some(
            b.record(b.data[a][1])
                .chunks(2)
                .map(|w| (w[0], w[1]))
                .collect(),
        ),
        Tag::NewStruct => Some(
            b.record(b.data[a][1])
                .iter()
                .enumerate()
                .map(|(k, &v)| (u32_of(k), v))
                .collect(),
        ),
        _ => None,
    }
}

/// Whether instruction `i` leaves place `target` as it is. Anything not named
/// here may write it. A uniqueness check of what holds the place only moves
/// the same contents to a buffer of its own.
fn leaves(
    b: &Body,
    cx: &dyn OwnCx,
    topo: &Topology,
    f: &Facts,
    i: usize,
    target: &Resolved,
) -> bool {
    match b.tags[i] {
        Tag::Read => b.data[i][1] != ReadMode::Move as u32,
        Tag::Ref => b.data[i][1] & ref_word::MUT == 0,
        Tag::Prim | Tag::NewStruct | Tag::CopyStruct | Tag::NewList | Tag::NewMap => true,
        Tag::Call => Callee::from_words(b.record(b.data[i][0])).is_some_and(|c| !cx.may_write(&c)),
        Tag::IsUnique => {
            let check = f.resolve(b, topo, b.data[i][0], i);
            check.root == target.root
                && check.steps.len() < target.steps.len()
                && f.same_steps(b, &check.steps, &target.steps[..check.steps.len()])
        }
        _ => false,
    }
}

/// One store's rewrite: its kept reads, its changed (field, value) pairs.
struct Narrowed {
    kept: Vec<u32>,
    changed: Vec<(u32, u32)>,
}

/// The rewrite of store `s`, if it keeps a counted field of its own place.
fn rewrite(b: &Body, cx: &dyn OwnCx, topo: &Topology, f: &Facts, s: usize) -> Option<Narrowed> {
    let a = Ref(b.data[s][1]).as_inst()?.0 as usize;
    let pairs = parts(b, a)?;
    if f.uses[a] != 1 || !cx.in_place_struct(b.ty[a]) || topo.blk[a] != topo.blk[s] {
        return None;
    }
    let mut target = f.resolve(b, topo, b.data[s][0], s);
    if !matches!(target.root, Root::Local(_) | Root::Value(_)) {
        return None;
    }
    // A spread's base supplies every field it doesn't list, so it must be the
    // place too, and what it supplies is kept.
    let mut first = s;
    let mut counts = false;
    if b.tags[a] == Tag::CopyStruct {
        let base = b.data[a][0];
        if !borrowed_read(b, base) || topo.blk[base as usize] != topo.blk[s] {
            return None;
        }
        let from = f.resolve(b, topo, b.data[base as usize][0], s);
        if from.root != target.root || !f.same_steps(b, &from.steps, &target.steps) {
            return None;
        }
        first = base as usize;
        counts = cx.counted(b.ty[a]);
    }
    let (mut kept, mut changed) = (Vec::new(), Vec::new());
    for &(field, value) in &pairs {
        let read = Ref(value).as_inst().map(|r| r.0 as usize);
        let is_kept = read.is_some_and(|r| {
            b.tags[r] == Tag::Read
                && b.data[r][1] == ReadMode::Copy as u32
                && f.uses[r] == 1
                && topo.blk[r] == topo.blk[s]
                && {
                    let from = f.resolve(b, topo, b.data[r][0], s);
                    target.steps.push(Proj::Field(field));
                    let same =
                        from.root == target.root && f.same_steps(b, &from.steps, &target.steps);
                    target.steps.pop();
                    same
                }
        });
        if let (true, Some(r)) = (is_kept, read) {
            counts |= cx.counted(b.ty[r]);
            kept.push(u32_of(r));
        } else {
            changed.push((field, value));
        }
    }
    if !counts || changed.is_empty() {
        return None;
    }
    let first = kept.iter().fold(first, |at, &r| at.min(r as usize));
    (first..s)
        .all(|i| leaves(b, cx, topo, f, i, &target))
        .then_some(Narrowed { kept, changed })
}

/// A record for place `word` with one more field step.
fn field_place(b: &mut Body, word: u32, field: u32) -> u32 {
    let words: Vec<u32> = if word & place::BARE_LOCAL != 0 {
        vec![place::ROOT_LOCAL, word & !place::BARE_LOCAL]
    } else {
        b.record(word).to_vec()
    };
    let at = u32_of(b.extra.len());
    b.extra.push(u32_of(words.len() + 1));
    b.extra.extend(words);
    b.extra.push(place::PROJ_FIELD | field);
    at
}

/// Whether `b` stores a struct it constructs, the only stores [`run`] reads.
pub(crate) fn applies(b: &Body) -> bool {
    (0..b.len()).any(|s| {
        b.tags[s] == Tag::Store
            && Ref(b.data[s][1])
                .as_inst()
                .is_some_and(|a| matches!(b.tags[a.0 as usize], Tag::CopyStruct | Tag::NewStruct))
    })
}

/// Narrows each such store, as edits. The kept reads and the construction go,
/// and each changed field gets a `Store` of its own, which the overwrite plan
/// then ends the old value of.
pub(crate) fn run(b: &mut Body, cx: &dyn OwnCx, topo: &Topology, edits: &mut splice::Tables) {
    let f = Facts::of(b);
    for s in 0..b.len() {
        if b.tags[s] != Tag::Store {
            continue;
        }
        let Some(Narrowed { kept, changed }) = rewrite(b, cx, topo, &f, s) else {
            continue;
        };
        let a = b.data[s][1] as usize;
        let word = b.data[s][0];
        for (k, &(field, value)) in changed.iter().enumerate() {
            let narrow = field_place(b, word, field);
            if k == 0 {
                b.data[s] = [narrow, value];
            } else {
                let at = topo.pos[s];
                edits.insert(
                    topo.blk[s],
                    at,
                    Tag::Store,
                    [narrow, value],
                    b.ty[s],
                    b.syn[s],
                );
            }
        }
        edits.remove(u32_of(a));
        for r in kept {
            edits.remove(r);
        }
    }
}
