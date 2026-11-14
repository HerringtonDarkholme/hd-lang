use super::{EnumBoxing, LayoutEnv, sccs};
use hd_base::{DefId, HashMap, HashSet};
use hd_types::{InternPool, Ty, TyData};

/// Declaration and type facts shared by lazy enum-boxing queries in one program.
/// A completed nominal closure cannot lead back to newly discovered declarations.
/// Held-parameter summaries still activate new arguments at a cached declaration.
#[derive(Default)]
pub struct EnumBoxingCache {
    /// None records a declaration that is not an enum.
    done: HashMap<DefId, Option<EnumFacts>>,
    /// None means nominal discovery finished; Some holds the completed value
    /// graph's projection flag. Even a shared type DAG is read only once.
    types: HashMap<Ty, Option<bool>>,
}

struct EnumFacts {
    boxing: EnumBoxing,
    used: HashSet<usize>,
}

struct ValueType {
    ty: Ty,
    edges: Vec<Ty>,
    nominal: Option<DefId>,
    opaque: bool,
}

impl EnumBoxingCache {
    /// Classifies `def`, retaining reached declarations and type facts.
    /// Cached queries perform no declaration, type, parameter or graph visits.
    pub fn classify(&mut self, pool: &InternPool, env: &dyn LayoutEnv, def: DefId) -> EnumBoxing {
        if let Some(known) = self.done.get(&def) {
            return known.as_ref().map_or(EnumBoxing::Value, |f| f.boxing);
        }
        let mut declarations = HashMap::default();
        let mut pending = vec![def];
        let mut seen = HashSet::from_iter([def]);
        while let Some(e) = pending.pop() {
            crate::work::enum_step();
            let Some(variants) = env.enum_declared(e) else {
                self.done.insert(e, None);
                continue;
            };
            let mut todo: Vec<_> = variants.iter().flatten().copied().collect();
            while let Some(t) = todo.pop() {
                crate::work::enum_step();
                if self.types.contains_key(&t) {
                    continue;
                }
                self.types.insert(t, None);
                // Nominal discovery includes arguments and reference targets,
                // as the original declaration closure did. Value edges below
                // select only the positions that contribute to boxing.
                let children = match pool.get(t) {
                    TyData::Adt { def, args } => {
                        if !self.done.contains_key(&def) && seen.insert(def) {
                            pending.push(def);
                        }
                        pool.list_items(args).to_vec()
                    }
                    TyData::Tuple { elems, rest } => {
                        pool.list_items(elems).iter().copied().chain(rest).collect()
                    }
                    TyData::Ref { inner, .. } => vec![inner],
                    _ => Vec::new(),
                };
                todo.extend(children);
            }
            declarations.insert(e, variants);
        }
        let mut nodes: Vec<ValueType> = Vec::new();
        let mut index = HashMap::default();
        let mut used: HashMap<DefId, HashSet<usize>> = HashMap::default();
        let mut waiting: HashMap<(DefId, usize), Vec<(usize, Ty)>> = HashMap::default();
        let mut todo: Vec<_> = declarations.values().flatten().flatten().copied().collect();
        while let Some(t) = todo.pop() {
            crate::work::enum_step();
            if self.types.get(&t).is_some_and(Option::is_some) || index.contains_key(&t) {
                continue;
            }
            let at = nodes.len();
            index.insert(t, at);
            nodes.push(ValueType {
                ty: t,
                edges: Vec::new(),
                nominal: None,
                opaque: false,
            });
            let (edges, nominal, opaque) = match pool.get(t) {
                TyData::Tuple { elems, rest } => (
                    pool.list_items(elems).iter().copied().chain(rest).collect(),
                    None,
                    false,
                ),
                TyData::Adt { def, args }
                    if declarations.contains_key(&def)
                        || self.done.get(&def).is_some_and(Option::is_some) =>
                {
                    let known = self.done.get(&def).and_then(Option::as_ref);
                    let mut edges = Vec::new();
                    for (j, &a) in pool.list_items(args).iter().enumerate() {
                        crate::work::enum_step();
                        if known.is_some_and(|k| k.used.contains(&j))
                            || used.get(&def).is_some_and(|u| u.contains(&j))
                        {
                            edges.push(a);
                        } else if known.is_none() {
                            waiting.entry((def, j)).or_default().push((at, a));
                        }
                    }
                    (edges, Some(def), false)
                }
                TyData::Param(p) => {
                    // enum_declared's contract names the declaring enum's own
                    // parameters. A cached owner's parameter was activated with
                    // it already; each new interned parameter node runs once.
                    used.entry(p.owner)
                        .or_default()
                        .insert(usize::from(p.index));
                    for (parent, arg) in waiting
                        .remove(&(p.owner, usize::from(p.index)))
                        .unwrap_or_default()
                    {
                        crate::work::enum_step();
                        nodes[parent].edges.push(arg);
                        todo.push(arg);
                    }
                    (Vec::new(), None, false)
                }
                TyData::Assoc { .. } => (Vec::new(), None, true),
                _ => (Vec::new(), None, false),
            };
            todo.extend(edges.iter().copied());
            nodes[at].edges = edges;
            nodes[at].nominal = nominal;
            nodes[at].opaque = opaque;
        }
        // Projection flags flow only through type edges, not into another
        // enum's declaration: that enum decides its own instance's boxing.
        let mut reverse = vec![Vec::new(); nodes.len()];
        for (at, node) in nodes.iter_mut().enumerate() {
            let mut inherited = false;
            for &child in &node.edges {
                crate::work::enum_step();
                if let Some(&to) = index.get(&child) {
                    reverse[to].push(at);
                } else if self.types.get(&child) == Some(&Some(true)) {
                    inherited = true;
                }
            }
            node.opaque |= inherited;
        }
        let mut opaque: Vec<_> = nodes.iter().map(|n| n.opaque).collect();
        let mut todo: Vec<_> = (0..nodes.len()).filter(|&at| opaque[at]).collect();
        while let Some(at) = todo.pop() {
            for &parent in &reverse[at] {
                crate::work::enum_step();
                if !opaque[parent] {
                    opaque[parent] = true;
                    todo.push(parent);
                }
            }
        }
        // Keep shared type nodes in the SCC graph. Expanding their paths once
        // per enum would multiply work by declaration width and type depth.
        let enums: Vec<_> = declarations
            .iter()
            .map(|(&d, fields)| (d, fields))
            .collect();
        let enum_index: HashMap<_, _> = enums
            .iter()
            .enumerate()
            .map(|(i, (d, _))| (*d, i))
            .collect();
        let mut successors = vec![Vec::new(); enums.len() + nodes.len()];
        for (at, (_, fields)) in enums.iter().enumerate() {
            let out = &mut successors[at];
            for t in fields.iter().flatten() {
                crate::work::enum_step();
                if let Some(&to) = index.get(t) {
                    out.push(enums.len() + to);
                }
            }
        }
        for (at, node) in nodes.iter().enumerate() {
            let out = &mut successors[enums.len() + at];
            if let Some(to) = node.nominal.and_then(|d| enum_index.get(&d)) {
                out.push(*to);
            }
            for t in &node.edges {
                crate::work::enum_step();
                if let Some(&to) = index.get(t) {
                    out.push(enums.len() + to);
                }
            }
        }
        let components = sccs(&successors);
        let mut sizes = vec![0usize; successors.len()];
        for &component in &components {
            crate::work::enum_step();
            sizes[component] += 1;
        }
        for (at, (e, fields)) in enums.into_iter().enumerate() {
            let projection = fields.iter().flatten().any(|t| {
                crate::work::enum_step();
                index
                    .get(t)
                    .map_or_else(|| self.types.get(t) == Some(&Some(true)), |&to| opaque[to])
            });
            let boxing = if sizes[components[at]] > 1 {
                EnumBoxing::Boxed
            } else if projection {
                EnumBoxing::ByInstance
            } else {
                EnumBoxing::Value
            };
            self.done.insert(
                e,
                Some(EnumFacts {
                    boxing,
                    used: used.remove(&e).unwrap_or_default(),
                }),
            );
        }
        for (node, opaque) in nodes.into_iter().zip(opaque) {
            self.types.insert(node.ty, Some(opaque));
        }
        self.done
            .get(&def)
            .and_then(Option::as_ref)
            .map_or(EnumBoxing::Value, |f| f.boxing)
    }
}
