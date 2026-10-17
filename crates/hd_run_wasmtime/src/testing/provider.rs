//! The `TestRunner` and `PropertyRunner` providers, which only a test
//! binds: each call acts on the case's shared `State`, with the run's
//! deadline for its I/O.

use wasmtime::{Caller, Val};

use super::State;
use crate::abi::{HostMethod, Id};
use crate::host::{Host, Ret, Stop, int_arg, text};

/// A `TestRunner` or `PropertyRunner` call; none of them waits.
pub(crate) fn call(
    method: &'static HostMethod,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match method.id {
        Id::TestRunnerRow => {
            let state = runner(caller)?;
            Ret::int(i64::from(
                state.row(
                    u32::try_from(int_arg(params, 0)?)
                        .map_err(|_| Stop::Contract("invalid row count".into()))?,
                ),
            ))
        }
        Id::TestRunnerReportTimeout => {
            let state = runner(caller)?;
            let millis = int_arg(params, 0)?;
            state.report_timeout(millis);
            caller.data_mut().report_timeout(millis);
            state.set_io_deadline(caller.data().deadline());
            wasmtime::AsContextMut::as_context_mut(caller).set_epoch_deadline(1);
            Ret::void()
        }
        Id::TestRunnerSnapshotCheck => {
            let state = runner(caller)?;
            let value = text(caller, int_arg(params, 0)?)?;
            let message = state.snapshot_check(&value);
            caller.data().check_deadline()?;
            Ret::value(&message.map_err(Stop::Internal)?)
        }
        Id::TestRunnerTempDir => {
            let state = runner(caller)?;
            Ret::value(&state.temp_dir().map_err(Stop::Internal)?)
        }
        Id::PropertyRunnerStart => {
            let state = runner(caller)?;
            let mut counts = [0; 3];
            for (index, count) in counts.iter_mut().enumerate() {
                *count = u32::try_from(int_arg(params, index)?)
                    .map_err(|_| Stop::Contract("invalid property count".into()))?;
            }
            let case = state.start(counts[0], counts[1], counts[2]);
            caller.data().check_deadline()?;
            Ret::value(&case.map_err(Stop::Internal)?.ok_or(Stop::EndCase)?)
        }
        Id::PropertyRunnerRecord => {
            let state = runner(caller)?;
            state.record(int_arg(params, 0)?);
            Ret::void()
        }
        Id::PropertyRunnerShow => {
            let state = runner(caller)?;
            state.show(&text(caller, int_arg(params, 0)?)?);
            Ret::void()
        }
        _ => return Err(crate::host::misrouted(method)),
    })
}

/// The test runner state a `TestRunner` or `PropertyRunner` call acts on,
/// with the run's deadline for its I/O.
fn runner(caller: &mut Caller<'_, Host>) -> wasmtime::Result<State> {
    let state = caller
        .data()
        .testing
        .clone()
        .ok_or_else(|| Stop::Contract("no test runner binds this run".into()))?;
    state.set_io_deadline(caller.data().deadline());
    Ok(state)
}
