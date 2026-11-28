//! What `tests/unit` reaches inside the driver: the feature `testing`,
//! which only the crate's own dev-dependency turns on, so a normal build
//! neither compiles nor exports it. The unit tests live outside `src` so
//! that a test build compiles this library once, not again in test mode
//! (`[lib] test = false`).
//!
//! It holds wrappers of crate-root items the tests call, re-exports from
//! the private modules whose algorithms they test whole, and the switches
//! of the checks `own::test_hooks` runs inside a build.
//!
//! Cost: nothing in a normal build; with the feature, one thread-local
//! read per hook a build passes (`own::test_hooks`).

pub use crate::own::place_types::substitute;
#[cfg(not(target_arch = "wasm32"))]
pub use crate::own::test_hooks::metadata::{arm_metadata_checks, metadata_checks};
pub use crate::own::test_hooks::{stored_bodies_verified, verify_stored_bodies};
pub use crate::own::{RefFacts, TypeFacts, TypeTable, read_only_factories, ref_facts};
pub use crate::package_result::{read_diags, write_diags};
pub use crate::skims::FileSkim;

/// The line, frame-name and module tables behind `Positions`.
pub mod positions {
    pub use crate::positions::{
        Body, Module, ModuleInput, ModuleSource, Positions, path_of, quoted, text_line_starts,
        write_frame_names,
    };

    /// The frame names `write_frame_names` wrote, by body path, each with
    /// whether its frames are left out.
    #[must_use]
    pub fn read_frame_names(bytes: &[u8]) -> Option<hd_base::HashMap<String, (String, bool)>> {
        Some(
            crate::positions::read_frames(bytes)?
                .0
                .into_iter()
                .map(|(path, name, hidden)| (path, (name, hidden)))
                .collect(),
        )
    }

    /// The LF line starts of a parse's lines: the lexer records bare CR
    /// lines too, and positions count LF.
    #[must_use]
    pub fn lf_line_starts(parse: &hd_syntax::Parse, text: &str) -> Vec<u32> {
        parse
            .tokens
            .line_start
            .iter()
            .copied()
            .filter(|&start| start == 0 || text.as_bytes().get(start as usize - 1) == Some(&b'\n'))
            .collect()
    }

    /// The position inputs an output keeps, one per file.
    #[must_use]
    pub fn position_inputs(out: &crate::Output) -> &[std::sync::Arc<ModuleInput>] {
        &out.position_inputs
    }

    /// [`position_inputs`], to replace.
    pub fn position_inputs_mut(out: &mut crate::Output) -> &mut Vec<std::sync::Arc<ModuleInput>> {
        &mut out.position_inputs
    }

    /// Positions over `modules` (module name, path, source) and `tests`.
    #[must_use]
    pub fn positions_of(
        modules: Vec<(String, String, std::sync::Arc<ModuleSource>)>,
        tests: hd_base::HashMap<String, String>,
    ) -> Positions {
        Positions { modules, tests }
    }

    /// The modules `positions` holds: module name, path and source.
    #[must_use]
    pub fn position_modules(
        positions: &Positions,
    ) -> &[(String, String, std::sync::Arc<ModuleSource>)] {
        &positions.modules
    }
}

/// The optimizer pipeline's cache keys and table checks.
pub mod pipeline {
    pub use crate::pipeline::{
        Row, check_distinct, content_key, key, validate_table, validate_table_of,
    };
}

/// Reports the stage stop `e` into `diags`.
pub fn report_stop(
    diags: &mut hd_diag::DiagBuf,
    stage: hd_base::Stage,
    e: &hd_base::NotImplemented,
) {
    crate::diagnostics::report_stop(diags, stage, e);
}

/// The cache layout version an entry's header carries.
pub const LAYOUT: u16 = crate::LAYOUT;

/// A validated cache payload over its entry's allocation.
pub struct CacheSection(crate::CacheSection);

impl CacheSection {
    /// The entry allocation the section's bytes stay in.
    #[must_use]
    pub fn backing(&self) -> &std::sync::Arc<[u8]> {
        &self.0.bytes
    }
}

impl AsRef<[u8]> for CacheSection {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

/// The sections of the cache entry `bytes` stored under `key` by
/// `toolchain`, or none when its header does not match.
#[must_use]
pub fn cache_sections(
    bytes: &std::sync::Arc<[u8]>,
    key: hd_base::Hash128,
    toolchain: hd_base::Hash128,
) -> Option<Vec<CacheSection>> {
    crate::cache_sections(bytes, key, toolchain).map(|s| s.into_iter().map(CacheSection).collect())
}

/// The declarations of a program, by `DefId`, across checks.
#[derive(Default)]
pub struct ProgramItems(crate::ProgramItems);

impl ProgramItems {
    pub fn add_checked(&mut self, items: hd_resolve::Items) {
        self.0.add_checked(items);
    }

    #[must_use]
    pub fn get(&self, def: hd_base::DefId) -> Option<&hd_resolve::Item> {
        self.0.get(def)
    }

    /// The `DefId` slots indexed so far.
    #[must_use]
    pub fn slots(&self) -> usize {
        self.0.slots.len()
    }
}

/// Whether a run verifies its TIR.
#[must_use]
pub const fn verifies_tir(debug: bool, checked: bool, requested: bool) -> bool {
    crate::verifies_tir(debug, checked, requested)
}

/// Checks the host declarations of `packages` against the modules of `table`.
pub fn check_host_declarations(
    table: &hd_project::ModuleTable,
    texts: &crate::Texts<'_>,
    host_marker: &dyn Fn(usize) -> bool,
    parsed: &[std::sync::OnceLock<hd_syntax::Parse>],
    packages: &[crate::HostPackage],
    declaration_sources: &[&dyn hd_project::SourceSet],
    diags: &mut hd_diag::DiagBuf,
) {
    crate::check_host_declarations(
        table,
        texts,
        host_marker,
        parsed,
        packages,
        declaration_sources,
        diags,
    );
}

/// Texts already read, one per module.
#[must_use]
pub fn texts_ready(texts: Vec<std::sync::Arc<str>>) -> crate::Texts<'static> {
    crate::Texts::ready(texts)
}

/// A secondary label anchored to its declaration: module, anchor, message.
pub type Label = (String, hd_resolve::anchor::Anchor, String);

/// `labels` written to an interface entry and read back, and whether the
/// reader ended clean at the end.
#[must_use]
pub fn labels_round_trip(labels: &[Label]) -> (Vec<Label>, bool) {
    let labels: Vec<_> = labels
        .iter()
        .map(|(module, anchor, message)| crate::IfaceLabel {
            module: module.clone(),
            anchor: *anchor,
            message: message.clone(),
        })
        .collect();
    let mut w = hd_base::wire::Writer::default();
    crate::write_labels(&mut w, &labels);
    let mut r = hd_base::wire::Reader::new(&w.bytes);
    let got = crate::read_labels(&mut r)
        .into_iter()
        .map(|l| (l.module, l.anchor, l.message))
        .collect();
    (got, r.ok() && r.at_end())
}

/// The overlap graph of `found` over no unit interfaces.
#[must_use]
pub fn encode_overlaps(found: &[(hd_base::DefId, hd_base::DefId, String)]) -> Option<Vec<u8>> {
    crate::encode_overlaps(found, &[])
}

/// One trait's coherence row: its key, stable path and findings.
pub type CohRow = (hd_base::Hash128, String, Vec<(String, String, String)>);

/// The coherence rows `traits`, with no unit rows, encoded.
#[must_use]
pub fn encode_coh_rows(traits: Vec<CohRow>) -> Vec<u8> {
    let rows = crate::CohRows {
        traits: traits
            .into_iter()
            .map(|(key, trait_, found)| crate::CohRow { key, trait_, found })
            .collect(),
        units: Vec::new(),
    };
    crate::encode_coh_rows(&rows)
}

/// The trait rows of encoded coherence rows.
#[must_use]
pub fn decode_coh_rows(bytes: &[u8]) -> Option<Vec<CohRow>> {
    let rows = crate::decode_coh_rows(bytes)?;
    Some(
        rows.traits
            .into_iter()
            .map(|r| (r.key, r.trait_, r.found))
            .collect(),
    )
}

/// The checked entry of module `m` in `out`, if it was checked.
#[must_use]
pub fn checked_entry(out: &crate::Output, m: usize) -> Option<&std::sync::Arc<[u8]>> {
    out.checks[m].as_ref()
}

/// The bodies of a checked program entry: each body's key and byte range.
#[must_use]
pub fn body_index(entry: &[u8]) -> Option<Vec<(hd_base::Hash128, std::ops::Range<usize>)>> {
    crate::body_index(entry)
}

/// The driver's immutable unit set, for internal cost and algebra checks.
pub use crate::contexts::UnitSet;

#[path = "../tests/support/context_graphs.rs"]
mod context_graphs;
pub use context_graphs::unit_contexts;
