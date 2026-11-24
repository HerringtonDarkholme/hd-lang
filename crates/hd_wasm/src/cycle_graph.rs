//! Incremental SCC summaries over fully explored type edges. Interned type
//! shapes have a deterministic structural height. Use that well-founded
//! rank to recognize finite descent without repeated containment searches.
use hd_base::{DefId, HashMap};
use hd_types::{InternPool, Ty};
use std::collections::BTreeSet;

/// Each interned type child/edge is read at most twice, across all roots.
/// Type allocation order and parallel interning do not affect this rank.
#[derive(Default)]
pub struct Ranks {
    done: HashMap<Ty, u32>,
    #[cfg(feature = "testing")]
    pub visits: usize,
}
impl Ranks {
    pub fn get(&mut self, pool: &InternPool, root: Ty) -> u32 {
        #[cfg(feature = "testing")]
        {
            self.visits += 1;
        }
        if let Some(&rank) = self.done.get(&root) {
            return rank;
        }
        let mut work = vec![(root, false)];
        let mut children = Vec::new();
        while let Some((ty, ready)) = work.pop() {
            #[cfg(feature = "testing")]
            {
                self.visits += 1;
            }
            if self.done.contains_key(&ty) {
                continue;
            }
            children.clear();
            pool.types().children(ty, &mut children);
            #[cfg(feature = "testing")]
            {
                self.visits += children.len();
            }
            if ready || children.is_empty() {
                let rank = children
                    .iter()
                    .map(|t| {
                        // ruled out: every argument child completed before
                        // its parent's postorder frame.
                        *crate::emit::required(self.done.get(t), "completed rank child")
                    })
                    .max()
                    .map_or(0, |height| height.saturating_add(1));
                self.done.insert(ty, rank);
            } else {
                work.push((ty, true));
                work.extend(children.iter().rev().map(|&t| (t, false)));
            }
        }
        // ruled out: the immutable type DAG was completed in postorder.
        *crate::emit::required(self.done.get(&root), "completed type rank")
    }
}

#[derive(Clone, Copy, Default)]
pub struct Facts {
    pub value: bool,
    pub object: bool,
    pub recursive: bool,
    pub unknown: bool,
}

pub struct Shape {
    pub parts: Vec<Ty>,
    pub value_open: bool,
    pub object_open: bool,
    pub declaration: Option<DefId>,
    pub rank: u32,
}

#[derive(Default)]
pub struct Graph {
    done: HashMap<Ty, Facts>,
    #[cfg(feature = "testing")]
    pub visits: usize,
}

impl Graph {
    /// Each new type/edge is visited once. Ordered ancestor queries inspect
    /// at most `limit` entries after O(log depth) lookup. No native recursion.
    pub fn get(&mut self, root: Ty, limit: usize, mut shape: impl FnMut(Ty) -> Shape) -> Facts {
        #[cfg(feature = "testing")]
        {
            self.visits += 1;
        }
        if let Some(&facts) = self.done.get(&root) {
            return facts;
        }
        let mut nodes = HashMap::default();
        let mut ancestors: HashMap<DefId, BTreeSet<(u32, u32)>> = HashMap::default();
        hd_base::scc::Online::default().visit(root, |event| {
            match event {
                hd_base::scc::Event::Enter(ty, out) => {
                    if self.done.contains_key(&ty) {
                        return false;
                    }
                    let mut data = shape(ty);
                    #[cfg(feature = "testing")]
                    {
                        self.visits += 1;
                    }
                    let cut = data.declaration.is_some_and(|def| {
                        ancestors.get(&def).is_some_and(|types| {
                            types.range(..=(data.rank, u32::MAX)).take(limit).count() == limit
                        })
                    });
                    if cut {
                        // Unfolding is unknown, never a proof of non-recursion.
                        data.parts.clear();
                        data.value_open = true;
                        data.object_open = true;
                    }
                    if let Some(def) = data.declaration {
                        ancestors.entry(def).or_default().insert((data.rank, ty.0));
                    }
                    out.extend_from_slice(&data.parts);
                    nodes.insert(ty, (data, cut));
                }
                hd_base::scc::Event::Edge(_, _) => {
                    #[cfg(feature = "testing")]
                    {
                        self.visits += 1;
                    }
                }
                hd_base::scc::Event::Finish(ty) => {
                    #[cfg(feature = "testing")]
                    {
                        self.visits += 1;
                    }
                    let data = &nodes[&ty].0;
                    if let Some(def) = data.declaration
                        && let Some(types) = ancestors.get_mut(&def)
                    {
                        types.remove(&(data.rank, ty.0));
                    }
                }
                hd_base::scc::Event::Component(members, cyclic) => {
                    let inside: hd_base::HashSet<Ty> = members.iter().copied().collect();
                    #[cfg(feature = "testing")]
                    {
                        self.visits += members.len();
                    }
                    let mut value = false;
                    let mut unknown = false;
                    for member in members {
                        #[cfg(feature = "testing")]
                        {
                            self.visits += 1;
                        }
                        let (data, cut) = &nodes[member];
                        value |= data.value_open;
                        unknown |= cut;
                        for child in &data.parts {
                            #[cfg(feature = "testing")]
                            {
                                self.visits += 1;
                            }
                            if inside.contains(child) {
                                continue;
                            }
                            // ruled out: an edge leaving a completed SCC reaches
                            // an already completed SCC or an earlier cached query.
                            let facts = crate::emit::required(
                                self.done.get(child),
                                "completed cycle successor",
                            );
                            value |= facts.value;
                            unknown |= facts.unknown;
                        }
                    }
                    for member in members {
                        #[cfg(feature = "testing")]
                        {
                            self.visits += 1;
                        }
                        let data = &nodes[member].0;
                        let mut object = data.object_open;
                        for child in &data.parts {
                            #[cfg(feature = "testing")]
                            {
                                self.visits += 1;
                            }
                            object |= if inside.contains(child) {
                                value
                            } else {
                                // ruled out: outgoing facts were read in the preceding loop.
                                crate::emit::required(
                                    self.done.get(child),
                                    "completed object successor",
                                )
                                .value
                            };
                        }
                        self.done.insert(
                            *member,
                            Facts {
                                value,
                                object,
                                recursive: cyclic || unknown,
                                unknown,
                            },
                        );
                    }
                }
            }
            true
        });
        // ruled out: the root's DFS frame closed its SCC before the work ended.
        *crate::emit::required(self.done.get(&root), "completed cycle root")
    }
}
