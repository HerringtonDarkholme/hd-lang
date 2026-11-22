//! All next-use queries of copy reads together (`future/tests.rs` keeps the
//! per-read walk they replace, as the reference). Touches and queries meet at their
//! common statement block; different arms never meet at a later position.
//! Three counting-sort passes group dense (block, root, position) keys.
//! Work and storage are proportional to the statement-chain entries, not
//! reads times body size. No body, block suffix or root-use list is walked
//! afresh for a query.

use super::{Source, source};
use crate::ir::{Body, NONE, Root, Tag, Topology, Word, slots, u32_of};
use crate::own::values::statement_chain;

const TOUCH: u8 = 0;
const STORE: u8 = 1;
const QUERY: u8 = 2;
const LOOP: u8 = 3;

#[derive(Clone, Copy, Default)]
struct Event {
    block: u32,
    root: u32,
    pos: u32,
    read: u32,
    stop: u32,
    kind: u8,
}

fn key(src: Source, locals: usize) -> Option<u32> {
    match src.root {
        Some(Root::Local(local)) => Some(local),
        Some(Root::Value(value)) => Some(u32_of(locals) + value),
        _ => None,
    }
}

fn roots(b: &Body, i: usize, out: &mut Vec<u32>) {
    out.clear();
    if matches!(b.tags[i], Tag::Read | Tag::Ref | Tag::IsUnique)
        && let Some(local) = b.view(b.data[i][0]).local()
    {
        out.push(local);
    }
    if b.tags[i] == Tag::Closure {
        let rec = b.record(b.data[i][1]);
        for cap in rec[0]..rec[0] + rec[1] {
            out.push(b.cap_local[cap as usize].raw());
        }
    }
    if b.tags[i] != Tag::Drop {
        slots(b, i, |_, value, word| {
            if matches!(word, Word::Value(_)) && (value as usize) < b.len() {
                out.push(u32_of(b.local_ty.len()) + value);
            }
        });
    }
}

/// For each statement and possible common ancestor depth: the next exit
/// that leaves that ancestor. `floor` is the shallowest depth any later
/// exit leaves. A loop jump leaves its body, but can reach after its Loop.
fn exits(b: &Body, topo: &Topology) -> (Vec<Vec<u32>>, Vec<u32>, Vec<usize>) {
    let mut depth = vec![0usize; b.len()];
    for i in (0..b.len()).rev() {
        if topo.is_root[i] {
            continue;
        }
        if b.tags[i] == Tag::Block && topo.owner[i] != NONE {
            let parent = topo.blk[topo.owner[i] as usize];
            if parent != NONE {
                depth[i] = depth[parent as usize] + 1;
            }
        } else if topo.blk[i] != NONE {
            depth[i] = depth[topo.blk[i] as usize];
        }
    }
    let mut next = vec![Vec::new(); b.len()];
    let mut floor = vec![NONE; b.len()];
    for block in 0..b.len() {
        if b.tags[block] != Tag::Block {
            continue;
        }
        let mut following = vec![NONE; depth[block] + 1];
        let mut shallowest = NONE;
        for (pos, &inst) in b.record(b.data[block][0]).iter().enumerate().rev() {
            let i = inst as usize;
            next[i].clone_from(&following);
            floor[i] = shallowest;
            let leaves = match b.tags[i] {
                Tag::Return | Tag::Unreachable => Some(0),
                Tag::Break | Tag::Continue => b
                    .label_inst
                    .get(b.data[i][0] as usize)
                    .filter(|&&target| target != NONE)
                    .map(|&target| {
                        depth[target as usize] + usize::from(b.tags[target as usize] != Tag::Block)
                    }),
                _ => None,
            };
            if let Some(leaves) = leaves {
                shallowest = shallowest.min(u32_of(leaves));
                for at in following.iter_mut().skip(leaves) {
                    *at = u32_of(pos);
                }
            }
        }
    }
    (next, floor, depth)
}

fn sort(events: &mut Vec<Event>, domain: usize) {
    let mut scratch = vec![Event::default(); events.len()];
    let mut offsets = vec![0usize; domain];
    for column in 0..3 {
        offsets.fill(0);
        let key = |event: &Event| match column {
            0 => event.pos as usize,
            1 => event.root as usize,
            _ => event.block as usize,
        };
        for event in events.iter() {
            offsets[key(event)] += 1;
        }
        let mut sum = 0;
        for offset in &mut offsets {
            let count = *offset;
            *offset = sum;
            sum += count;
        }
        for event in events.iter() {
            let offset = &mut offsets[key(event)];
            scratch[*offset] = *event;
            *offset += 1;
        }
        std::mem::swap(events, &mut scratch);
    }
}

pub(super) fn uses(b: &Body, topo: &Topology) -> Vec<bool> {
    let (next_exit, floor, depth) = exits(b, topo);
    let mut events = Vec::new();
    let mut chain = Vec::new();
    let mut touched = Vec::new();
    let domain = b.len() + b.local_ty.len() + 1;
    let mut seen = vec![NONE; domain];
    for i in 0..b.len() {
        statement_chain(topo, u32_of(i), &mut chain);
        roots(b, i, &mut touched);
        for &root in &touched {
            if seen[root as usize] == u32_of(i) {
                continue;
            }
            seen[root as usize] = u32_of(i);
            for &(block, pos, _) in &chain {
                events.push(Event {
                    block,
                    root,
                    pos,
                    kind: TOUCH,
                    ..Event::default()
                });
            }
        }
        if b.tags[i] == Tag::Store {
            let place = b.view(b.data[i][0]);
            if place.whole()
                && let Some(root) = place.local()
            {
                for &(block, pos, _) in &chain {
                    events.push(Event {
                        block,
                        root,
                        pos,
                        kind: STORE,
                        ..Event::default()
                    });
                }
            }
        }
        if b.tags[i] != Tag::Read || b.data[i][1] != crate::ir::ReadMode::Copy as u32 {
            continue;
        }
        let Some(root) = key(source(b, b.data[i][0]), b.local_ty.len()) else {
            continue;
        };
        let mut inner_floor = NONE;
        for &(block, pos, inst) in &chain {
            let at = depth[block as usize];
            if inner_floor > u32_of(at) {
                let stop = next_exit[inst as usize].get(at).copied().unwrap_or(NONE);
                events.push(Event {
                    block,
                    root,
                    pos,
                    read: u32_of(i),
                    stop,
                    kind: QUERY,
                });
            }
            inner_floor = inner_floor.min(floor[inst as usize]);
        }
        let inner = topo.loop_of[i];
        if inner != NONE {
            let block = b.data[inner as usize][0];
            events.push(Event {
                block,
                root,
                read: u32_of(i),
                stop: u32::from(touched.contains(&root)),
                kind: LOOP,
                ..Event::default()
            });
        }
    }
    sort(&mut events, domain);
    let mut out = vec![false; b.len()];
    let mut lo = 0;
    while lo < events.len() {
        let mut hi = lo + 1;
        while hi < events.len()
            && (events[hi].block, events[hi].root) == (events[lo].block, events[lo].root)
        {
            hi += 1;
        }
        let group = &events[lo..hi];
        let total = group.iter().filter(|event| event.kind == TOUCH).count();
        let renewed = group.iter().any(|event| event.kind == STORE);
        let mut next_touch = NONE;
        let mut end = group.len();
        while end > 0 {
            let mut start = end - 1;
            while start > 0 && group[start - 1].pos == group[end - 1].pos {
                start -= 1;
            }
            for event in &group[start..end] {
                if event.kind == QUERY && next_touch != NONE && next_touch < event.stop
                    || event.kind == LOOP && !renewed && total > event.stop as usize
                {
                    out[event.read as usize] = true;
                }
            }
            if group[start..end].iter().any(|event| event.kind == TOUCH) {
                next_touch = group[end - 1].pos;
            }
            end = start;
        }
        lo = hi;
    }
    out
}

#[cfg(test)]
mod tests;
