//! Move-path candidate index, following rustc's `MovePath` parent/child tree.
//! It narrows queries only; the verifier's overlap predicates remain the judge.
//! Construction is O(projection words + roots + fact rows) expected work and
//! space. Queries visit compatible path nodes and facts, not sibling fields.
//! Cross-root aliases are handled by the caller's sparse live-word masks.
use super::{Place, PlaceRoot, Proj, Ref, Ty};
use hd_base::HashMap;

#[derive(Default)]
struct Node {
    children: [Vec<usize>; 4],
    facts: Vec<usize>,
}

#[derive(Default)]
pub(super) struct Places {
    roots: HashMap<PlaceRoot, usize>,
    edges: HashMap<(usize, Proj), usize>,
    nodes: Vec<Node>,
}

fn root(root: PlaceRoot) -> PlaceRoot {
    // same_root compares globals by definition, independently of type.
    match root {
        PlaceRoot::Global(def, _) => PlaceRoot::Global(def, Ty::VOID),
        other => other,
    }
}

fn step(step: Proj) -> (usize, Proj) {
    match step {
        Proj::Field(_) => (0, step),
        Proj::Variant(_) => (1, step),
        Proj::Deref => (2, step),
        // The verifier conservatively ignores element indices.
        Proj::Elem(_) => (3, Proj::Elem(Ref(super::NONE))),
    }
}

impl Places {
    pub(super) fn insert(&mut self, place: &Place, fact: usize) {
        crate::work::empty_place_work(1 + place.proj.len());
        let mut node = if let Some(&node) = self.roots.get(&root(place.root)) {
            node
        } else {
            let node = self.nodes.len();
            self.nodes.push(Node::default());
            self.roots.insert(root(place.root), node);
            node
        };
        for &projection in &place.proj {
            let (kind, projection) = step(projection);
            node = if let Some(&child) = self.edges.get(&(node, projection)) {
                child
            } else {
                let child = self.nodes.len();
                self.nodes.push(Node::default());
                self.nodes[node].children[kind].push(child);
                self.edges.insert((node, projection), child);
                child
            };
        }
        self.nodes[node].facts.push(fact);
    }

    /// Prefixes and descendants may overlap. Unlike projection kinds are
    /// conservative matches, as in `Cx::overlap`; equal kinds select one edge.
    /// Cost: query projections + visited candidate paths and facts, never
    /// a scan of sibling fields or unrelated roots. No native recursion.
    pub(super) fn query(
        &self,
        place: &Place,
        stack: &mut Vec<(usize, usize)>,
        out: &mut Vec<usize>,
    ) {
        crate::work::empty_place_work(1);
        stack.clear();
        if let Some(&node) = self.roots.get(&root(place.root)) {
            stack.push((node, 0));
        }
        while let Some((node, depth)) = stack.pop() {
            crate::work::empty_place_work(1 + self.nodes[node].facts.len());
            out.extend_from_slice(&self.nodes[node].facts);
            if let Some(&projection) = place.proj.get(depth) {
                let (kind, projection) = step(projection);
                if let Some(&child) = self.edges.get(&(node, projection)) {
                    stack.push((child, depth + 1));
                }
                for (other, children) in self.nodes[node].children.iter().enumerate() {
                    if other != kind {
                        stack.extend(children.iter().map(|&child| (child, depth + 1)));
                    }
                }
            } else {
                stack.extend(
                    self.nodes[node]
                        .children
                        .iter()
                        .flatten()
                        .map(|&child| (child, depth)),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Cx, same_root};
    use super::*;
    use crate::ir::{Body, BodyKind, Topology};
    use hd_base::{DefId, LocalId, SliceSort as _};

    #[test]
    fn candidates_agree_with_the_existing_overlap_predicate_for_all_step_kinds() {
        let body = Body::new(DefId::from_raw(0), BodyKind::Fn);
        let topo = Topology::new(&body);
        let cx = Cx {
            b: &body,
            topo: &topo,
            places: &[],
            addressed: &[true, true],
            reference_calls: &[],
            reference_values: &[],
            reference_sources: &[],
            reference_roots: &[],
            frame_lends: &[],
            neutral_stores: &[],
        };
        let projections = [
            vec![],
            vec![Proj::Field(0)],
            vec![Proj::Field(1)],
            vec![Proj::Field(0), Proj::Field(2)],
            vec![Proj::Variant(0)],
            vec![Proj::Variant(1)],
            vec![Proj::Deref],
            vec![Proj::Elem(Ref(3))],
            vec![Proj::Elem(Ref(4))],
            vec![Proj::Field(1), Proj::Elem(Ref(4))],
            vec![Proj::Deref, Proj::Field(1)],
        ];
        let mut places = Vec::new();
        for root in [
            PlaceRoot::Local(LocalId::from_raw(0)),
            PlaceRoot::Local(LocalId::from_raw(1)),
            PlaceRoot::Global(DefId::from_raw(9), Ty::I32),
            PlaceRoot::Global(DefId::from_raw(9), Ty::STRING),
        ] {
            for proj in &projections {
                places.push(Place {
                    root,
                    proj: proj.clone(),
                });
            }
        }
        let mut index = Places::default();
        for (fact, place) in places.iter().enumerate() {
            index.insert(place, fact);
        }
        let mut stack = Vec::new();
        for query in &places {
            let mut found = Vec::new();
            index.query(query, &mut stack, &mut found);
            found.sort_stable();
            let expected: Vec<_> = places
                .iter()
                .enumerate()
                .filter(|(_, place)| same_root(place.root, query.root) && cx.overlap(place, query))
                .map(|(fact, _)| fact)
                .collect();
            assert_eq!(found, expected, "{query:?}");
        }
    }

    fn measured(width: u32, depth: usize) -> u64 {
        let ((), work) = crate::work::count_empty_place_work(|| {
            let mut index = Places::default();
            let mut places = Vec::new();
            for field in 0..width {
                let mut proj = vec![Proj::Field(0); depth];
                proj.push(Proj::Field(field));
                let place = Place {
                    root: PlaceRoot::Local(LocalId::from_raw(0)),
                    proj,
                };
                index.insert(&place, field as usize);
                places.push(place);
            }
            let (mut stack, mut found) = (Vec::new(), Vec::new());
            for (field, place) in places.iter().enumerate() {
                found.clear();
                index.query(place, &mut stack, &mut found);
                assert_eq!(found, [field]);
            }
        });
        work
    }

    #[test]
    fn sibling_field_queries_are_linear_at_width_one_thousand_and_depth_twenty() {
        let shallow = (measured(100, 1), measured(1000, 1));
        let deep = (measured(100, 20), measured(1000, 20));
        eprintln!("place-index work: shallow {shallow:?}, deep {deep:?}");
        for (small, large) in [shallow, deep] {
            assert!(small > 0 && large <= small * 11);
        }
        assert!(deep.0 <= shallow.0 * 21 && deep.1 <= shallow.1 * 21);
    }
}

#[cfg(test)]
mod reference_source_tests {
    use super::super::{reference_sources, reference_values};
    use crate::ir::{Body, BodyKind, Place, Tag, TirBuilder, TirSink, ref_word};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    fn aliases(width: usize, depth: usize) -> (Body, u32, Vec<u32>, usize) {
        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let at = NodeIdx::NONE;
        let block = builder.open_block();
        let local = builder.local(Ty::I32, Symbol::NONE, 0, at);
        let word = builder.place(&Place::local(local));
        let address = builder.emit(Tag::Ref, word, ref_word::LOCAL, Ty::I32, at);
        let first_store = builder.body_mut().len();
        let mut targets = Vec::new();
        for _ in 0..width {
            let mut value = address;
            for _ in 0..depth {
                let alias = builder.local(Ty::I32, Symbol::NONE, 0, at);
                builder.set(alias, value, at);
                value = builder.get(alias, Ty::I32, at);
            }
            targets.push(value.0);
        }
        let block = builder.close_block(block, None, Ty::VOID, at);
        (
            builder
                .finish(block, &[])
                .expect("generated reference aliases"),
            address.0,
            targets,
            first_store,
        )
    }

    fn resolved(body: &Body) -> (Vec<u32>, u64) {
        let values = reference_values(body);
        let places: Vec<_> = (0..body.len())
            .map(|i| {
                if matches!(body.tags[i], Tag::Read | Tag::Ref) {
                    body.decode_place(body.data[i][0])
                } else {
                    None
                }
            })
            .collect();
        crate::work::count_empty_place_work(|| reference_sources(body, &values, &places))
    }

    #[test]
    fn aliases_resolve_once_at_width_one_thousand_and_depth_twenty() {
        let mut measured = Vec::new();
        for depth in [1, 20] {
            let mut counts = Vec::new();
            for width in [100, 1000] {
                let (body, address, targets, _) = aliases(width, depth);
                let (sources, work) = resolved(&body);
                for target in targets {
                    assert_eq!(sources[target as usize], address);
                }
                counts.push(work);
            }
            assert!(counts[0] > 0 && counts[1] <= counts[0] * 11, "{counts:?}");
            measured.push(counts);
        }
        eprintln!("reference-source work: {measured:?}");
        assert!(measured[1][1] <= measured[0][1] * 21);
    }

    #[test]
    fn a_cyclic_reference_definition_stays_unknown() {
        let (mut body, _, targets, first_store) = aliases(1, 20);
        body.data[first_store][1] = targets[0];
        let (sources, _) = resolved(&body);
        assert_eq!(sources[targets[0] as usize], crate::ir::NONE);
    }
}
