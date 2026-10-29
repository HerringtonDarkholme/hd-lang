//! `Console` and `ConsoleInput`. A write never suspends: it goes straight
//! to its stream when it can, and otherwise blocks until the ordered
//! writer thread has written it (`module.console.write-sync.never-suspends`).
//! `write_error_sync` uses that same queue after prior standard output
//! (`module.console.write-error-sync.raw`);
//! `read_line` and `read_all` are pending until the reactor's reader thread
//! has read the next line, or the rest, of standard input, so a program's
//! other operations go on while it waits for input.

use wasmtime::{Caller, Val};

use crate::abi::{ConsoleError, ConsoleStream, HostMethod, Id};
use crate::host::{Host, OutputError, Ret, Stop, bytes, handle, int_arg};
use crate::runtime::reactor::{Stream, Work};

/// A `Console.write_sync` or `Console.write_error_sync` call, or a
/// `ConsoleInput` `.start`, or its `.finish` when `finish`.
pub(crate) fn call(
    method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match method.id {
        Id::ConsoleWriteSync | Id::ConsoleWriteErrorSync => {
            let line = bytes(caller, int_arg(params, 0)?)?;
            let out = method.id == Id::ConsoleWriteSync;
            if !caller.data().grants.allows("Console") {
                let stream = if out {
                    ConsoleStream::Stdout
                } else {
                    ConsoleStream::Stderr
                };
                return Ok(Ret::value(&Err::<(), _>(ConsoleError::NotGranted {
                    stream,
                })));
            }
            // Normalize invalid UTF-8 through the text boundary.
            let line = String::from_utf8_lossy(&line).into_owned().into_bytes();
            let to = if out { Stream::Out } else { Stream::Err };
            let result = caller
                .data_mut()
                .write_console(to, line)
                .map_err(|error| match error {
                    OutputError::Closed => ConsoleError::Closed,
                    OutputError::Other(code) => ConsoleError::Other { code },
                });
            Ret::value(&result)
        }
        // `Result[string?, ConsoleError]`: the line, `.None` at the end of
        // input, or `.Err(.Other(code))` when it cannot be read.
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
                // `Result[string, ConsoleError]`: all the rest, or the host
                // read error's numeric code.
                Ret::value(
                    &line
                        .map(Option::unwrap_or_default)
                        .map_err(|code| ConsoleError::Other { code }),
                )
            } else {
                Ret::value(&line.map_err(|code| ConsoleError::Other { code }))
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
