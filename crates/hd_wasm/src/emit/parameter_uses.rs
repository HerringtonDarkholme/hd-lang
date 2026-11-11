//! Immutable parameter-use facts for instance loan queries.
//!
//! Observe each instruction once. A shared nonbare place is classified once,
//! so work is O(instructions + distinct local-place projection steps), with
//! expected constant-time lookups and no traversal controlled by native depth.

use hd_base::{HashMap, HashSet};
use hd_tir::ir::{Body, Proj, ReadMode, Root, Tag, place};

#[derive(Clone, Copy)]
struct PlaceFacts {
    local: Option<usize>,
    value: Option<u32>,
}

#[derive(Default)]
pub(super) struct ParameterUses {
    reads: HashMap<usize, Vec<usize>>,
    moved: HashSet<u32>,
    always: HashSet<usize>,
    framed: HashSet<usize>,
    places: HashMap<u32, PlaceFacts>,
}

impl ParameterUses {
    /// One instruction visit, plus one first place decode and each projection
    /// actually inspected. The caller supplies instructions in body order.
    pub(super) fn observe(&mut self, body: &Body, index: usize, visit: &mut impl FnMut()) {
        visit();
        let tag = body.tags[index];
        if !matches!(tag, Tag::Read | Tag::Store | Tag::Ref) {
            return;
        }
        let word = body.data[index][0];
        if word & place::BARE_LOCAL != 0 {
            let local = (word & !place::BARE_LOCAL) as usize;
            if tag == Tag::Read {
                self.reads.entry(local).or_default().push(index);
            } else {
                self.always.insert(local);
            }
            return;
        }
        let facts = *self.places.entry(word).or_insert_with(|| {
            visit();
            let view = body.view(word);
            let local = view.local().filter(|_| {
                view.steps(body).all(|step| {
                    visit();
                    matches!(step, Proj::Field(_) | Proj::Variant(_))
                })
            });
            PlaceFacts {
                local: local.map(|local| local as usize),
                value: match view.root {
                    Root::Value(value) => Some(value),
                    _ => None,
                },
            }
        });
        if let Some(local) = facts.local {
            if tag == Tag::Read {
                self.framed.insert(local);
            } else {
                self.always.insert(local);
            }
        }
        if tag == Tag::Read
            && body.data[index][1] == ReadMode::Move as u32
            && let Some(value) = facts.value
        {
            self.moved.insert(value);
        }
    }

    pub(super) fn reads(&self, local: usize) -> &[usize] {
        self.reads.get(&local).map_or(&[], Vec::as_slice)
    }

    pub(super) fn moved(&self, value: u32) -> bool {
        self.moved.contains(&value)
    }

    pub(super) fn rejects(&self, local: usize, framed: bool) -> bool {
        self.always.contains(&local) || framed && self.framed.contains(&local)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hd_base::DefId;
    use hd_tir::ir::BodyKind;
    use hd_types::Ty;
    use std::num::TryFromIntError;

    // These tests exercise instruction/place metadata only. Local field types
    // are irrelevant to the query; no type/layout computation is requested.
    fn body(locals: usize) -> Body {
        let mut body = Body::new(DefId::from_raw(1), BodyKind::Fn);
        body.local_ty.resize(locals, Ty::I32);
        body
    }

    fn emit(body: &mut Body, tag: Tag, word: u32, mode: u32) -> usize {
        let index = body.len();
        body.tags.push(tag);
        body.data.push([word, mode]);
        index
    }

    fn record(
        body: &mut Body,
        root: u32,
        value: u32,
        steps: &[u32],
    ) -> Result<u32, TryFromIntError> {
        let word = u32::try_from(body.extra.len())?;
        body.extra.push(u32::try_from(2 + steps.len())?);
        body.extra.extend([root, value]);
        body.extra.extend_from_slice(steps);
        Ok(word)
    }

    fn census(body: &Body) -> (ParameterUses, usize) {
        let mut out = ParameterUses::default();
        let mut visits = 0;
        for index in 0..body.len() {
            out.observe(body, index, &mut || visits += 1);
        }
        (out, visits)
    }

    #[test]
    fn bare_reads_and_in_place_rejections_preserve_the_scan_rules() -> Result<(), TryFromIntError> {
        let mut body = body(8);
        let first = emit(
            &mut body,
            Tag::Read,
            place::BARE_LOCAL,
            ReadMode::Use as u32,
        );
        emit(&mut body, Tag::Store, place::BARE_LOCAL | 1, 0);
        emit(&mut body, Tag::Ref, place::BARE_LOCAL | 2, 0);
        for (tag, local, steps) in [
            (Tag::Store, 3, vec![place::PROJ_FIELD]),
            (Tag::Ref, 4, vec![place::PROJ_VARIANT]),
            (Tag::Read, 5, vec![place::PROJ_FIELD]),
            (Tag::Store, 6, vec![place::PROJ_DEREF]),
            (Tag::Ref, 7, vec![place::PROJ_ELEM, 0]),
        ] {
            let word = record(&mut body, place::ROOT_LOCAL, local, &steps)?;
            emit(&mut body, tag, word, ReadMode::Use as u32);
        }
        let first_word = u32::try_from(first)?;
        // Moving any projection of a value prevents lending the original read.
        let moved = record(
            &mut body,
            place::ROOT_VALUE,
            first_word,
            &[place::PROJ_FIELD],
        )?;
        emit(&mut body, Tag::Read, moved, ReadMode::Move as u32);
        let (facts, _) = census(&body);
        assert_eq!(facts.reads(0), [first]);
        assert!(facts.reads(5).is_empty());
        assert!(facts.reads(100).is_empty());
        assert!(facts.moved(first_word));
        assert!(!facts.moved(1));
        for local in 0..8 {
            assert_eq!(facts.rejects(local, false), (1..=4).contains(&local));
            assert_eq!(facts.rejects(local, true), (1..=5).contains(&local));
        }
        Ok(())
    }

    #[test]
    fn nonlocal_and_indirect_places_do_not_reject_the_named_local() -> Result<(), TryFromIntError> {
        let mut body = body(1);
        let first = emit(
            &mut body,
            Tag::Read,
            place::BARE_LOCAL,
            ReadMode::Use as u32,
        );
        let first_word = u32::try_from(first)?;
        for steps in [vec![place::PROJ_DEREF], vec![place::PROJ_ELEM, first_word]] {
            let word = record(&mut body, place::ROOT_LOCAL, 0, &steps)?;
            for tag in [Tag::Read, Tag::Store, Tag::Ref] {
                emit(&mut body, tag, word, ReadMode::Use as u32);
            }
        }
        let word = record(&mut body, place::ROOT_VALUE, first_word, &[])?;
        emit(&mut body, Tag::Read, word, ReadMode::Use as u32);
        let (facts, _) = census(&body);
        assert!(!facts.rejects(0, false));
        assert!(!facts.rejects(0, true));
        assert!(!facts.moved(first_word));
        Ok(())
    }

    #[test]
    fn shared_deep_places_are_walked_once_at_width_thousand_and_depth_thousand()
    -> Result<(), TryFromIntError> {
        for (width, depth) in [(100, 20), (1_000, 20), (20, 1_000)] {
            let mut body = body(width);
            for local in 0..width {
                let local_word = u32::try_from(local)?;
                emit(
                    &mut body,
                    Tag::Read,
                    place::BARE_LOCAL | local_word,
                    ReadMode::Use as u32,
                );
                let steps = vec![place::PROJ_FIELD; depth];
                let word = record(&mut body, place::ROOT_LOCAL, local_word, &steps)?;
                for _ in 0..20 {
                    emit(&mut body, Tag::Read, word, ReadMode::Use as u32);
                }
            }
            let (facts, visits) = census(&body);
            assert_eq!(visits, body.len() + width * (depth + 1));
            for local in 0..width {
                assert_eq!(facts.reads(local), [local * 21]);
                assert!(!facts.rejects(local, false));
                assert!(facts.rejects(local, true));
            }
        }
        Ok(())
    }
}
