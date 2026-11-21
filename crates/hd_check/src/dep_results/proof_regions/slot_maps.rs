//! Query-local, interned slot maps. Like Cranelift's EntityList/ListPool,
//! a handle copies without copying its elements. Here maps are immutable
//! expressions too: slicing, concatenation and union retain shared inputs.

use hd_base::{HashMap, HashSet};

use super::Origin;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct MapId(usize);

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
enum Key {
    Empty(usize),
    #[cfg(test)]
    Root {
        port: usize,
        start: usize,
        len: usize,
    },
    Group(Box<[Origin]>),
    Slice {
        of: MapId,
        start: usize,
        len: usize,
    },
    Concat(Box<[MapId]>),
    Union(Box<[MapId]>),
    All(MapId),
    Repeat {
        of: MapId,
        len: usize,
    },
}

struct Map {
    key: Key,
    len: usize,
    /// Prefix ends for a concatenation; slicing enters only intersecting
    /// parts, found by binary search rather than a scan of the prefix.
    ends: Box<[usize]>,
}

#[derive(Default)]
pub(super) struct Maps {
    maps: Vec<Map>,
    ids: HashMap<Key, MapId>,
}

impl Maps {
    fn charge(n: usize) {
        crate::work::scan_work(n);
    }

    fn intern(&mut self, key: Key, len: usize) -> MapId {
        Self::charge(
            1 + match &key {
                Key::Group(xs) => xs.len(),
                Key::Concat(xs) | Key::Union(xs) => xs.len(),
                _ => 0,
            },
        );
        if let Some(&id) = self.ids.get(&key) {
            return id;
        }
        let mut end = 0;
        let ends = match &key {
            Key::Concat(parts) => parts
                .iter()
                .map(|&p| {
                    end += self.len(p);
                    end
                })
                .collect(),
            _ => Box::default(),
        };
        let id = MapId(self.maps.len());
        self.ids.insert(key.clone(), id);
        self.maps.push(Map { key, len, ends });
        id
    }

    pub(super) fn len(&self, id: MapId) -> usize {
        self.maps[id.0].len
    }

    pub(super) fn empty(&mut self, len: usize) -> MapId {
        self.intern(Key::Empty(len), len)
    }

    #[cfg(test)]
    pub(super) fn root(&mut self, port: usize, len: usize) -> MapId {
        self.intern(
            #[cfg(test)]
            Key::Root {
                port,
                start: 0,
                len,
            },
            len,
        )
    }

    pub(super) fn group(&mut self, origins: impl IntoIterator<Item = Origin>) -> MapId {
        let mut seen = HashSet::default();
        let origins: Box<_> = origins
            .into_iter()
            .filter(|&o| {
                Self::charge(1);
                seen.insert(o)
            })
            .collect();
        self.intern(Key::Group(origins), 1)
    }

    pub(super) fn slice(&mut self, of: MapId, start: usize, len: usize) -> MapId {
        if start == 0 && len == self.len(of) {
            return of;
        }
        let key = match self.maps[of.0].key {
            #[cfg(test)]
            Key::Root {
                port, start: base, ..
            } => Key::Root {
                port,
                start: base + start,
                len,
            },
            Key::Slice {
                of, start: base, ..
            } => Key::Slice {
                of,
                start: base + start,
                len,
            },
            _ => Key::Slice { of, start, len },
        };
        self.intern(key, len)
    }

    pub(super) fn concat(&mut self, parts: impl IntoIterator<Item = MapId>) -> MapId {
        let parts: Box<_> = parts
            .into_iter()
            .filter(|&p| {
                Self::charge(1);
                self.len(p) != 0
            })
            .collect();
        match parts.as_ref() {
            [] => self.empty(0),
            [one] => *one,
            _ => {
                let len = parts.iter().map(|&p| self.len(p)).sum();
                self.intern(Key::Concat(parts), len)
            }
        }
    }

    pub(super) fn union(&mut self, a: MapId, b: MapId) -> MapId {
        Self::charge(1);
        if a == b {
            return a;
        }
        let parts = if a.0 < b.0 { [a, b] } else { [b, a] };
        self.intern(Key::Union(Box::new(parts)), self.len(a))
    }

    pub(super) fn all(&mut self, of: MapId) -> MapId {
        if self.len(of) == 1 {
            return of;
        }
        self.intern(Key::All(of), 1)
    }

    pub(super) fn repeat(&mut self, of: MapId, len: usize) -> MapId {
        if len == 1 {
            return of;
        }
        self.intern(Key::Repeat { of, len }, len)
    }

    /// Expands only a distinct proof leaf's required range. Unions walk
    /// their DAG with a visited set; no intermediate union copies a list.
    pub(super) fn expand(&mut self, root: MapId, start: usize, len: usize) -> Vec<Vec<Origin>> {
        let mut out = vec![Vec::new(); len];
        let mut atoms = HashSet::default();
        let mut seen = HashSet::default();
        let mut stack = vec![(root, start, len, 0, false)];
        while let Some((id, start, len, offset, flat)) = stack.pop() {
            Self::charge(1);
            if !seen.insert((id, start, len, offset, flat)) {
                continue;
            }
            let node = &self.maps[id.0];
            match &node.key {
                Key::Empty(_) => {}
                #[cfg(test)]
                Key::Root {
                    port, start: base, ..
                } => {
                    for k in 0..len {
                        Self::charge(1);
                        let at = offset + if flat { 0 } else { k };
                        let origin = Origin::Input {
                            port: *port,
                            slot: base + start + k,
                        };
                        if atoms.insert((at, origin)) {
                            out[at].push(origin);
                        }
                    }
                }
                Key::Group(group) => {
                    for &origin in group {
                        Self::charge(1);
                        if atoms.insert((offset, origin)) {
                            out[offset].push(origin);
                        }
                    }
                }
                Key::Slice {
                    of, start: base, ..
                } => {
                    stack.push((*of, base + start, len, offset, flat));
                }
                Key::Concat(parts) => {
                    let first = node.ends.partition_point(|&end| end <= start);
                    let stop = start + len;
                    for k in first..parts.len() {
                        let base = if k == 0 { 0 } else { node.ends[k - 1] };
                        if base >= stop {
                            break;
                        }
                        let lo = start.max(base);
                        let hi = stop.min(node.ends[k]);
                        stack.push((
                            parts[k],
                            lo - base,
                            hi - lo,
                            offset + if flat { 0 } else { lo - start },
                            flat,
                        ));
                    }
                }
                Key::Union(parts) => {
                    stack.extend(parts.iter().map(|&p| (p, start, len, offset, flat)));
                }
                Key::All(of) => stack.push((*of, 0, self.len(*of), offset, true)),
                Key::Repeat { of, .. } => {
                    for k in 0..len {
                        stack.push((*of, 0, 1, offset + if flat { 0 } else { k }, flat));
                    }
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_maps_and_deep_shared_unions_count_linear_work() {
        for width in [100, 1000] {
            let ((), work) = crate::work::count_scan_work(|| {
                let mut maps = Maps::default();
                let root = maps.root(0, width);
                let other = maps.root(1, width);
                let shared = maps.union(root, other);
                let mut at = shared;
                for depth in 0..20 {
                    let next = maps.root(depth + 2, width);
                    at = maps.union(at, next);
                    for _ in 0..width {
                        at = maps.union(at, at);
                    }
                }
                let result = maps.expand(at, 0, width);
                assert_eq!(result.len(), width);
                for (slot, origins) in result.iter().enumerate() {
                    assert_eq!(origins.len(), 22);
                    for port in 0..22 {
                        assert!(origins.contains(&Origin::Input { port, slot }));
                    }
                }
            });
            assert_eq!(work, (42 * width + 149) as u64);
        }
    }

    #[test]
    fn slices_concat_and_collapsed_positions_keep_their_sources() {
        let mut maps = Maps::default();
        let root = maps.root(0, 4);
        let a = maps.slice(root, 0, 2);
        let b = maps.slice(root, 2, 2);
        let both = maps.concat([b, a]);
        let middle = maps.slice(both, 1, 2);
        assert_eq!(
            maps.expand(middle, 0, 2),
            [
                vec![Origin::Input { port: 0, slot: 3 }],
                vec![Origin::Input { port: 0, slot: 0 }],
            ]
        );
        let all = maps.all(both);
        assert_eq!(maps.expand(all, 0, 1)[0].len(), 4);
    }
}
