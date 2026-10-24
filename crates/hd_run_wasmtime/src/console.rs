//! `Console` and `ConsoleInput`. Output stays pending until the ordered
//! writer thread writes its chunk, so a full pipe cannot block polling.
//! `write_error` uses that same queue after prior standard output
//! (`module.console.write-error.raw`);
//! `read_line` and `read_all` are pending until the reactor's reader thread
//! has read the next line, or the rest, of standard input, so a program's
//! other operations go on while it waits for input.

use wasmtime::{Caller, Val};

use crate::abi::{ConsoleError, HostMethod, Id};
use crate::host::{Host, Ret, Stop, bytes, handle, int_arg};
use crate::runtime::reactor::{Stream, Work};

/// A `Console.write`, `Console.write_error` or
/// `ConsoleInput.read_line` `.start`, or its
/// `.finish` when `finish`.
pub(crate) fn call(
    method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match method.id {
        Id::ConsoleWrite | Id::ConsoleWriteError if finish => {
            let h = int_arg(params, 0)?;
            caller.data_mut().reactor.remove(handle(h));
            Ret::value(&Ok::<(), ConsoleError>(()))
        }
        Id::ConsoleWrite | Id::ConsoleWriteError => {
            let line = bytes(caller, int_arg(params, 0)?)?;
            if !caller.data().grants.allows("Console") {
                let subject = if method.id == Id::ConsoleWrite {
                    "stdout"
                } else {
                    "stderr"
                };
                return Ret::ready(crate::codec::encode(&Err::<(), _>(
                    ConsoleError::NotGranted {
                        subject: subject.into(),
                    },
                )));
            }
            // Normalize invalid UTF-8 through the text boundary.
            let line = String::from_utf8_lossy(&line).into_owned().into_bytes();
            let to = if method.id == Id::ConsoleWrite {
                Stream::Out
            } else {
                Stream::Err
            };
            Ret::pair(1, caller.data_mut().write_console(to, line))
        }
        // `Result[string?, ConsoleError]`: the line, `.None` at the end of
        // input, `.Err(.Closed)` when it cannot be read.
        Id::ConsoleInputReadLine | Id::ConsoleInputReadAll if finish => {
            let h = handle(int_arg(params, 0)?);
            let Some(Work::Read(Some(line))) =
                caller.data_mut().reactor.remove(h).map(|op| op.work)
            else {
                return Err(Stop::Contract(
                    "a finish of an operation that is not completed".into(),
                )
                .into());
            };
            if method.id == Id::ConsoleInputReadAll {
                // `Result[string, ConsoleError]`: all the rest, or `.Err(.Closed)`.
                Ret::value(
                    &line
                        .map(Option::unwrap_or_default)
                        .map_err(|()| ConsoleError::Closed),
                )
            } else {
                Ret::value(&line.map_err(|()| ConsoleError::Closed))
            }
        }
        // Make a completed prompt visible before asking the reader thread
        // for input. Pending until that thread delivers the line.
        Id::ConsoleInputReadLine => {
            let host = caller.data_mut();
            host.flush();
            Ret::pair(1, host.reactor.read_line())
        }
        Id::ConsoleInputReadAll => {
            let host = caller.data_mut();
            host.flush();
            Ret::pair(1, host.reactor.read_all())
        }
        _ => return Err(crate::host::misrouted(method)),
    })
}
