//! Storage reached by a write, including fully visible local-reference aliases.
//! This classifies effects; it does not remove an address, loan or check.
//! One store/reference census and one topological value walk cost O(N + P)
//! time and O(N + L) space, for instructions N, projection words P and locals L.
//! Alias depth adds instructions, never recursive walks or repeated rescans.

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
