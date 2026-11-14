//! The generic parameters of one kind in scope while a header is lowered,
//! indexed by name and by type as they are declared (#1857).
//!
//! Lowering pushes a declaration's parameters, looks names up at each use,
//! and truncates back to the enclosing scope's length when the
//! declaration's signature is done. Each of the three is O(1) here: an
//! entry records the previous entry of the same name, so truncation
//! restores the innermost one without a scan. A linear scan per use cost
//! O(parameters in scope), and past `hd_base::limits::GENERIC_PARAMS` the
//! rejected names still bind, so n uses of n names cost n² / 2.

use std::ops::Deref;

use hd_base::{HashMap, Symbol};
use hd_types::Ty;

/// What an entry is found by: its name, and for a type parameter its type.
pub(crate) trait Entry {
    fn name(&self) -> Symbol;
    /// The type a type parameter stands for, by which a projection or a
    /// `Fn` input finds its bounds; `None` for other kinds.
    fn ty(&self) -> Option<Ty> {
        None
    }
}

/// No entry: the end of a name's chain of shadowed entries.
const NONE: usize = usize::MAX;

/// Entries in declaration order, outermost first.
#[derive(Clone)]
pub(crate) struct Scoped<T> {
    items: Vec<T>,
    /// Each name's first (outermost) and last (innermost) entry.
    by_name: HashMap<Symbol, (usize, usize)>,
    /// Per entry, the previous entry of the same name, or [`NONE`].
    shadows: Vec<usize>,
    /// Each type's first entry.
    by_ty: HashMap<Ty, usize>,
}

impl<T> Default for Scoped<T> {
    fn default() -> Self {
        Scoped {
            items: Vec::new(),
            by_name: HashMap::default(),
            shadows: Vec::new(),
            by_ty: HashMap::default(),
        }
    }
}

impl<T> Deref for Scoped<T> {
    type Target = [T];
    fn deref(&self) -> &[T] {
        &self.items
    }
}

impl<T: Entry> Scoped<T> {
    /// Appends `item`, the innermost entry of its name; returns its index.
    pub(crate) fn push(&mut self, item: T) -> usize {
        let at = self.items.len();
        let name = item.name();
        let entry = self.by_name.entry(name).or_insert((at, NONE));
        let shadowed = std::mem::replace(&mut entry.1, at);
        if let Some(t) = item.ty() {
            self.by_ty.entry(t).or_insert(at);
        }
        self.shadows.push(shadowed);
        self.items.push(item);
        at
    }

    /// Drops the entries from `len` on, innermost first, so each name's
    /// innermost entry is the one it shadowed.
    pub(crate) fn truncate(&mut self, len: usize) {
        for at in (len..self.items.len()).rev() {
            let (name, shadowed) = (self.items[at].name(), self.shadows[at]);
            if shadowed == NONE {
                self.by_name.remove(&name);
            } else {
                self.by_name.entry(name).and_modify(|e| e.1 = shadowed);
            }
            if let Some(t) = self.items[at].ty()
                && self.by_ty.get(&t) == Some(&at)
            {
                self.by_ty.remove(&t);
            }
        }
        self.items.truncate(len);
        self.shadows.truncate(len);
    }

    /// The outermost entry named `name`.
    pub(crate) fn first(&self, name: Symbol) -> Option<&T> {
        self.first_index(name).map(|i| &self.items[i])
    }

    /// The index of the outermost entry named `name`.
    pub(crate) fn first_index(&self, name: Symbol) -> Option<usize> {
        crate::work::scope_probe();
        self.by_name.get(&name).map(|&(first, _)| first)
    }

    /// The innermost entry named `name`, which every use of it means.
    pub(crate) fn last(&self, name: Symbol) -> Option<&T> {
        self.last_index(name).map(|i| &self.items[i])
    }

    /// The index of the innermost entry named `name`.
    pub(crate) fn last_index(&self, name: Symbol) -> Option<usize> {
        crate::work::scope_probe();
        self.by_name.get(&name).map(|&(_, last)| last)
    }

    pub(crate) fn contains(&self, name: Symbol) -> bool {
        crate::work::scope_probe();
        self.by_name.contains_key(&name)
    }

    /// The first entry for the type `t`.
    pub(crate) fn of_ty(&self, t: Ty) -> Option<&T> {
        crate::work::scope_probe();
        self.by_ty.get(&t).map(|&i| &self.items[i])
    }

    /// The entry at `i`, whose name and type the caller leaves as they are.
    pub(crate) fn get_mut(&mut self, i: usize) -> Option<&mut T> {
        self.items.get_mut(i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    struct E(Symbol, Ty);

    impl Entry for E {
        fn name(&self) -> Symbol {
            self.0
        }
        fn ty(&self) -> Option<Ty> {
            Some(self.1)
        }
    }

    fn sym(i: u32) -> Symbol {
        Symbol::from_raw(i)
    }

    #[test]
    fn truncation_restores_the_shadowed_entry() {
        let mut s = Scoped::default();
        let (t, u) = (Ty::VOID, Ty::POISON);
        assert_eq!(s.push(E(sym(1), t)), 0);
        s.push(E(sym(2), t));
        let inner = s.len();
        s.push(E(sym(1), u));
        assert_eq!(s.last(sym(1)), Some(&E(sym(1), u)));
        assert_eq!(s.first(sym(1)), Some(&E(sym(1), t)));
        assert_eq!(s.of_ty(u), Some(&E(sym(1), u)));
        s.truncate(inner);
        assert_eq!(s.last(sym(1)), Some(&E(sym(1), t)));
        assert_eq!(s.of_ty(u), None);
        assert_eq!(s.of_ty(t), Some(&E(sym(1), t)));
        s.truncate(1);
        assert!(!s.contains(sym(2)));
        assert_eq!(s.first_index(sym(1)), Some(0));
        s.truncate(0);
        assert!(s.is_empty() && !s.contains(sym(1)) && s.of_ty(t).is_none());
    }
}
