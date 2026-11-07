//! One count statement shape, independent of the boundary that runs it.

use super::{Body, NONE, Tag, Topology};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum At {
    Value,
    Operation,
    Entry,
    Exit,
    Cold,
    CacheRead,
    CacheWrite,
    Materialize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Part {
    Result,
    Local(u32),
    Row,
    Operand(u32),
    Child,
}

/// The record in a retain/release's second word. Its first word names
/// the instruction whose value, operation or function boundary it counts.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Info {
    pub at: At,
    pub part: Part,
    pub reason: u32,
}

impl Info {
    #[must_use]
    pub const fn new(at: At, part: Part, reason: u32) -> Self {
        Self { at, part, reason }
    }

    #[must_use]
    pub fn read(b: &Body, i: usize) -> Option<Self> {
        if !matches!(b.tags.get(i), Some(Tag::Retain | Tag::Release)) {
            return None;
        }
        let [at, part, index, reason] = *b.record(b.data[i][1]) else {
            return None;
        };
        let at = match at {
            0 => At::Value,
            1 => At::Operation,
            2 => At::Entry,
            3 => At::Exit,
            4 => At::Cold,
            5 => At::CacheRead,
            6 => At::CacheWrite,
            7 => At::Materialize,
            _ => return None,
        };
        let part = match (part, index) {
            (0, 0) => Part::Result,
            (1, l) if (l as usize) < b.local_ty.len() => Part::Local(l),
            (2, 0) => Part::Row,
            (3, n) => Part::Operand(n),
            (4, 0) => Part::Child,
            _ => return None,
        };
        (reason < 9).then_some(Self { at, part, reason })
    }

    pub fn write(self, b: &mut Body) -> u32 {
        let (part, index) = match self.part {
            Part::Result => (0, 0),
            Part::Local(l) => (1, l),
            Part::Row => (2, 0),
            Part::Operand(n) => (3, n),
            Part::Child => (4, 0),
        };
        let record = u32::try_from(b.extra.len()).expect("count record");
        b.extra
            .extend([4, self.at as u32, part, index, self.reason]);
        record
    }
}

/// A nonexecuting statement, lowered at its target's operation/boundary.
#[must_use]
pub fn boundary(b: &Body, i: usize) -> bool {
    Info::read(b, i).is_some_and(|c| c.at != At::Value)
}

#[must_use]
pub fn find(b: &Body, tag: Tag, target: u32, at: At, part: Part) -> Option<Info> {
    b.tags.iter().enumerate().find_map(|(i, &t)| {
        (t == tag && b.data[i][0] == target)
            .then(|| Info::read(b, i))
            .flatten()
            .filter(|c| c.at == at && c.part == part)
    })
}

/// A record in the adjacent operation bundle for `target`. Checking the
/// preceding record suffices: the verifier also checks that record's link.
#[must_use]
pub fn adjacent(b: &Body, topo: &Topology, target: u32, statement: u32) -> bool {
    let t = target as usize;
    let i = statement as usize;
    if t >= b.len()
        || i >= b.len()
        || topo.blk[t] == NONE
        || topo.blk[t] != topo.blk[i]
        || topo.pos[t] >= topo.pos[i]
    {
        return false;
    }
    let previous = b.record(b.data[topo.blk[i] as usize][0])[(topo.pos[i] - 1) as usize];
    previous == target
        || (b.data[previous as usize][0] == target
            && Info::read(b, previous as usize).is_some_and(|c| c.at == At::Operation))
}

/// Reads one action from the adjacent count bundle, without scanning a body.
#[must_use]
pub fn following(b: &Body, topo: &Topology, target: u32, tag: Tag) -> Option<Info> {
    let block = *topo.blk.get(target as usize)?;
    if block == NONE {
        return None;
    }
    for &next in &b.record(b.data[block as usize][0])[topo.pos[target as usize] as usize + 1..] {
        let i = next as usize;
        let info = Info::read(b, i)?;
        if b.data[i][0] != target {
            return None;
        }
        if b.tags[i] == tag {
            return Some(info);
        }
        if info.at != At::Operation {
            return None;
        }
    }
    None
}
