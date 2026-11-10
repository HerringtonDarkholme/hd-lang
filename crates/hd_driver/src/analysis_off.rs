//! The analysis hook of a build without the `analysis` feature, such as the
//! web compiler: no consumer can be attached. Each method is the no-op twin
//! of one in `analysis.rs`, so the run calls them without a feature gate.

use std::convert::Infallible;
use std::marker::PhantomData;

use hd_base::{Hash128, NodeIdx};
use hd_check::body::BodyCx;
use hd_diag::DiagBuf;

use crate::{Lines, Run};

/// No consumer exists.
pub(crate) struct Sink<'a>(Infallible, PhantomData<&'a ()>);

/// No lint run exists.
pub(crate) struct Lint<'a>(Infallible, PhantomData<&'a ()>);

impl<'a> Lint<'a> {
    pub(crate) fn new(sink: Option<Sink<'a>>, _: usize) -> Option<Self> {
        sink.map(|sink| match sink.0 {})
    }
}

impl Run<'_> {
    /// Proves the run holds no lint state.
    fn no_lint(&self) {
        if let Some(lint) = &self.lint {
            match lint.0 {}
        }
    }

    pub(crate) fn collect_checked_facts(&self, _: usize) -> bool {
        self.no_lint();
        false
    }

    pub(crate) fn lint_runs(&self, _: usize) -> bool {
        self.no_lint();
        false
    }

    pub(crate) fn lint_cached(&self, _: usize, _: Hash128) -> Option<DiagBuf> {
        self.no_lint();
        None
    }

    pub(crate) fn lint_keep(&self, _: usize, _: DiagBuf) {
        self.no_lint();
    }

    pub(crate) fn lint_module(&self, _: usize, _: &BodyCx<'_>, _: &[NodeIdx]) {
        self.no_lint();
    }

    pub(crate) fn lint_store(&self, _: usize, _: Hash128, _: &Lines<'_>) {
        self.no_lint();
    }

    pub(crate) fn lint_dbg_in_dependencies(&self) {
        self.no_lint();
    }

    pub(crate) fn lint_findings(&self) -> DiagBuf {
        self.no_lint();
        DiagBuf::default()
    }
}
