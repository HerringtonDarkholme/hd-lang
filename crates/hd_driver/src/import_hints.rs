//! Import advice from the embedded std, independent of a program's checked
//! interfaces. Like rust-analyzer's `ImportMap`, index names and re-export
//! identities without checking bodies. Unlike its workspace query, std is
//! immutable for this compiler binary, so one process-wide index suffices.

use std::sync::{Arc, OnceLock};

use hd_base::{FileId, HashSet, ModuleId};
use hd_intern::{PathTable, ShardedInterner};
use hd_project::ModuleTable;
use hd_resolve::iface::ImportIndex;
use hd_resolve::{Cx, KnownItems, ModIn, Names, Src, UnitIface, World};
use hd_types::InternPool;

pub(crate) fn index() -> &'static ImportIndex {
    static INDEX: OnceLock<ImportIndex> = OnceLock::new();
    INDEX.get_or_init(build)
}

/// A names-only view: all embedded modules participate in resolution, not
/// in the program's source-cycle graph or checking closure.
struct Modules(HashSet<String>);

impl World for Modules {
    fn import_hints(&self) -> hd_resolve::iface::ImportHints<'_> {
        hd_resolve::iface::ImportHints::Index(&hd_resolve::iface::EMPTY_IMPORT_INDEX)
    }

    fn module_unit(&self, module: &str) -> Option<u32> {
        self.0.contains(module).then_some(0)
    }
    fn usable(&self, module: &str) -> bool {
        self.0.contains(module)
    }
    fn test_only(&self, _module: &str) -> bool {
        false
    }
    fn published(&self, module: &str) -> Option<bool> {
        self.0.contains(module).then_some(false)
    }
    fn iface(&self, _unit: u32) -> Option<Arc<UnitIface>> {
        None
    }
}

fn build() -> ImportIndex {
    // Cost: parse embedded bytes and index declarations/uses once, only on
    // the first missing-name diagnostic. Memoized export chains cost O(E)
    // for E links; later hints probe one index plus their matching names.
    let std = crate::embedded_std();
    let table = ModuleTable::discover("std", &std.sources);
    let texts: Vec<_> = table
        .modules
        .iter()
        .map(|m| {
            std.texts
                .get(&table.files[m.file.idx()])
                .map_or("", AsRef::as_ref)
        })
        .collect();
    let parsed: Vec<_> = texts
        .iter()
        .map(|text| hd_syntax::parse(text.as_bytes()))
        .collect();
    let pool = InternPool::new();
    let paths = PathTable::new();
    let syms = ShardedInterner::default();
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let world = Modules(table.modules.iter().map(|m| m.path.clone()).collect());
    let mods: Vec<_> = table
        .modules
        .iter()
        .enumerate()
        .map(|(i, m)| ModIn {
            path: m.path.clone(),
            roots: table.use_roots(ModuleId::from_raw(crate::u32_of(i))),
            src: Src {
                parse: &parsed[i],
                text: texts[i],
                file: FileId::from_raw(m.file.raw()),
            },
            seeds: hd_resolve::seed::items(&names, &m.path),
            tests: false,
            session: None,
        })
        .collect();
    let exports = hd_resolve::lower::public_exports(
        &Cx {
            names,
            unit: 0,
            world: &world,
        },
        &mods,
    );
    ImportIndex::new(&names, &exports)
}

#[cfg(test)]
mod tests {
    #[test]
    fn embedded_advice_keeps_reexports_module_leaves_and_runtime_privacy() {
        let index = super::build();
        for (name, path) in [
            ("json", "std.encoding.json"),
            ("min", "std.cmp.min"),
            ("StringBuilder", "std.text.StringBuilder"),
            ("Eq", "std.cmp.Eq"),
        ] {
            assert_eq!(index.hint(name).path.as_deref(), Some(path));
        }
        let ambiguous = index.hint("max");
        assert!(ambiguous.path.is_none());
        assert!(ambiguous.text.contains("`std.cmp`, `std.validate`"));
        for hidden in ["TextBuilderValue", "TextBuffer", "missing_name"] {
            assert!(index.hint(hidden).text.is_empty(), "{hidden}");
        }
    }
}
