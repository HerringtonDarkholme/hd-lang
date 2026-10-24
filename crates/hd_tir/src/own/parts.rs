//! Part places: the slots of the parts a `move` read or a `Drop` takes of
//! one root, and whether the parts taken leave a counted leaf. Known-null
//! and the verifier's `Balance` share this type arithmetic; each tracks
//! on its own which slots a path took.

use std::ops::Range;

use hd_types::Ty;

use super::{OwnCx, Tri, fill};
use crate::ir::{Body, NONE, ReadMode, Tag, place, u32_of};

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
    word: Vec<u32>,
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
            .sort_unstable_by(|p, q| p.0.cmp(&q.0).then_with(|| proj(p.2).cmp(proj(q.2))));
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
            return cx.counted(ty) != Tri::No;
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
                    if cx.counted(ft) != Tri::No
                        && (s0 == s1 || self.uncovered(b, cx, ft, (s0, s1), step + 1))
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
