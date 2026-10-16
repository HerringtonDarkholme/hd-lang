use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use super::case::TestCase;
use crate::abi::PropertyCase;
use crate::property::{Plan, Property};
use crate::testing_files::{Files, load_regression};

#[derive(Clone, Default)]
pub(crate) struct BodyState {
    pub(crate) draws: Vec<i64>,
    pub(crate) shown: Option<String>,
    pub(crate) plan: Option<Plan>,
}

pub(crate) struct Inner {
    case: TestCase,
    row: u32,
    pub(crate) rows: Option<u32>,
    pub(crate) property: Option<Property>,
    pub(crate) body: BodyState,
    timeout: Option<i64>,
    ended: bool,
    files: Files,
}

/// Shared by replacement stores, but exclusively run by one property
/// worker. A store owns a clone, not a second property controller.
#[derive(Clone)]
pub struct State(Arc<Mutex<Inner>>);

impl State {
    #[must_use]
    pub fn new(case: TestCase) -> Self {
        let files = Files::new(&case);
        Self(Arc::new(Mutex::new(Inner {
            case,
            row: 0,
            rows: None,
            property: None,
            body: BodyState::default(),
            timeout: None,
            ended: false,
            files,
        })))
    }

    pub(crate) fn lock(&self) -> MutexGuard<'_, Inner> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn begin_body(&self, row: u32, ordinal: u64) {
        let mut state = self.lock();
        state.row = row;
        state.body = BodyState::default();
        state.timeout = None;
        state.ended = false;
        let row = state.rows.map(|_| row);
        state.files.begin(ordinal, row);
    }

    #[must_use]
    pub fn row(&self, count: u32) -> u32 {
        let mut state = self.lock();
        if state.rows.is_none() && state.property.is_none() && state.case.kind == "it_each" {
            state.rows = Some(count);
            let row = state.row;
            state.files.set_row(row);
        }
        state.row
    }

    pub fn report_timeout(&self, millis: i64) {
        self.lock().timeout = Some(millis);
    }
    #[must_use]
    pub fn current_row(&self) -> u32 {
        self.lock().row
    }
    #[must_use]
    pub fn timeout_ms(&self) -> Option<i64> {
        self.lock().timeout
    }
    #[must_use]
    pub fn ended(&self) -> bool {
        self.lock().ended
    }
    pub fn snapshot_check(&self, actual: &str) -> Result<String, String> {
        self.lock().files.snapshot_check(actual)
    }
    pub(crate) fn set_io_deadline(&self, deadline: Option<std::time::Instant>) {
        self.lock().files.deadline = deadline;
    }
    pub fn temp_dir(&self) -> Result<String, String> {
        self.lock().files.temp_dir()
    }
    pub fn cleanup_body(&self) {
        self.lock().files.cleanup();
    }

    pub fn start(
        &self,
        cases: u32,
        shrink: u32,
        examples: u32,
    ) -> Result<Option<PropertyCase>, String> {
        let mut state = self.lock();
        if state.property.is_none() {
            let replays = load_regression(state.case.regression.as_deref(), state.files.deadline)?;
            state.property = Some(Property::new(
                cases,
                shrink,
                examples,
                state.case.seed.unwrap_or(0),
                replays,
            ));
        }
        let plan = state
            .property
            .as_mut()
            .expect("property initialized")
            .next()?;
        state.ended = plan.is_none();
        let case = plan.as_ref().map(Plan::case);
        state.body.plan = plan;
        Ok(case)
    }

    pub fn record(&self, value: i64) {
        self.lock().body.draws.push(value);
    }
    pub fn show(&self, text: &str) {
        let mut state = self.lock();
        if state.body.shown.is_none() {
            state.body.shown = Some(text.into());
        }
    }
}
