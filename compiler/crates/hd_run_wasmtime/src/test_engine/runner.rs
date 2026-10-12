use std::sync::Arc;
use std::time::{Duration, Instant};

use super::engine::{PreparedTest, TestEnvironment};
use crate::host::{Host, HostIo, Stop};
use crate::testing::{BodyResult, BodyRunner, State, TestCase};
use crate::{Shared, WasmtimeInstance};
use hd_run::{HostSetup, Instance, Outcome};

pub struct NativeBodyRunner {
    prepared: PreparedTest,
    case: TestCase,
    environment: TestEnvironment,
    setup: HostSetup,
    pub(crate) instance: Option<WasmtimeInstance>,
    out: Shared,
    err: Shared,
    deadline: Option<(u32, Option<Instant>)>,
}

impl NativeBodyRunner {
    #[must_use]
    pub fn new(
        prepared: PreparedTest,
        case: TestCase,
        environment: TestEnvironment,
        setup: HostSetup,
    ) -> Self {
        Self {
            prepared,
            case,
            environment,
            setup,
            instance: None,
            out: Shared::default(),
            err: Shared::default(),
            deadline: None,
        }
    }

    fn fresh(&self, state: State) -> Result<WasmtimeInstance, String> {
        let mut host = Host::new(
            HostIo {
                out: Box::new(self.out.clone()),
                err: Box::new(self.err.clone()),
                input: None,
            },
            self.setup.args.clone(),
            self.environment.program.clone(),
            self.setup.grants.clone(),
            None,
            self.setup
                .limits
                .memory_bytes
                .min(self.prepared.engine.max_memory_bytes),
        );
        host.testing = Some(state);
        host.environment = Some(self.environment.clone());
        host.set_deadline(self.deadline.and_then(|(_, deadline)| deadline));
        let mut store = crate::new_store(&self.prepared.engine.engine, host)
            .map_err(|error| format!("{error:?}"))?;
        // Dynamic report_timeout works even when no CLI limit was supplied.
        store.epoch_deadline_callback(|cx| {
            cx.data().check_deadline()?;
            Ok(wasmtime::UpdateDeadline::Continue(1))
        });
        store.set_epoch_deadline(1);
        let instance = match self.prepared.pre.instantiate(&mut store) {
            Ok(instance) => instance,
            Err(error)
                if crate::limits::is_heap_exhausted(&error)
                    || matches!(error.downcast_ref::<Stop>(), Some(Stop::TimeLimit)) =>
            {
                let mut instance =
                    WasmtimeInstance::stopped(store, Arc::clone(&self.prepared.engine.engine));
                instance.trapped(&error);
                return Ok(instance);
            }
            Err(error) => return Err(format!("{error:#}")),
        };
        let mut instance = WasmtimeInstance::from_instance(
            store,
            Arc::clone(&self.prepared.engine.engine),
            instance,
            &format!("hd.init.{}", self.case.init),
            &format!("hd.test.{}", self.case.test),
        )
        .map_err(|error| format!("{error:?}"))?;
        instance.close_on_end = false;
        Ok(instance)
    }
}

impl BodyRunner for NativeBodyRunner {
    fn run_body(&mut self, state: State, reuse: bool) -> Result<BodyResult, String> {
        let start = Instant::now();
        let row = state.current_row();
        if self.deadline.is_none_or(|(previous, _)| previous != row) {
            self.deadline = Some((
                row,
                self.setup
                    .limits
                    .wall_ms
                    .and_then(|millis| start.checked_add(Duration::from_millis(millis))),
            ));
        }
        state.set_io_deadline(self.deadline.and_then(|(_, deadline)| deadline));
        let reusable = reuse
            && self.instance.as_ref().is_some_and(|instance| {
                instance.done == Some(Outcome::Exit(0)) && !instance.last_trapped
            });
        if reusable {
            let instance = self.instance.as_mut().expect("reusable instance");
            instance.done = None;
            instance.store.data_mut().reported = false;
            instance.store.data_mut().begin_test_body();
        } else {
            self.instance = None;
            self.instance = Some(self.fresh(state.clone())?);
        }
        let instance = self.instance.as_mut().expect("instance created");
        let outcome = if reusable || instance.init() == Outcome::Exit(0) {
            hd_run::drive(instance, usize::MAX)
        } else {
            instance
                .done
                .clone()
                .unwrap_or_else(|| Outcome::Internal("test init did not finish".into()))
        };
        instance.store.data_mut().flush();
        let mut trapped = instance.last_trapped;
        let frames = std::mem::take(&mut instance.last_frames);
        let status = match outcome {
            Outcome::Exit(status) => i64::from(status),
            Outcome::Panic(report) => {
                trapped = true;
                instance
                    .store
                    .data_mut()
                    .err(&format!("panic: {}: {}\n", report.category, report.message));
                3
            }
            Outcome::Internal(message) => return Err(message),
        };
        let ended = state.ended();
        let result = BodyResult {
            status,
            trapped,
            ended,
            stdout: String::from_utf8_lossy(&self.out.take()).into_owned(),
            stderr: String::from_utf8_lossy(&self.err.take()).into_owned(),
            us: u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX),
            panic: None,
            frames,
        };
        if trapped || status != 0 || ended {
            self.instance = None;
        }
        Ok(result)
    }

    fn finish(&mut self) {
        self.instance = None;
    }
}
