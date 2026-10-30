#![forbid(unsafe_code)]
//! `hd_resolve`: use resolution, the folder interface builder, derived
//! heads, orphan and visibility checks, and interface validation
//! (`crates/hd_resolve/DESIGN.md` §4.9, §4.10, §4.12; `docs/compiler/data-structures.md`
//! §3.15): module scopes over the full parser's tree, header lowering to
//! `hd_types` types, folder interfaces and their blobs, impl tables for
//! the solver, and the overlap check.

use std::sync::{Arc, Mutex};

use hd_base::{HashMap, HashSet};

use hd_base::{DefId, ModuleId, Symbol};

pub mod anchor;
pub mod assoc;
pub mod const_eval;
pub mod error_type;
pub mod facts;
pub mod header;
pub mod iface;
pub mod known;
pub mod literal;
pub mod lower;
pub mod seed;
pub mod variance;
pub mod view;

pub use header::Universe;
pub use iface::{
    Export, Field, FnSig, FolderIface, Generic, HeadKind, ImplKind, Item, ItemData, Items, Lookup,
    LookupDecls, Names, TraitData, Variant, declares_assoc_function, declares_ref_receiver,
    decode_items, decode_lazy, decode_module_uses, decode_private_names, deep_hash, encode_docs,
    encode_items, encode_module_uses, encode_private_names, fill_trait_args, fill_type_args,
    folder_iface, impl_table, interface_items, lifetime_arg, mentioned_defs, private_impl_items,
    show_ty, show_ty_in, show_ty_rows_in,
};
pub use known::KnownItems;
pub use lower::{
    Cx, FolderOut, Head, Kinds, LocalItem, LocalSite, ModIn, ModOut, PRELUDE, Reads, Recorded,
    SessionScope, USE_CODES, UseDecl, World, body_nodes, build_folder, head_name_span, heads,
    is_prelude_name, is_test_prelude_name, local_at, local_items, prelude_modules,
    test_prelude_item, test_use_decls, tests_items, traitless_block, use_decls, use_diags,
};
pub use view::Src;

/// What a module-level name is bound to (§3.15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BindingKind {
    Item,
    Module,
    Prelude,
    Ambiguous,
    Poison,
}

/// 8 bytes: a kind and a value (a `DefId`, `ModuleId` or candidate range).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Binding {
    pub kind: BindingKind,
    pub value: u32,
}
const _: () = assert!(core::mem::size_of::<Binding>() == 8);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Origin {
    Own,
    Use,
    PubUse,
    Prelude,
}

/// A module's frozen scope after `ModulePrep` (§3.15).
#[derive(Clone, Debug, Default)]
pub struct ModuleScope {
    pub module: Option<ModuleId>,
    pub name: Vec<Symbol>,
    pub binding: Vec<Binding>,
    pub origin: Vec<Origin>,
    pub use_row: Vec<u32>,
    pub index: HashMap<Symbol, u32>,
    pub def: Vec<DefId>,
    /// Module paths that `Module` bindings name, by binding value.
    pub modules: Vec<String>,
    /// The names looked up so far, while a folder's interface is built
    /// ([`ModuleScope::track`]): only a use whose name a lookup consulted
    /// counts among the reads that key the interface (#248).
    pub looked_up: Option<Arc<Mutex<HashSet<Symbol>>>>,
}

impl ModuleScope {
    /// Binds a name; a second binding from a different origin is ambiguous.
    pub fn bind(&mut self, name: Symbol, b: Binding, origin: Origin, use_row: u32) {
        if let Some(&row) = self.index.get(&name) {
            let r = row as usize;
            if b.kind == BindingKind::Poison {
                return;
            }
            if self.binding[r].kind == BindingKind::Poison {
                self.binding[r] = b;
                self.origin[r] = origin;
                self.use_row[r] = use_row;
                return;
            }
            if self.binding[r] != b && self.origin[r] != Origin::Own {
                self.binding[r] = Binding {
                    kind: BindingKind::Ambiguous,
                    value: row,
                };
            }
            return;
        }
        let row = u32::try_from(self.name.len()).expect("scope rows");
        self.name.push(name);
        self.binding.push(b);
        self.origin.push(origin);
        self.use_row.push(use_row);
        self.index.insert(name, row);
    }
    /// Whether `name` is bound here by the prelude or a `use`, not by a
    /// declaration of the module itself.
    #[must_use]
    pub fn bound_from_outside(&self, name: Symbol) -> bool {
        self.index
            .get(&name)
            .is_some_and(|&r| self.origin[r as usize] != Origin::Own)
    }

    #[must_use]
    pub fn lookup(&self, name: Symbol) -> Option<Binding> {
        if let Some(seen) = &self.looked_up {
            seen.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .insert(name);
        }
        self.index.get(&name).map(|&r| self.binding[r as usize])
    }

    /// Starts noting each name [`ModuleScope::lookup`] is asked for, in
    /// this scope and every copy of it.
    pub fn track(&mut self) {
        self.looked_up = Some(Arc::default());
    }

    /// The names looked up since [`ModuleScope::track`].
    #[must_use]
    pub fn looked_up(&self) -> HashSet<Symbol> {
        self.looked_up
            .as_ref()
            .map_or_else(HashSet::default, |seen| {
                seen.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .clone()
            })
    }

    /// This scope with `inner` nested in it: each name `inner` binds
    /// replaces this scope's binding of that name, as a nested scope's
    /// names shadow the enclosing ones (`names.tests.shadow`).
    #[must_use]
    pub fn overlay(&self, inner: &ModuleScope) -> ModuleScope {
        let mut out = self.clone();
        out.nest(inner);
        out
    }

    /// Nests `inner` in this scope in place, as [`ModuleScope::overlay`]
    /// does, at the cost of `inner`'s names only.
    pub fn nest(&mut self, inner: &ModuleScope) {
        let base = u32::try_from(self.modules.len()).expect("scope modules");
        self.modules.extend(inner.modules.iter().cloned());
        for (r, &name) in inner.name.iter().enumerate() {
            if !self.index.contains_key(&name) {
                let row = u32::try_from(self.name.len()).expect("scope rows");
                self.name.push(name);
                self.binding.push(inner.binding[r]);
                self.origin.push(inner.origin[r]);
                self.use_row.push(inner.use_row[r]);
                self.index.insert(name, row);
            }
            let row = self.index[&name];
            let mut b = inner.binding[r];
            match b.kind {
                BindingKind::Module => b.value += base,
                BindingKind::Ambiguous => b.value = row,
                _ => {}
            }
            let at = row as usize;
            self.binding[at] = b;
            self.origin[at] = inner.origin[r];
            self.use_row[at] = inner.use_row[r];
        }
    }
}

/// What a name of a module's scope stands for, spelled for readers outside
/// the run's tables (`hd doc` links).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Named {
    /// A declared item, by its declaring module and its name.
    Item { module: String, name: String },
    /// A module, by its path.
    Module(String),
}

impl ModuleScope {
    /// The names this scope binds to one item or module, as text. Names
    /// that are ambiguous or poisoned bind nothing readable and are left out.
    #[must_use]
    pub fn named(&self, names: &iface::Names<'_>) -> Vec<(String, Named)> {
        self.name
            .iter()
            .zip(&self.binding)
            .filter_map(|(&n, b)| {
                let target = match b.kind {
                    BindingKind::Item | BindingKind::Prelude => {
                        let d = DefId::from_raw(b.value);
                        Named::Item {
                            module: names.module_of(d),
                            name: names.display_name(d).to_owned(),
                        }
                    }
                    BindingKind::Module => {
                        Named::Module(self.modules.get(b.value as usize)?.clone())
                    }
                    BindingKind::Ambiguous | BindingKind::Poison => return None,
                };
                Some((names.text(n).to_owned(), target))
            })
            .collect()
    }
}

/// The use worklist's state per export (§3.15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ExportState {
    Resolved,
    Pending,
    Visiting,
    Loop,
}

/// A folder's exports, chains followed (§3.15), frozen into the blob.
#[derive(Clone, Debug, Default)]
pub struct FolderExports {
    pub module: Vec<ModuleId>,
    pub name: Vec<Symbol>,
    pub target: Vec<DefId>,
    pub state: Vec<ExportState>,
}

/// The orphan rule (§4.12): an impl must live in the module of its trait
/// or of its self type's head.
#[must_use]
pub fn orphan_ok(
    impl_module: ModuleId,
    trait_module: Option<ModuleId>,
    self_head_module: Option<ModuleId>,
) -> bool {
    trait_module == Some(impl_module) || self_head_module == Some(impl_module)
}

#[cfg(test)]
mod tests {
    use super::{Binding, BindingKind, ModuleScope, Origin, orphan_ok};
    use hd_base::{ModuleId, Symbol};

    #[test]
    fn a_name_from_two_uses_is_ambiguous_but_own_wins() {
        let mut s = ModuleScope::default();
        let x = Symbol::from_raw(1);
        s.bind(
            x,
            Binding {
                kind: BindingKind::Item,
                value: 1,
            },
            Origin::Use,
            0,
        );
        s.bind(
            x,
            Binding {
                kind: BindingKind::Item,
                value: 2,
            },
            Origin::Use,
            1,
        );
        assert_eq!(s.lookup(x).expect("bound").kind, BindingKind::Ambiguous);
        let y = Symbol::from_raw(2);
        s.bind(
            y,
            Binding {
                kind: BindingKind::Item,
                value: 1,
            },
            Origin::Own,
            u32::MAX,
        );
        s.bind(
            y,
            Binding {
                kind: BindingKind::Item,
                value: 2,
            },
            Origin::Use,
            0,
        );
        assert_eq!(s.lookup(y).expect("bound").value, 1);
    }

    #[test]
    fn orphan_rule() {
        let m = ModuleId::from_raw;
        assert!(orphan_ok(m(1), Some(m(1)), Some(m(2))));
        assert!(orphan_ok(m(2), Some(m(1)), Some(m(2))));
        assert!(!orphan_ok(m(3), Some(m(1)), Some(m(2))));
    }
}
