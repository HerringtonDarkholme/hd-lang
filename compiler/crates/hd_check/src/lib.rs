//! `hd_check`: name resolution, header lowering and body checking that emits
//! TIR (checking-and-tir.md §4.13). It reads syntax and interfaces and writes
//! TIR; nothing downstream reads syntax.

pub mod body;
pub mod resolve;
pub mod stages;

use hd_iface::{CItem, HeaderItem, item_path};
use hd_syntax::SyntaxKind;
use hd_tir::world::World;

pub use body::{Checked, check_fn};
pub use resolve::{Cst, Scope, lower_headers, module_scope, use_decls};

/// Which A1 representation rule the checker writes into each body's summary
/// (codegen.md §13.2). It changes TIR, so it is part of the toolchain key.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum A1Rule {
    /// A bounded type parameter is always exact (walking skeleton, SK-2).
    #[default]
    Bounded,
    /// Exact only on a trait call in the body itself; kept to reproduce SK-2.
    Literal,
}

/// Checks every body of a module, in source order: functions, then the
/// methods of each impl as they appear. `headers` are the module's lowered
/// headers, already loaded into `w`.
pub fn check_module_bodies(
    w: &mut World,
    cst: &Cst<'_>,
    module: &str,
    scope: &Scope,
    headers: &[HeaderItem],
    rule: A1Rule,
) -> Vec<(String, Checked)> {
    let mut impl_paths = headers
        .iter()
        .filter(|h| matches!(h.item, CItem::Impl { .. }))
        .map(|h| h.path.clone());
    let mut out = Vec::new();
    for item in cst.root().children() {
        match item.kind() {
            SyntaxKind::FnDecl => {
                let path = item_path(module, cst.first_ident(item));
                let def = w.def(&path);
                out.push((path, check_fn(w, cst, scope, def, item, rule)));
            }
            SyntaxKind::ImplDecl => {
                let Some(impl_path) = impl_paths.next() else {
                    continue;
                };
                for f in item.children().filter(|c| c.kind() == SyntaxKind::FnDecl) {
                    let path = format!("{impl_path}.{}", cst.first_ident(f));
                    let def = w.def(&path);
                    out.push((path, check_fn(w, cst, scope, def, f, rule)));
                }
            }
            _ => {}
        }
    }
    out
}
