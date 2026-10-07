//! The structured "not implemented" result of a skeleton stage. A stage
//! that the architecture names but the code does not carry yet returns
//! `Err(NotImplemented)`, never panics, so the driver can count how far a
//! package gets (architecture skeleton, 2026-10-07).

use core::fmt;

/// Every stage of the pipeline, in run order (design-overview.md §1.2,
/// codegen.md §11.2, scheduler.md §6.1).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Stage {
    Discover,
    Skim,
    Parse,
    FolderGraph,
    FolderIface,
    HeaderCheck,
    ModulePrep,
    Body,
    ModuleFinish,
    TestOverlay,
    Coherence,
    InitOrder,
    PackageResult,
    Collect,
    Emit,
    Link,
    Precompile,
    Run,
}

impl Stage {
    pub const ALL: [Stage; 18] = [
        Stage::Discover,
        Stage::Skim,
        Stage::Parse,
        Stage::FolderGraph,
        Stage::FolderIface,
        Stage::HeaderCheck,
        Stage::ModulePrep,
        Stage::Body,
        Stage::ModuleFinish,
        Stage::TestOverlay,
        Stage::Coherence,
        Stage::InitOrder,
        Stage::PackageResult,
        Stage::Collect,
        Stage::Emit,
        Stage::Link,
        Stage::Precompile,
        Stage::Run,
    ];

    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Stage::Discover => "Discover",
            Stage::Skim => "Skim",
            Stage::Parse => "Parse",
            Stage::FolderGraph => "FolderGraph",
            Stage::FolderIface => "FolderIface",
            Stage::HeaderCheck => "HeaderCheck",
            Stage::ModulePrep => "ModulePrep",
            Stage::Body => "Body",
            Stage::ModuleFinish => "ModuleFinish",
            Stage::TestOverlay => "TestOverlay",
            Stage::Coherence => "Coherence",
            Stage::InitOrder => "InitOrder",
            Stage::PackageResult => "PackageResult",
            Stage::Collect => "Collect",
            Stage::Emit => "Emit",
            Stage::Link => "Link",
            Stage::Precompile => "Precompile",
            Stage::Run => "Run",
        }
    }
}

/// A stage, or a component inside one, that is not implemented for this
/// input. `what` names the construct or component in plain words.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotImplemented {
    pub stage: Stage,
    pub what: String,
}

impl NotImplemented {
    #[must_use]
    pub fn new(stage: Stage, what: impl Into<String>) -> Self {
        Self {
            stage,
            what: what.into(),
        }
    }
}

impl fmt::Display for NotImplemented {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "stage {} not implemented: {}",
            self.stage.name(),
            self.what
        )
    }
}

impl std::error::Error for NotImplemented {}

pub type StageResult<T> = Result<T, NotImplemented>;
