//! The structured "not implemented" result of a skeleton stage. A stage
//! that the architecture names but the code does not carry yet returns
//! `Err(NotImplemented)`, never panics, so the driver can count how far a
//! package gets (architecture skeleton, 2026-10-07).

use core::fmt;

use crate::Span;

/// Every stage of the pipeline, in run order (`docs/compiler/design-overview.md` §1.2,
/// `crates/hd_mono/DESIGN.md` §11.2, `crates/hd_sched/DESIGN.md` §6.1).
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
    pub const ALL: [Stage; 17] = [
        Stage::Discover,
        Stage::Skim,
        Stage::Parse,
        Stage::FolderGraph,
        Stage::FolderIface,
        Stage::HeaderCheck,
        Stage::ModulePrep,
        Stage::Body,
        Stage::ModuleFinish,
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
///
/// A stage also stops this way when a check of the compiler's own work
/// fails, such as the TIR verifier ([`NotImplemented::bug`]): `bug` is
/// set, and the driver reports a compiler bug rather than a construct
/// this build lacks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotImplemented {
    pub stage: Stage,
    pub what: String,
    /// Where the construct is, when the stage knows it.
    pub span: Option<Span>,
    /// An internal check failed: the input is not unsupported, the
    /// compiler has a bug.
    pub bug: bool,
}

impl NotImplemented {
    #[must_use]
    pub fn new(stage: Stage, what: impl Into<String>) -> Self {
        Self {
            stage,
            what: what.into(),
            span: None,
            bug: false,
        }
    }

    /// A failed internal check of `stage`: `check` names the check and
    /// what it found, such as `TIR verifier: ...`.
    #[must_use]
    pub fn bug(stage: Stage, check: impl Into<String>) -> Self {
        Self {
            bug: true,
            ..Self::new(stage, check)
        }
    }

    /// Sets the span if none is set yet, so the innermost node that knows
    /// its position wins as the error unwinds.
    #[must_use]
    pub fn at(mut self, span: Span) -> Self {
        self.span.get_or_insert(span);
        self
    }
}

impl fmt::Display for NotImplemented {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.bug {
            write!(f, "stage {} hit a bug: {}", self.stage.name(), self.what)
        } else {
            write!(
                f,
                "stage {} not implemented: {}",
                self.stage.name(),
                self.what
            )
        }
    }
}

impl std::error::Error for NotImplemented {}

pub type StageResult<T> = Result<T, NotImplemented>;
