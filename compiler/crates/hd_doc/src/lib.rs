#![forbid(unsafe_code)]
//! `hd_doc`: `hd doc` rendering from folder interfaces (design-overview.md
//! §2.1, commands.md §7.7, `future-work/HD_DOC.md`). Not implemented yet:
//! the entry point reports the stage.

use hd_base::{NotImplemented, Stage, StageResult};
use hd_iface::FolderIface;

/// What `hd doc` asks for: a folder, an item, or a search.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DocQuery {
    Folder(String),
    Item(String),
    Search(String),
}

/// Renders an answer from interfaces only, never from bodies.
pub fn render(_ifaces: &[&FolderIface], q: &DocQuery) -> StageResult<String> {
    Err(NotImplemented::new(Stage::PackageResult, format!("hd doc rendering for {q:?}")))
}

#[cfg(test)]
mod tests {
    use super::{DocQuery, render};

    #[test]
    fn reports_not_implemented() {
        assert!(render(&[], &DocQuery::Folder("std".into())).is_err());
    }
}
