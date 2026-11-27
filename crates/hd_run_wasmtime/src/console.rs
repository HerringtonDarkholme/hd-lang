//! `Console` and `ConsoleInput`. A write never suspends: it goes straight
//! to its stream when it can, and otherwise blocks until the ordered
//! writer thread has written it (`module.console.write-sync.never-suspends`).
//! `write_error_sync` uses that same queue after prior standard output
//! (`module.console.write-error-sync.raw`);
//! `read_line` and `read_all` are pending until the reactor's reader thread
//! has read the next line, or the rest, of standard input, so a program's
//! other operations go on while it waits for input. `stdin`'s handle reads
//! the next bytes the same way, as soon as any arrive.

use wasmtime::{Caller, Val};

use crate::abi::{ConsoleError, ConsoleStream, HostMethod, Id, IoError, StdinHandle};
use crate::host::{Host, OutputError, Ret, Stop, bytes, count_arg, handle, int_arg};
use crate::runtime::reactor::{Stream, Work};

/// The number of standard input's one handle (`ConsoleInput.stdin`).
const STDIN: u64 = 1;

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
        Id::ConsoleInputReadLine | Id::ConsoleInputReadAll | Id::ConsoleInputStdinHandleRead
            if finish =>
        {
            let h = handle(int_arg(params, 0)?);
            let Some(Work::Read(Some(line))) =
                caller.data_mut().reactor.remove(h).map(|op| op.work)
            else {
                return Err(Stop::Contract(
                    "a finish of an operation that is not completed".into(),
                )
                .into());
            };
            let text = |bytes: Vec<u8>| String::from_utf8_lossy(&bytes).into_owned();
            match method.id {
                // `Result[List[u8], IoError]`: the bytes exactly as read,
                // empty at the end, or the host read error's code.
                Id::ConsoleInputStdinHandleRead => {
                    Ret::value(&line.map(Option::unwrap_or_default).map_err(|code| {
                        IoError::Other {
                            message: format!("standard input read error {code}"),
                        }
                    }))
                }
                // `Result[string, ConsoleError]`: all the rest, or the host
                // read error's numeric code.
                Id::ConsoleInputReadAll => Ret::value(
                    &line
                        .map(|rest| text(rest.unwrap_or_default()))
                        .map_err(|code| ConsoleError::Other { code }),
                ),
                _ => Ret::value(
                    &line
                        .map(|found| found.map(text))
                        .map_err(|code| ConsoleError::Other { code }),
                ),
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
        // Standard input's one handle: its reads share the buffered input
        // with `read_line` and `read_all`, so none loses bytes another read.
        Id::ConsoleInputStdin => Ret::value::<StdinHandle>(&crate::codec::Handle(STDIN)),
        // `(handle, max)`: pending until any input is buffered, or it ends.
        Id::ConsoleInputStdinHandleRead => {
            if u64::try_from(int_arg(params, 0)?).ok() != Some(STDIN) {
                return Err(
                    Stop::Contract("a read of an unknown standard input handle".into()).into(),
                );
            }
            let max = count_arg(params, 1)?;
            let host = caller.data_mut();
            host.flush();
            Ret::pair(1, host.reactor.read_bytes(max))
        }
        _ => return Err(crate::host::misrouted(method)),
    })
}
