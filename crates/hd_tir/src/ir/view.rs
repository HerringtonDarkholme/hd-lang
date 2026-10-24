//! A place word read where it lies ([`Body::view`]): its root and the
//! `extra` range of its projection words, which the ownership passes, the
//! verifier and the shape helpers decode without allocating.

use hd_base::DefId;
use hd_types::Ty;

use super::{Body, NONE, Proj, Ref, place};

/// The step codes [`PlaceView::push_steps`] writes: a field's index, a
/// variant's index with `VARIANT` set, and `DEREF` and `ELEM`.
pub const ELEM: u32 = NONE - 1;
pub const DEREF: u32 = NONE - 2;
pub const VARIANT: u32 = 1 << 30;

/// What a place starts at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Root {
    /// A local of the body, `$n`.
    Local(u32),
    /// An instruction's value, `%n`.
    Value(u32),
    /// A constant value, `%cK` (its `Ref` word).
    Const(u32),
    /// A top-level binding and its type, `@d:t`.
    Global(DefId, Ty),
    /// The target of the pointer value `p`, `ptr %p`.
    Ptr(u32),
    /// A frame slot, or a malformed word.
    Other,
}

/// A decoded place word: its root, and the `extra` range `from..to` of its
/// projection words (empty for a bare local or [`Root::Other`]).
#[derive(Clone, Copy, Debug)]
pub struct PlaceView {
    pub root: Root,
    pub from: u32,
    pub to: u32,
}

/// The projections of a [`PlaceView`], left to right.
pub struct Steps<'a>(&'a [u32]);

impl Iterator for Steps<'_> {
    type Item = Proj;

    fn next(&mut self) -> Option<Proj> {
        let (&w, rest) = self.0.split_first()?;
        self.0 = rest;
        Some(match w & place::PROJ_KIND {
            place::PROJ_FIELD => Proj::Field(w & place::PROJ_INDEX),
            place::PROJ_DEREF => Proj::Deref,
            place::PROJ_ELEM => {
                let (&index, rest) = self.0.split_first().unwrap_or((&NONE, &[]));
                self.0 = rest;
                Proj::Elem(Ref(index))
            }
            _ => Proj::Variant(w & place::PROJ_INDEX),
        })
    }
}

impl Body {
    /// Decodes place word `word` ([`place`]) where it lies. A value root
    /// must be a constant or an instruction of the body, and a record
    /// local a local of it; any other root, or a record out of range, is
    /// [`Root::Other`].
    #[must_use]
    pub fn view(&self, word: u32) -> PlaceView {
        let other = PlaceView {
            root: Root::Other,
            from: 0,
            to: 0,
        };
        if word & place::BARE_LOCAL != 0 {
            return PlaceView {
                root: Root::Local(word & !place::BARE_LOCAL),
                ..other
            };
        }
        let Some(&n) = self.extra.get(word as usize) else {
            return other;
        };
        let to = match word.checked_add(1).and_then(|x| x.checked_add(n)) {
            Some(to) if to as usize <= self.extra.len() => to,
            _ => return other,
        };
        let (root, k) = match self.extra[word as usize + 1..to as usize] {
            [place::ROOT_LOCAL, l, ..] if (l as usize) < self.local_ty.len() => (Root::Local(l), 2),
            [place::ROOT_VALUE, v, ..] if Ref(v).as_inst().is_none() => (Root::Const(v), 2),
            [place::ROOT_VALUE, v, ..] if (v as usize) < self.len() => (Root::Value(v), 2),
            [place::ROOT_GLOBAL, d, t, ..] => (Root::Global(DefId::from_raw(d), Ty(t)), 3),
            [place::ROOT_PTR, p, ..] => (Root::Ptr(p), 2),
            _ => return other,
        };
        PlaceView {
            root,
            from: word + 1 + k,
            to,
        }
    }
}

impl PlaceView {
    /// Whether the place is all of its root: no projection.
    #[must_use]
    pub fn whole(self) -> bool {
        self.from == self.to
    }

    /// The local the place is rooted at.
    #[must_use]
    pub fn local(self) -> Option<u32> {
        match self.root {
            Root::Local(l) => Some(l),
            _ => None,
        }
    }

    /// The instruction value the place is rooted at.
    #[must_use]
    pub fn value(self) -> Option<u32> {
        match self.root {
            Root::Value(v) => Some(v),
            _ => None,
        }
    }

    #[must_use]
    pub fn steps(self, b: &Body) -> Steps<'_> {
        Steps(&b.extra[self.from as usize..self.to as usize])
    }

    /// The first projection.
    #[must_use]
    pub fn first(self, b: &Body) -> Option<Proj> {
        self.steps(b).next()
    }

    /// Whether every step is a field or a variant, which name a part of
    /// the value at the root itself.
    #[must_use]
    pub fn in_place(self, b: &Body) -> bool {
        self.steps(b)
            .all(|p| matches!(p, Proj::Field(_) | Proj::Variant(_)))
    }

    /// How many dereferences the steps hold, and whether an element is
    /// among them.
    #[must_use]
    pub fn kinds(self, b: &Body) -> (u32, bool) {
        self.steps(b).fold((0, false), |(d, e), p| match p {
            Proj::Deref => (d + 1, e),
            Proj::Elem(_) => (d, true),
            _ => (d, e),
        })
    }

    /// The first-level part the place names: the field of its first step
    /// (after a variant), or `None` for all of the root or a step that is
    /// no part of it.
    #[must_use]
    pub fn first_part(self, b: &Body) -> Option<u32> {
        let mut s = self.steps(b);
        match s.next()? {
            Proj::Field(k) => Some(k),
            Proj::Variant(_) => match s.next()? {
                Proj::Field(k) => Some(k),
                _ => None,
            },
            _ => None,
        }
    }

    /// Appends the step codes ([`ELEM`], [`DEREF`], [`VARIANT`]) to `out`;
    /// how many dereferences there are, and whether an element is among
    /// them.
    pub fn push_steps(self, b: &Body, out: &mut Vec<u32>) -> (u32, bool) {
        let (mut derefs, mut elem) = (0, false);
        for p in self.steps(b) {
            out.push(match p {
                Proj::Field(k) => k,
                Proj::Variant(k) => VARIANT | k,
                Proj::Deref => {
                    derefs += 1;
                    DEREF
                }
                Proj::Elem(_) => {
                    elem = true;
                    ELEM
                }
            });
        }
        (derefs, elem)
    }
}
