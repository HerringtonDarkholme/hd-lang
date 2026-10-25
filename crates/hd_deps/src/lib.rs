#![forbid(unsafe_code)]
//! `hd_deps`: a package's remote dependencies (`spec/cli/command-line.md`,
//! Dependencies; `spec/lang/10-modules.md`, Version Selection and
//! Integrity). Minimal version selection over the manifests a package
//! reaches, the version cache, fetching with the system's `git`, tree and
//! manifest hashes, and `hd.sum`. Requirement and version parsing are
//! `hd_project`'s, which checks a manifest on its own.

pub mod cache;
mod change;
mod fetch;
pub mod git;
pub mod query;
mod select;
pub mod sum;
pub mod target;
pub mod version;

pub use cache::{Cache, Entry};
pub use change::{lowered, newest_release};
pub use fetch::{Failure, Remote, Resolved};
pub use select::{
    Edited, Mode, Package, Place, Problem, Selection, Source, select, select_edited,
    select_executable_entry,
};
pub use sum::{Sum, SumError};
