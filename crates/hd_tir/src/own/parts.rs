//! Part places: the slots of the parts a `move` read or a `Drop` takes of
//! one root, and whether the parts taken leave a counted leaf. Known-null
//! and the verifier's `Balance` share this type arithmetic; each tracks
//! on its own which slots a path took.

use hd_base::{HashMap, HashSet, SliceSort as _};
use std::ops::Range;

use hd_types::Ty;

use super::{OwnCx, fill};
use crate::ir::{Body, NONE, ReadMode, Tag, place, u32_of};

/// Empty-payload edge facts for one body. LLVM's `LazyValueInfo` computes
/// a switch default by excluding cases that target other successors;
/// explicit cases sharing the default remain possible. Here the finite
/// variant set makes that a single pass, cached before the flow walk.
/// Cost: O(instructions + case words + distinct types' variant fields),
/// with O(1) edge queries, independent of flow rounds and nesting depth.
#[derive(Default)]
pub(crate) struct EmptyVariants {
    edges: HashMap<(u32, u32), u32>,
    #[cfg(test)]
    visits: usize,
}

impl EmptyVariants {
    pub fn build(b: &Body, cx: &dyn OwnCx) -> Self {
        struct Variants {
            empty: Vec<bool>,
            nonempty: usize,
            excluded: Vec<usize>,
            complete: bool,
        }
        let mut out = Self::default();
        let mut addressed = HashSet::default();
        for i in 0..b.len() {
            #[cfg(test)]
            {
                out.visits += 1;
            }
            if matches!(b.tags[i], Tag::Ref | Tag::Store)
                && let Some(v) = b.view(b.data[i][0]).value()
            {
                addressed.insert(v);
            }
        }
        let mut types: HashMap<Ty, Variants> = HashMap::default();
        let mut explicit: HashMap<(Ty, u32), bool> = HashMap::default();
        let mut parts = Vec::new();
        let mut blocks: HashMap<u32, bool> = HashMap::default();
        for x in 0..b.len() {
            #[cfg(test)]
            {
                out.visits += 1;
            }
            if b.tags[x] != Tag::SwitchTag {
                continue;
            }
            let [value, record] = b.data[x];
            let Some(v) = crate::ir::Ref(value).as_inst().map(|v| v.0 as usize) else {
                continue;
            };
            if v >= b.len()
                || b.is_borrow(value)
                || addressed.contains(&value)
                || !cx.counted(b.ty[v])
            {
                continue;
            }
            let ty = b.ty[v];
            let info = types.entry(ty).or_insert_with(|| {
                let mut empty = Vec::new();
                let complete = cx.empty_variants(ty, &mut empty);
                Variants {
                    nonempty: empty.iter().filter(|&&empty| !empty).count(),
                    excluded: vec![usize::MAX; empty.len()],
                    empty,
                    complete,
                }
            });
            let Some((&default, cases)) = b.record(record).split_last() else {
                continue;
            };
            blocks.clear();
            let mut remaining = info.nonempty;
            for case in cases.chunks_exact(2) {
                #[cfg(test)]
                {
                    out.visits += 1;
                }
                let (tag, block) = (case[0], case[1]);
                let empty = if info.complete {
                    info.empty.get(tag as usize).copied().unwrap_or(false)
                } else {
                    *explicit.entry((ty, tag)).or_insert_with(|| {
                        cx.parts(ty, Some(tag), &mut parts)
                            && !parts.iter().any(|&field| cx.counted(field))
                    })
                };
                blocks
                    .entry(block)
                    .and_modify(|all| *all &= empty)
                    .or_insert(empty);
                if block != default
                    && info.empty.get(tag as usize) == Some(&false)
                    && info.excluded[tag as usize] != x
                {
                    info.excluded[tag as usize] = x;
                    remaining -= 1;
                }
            }
            let empty_default = info.complete && !info.empty.is_empty() && remaining == 0;
            blocks
                .entry(default)
                .and_modify(|all| *all &= empty_default)
                .or_insert(empty_default);
            for (&block, &empty) in &blocks {
                #[cfg(test)]
                {
                    out.visits += 1;
                }
                if empty && block != NONE {
                    out.edges.insert((u32_of(x), block), value);
                }
            }
        }
        out
    }

    pub fn get(&self, switch: u32, block: u32) -> Option<u32> {
        self.edges.get(&(switch, block)).copied()
    }
}

/// The `move` reads and the `Drop`s that `drop` keeps, by index, with the
/// place word each takes all or a part of.
pub(crate) fn takes(b: &Body, drop: impl Fn(usize) -> bool) -> impl Iterator<Item = (usize, u32)> {
    (0..b.len()).filter_map(move |i| {
        let [a, w] = b.data[i];
        let take = match b.tags[i] {
            Tag::Read => w == ReadMode::Move as u32,
            Tag::Drop => drop(i),
            _ => false,
        };
        take.then_some((i, a))
    })
}

/// A body's part slots: each distinct part place of one root (a key) that
/// a `move` read or a `Drop` (an item) takes, with scratch for
/// [`Slots::rest`].
#[derive(Default)]
pub(crate) struct Slots {
    /// Per item, its slot, or `NONE`.
    pub of: Vec<u32>,
    /// Per key, its slots in the order of their projection words, in
    /// compressed rows, with a place word of each.
    start: Vec<u32>,
    pub word: Vec<u32>,
    takes: Vec<(u32, u32, u32)>,
    paths: Vec<(u32, u32)>,
    tys: Vec<Ty>,
    parts: Vec<Ty>,
}

impl Slots {
    /// Numbers the part takes `(key, item, place word)` of `items` items
    /// over `keys` keys: one slot per distinct projection of a key.
    /// Returns how many slots.
    pub fn build(
        &mut self,
        b: &Body,
        items: usize,
        keys: usize,
        takes: impl Iterator<Item = (u32, u32, u32)>,
    ) -> u32 {
        self.takes.clear();
        self.takes.extend(takes);
        let proj = |word: u32| {
            let p = b.view(word);
            &b.extra[p.from as usize..p.to as usize]
        };
        self.takes
            .sort_stable_by(|p, q| p.0.cmp(&q.0).then_with(|| proj(p.2).cmp(proj(q.2))));
        fill(&mut self.of, items, NONE);
        fill(&mut self.start, keys + 1, 0);
        self.word.clear();
        for x in 0..self.takes.len() {
            let (key, item, word) = self.takes[x];
            let (k0, _, w0) = self.takes[x.saturating_sub(1)];
            if x == 0 || k0 != key || proj(w0) != proj(word) {
                self.word.push(word);
                self.start[key as usize + 1] += 1;
            }
            self.of[item as usize] = u32_of(self.word.len() - 1);
        }
        for k in 0..keys {
            self.start[k + 1] += self.start[k];
        }
        u32_of(self.word.len())
    }

    /// The slots of key `key`.
    pub fn of_key(&self, key: u32) -> Range<u32> {
        self.start[key as usize]..self.start[key as usize + 1]
    }

    /// Whether a value of type `ty` at key `key` still holds a counted
    /// leaf beside the slots `taken` (by slot and place word) says the path
    /// took: a counted field that no taken place covers, or a step whose
    /// type's parts `cx` does not give.
    pub fn rest(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        key: u32,
        ty: Ty,
        mut taken: impl FnMut(u32, u32) -> bool,
    ) -> bool {
        self.paths.clear();
        for s in self.of_key(key) {
            let word = self.word[s as usize];
            if taken(s, word) {
                let p = b.view(word);
                self.paths.push((p.from, p.to));
            }
        }
        self.tys.clear();
        let end = self.paths.len();
        self.uncovered(b, cx, ty, (0, end), 0)
    }

    /// Whether a value of type `ty` holds a counted leaf that none of the
    /// part places `self.paths[lo..hi]` covers, at projection step
    /// `depth`: a value is one variant on a path. The paths are the
    /// slots' distinct projections in order, and share their first
    /// `depth` steps, so a place that ends at `depth` comes first, and the
    /// places of one variant, of the fields, and of one field are each a
    /// run.
    fn uncovered(
        &mut self,
        b: &Body,
        cx: &dyn OwnCx,
        ty: Ty,
        (lo, hi): (usize, usize),
        depth: u32,
    ) -> bool {
        let word = |p: (u32, u32), d: u32| (p.0 + d < p.1).then(|| b.extra[(p.0 + d) as usize]);
        if lo == hi {
            return cx.counted(ty);
        }
        if word(self.paths[lo], depth).is_none() {
            return false;
        }
        let mut x = lo;
        while x < hi {
            let w = word(self.paths[x], depth).unwrap_or(0);
            let variant = match w & place::PROJ_KIND {
                place::PROJ_VARIANT => Some(w & place::PROJ_INDEX),
                place::PROJ_FIELD => None,
                _ => return true,
            };
            let kind = |v: u32| v & place::PROJ_KIND == place::PROJ_FIELD;
            let group = self.paths[x..hi]
                .iter()
                .take_while(|&&p| {
                    word(p, depth).is_some_and(|v| v == w || variant.is_none() && kind(v))
                })
                .count();
            let (end, step) = (x + group, depth + u32::from(variant.is_some()));
            // A place that ends at the variant covers all of it.
            if word(self.paths[x], step).is_some() {
                if !cx.parts(ty, variant, &mut self.parts) {
                    return true;
                }
                let t0 = self.tys.len();
                self.tys.extend_from_slice(&self.parts);
                for j in t0..self.tys.len() {
                    let field = Some(place::PROJ_FIELD | u32_of(j - t0));
                    let run = &self.paths[x..end];
                    let s0 = x + run.partition_point(|&p| word(p, step) < field);
                    let s1 = x + run.partition_point(|&p| word(p, step) <= field);
                    let ft = self.tys[j];
                    if cx.counted(ft) && (s0 == s1 || self.uncovered(b, cx, ft, (s0, s1), step + 1))
                    {
                        self.tys.truncate(t0);
                        return true;
                    }
                }
                self.tys.truncate(t0);
            }
            x = end;
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::EmptyVariants;
    use crate::ir::{Body, BodyKind, Callee, ReadMode, Tag, u32_of};
    use crate::own::Pass;
    use hd_base::DefId;
    use hd_types::Ty;
    use std::cell::Cell;

    struct Cx {
        empty: Vec<bool>,
        queries: Cell<usize>,
        fields: Cell<usize>,
    }

    crate::own::test_cx::own_cx!(Cx, [temporary_owner_name, received], {
        fn counted(&self, _: Ty) -> bool {
            true
        }
        fn pass(&self, _: &Callee, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn pass_value(&self, _: Ty, _: usize, _: usize) -> Pass {
            Pass::Lent
        }
        fn may_write(&self, _: &Callee) -> bool {
            false
        }
        fn parts(&self, _: Ty, _: Option<u32>, _: &mut Vec<Ty>) -> bool {
            panic!("a complete variant classification must not repeat parts queries")
        }
        fn empty_variants(&self, _: Ty, out: &mut Vec<bool>) -> bool {
            self.queries.set(self.queries.get() + 1);
            self.fields.set(self.fields.get() + self.empty.len());
            out.clone_from(&self.empty);
            true
        }

        fn holds_coll(&self, _: Ty) -> bool {
            false
        }
    });

    fn context(empty: Vec<bool>) -> Cx {
        Cx {
            empty,
            queries: Cell::new(0),
            fields: Cell::new(0),
        }
    }

    fn inst(b: &mut Body, tag: Tag, a: u32, w: u32) -> u32 {
        let i = u32_of(b.len());
        b.tags.push(tag);
        b.data.push([a, w]);
        b.ty.push(Ty::STRING);
        i
    }

    fn switch(b: &mut Body, cases: &[u32]) -> u32 {
        let r = u32_of(b.extra.len());
        b.extra.push(u32_of(cases.len()));
        b.extra.extend_from_slice(cases);
        inst(b, Tag::SwitchTag, 0, r)
    }

    fn body() -> Body {
        let mut b = Body::new(DefId::from_raw(1), BodyKind::Fn);
        inst(&mut b, Tag::Poison, 0, 0);
        b
    }

    #[test]
    fn switch_default_excludes_only_tags_that_target_another_block() {
        let cx = context(vec![false, true, false, true]);
        let mut b = body();
        let all_counted_explicit = switch(&mut b, &[0, 100, 2, 102, 103]);
        let shared_counted_default = switch(&mut b, &[0, 100, 2, 103, 103]);
        let shared_empty_default = switch(&mut b, &[0, 100, 1, 103, 2, 102, 103]);
        let mixed_explicit_target = switch(&mut b, &[0, 100, 1, 100, 2, 102, 103]);
        let duplicate_tag = switch(&mut b, &[0, 100, 0, 100, 103]);
        let edges = EmptyVariants::build(&b, &cx);
        assert_eq!(edges.get(all_counted_explicit, 103), Some(0));
        assert_eq!(edges.get(shared_counted_default, 103), None);
        assert_eq!(edges.get(shared_empty_default, 103), Some(0));
        assert_eq!(edges.get(mixed_explicit_target, 100), None);
        assert_eq!(edges.get(duplicate_tag, 103), None);
        assert_eq!(cx.queries.get(), 1);
        assert_eq!(cx.fields.get(), 4);
    }

    #[test]
    fn switch_edges_do_not_empty_borrowed_or_addressed_values_or_unknown_layouts() {
        let mut b = body();
        let sw = switch(&mut b, &[0, 100, 101]);
        assert_eq!(
            EmptyVariants::build(&b, &context(vec![false, true])).get(sw, 101),
            Some(0)
        );
        assert_eq!(
            EmptyVariants::build(&b, &context(vec![])).get(sw, 101),
            None
        );
        let borrow = inst(&mut b, Tag::Read, 0, ReadMode::Borrow as u32);
        b.data[sw as usize][0] = borrow;
        assert_eq!(
            EmptyVariants::build(&b, &context(vec![false, true])).get(sw, 101),
            None
        );
        b.data[sw as usize][0] = 0;
        let place = u32_of(b.extra.len());
        b.extra.extend([2, crate::ir::place::ROOT_VALUE, 0]);
        inst(&mut b, Tag::Ref, place, 0);
        assert_eq!(
            EmptyVariants::build(&b, &context(vec![false, true])).get(sw, 101),
            None
        );
    }

    #[test]
    fn switch_edge_work_is_linear_in_width_and_independent_of_nesting() {
        for width in [1_000, 10_000] {
            for depth in [1, 20] {
                let mut b = body();
                let cx = context((0..width).map(|k| k == width - 1).collect());
                let block = |b: &mut Body, items: &[u32]| {
                    let r = u32_of(b.extra.len());
                    b.extra.push(u32_of(items.len()));
                    b.extra.extend_from_slice(items);
                    inst(b, Tag::Block, r, crate::ir::NONE)
                };
                let mut default = block(&mut b, &[]);
                let mut switches = Vec::new();
                for _ in 0..depth {
                    let explicit = block(&mut b, &[]);
                    let mut cases: Vec<u32> =
                        (0..u32_of(width - 1)).flat_map(|k| [k, explicit]).collect();
                    cases.push(default);
                    let sw = switch(&mut b, &cases);
                    switches.push((sw, default));
                    default = block(&mut b, &[sw]);
                }
                let edges = EmptyVariants::build(&b, &cx);
                assert_eq!(cx.queries.get(), 1);
                assert_eq!(cx.fields.get(), width);
                assert_eq!(edges.visits, 2 * b.len() + depth * (width + 1));
                for _ in 0..20 {
                    for &(sw, target) in &switches {
                        assert_eq!(edges.get(sw, target), Some(0));
                    }
                }
                assert_eq!(cx.queries.get(), 1, "flow rounds reuse edge facts");
            }
        }
    }
}
