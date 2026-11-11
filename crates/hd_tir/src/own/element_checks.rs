//! Straight-line facts for a private collection local. A mutable element loan
//! exposes the element, while a loan of the whole local can expose its
//! header. A call receiving only element loans and scalars cannot copy
//! or replace a private list header or change its length.

use hd_base::HashMap;
use hd_types::{Prim, Ty};

use crate::ir::{Body, Inst, Proj, Ref, Tag, local_flags, ref_word};

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Index {
    Value(u32),
    Local(u32),
}

/// Repeated uniqueness and bounds checks within straight-line blocks.
#[derive(Default)]
pub struct ElementChecks {
    unique: Vec<bool>,
    bounds: Vec<bool>,
}

fn scalar(ty: Ty) -> bool {
    // The primitive scalar IDs precede String, the first counted primitive.
    ty.0 < Ty::STRING.0
}

fn value_ty(body: &Body, value: u32) -> Option<Ty> {
    if let Some(i) = Ref(value).as_inst() {
        body.ty.get(i.0 as usize).copied()
    } else {
        body.consts
            .get((value & !Ref::CONST_BIT) as usize)
            .map(|&(ty, _)| ty)
    }
}

/// A direct element and fields of its value. Dereferencing a value
/// inside the element or indexing a nested collection has its own checks.
fn element(body: &Body, word: u32) -> Option<(u32, u32)> {
    let place = body.view(word);
    let local = place.local()?;
    let mut steps = place.steps(body);
    let Proj::Elem(index) = steps.next()? else {
        return None;
    };
    steps
        .all(|step| matches!(step, Proj::Field(_)))
        .then_some((local, index.0))
}

fn element_ref(body: &Body, i: usize) -> Option<(u32, u32)> {
    (body.tags.get(i) == Some(&Tag::Ref) && body.data[i][1] & ref_word::LOCAL != 0)
        .then(|| element(body, body.data[i][0]))
        .flatten()
}

fn private_locals(body: &Body, elements: &[Option<(u32, u32)>]) -> Vec<bool> {
    let mut private: Vec<_> = body
        .local_flags
        .iter()
        .map(|&f| f & (local_flags::CAPTURED | local_flags::CAPTURED_ASSIGNED) == 0)
        .collect();
    for local in &body.cap_local {
        private[local.idx()] = false;
    }
    for (i, &tag) in body.tags.iter().enumerate() {
        if tag == Tag::Ref
            && elements[i].is_none()
            && let Some(l) = body.view(body.data[i][0]).local()
            && let Some(slot) = private.get_mut(l as usize)
        {
            *slot = false;
        }
    }
    private
}

/// Return the direct call's arguments if there is no hidden provider.
/// The result cannot carry a new collection owner or an element loan.
fn call_args(body: &Body, i: usize) -> Option<&[u32]> {
    if body.ty[i] != Ty::VOID || !matches!(body.record(body.data[i][0]), [0, _, _]) {
        return None;
    }
    body.record(body.data[i][1]).strip_suffix(&[0, 0, 0])
}

impl ElementChecks {
    /// Collect repeated uniqueness and index checks in one pass over the
    /// block members. Each block and barrier advances the epoch without
    /// clearing a table. The work is linear in instructions, projections
    /// and call arguments, with one entry per check, local or instruction.
    #[must_use]
    pub fn new(body: &Body) -> Self {
        if !body.tags.contains(&Tag::Call) || !body.tags.contains(&Tag::Ref) {
            return Self::default();
        }
        // Decode each projection once, even if its reference is passed to
        // many calls. Argument checks use the indexed result.
        let elements: Vec<_> = body
            .tags
            .iter()
            .enumerate()
            .map(|(i, &tag)| match tag {
                Tag::Read | Tag::Store => element(body, body.data[i][0]),
                Tag::Ref => element_ref(body, i),
                _ => None,
            })
            .collect();
        let private = private_locals(body, &elements);
        let private = |l: u32| private.get(l as usize) == Some(&true);
        let mut unique = vec![0; body.local_ty.len()];
        let mut reads = vec![0; body.len()];
        let mut loans = vec![0; body.len()];
        let mut checked = HashMap::default();
        let mut epoch = 0usize;
        let mut args = Vec::new();
        let mut facts = Self {
            unique: vec![false; body.len()],
            bounds: vec![false; body.len()],
        };
        let index = |value: u32, epoch, reads: &[usize]| {
            if value_ty(body, value) != Some(Ty::prim(Prim::Usize)) {
                return None;
            }
            let local = Ref(value).as_inst().and_then(|i| {
                let i = i.0 as usize;
                (reads.get(i) == Some(&epoch))
                    .then(|| body.local_read(i))
                    .flatten()
                    .map(hd_base::LocalId::raw)
            });
            Some(local.map_or(Index::Value(value), Index::Local))
        };
        for (block, &tag) in body.tags.iter().enumerate() {
            if tag != Tag::Block {
                continue;
            }
            epoch += 1;
            for &member in body.record(body.data[block][0]) {
                let i = member as usize;
                let Some(&tag) = body.tags.get(i) else {
                    epoch += 1;
                    continue;
                };
                let [place, _] = body.data[i];
                let preserved = match tag {
                    Tag::IsUnique => {
                        let p = body.view(place);
                        if let Some(l) = p.local().filter(|&l| p.whole() && private(l)) {
                            facts.unique[i] = unique[l as usize] == epoch;
                            unique[l as usize] = epoch;
                            true
                        } else {
                            false
                        }
                    }
                    Tag::Read | Tag::Store => {
                        if let Some((root, value)) = elements[i].filter(|&(l, _)| private(l)) {
                            if let Some(key) = index(value, epoch, &reads) {
                                facts.bounds[i] = checked.get(&(root, key)) == Some(&epoch);
                            }
                            true
                        } else if tag == Tag::Read && scalar(body.ty[i]) {
                            if body.local_read(i).is_some_and(|l| private(l.raw())) {
                                reads[i] = epoch;
                            }
                            true
                        } else {
                            false
                        }
                    }
                    // A shared short loan may suppress its preceding
                    // IsUnique. Clear the epoch before anything relies on it.
                    Tag::Ref
                        if body.data[i][1] & ref_word::MUT != 0
                            && elements[i].is_some_and(|(l, _)| private(l)) =>
                    {
                        loans[i] = epoch;
                        true
                    }
                    Tag::Call => {
                        args.clear();
                        let element_only = call_args(body, i).is_some_and(|values| {
                            values.iter().all(|&value| {
                                if value_ty(body, value).is_some_and(scalar) {
                                    return true;
                                }
                                let Some(j) =
                                    Ref(value).as_inst().map(|inst: Inst| inst.0 as usize)
                                else {
                                    return false;
                                };
                                if loans.get(j) != Some(&epoch) {
                                    return false;
                                }
                                let Some((root, value)) = elements[j] else {
                                    return false;
                                };
                                args.push((root, value));
                                true
                            })
                        });
                        if element_only && !args.is_empty() {
                            for &(root, value) in &args {
                                if let Some(key) = index(value, epoch, &reads) {
                                    checked.insert((root, key), epoch);
                                }
                            }
                            true
                        } else {
                            false
                        }
                    }
                    Tag::Drop => {
                        // Before loan-end lowering this is the drop of the
                        // plain element reference, which holds no count.
                        let p = body.view(place);
                        p.whole()
                            && p.value()
                                .is_some_and(|v| loans.get(v as usize) == Some(&epoch))
                    }
                    Tag::Prim | Tag::LoanEnd => true,
                    _ => false,
                };
                if !preserved {
                    epoch += 1;
                }
            }
        }
        facts
    }

    #[must_use]
    pub(crate) fn repeats_unique(&self, i: usize) -> bool {
        self.unique.get(i) == Some(&true)
    }

    /// Only an element address passed to an executed call establishes a
    /// bounds fact. A read can be deferred by the emitter, so a read alone
    /// never establishes the fact for a later access.
    #[must_use]
    pub fn repeats_bounds(&self, i: usize) -> bool {
        self.bounds.get(i) == Some(&true)
    }
}
