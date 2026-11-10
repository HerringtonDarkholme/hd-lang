//! Temporary values borrowed by local references get explicit owner locals.
//! This is the same lifetime mechanism as a named collection or object:
//! frame analysis follows its references and holders through their last uses.

use super::{OwnCx, lending_coercion, owned_value, splice};
use crate::ir::{Body, NONE, ReadMode, Root, Tag, Topology, local_flags, place, u32_of};
use hd_base::Symbol;
use hd_types::Ty;

/// Per-body identity maps, reused across bodies without scanning old capacity.
#[derive(Default)]
pub(crate) struct Tables {
    owners: Vec<u32>,
    places: Vec<u32>,
    first: Vec<u32>,
    nested: Vec<bool>,
    sources: Vec<u32>,
    seen: Vec<bool>,
    pending: Vec<usize>,
}

pub(crate) fn run(
    body: &mut Body,
    cx: &dyn OwnCx,
    topo: &mut Topology,
    tables: &mut Tables,
    edits: &mut splice::Tables,
) -> Result<(), String> {
    normalize(
        body,
        |body, i| {
            if lending_coercion(body, cx, i) {
                Some(body.data[i][0] as usize)
            } else {
                reference_value(body, i, |ty| cx.holds_ref(ty) && !cx.counted_refs_only(ty))
            }
        },
        |ty| cx.reference_target(ty).is_none() || cx.counted_refs_only(ty),
        || cx.temporary_owner_name(),
        topo,
        tables,
        edits,
        |_| {},
    )
}

fn reference_value(body: &Body, i: usize, local: impl Fn(Ty) -> bool) -> Option<usize> {
    if body.tags[i] == Tag::Ref && local(body.ty[i]) {
        match body.view(body.data[i][0]).root {
            Root::Value(value) => Some(value as usize),
            _ => None,
        }
    } else {
        None
    }
}

/// Scans instructions a bounded number of times, walks each Read root once,
/// and copies each shared place record once. `measure` counts those actual visits and bulk copies for the cost test.
fn normalize(
    body: &mut Body,
    source: impl Fn(&Body, usize) -> Option<usize>,
    owns_storage: impl Fn(Ty) -> bool,
    owner_name: impl Fn() -> Symbol,
    topo: &mut Topology,
    tables: &mut Tables,
    edits: &mut splice::Tables,
    mut measure: impl FnMut(usize),
) -> Result<(), String> {
    let direct = |body: &Body, i: usize| source(body, i).filter(|value| *value < body.len());
    let found = (0..body.len()).any(|i| {
        measure(1);
        direct(body, i).is_some()
    });
    if !found {
        return Ok(());
    }

    // A Read already denotes storage. Follow value-backed Read roots once
    // per instruction, stopping at a named/global/dereferenced place. Own
    // the actual temporary behind a projected field, never a copied header.
    tables.sources.clear();
    tables.sources.resize(body.len(), NONE);
    tables.seen.clear();
    tables.seen.resize(body.len(), false);
    tables.pending.clear();
    measure(2 * body.len());
    for i in 0..body.len() {
        measure(1);
        let Some(value) = direct(body, i) else {
            continue;
        };
        if body.tags[value] == Tag::Read {
            if !tables.seen[value] {
                tables.seen[value] = true;
                tables.pending.push(value);
            }
        } else {
            tables.sources[i] = u32_of(value);
        }
    }
    while let Some(value) = tables.pending.pop() {
        measure(1);
        let Root::Value(parent) = body.view(body.data[value][0]).root else {
            continue;
        };
        let parent = parent as usize;
        if body.tags[parent] == Tag::Read {
            if !tables.seen[parent] {
                tables.seen[parent] = true;
                tables.pending.push(parent);
            }
        } else {
            tables.sources[value] = u32_of(parent);
        }
    }
    let candidate = |tables: &Tables, i: usize| {
        (tables.sources[i] != NONE).then_some(tables.sources[i] as usize)
    };
    let planned = tables.sources.iter().any(|source| {
        measure(1);
        *source != NONE
    });
    if !planned {
        return Ok(());
    }
    topo.build(body);
    tables.owners.clear();
    tables.owners.resize(body.len(), NONE);
    tables.places.clear();
    tables.places.resize(body.extra.len(), NONE);
    tables.first.clear();
    tables.first.resize(body.len(), NONE);
    tables.nested.clear();
    tables.nested.resize(body.len(), false);
    measure(3 * body.len() + body.extra.len());
    edits.clear();
    let instructions = body.len();
    // Delay a move only when every borrower is a statement of the source's
    // own block. Use execution positions, not instruction numbering. One
    // earlier nested borrower forces placement immediately after the source.
    for i in 0..instructions {
        measure(1);
        if let Some(value) = candidate(tables, i) {
            if topo.blk[value] == topo.blk[i] {
                tables.first[value] = tables.first[value].min(topo.pos[i]);
            } else {
                tables.nested[value] = true;
            }
        }
    }
    for i in 0..instructions {
        measure(1);
        let Some(value) = candidate(tables, i) else {
            continue;
        };
        // A block's value belongs to its owner; a borrow already has an
        // owner. Leave malformed or cross-sub-body values to the verifier.
        if !owned_value(body, value)
            || !owns_storage(body.ty[value])
            || topo.blk[value] == NONE
            || topo.sub[value] != topo.sub[i]
        {
            continue;
        }
        let local = if tables.owners[value] == NONE {
            let local = u32_of(body.local_ty.len());
            body.local_ty.push(body.ty[value]);
            body.local_name.push(owner_name());
            body.local_syn.push(body.syn[value]);
            body.local_flags
                .push(local_flags::ASSIGNED | local_flags::READ);
            // In the same block, move the value into its owner just before
            // the first reference: map-entry lookup has already lent it, so
            // the store transfers its sole count instead of copying it.
            // Across branches, bind at the producer so every path sees it.
            let position = if tables.nested[value] {
                topo.pos[value] + 1
            } else {
                tables.first[value]
            };
            edits.insert(
                topo.blk[value],
                position,
                Tag::Store,
                [place::BARE_LOCAL | local, u32_of(value)],
                Ty::VOID,
                body.syn[value],
            );
            tables.owners[value] = local;
            local
        } else {
            tables.owners[value]
        };
        if body.tags[i] == Tag::Coerce {
            // The existing lending-read form borrows a counted pointer
            // from its named owner without copying or changing its bits.
            body.tags[i] = Tag::Read;
            body.data[i] = [place::BARE_LOCAL | local, ReadMode::Use as u32];
            continue;
        }
        let word = body.data[i][0] as usize;
        let rewritten = if tables.places[word] == NONE {
            // Both roots use two words. Preserve every projection and its
            // evaluated index; never evaluate the source or index again.
            let words = body.extra[word] as usize + 1;
            let rewritten = u32_of(body.extra.len());
            body.extra.extend_from_within(word..word + words);
            body.extra[rewritten as usize + 1] = place::ROOT_LOCAL;
            body.extra[rewritten as usize + 2] = local;
            tables.places[word] = rewritten;
            measure(words);
            rewritten
        } else {
            tables.places[word]
        };
        body.data[i][0] = rewritten;
    }
    splice::apply(body, topo, edits)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::{BodyKind, Place, PlaceRoot, Proj, TirBuilder, TirSink, verify_checked};
    use hd_base::{DefId, NodeIdx};

    const LOCAL_REF: Ty = Ty(81);

    fn apply(body: &mut Body) -> Result<usize, String> {
        let mut work = 0;
        normalize(
            body,
            |body, i| reference_value(body, i, |ty| ty == LOCAL_REF),
            |ty| ty != LOCAL_REF,
            || Symbol::NONE,
            &mut Topology::default(),
            &mut Tables::default(),
            &mut splice::Tables::default(),
            |n| work += n,
        )?;
        assert_eq!(verify_checked(body), []);
        let mut root_work = 0;
        let roots = super::super::frame::read_roots(body, |n| root_work += n);
        assert!(root_work <= 6 * body.len(), "shared root work {root_work}");
        for (i, tag) in body.tags.iter().enumerate() {
            if *tag == Tag::Read {
                assert_eq!(roots[i], 0, "every projected read uses the one owner");
            }
        }
        Ok(work)
    }

    // This generated test enforces the work/dominance contract; runtime HD
    // and stage fixtures provide the compiler-behavior regression coverage.
    #[test]
    fn shared_paths_are_linear_and_owner_stores_dominate_every_borrower() -> Result<(), String> {
        for (width, depth, reads) in [
            (1000, 20, false),
            (20, 1000, false),
            (1000, 20, true),
            (20, 1000, true),
        ] {
            let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
            let block = builder.open_block();
            let mut value = builder.const_value(Ty::I32, 7);
            // One-field tuple types have opaque, body-global type handles.
            for level in 0..depth {
                let fields = builder.refs_record(&[value]);
                value = builder.emit(
                    Tag::NewTuple,
                    NONE,
                    fields,
                    Ty(100 + u32_of(level)),
                    NodeIdx::NONE,
                );
            }
            if reads {
                for level in (0..depth).rev() {
                    let path = builder.place(&Place {
                        root: PlaceRoot::Value(value),
                        proj: vec![Proj::Field(0)],
                    });
                    let child = if level == 0 {
                        Ty::I32
                    } else {
                        Ty(100 + u32_of(level - 1))
                    };
                    value =
                        builder.emit(Tag::Read, path, ReadMode::Use as u32, child, NodeIdx::NONE);
                }
            }
            let path = builder.place(&Place {
                root: PlaceRoot::Value(value),
                proj: if reads {
                    vec![]
                } else {
                    vec![Proj::Field(0); depth]
                },
            });
            for _ in 0..width {
                builder.emit(Tag::Ref, path, 0, LOCAL_REF, NodeIdx::NONE);
            }
            let root = builder.close_block(block, None, Ty::VOID, NodeIdx::NONE);
            let mut body = builder
                .finish(root, &[])
                .map_err(|errors| format!("{errors:?}"))?;
            let input = body.len() + body.extra.len();
            let work = apply(&mut body)?;
            assert!(work <= 16 * input, "{width} x {depth}: {work} for {input}");
            assert_eq!(body.local_ty.len(), 1, "one owner, not one per reference");
        }

        let mut builder = TirBuilder::new(DefId::from_raw(1), BodyKind::Fn);
        let block = builder.open_block();
        let one = builder.const_value(Ty::I32, 7);
        let fields = builder.refs_record(&[one]);
        let owner = builder.emit(Tag::NewTuple, NONE, fields, Ty(80), NodeIdx::NONE);
        let outer = builder.reserve();
        let path = builder.place(&Place {
            root: PlaceRoot::Value(owner),
            proj: vec![],
        });
        let branch = builder.open_block();
        builder.emit(Tag::Ref, path, 0, LOCAL_REF, NodeIdx::NONE);
        let branch = builder.close_block(branch, None, Ty::VOID, NodeIdx::NONE);
        let other = builder.open_block();
        let other = builder.close_block(other, None, Ty::VOID, NodeIdx::NONE);
        let arms = builder.refs_record(&[branch, other]);
        let yes = builder.const_value(Ty::BOOL, 1);
        let choose = builder.emit(Tag::If, yes.0, arms, Ty::VOID, NodeIdx::NONE);
        let outer = builder.fill(outer, Tag::Ref, path, 0, LOCAL_REF, NodeIdx::NONE);
        let root = builder.close_block(block, None, Ty::VOID, NodeIdx::NONE);
        let mut body = builder
            .finish(root, &[])
            .map_err(|errors| format!("{errors:?}"))?;
        // The lower-numbered outer Ref executes after the conditional.
        let record = body.data[root.0 as usize][0] as usize;
        let len = body.extra[record] as usize;
        let members = &mut body.extra[record + 1..record + 1 + len];
        let from = members
            .iter()
            .position(|i| *i == outer.0)
            .ok_or("outer reference absent")?;
        members[from..].rotate_left(1);
        assert!(outer.0 < choose.0);
        apply(&mut body)?;
        let topo = Topology::new(&body);
        let store = body
            .tags
            .iter()
            .position(|tag| *tag == Tag::Store)
            .ok_or("owner store absent")?;
        let choose = body
            .tags
            .iter()
            .position(|tag| *tag == Tag::If)
            .ok_or("conditional absent")?;
        let outer = body
            .tags
            .iter()
            .enumerate()
            .find_map(|(i, tag)| (*tag == Tag::Ref && topo.blk[i] == body.sub_root[0]).then_some(i))
            .ok_or("outer reference absent")?;
        assert_eq!(topo.blk[store], body.sub_root[0]);
        assert!(topo.pos[store] < topo.pos[choose]);
        assert!(topo.pos[store] < topo.pos[outer]);
        Ok(())
    }
}
