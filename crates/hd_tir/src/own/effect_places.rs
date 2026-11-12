//! Storage reached by a write, including fully visible local-reference aliases.
//! This classifies effects; it does not remove an address, loan or check.
//! One store/reference census and one topological value walk cost O(N + P)
//! time and O(N + L) space, for instructions N, projection words P and locals L.
//! Alias depth adds instructions, never recursive walks or repeated rescans.

use super::memory_effects::Origin;
use crate::ir::{Body, NONE, Proj, ReadMode, Ref, Root, Tag, local_flags, ref_word, u32_of};

#[cfg(test)]
thread_local! {
    static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

#[cfg(test)]
pub(super) fn work(n: usize) {
    WORK.with(|work| work.set(work.get() + n));
}

#[cfg(test)]
pub(super) fn take_work() -> usize {
    WORK.with(|work| work.replace(0))
}

pub(super) struct Places {
    storage: Vec<(bool, bool)>,
    /// Whether a reference's first dereference reaches private local storage.
    references: Vec<bool>,
    reference_sources: Vec<Origin>,
    storage_sources: Vec<Origin>,
    parameter_sources: Vec<Option<u32>>,
    stores: Vec<(u32, u32)>,
    ambiguous: Vec<bool>,
}

impl Places {
    #[cfg(test)]
    pub(super) fn new(b: &Body) -> Self {
        Self::new_with(b, |_| {})
    }

    pub(super) fn new_with(b: &Body, mut measure: impl FnMut(usize)) -> Self {
        let mut out = Self {
            storage: vec![(true, false); b.len()],
            references: vec![false; b.len()],
            reference_sources: vec![Origin::External; b.len()],
            storage_sources: vec![Origin::Private; b.len()],
            parameter_sources: vec![None; b.local_ty.len()],
            stores: vec![(0, NONE); b.local_ty.len()],
            ambiguous: vec![false; b.local_ty.len()],
        };
        // ruled out: invariant 21 forbids a mutation of Value-rooted
        // storage without a dereference or element. Indirect mutation of
        // a local reference variable requires observing its mutable
        // address, which the census below invalidates.
        for i in 0..b.len() {
            measure(1);
            #[cfg(test)]
            work(1);
            if let Some((l, whole)) = b.stored_local(i) {
                let entry = &mut out.stores[l.idx()];
                entry.0 += 1;
                entry.1 = if whole { u32_of(i) } else { NONE };
            }
            if b.tags[i] == Tag::Ref
                && b.data[i][1] & ref_word::MUT != 0
                && let Some(l) = b.local_in_place(b.data[i][0])
            {
                out.ambiguous[l.idx()] = true;
            }
        }
        // A parameter binding whose address is observed, or which is assigned,
        // may later name another object. It cannot seed a parameter-only proof.
        for (index, local) in super::params(b).into_iter().enumerate() {
            measure(1);
            #[cfg(test)]
            work(1);
            let l = local as usize;
            if out.stores[l].0 == 0
                && !out.ambiguous[l]
                && b.local_flags[l] & local_flags::CAPTURED_ASSIGNED == 0
            {
                out.parameter_sources[l] = Some(u32_of(index));
            }
        }
        for (l, &flags) in b.local_flags.iter().enumerate() {
            measure(1);
            #[cfg(test)]
            work(1);
            out.ambiguous[l] |= flags & (local_flags::PARAM | local_flags::CAPTURED_ASSIGNED) != 0;
        }
        for i in 0..b.len() {
            measure(1);
            #[cfg(test)]
            work(1);
            let [a, w] = b.data[i];
            out.reference_sources[i] = match b.tags[i] {
                Tag::Read => {
                    if matches!(
                        ReadMode::from_u32(w),
                        Some(ReadMode::Use | ReadMode::Borrow)
                    ) {
                        out.storage_sources[i] = out.source_at(b, a, i);
                    }
                    b.local_read(i).map_or(Origin::External, |local| {
                        out.local_source(b, u32_of(local.idx()), i)
                    })
                }
                Tag::Ref if w & ref_word::LOCAL != 0 => {
                    let (_, element) = out.get_with(b, a, &mut measure);
                    if element {
                        Origin::External
                    } else {
                        out.source_at(b, a, i)
                    }
                }
                Tag::Coerce if b.same_value_coercion(i) => {
                    if let Some(value) = Ref(a).as_inst() {
                        out.storage_sources[i] = out.storage_sources[value.0 as usize];
                    }
                    out.reference_source(a)
                }
                _ => Origin::External,
            };
            out.references[i] = match b.tags[i] {
                Tag::Read => {
                    if matches!(
                        ReadMode::from_u32(w),
                        Some(ReadMode::Use | ReadMode::Borrow)
                    ) {
                        out.storage[i] = out.get_with(b, a, &mut measure);
                    }
                    if let Some(l) = b.local_read(i)
                        && let (1, store) = out.stores[l.idx()]
                        && !out.ambiguous[l.idx()]
                        && store != NONE
                        && (store as usize) < i
                    {
                        out.reference(b.data[store as usize][1])
                    } else {
                        false
                    }
                }
                Tag::Ref if w & ref_word::LOCAL != 0 => {
                    let (own, elem) = out.get_with(b, a, &mut measure);
                    own && !elem
                }
                Tag::Coerce if b.same_value_coercion(i) => {
                    if let Some(value) = Ref(a).as_inst() {
                        out.storage[i] = out.storage[value.0 as usize];
                    }
                    out.reference(a)
                }
                // Unknown producers provide no private-address proof.
                _ => false,
            };
        }
        out
    }

    pub(super) fn value_source(&self, b: &Body, i: usize) -> super::memory_effects::ValueSource {
        use super::memory_effects::ValueSource;
        if b.same_value_coercion(i) {
            return ValueSource::Value(b.data[i][0]);
        }
        if b.tags[i] == Tag::Read
            && let Some(local) = b.local_read(i)
        {
            let l = local.idx();
            if let Some(parameter) = self.parameter_sources[l] {
                return ValueSource::Parameter(parameter);
            }
            let (count, store) = self.stores[l];
            if count == 1 && store != NONE && (store as usize) < i && !self.ambiguous[l] {
                return ValueSource::Value(b.data[store as usize][1]);
            }
        }
        ValueSource::Producer
    }

    /// The exact object a reference value points into. Unknown producers,
    /// reassigned bindings, nested reference fields and buffer elements stay
    /// external. This is cached once per producer, never walked per call.
    pub(super) fn reference_source(&self, word: u32) -> Origin {
        Ref(word).as_inst().map_or(Origin::External, |value| {
            self.reference_sources[value.0 as usize]
        })
    }

    fn local_source(&self, b: &Body, local: u32, before: usize) -> Origin {
        let l = local as usize;
        if let Some(index) = self.parameter_sources[l] {
            return Origin::Parameter(index);
        }
        let (count, store) = self.stores[l];
        if count == 1 && store != NONE && (store as usize) < before && !self.ambiguous[l] {
            return self.reference_source(b.data[store as usize][1]);
        }
        Origin::External
    }

    /// Where a direct write lands. A first exact dereference can identify a
    /// parameter; later dereferences may follow a reference stored inside it.
    pub(super) fn source_at(&self, b: &Body, word: u32, before: usize) -> Origin {
        let place = b.view(word);
        let (mut source, reference) = match place.root {
            Root::Local(local) => (Origin::Private, self.local_source(b, local, before)),
            Root::Value(value) => (
                self.storage_sources
                    .get(value as usize)
                    .copied()
                    .unwrap_or(Origin::External),
                self.reference_source(value),
            ),
            _ => return Origin::External,
        };
        for (at, projection) in place.steps(b).enumerate() {
            if projection == Proj::Deref {
                source = if at == 0 { reference } else { Origin::External };
            }
        }
        source
    }

    fn reference(&self, word: u32) -> bool {
        Ref(word)
            .as_inst()
            .is_some_and(|v| self.references.get(v.0 as usize) == Some(&true))
    }

    /// Whether the place is private storage, and whether it reaches a buffer.
    pub(super) fn get(&self, b: &Body, word: u32) -> (bool, bool) {
        self.get_with(b, word, &mut |_| {})
    }

    fn get_with(&self, b: &Body, word: u32, measure: &mut impl FnMut(usize)) -> (bool, bool) {
        let p = b.view(word);
        let (mut own, mut elem, reference) = match p.root {
            Root::Local(l) => {
                let (count, store) = self.stores[l as usize];
                let reference = count == 1
                    && store != NONE
                    && !self.ambiguous[l as usize]
                    && self.reference(b.data[store as usize][1]);
                (true, false, reference)
            }
            Root::Value(v) => {
                let (own, elem) = self
                    .storage
                    .get(v as usize)
                    .copied()
                    .unwrap_or((false, true));
                (own, elem, self.reference(v))
            }
            _ => return (false, true),
        };
        for (at, step) in p.steps(b).enumerate() {
            measure(1);
            #[cfg(test)]
            work(1);
            match step {
                // Only an exact known local address proves this dereference.
                // Fields of a reference-bearing aggregate and further derefs
                // still reach unknown storage, including pointer/buffer loans.
                Proj::Deref => own &= at == 0 && reference,
                Proj::Elem(_) => elem = true,
                Proj::Field(_) | Proj::Variant(_) => {}
            }
        }
        (own, elem)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Coercion, Place, PlaceRoot, TirBuilder, TirSink};
    use hd_base::{DefId, NodeIdx, Symbol};
    use hd_types::Ty;

    #[test]
    fn parameter_origins_require_one_exact_unchanged_reference() -> Result<(), String> {
        for control in 0..3 {
            let n = NodeIdx::from_raw(1);
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let block = b.open_block();
            let parameter = b.local(Ty::I32, Symbol::NONE, local_flags::PARAM, n);
            let second = b.local(Ty::I32, Symbol::NONE, local_flags::PARAM, n);
            let direct = b.place(&Place {
                root: PlaceRoot::Local(parameter),
                proj: vec![Proj::Deref, Proj::Field(0)],
            });
            let nested = b.place(&Place {
                root: PlaceRoot::Local(parameter),
                proj: vec![Proj::Deref, Proj::Field(0), Proj::Deref],
            });
            if control == 1 {
                let value = b.get(second, Ty::I32, n);
                b.set(parameter, value, n);
            } else if control == 2 {
                let place = b.place(&Place::local(parameter));
                b.emit(Tag::Ref, place, ref_word::LOCAL | ref_word::MUT, Ty::I32, n);
            }
            let value = b.get(parameter, Ty::I32, n);
            let alias = b.local(Ty::I32, Symbol::NONE, 0, n);
            b.set(alias, value, n);
            let value = b.get(alias, Ty::I32, n);
            let indirect = b.place(&Place {
                root: PlaceRoot::Value(value),
                proj: vec![Proj::Deref],
            });
            let root = b.close_block(block, None, Ty::VOID, n);
            let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            let places = Places::new(&body);
            let expected = if control == 0 {
                Origin::Parameter(0)
            } else {
                Origin::External
            };
            assert_eq!(places.source_at(&body, direct, body.len()), expected);
            assert_eq!(places.source_at(&body, indirect, body.len()), expected);
            assert_eq!(
                places.source_at(&body, nested, body.len()),
                Origin::External
            );
        }
        Ok(())
    }

    #[test]
    fn local_reference_effect_work_is_linear_across_alias_width_and_depth() -> Result<(), String> {
        for (width, depth) in [(1, 1), (1000, 1), (1, 20), (1000, 20)] {
            let n = NodeIdx::from_raw(1);
            let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
            let block = b.open_block();
            let local = b.local(Ty::I32, Symbol::NONE, 0, n);
            let source = b.place(&Place::local(local));
            let address = b.emit(Tag::Ref, source, ref_word::LOCAL, Ty::I32, n);
            let mut targets = Vec::new();
            for _ in 0..width {
                let mut value = address;
                for _ in 0..depth {
                    let alias = b.local(Ty::I32, Symbol::NONE, 0, n);
                    b.set(alias, value, n);
                    value = b.get(alias, Ty::I32, n);
                }
                targets.push(b.place(&Place {
                    root: PlaceRoot::Value(value),
                    proj: vec![Proj::Deref],
                }));
            }
            let root = b.close_block(block, None, Ty::VOID, n);
            let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
            let mut work = 0;
            let places = Places::new_with(&body, |n| work += n);
            for target in targets {
                assert_eq!(
                    places.get_with(&body, target, &mut |n| work += n),
                    (true, false)
                );
            }
            assert!(
                work <= 3 * body.len() + body.local_ty.len(),
                "{width}x{depth}: {work}"
            );
        }
        Ok(())
    }

    #[test]
    fn unknown_or_mutably_observed_reference_storage_stays_external() -> Result<(), String> {
        let n = NodeIdx::from_raw(1);
        let mut b = TirBuilder::new(DefId::from_raw(4), BodyKind::Fn);
        let block = b.open_block();
        let parameter = b.local(Ty::I32, Symbol::NONE, local_flags::PARAM, n);
        let local = b.local(Ty::I32, Symbol::NONE, 0, n);
        let source = b.place(&Place::local(local));
        let reference = b.emit(Tag::Ref, source, ref_word::LOCAL, Ty::I32, n);
        let alias = b.local(Ty::I32, Symbol::NONE, 0, n);
        b.set(alias, reference, n);
        let alias_place = b.place(&Place::local(alias));
        b.emit(
            Tag::Ref,
            alias_place,
            ref_word::LOCAL | ref_word::MUT,
            Ty::I32,
            n,
        );
        let read = b.get(alias, Ty::I32, n);
        let observed = b.place(&Place {
            root: PlaceRoot::Value(read),
            proj: vec![Proj::Deref],
        });
        let external = b.place(&Place {
            root: PlaceRoot::Local(parameter),
            proj: vec![Proj::Deref],
        });
        let pointer = b.place(&Place {
            root: PlaceRoot::Ptr(reference),
            proj: vec![],
        });
        let twice = b.place(&Place {
            root: PlaceRoot::Value(reference),
            proj: vec![Proj::Deref, Proj::Deref],
        });
        let index = b.const_value(Ty::prim(hd_types::Prim::Usize), 0);
        let element = b.place(&Place {
            root: PlaceRoot::Local(local),
            proj: vec![Proj::Elem(index)],
        });
        let buffer_reference = b.emit(Tag::Ref, element, ref_word::LOCAL, Ty::I32, n);
        let buffer = b.place(&Place {
            root: PlaceRoot::Value(buffer_reference),
            proj: vec![Proj::Deref],
        });
        let external_value = b.emit(Tag::Read, external, ReadMode::Use as u32, Ty::I32, n);
        let identity = b.coerce(Coercion::Identity, NONE, external_value, Ty::I32, n);
        let coerced_element = b.place(&Place {
            root: PlaceRoot::Value(identity),
            proj: vec![Proj::Elem(index)],
        });
        let root = b.close_block(block, None, Ty::VOID, n);
        let body = b.finish(root, &[]).map_err(|e| format!("{e:?}"))?;
        let places = Places::new(&body);
        assert_eq!(places.get(&body, coerced_element), (false, true));
        for target in [observed, external, pointer, twice, buffer] {
            assert!(!places.get(&body, target).0);
        }
        Ok(())
    }
}
