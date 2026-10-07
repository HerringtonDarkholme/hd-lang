#![forbid(unsafe_code)]
//! `hd_resolve`: use resolution, the folder interface builder, derived
//! heads, orphan and visibility checks, and interface validation
//! (resolution-and-interfaces.md §4.9, §4.10, §4.12; data-structures.md
//! §3.15): module scopes over the full parser's tree, header lowering to
//! `hd_types` types, folder interfaces and their blobs, impl tables for
//! the solver, and the stage-B header check.

use std::collections::HashMap;

use hd_base::{DefId, ModuleId, NotImplemented, Stage, StageResult, Symbol};

pub mod iface;
pub mod view;

pub use iface::{
    Field, FnSig, FolderIface, Generic, Head, HeadKind, Item, ItemData, Kinds, Lookup, Names,
    UseDecl, body_nodes, decode_items, deep_hash, encode_items, folder_iface, heads, impl_table,
    interface_items, lower_items, module_scope, use_decls,
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
}

impl ModuleScope {
    /// Binds a name; a second binding from a different origin is ambiguous.
    pub fn bind(&mut self, name: Symbol, b: Binding, origin: Origin, use_row: u32) {
        if let Some(&row) = self.index.get(&name) {
            let r = row as usize;
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
    #[must_use]
    pub fn lookup(&self, name: Symbol) -> Option<Binding> {
        self.index.get(&name).map(|&r| self.binding[r as usize])
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

/// Header validation stage B (§4.10.1; scheduler.md §6.1 `HeaderCheck(F)`):
/// bounds of written header types, impl supertraits, newtype bases and
/// delegation targets. Items without type arguments, supertraits,
/// newtypes or delegation have nothing to check; the rest is not
/// implemented yet.
pub fn header_check(pool: &hd_types::InternPool, items: &[Item]) -> StageResult<()> {
    let has_args = |t: hd_types::Ty| matches!(pool.get(t), hd_types::TyData::Adt { args, .. } if args != hd_types::TyList::EMPTY);
    for it in items {
        let tys: Vec<hd_types::Ty> = match &it.data {
            ItemData::Fn(s) | ItemData::Method { sig: s, .. } => {
                s.params.iter().map(|p| p.1).chain([s.ret]).collect()
            }
            ItemData::Data(fs) => fs.iter().map(|f| f.ty).collect(),
            ItemData::Impl { self_ty, .. } => vec![*self_ty],
            ItemData::Trait(_) => vec![],
        };
        if tys.into_iter().any(has_args) {
            return Err(NotImplemented::new(
                Stage::HeaderCheck,
                "bounds of header types with arguments",
            ));
        }
    }
    Ok(())
}

/// Derived impl heads (§4.10): `derive` lines become impl heads in the interface.
pub fn derive_heads(folder: &str) -> StageResult<Vec<String>> {
    Err(NotImplemented::new(
        Stage::FolderIface,
        format!("derived heads of folder {folder}"),
    ))
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
