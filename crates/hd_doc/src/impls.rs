//! Implementation heads per type (`cli.doc.impls`), from the package's
//! folder interfaces: every trait implementation of the package, in
//! whichever module, with derived ones marked. The blobs decode into
//! fresh run tables, so this needs nothing of the run but the bytes.

use hd_base::HashMap;
use std::sync::Arc;

use hd_base::DefId;
use hd_intern::{PathTable, ShardedInterner};
use hd_resolve::{ImplKind, ItemData, KnownItems, Names, decode_items, show_ty};
use hd_types::{InternPool, TyData};

/// The implementation lines of each type, by (module path, type name).
pub(crate) type ImplLines = HashMap<(String, String), Vec<String>>;

/// The implementation lines of the folders of the package whose module
/// root is `root`.
pub(crate) fn impl_lines(root: &str, ifaces: &[(String, Arc<[u8]>)]) -> ImplLines {
    let (pool, paths, syms) = (
        InternPool::new(),
        PathTable::new(),
        ShardedInterner::default(),
    );
    let known = KnownItems::new(&paths);
    let names = Names {
        pool: &pool,
        paths: &paths,
        syms: &syms,
        known: &known,
    };
    let below = format!("{root}.");
    let mut out = ImplLines::default();
    for (folder, blob) in ifaces {
        if folder != root && !folder.starts_with(&below) {
            continue;
        }
        let Some((items, _)) = decode_items(&names, blob) else {
            continue;
        };
        for it in items {
            let ItemData::Impl {
                trait_,
                trait_args,
                self_ty,
                kind,
                ..
            } = it.data
            else {
                continue;
            };
            if trait_ == DefId::NONE || matches!(kind, ImplKind::Template | ImplKind::TupleTemplate)
            {
                continue;
            }
            let TyData::Adt { def, .. } = pool.get(self_ty) else {
                continue;
            };
            let args: Vec<String> = pool
                .list_items(trait_args)
                .iter()
                .map(|a| show_ty(&names, *a))
                .collect();
            let args = if args.is_empty() {
                String::new()
            } else {
                format!("[{}]", args.join(", "))
            };
            let head = format!(
                "impl {}{args} for {}",
                names.display_name(trait_),
                show_ty(&names, self_ty)
            );
            let line = if matches!(kind, ImplKind::Derived | ImplKind::Derivation) {
                format!("`{head}` (derived)")
            } else {
                format!("`{head}`")
            };
            let lines = out
                .entry((names.module_of(def), names.display_name(def).to_owned()))
                .or_default();
            if !lines.contains(&line) {
                lines.push(line);
            }
        }
    }
    out
}
