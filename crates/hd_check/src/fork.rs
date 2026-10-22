//! A module's bodies checked on several threads (`crates/hd_tir/DESIGN.md`
//! §4.13.1). After M1, [`BodyCx::fork`] copies the module's checking state
//! once; each batch of bodies gets its own context from the copy
//! ([`BodyFork::cx`]), and after each body [`BodyCx::left`] takes what the
//! body added to that state. Folding those back in source order
//! ([`BodyCx::join`]) leaves the module's context as a serial check
//! would.

use std::cell::{OnceCell, RefCell};
use std::sync::Arc;

use hd_base::{DefId, HashMap};
use hd_resolve::{LocalItem, Lookup, ModuleScope, Names, Src};
use hd_types::solver::{GlobalMemo, ImplView, Solver};

use crate::MethodIndex;
use crate::body::{BodyCx, TestsView};
use crate::dbg::DbgRef;
use crate::init::{InitFacts, ModuleInit};
use crate::results::Results;

/// A module's checking context without its cells, and a copy of their
/// state after M1, which every thread may read.
pub struct BodyFork<'a> {
    names: Names<'a>,
    src: Src<'a>,
    scope: &'a ModuleScope,
    lookup: &'a Lookup<'a>,
    impls: &'a ImplView<'a>,
    global: &'a GlobalMemo,
    solver: &'a dyn Solver,
    methods: Arc<MethodIndex>,
    init: ModuleInit,
    results: Results,
    locals: &'a [LocalItem],
    file_name: &'a str,
    tests: Option<TestsView<'a>>,
    session: &'a [hd_base::PathId],
    fuel: u64,
    own_passes: bool,
}

// Every thread reads the one copy; `BodyLeft` comes back from them.
const _: fn() = || {
    fn shared<T: Sync>() {}
    fn sent<T: Send>() {}
    shared::<BodyFork<'static>>();
    sent::<BodyLeft>();
};

/// What one body checked in a forked context added to its module's
/// state.
pub struct BodyLeft {
    facts: HashMap<DefId, InitFacts>,
    dbg: Vec<DbgRef>,
    named: Vec<DefId>,
    reads: Vec<DefId>,
}

impl<'a> BodyCx<'a> {
    /// A copy of this context for bodies checked on other threads. Taken
    /// after M1, when no body waits on another.
    #[must_use]
    pub fn fork(&self) -> BodyFork<'a> {
        let methods = self
            .methods
            .get_or_init(|| Arc::new(MethodIndex::build(self.lookup)))
            .clone();
        BodyFork {
            names: self.names,
            src: self.src,
            scope: self.scope,
            lookup: self.lookup,
            impls: self.impls,
            global: self.global,
            solver: self.solver,
            methods,
            init: self.init.borrow().fork(),
            results: self.results.borrow().fork(),
            locals: self.locals,
            file_name: self.file_name,
            tests: self.tests,
            session: self.session,
            fuel: self.fuel,
            own_passes: self.own_passes,
        }
    }

    /// What the bodies checked in this forked context added since the
    /// last call.
    #[must_use]
    pub fn left(&self) -> BodyLeft {
        let (named, reads) = self.results.borrow_mut().drain();
        BodyLeft {
            facts: std::mem::take(&mut self.init.borrow_mut().facts),
            dbg: self.dbg_refs.take(),
            named,
            reads,
        }
    }

    /// Adds what a body checked in a forked context left, as if it had
    /// been checked in this one.
    pub fn join(&self, left: BodyLeft) {
        self.init.borrow_mut().facts.extend(left.facts);
        {
            let mut refs = self.dbg_refs.borrow_mut();
            for r in left.dbg {
                if !refs.contains(&r) {
                    refs.push(r);
                }
            }
        }
        self.results.borrow_mut().join(self, left.named, left.reads);
    }
}

impl<'a> BodyFork<'a> {
    /// A context for one batch of bodies, on any thread.
    #[must_use]
    pub fn cx(&self) -> BodyCx<'a> {
        BodyCx {
            names: self.names,
            src: self.src,
            scope: self.scope,
            lookup: self.lookup,
            impls: self.impls,
            global: self.global,
            solver: self.solver,
            methods: OnceCell::from(self.methods.clone()),
            init: RefCell::new(self.init.fork()),
            results: RefCell::new(self.results.fork()),
            dbg_refs: RefCell::default(),
            locals: self.locals,
            file_name: self.file_name,
            tests: self.tests,
            session: self.session,
            fuel: self.fuel,
            own_passes: self.own_passes,
        }
    }
}
