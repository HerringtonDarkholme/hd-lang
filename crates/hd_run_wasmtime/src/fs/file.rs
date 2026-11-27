//! `FsRead.open` and the `std.fs.File` it returns
//! (`std-fs.open.decl`). An open file lives in the instance's handle
//! registry (`native::Handles`) under [`FILE`], so its number is checked
//! in O(1) and never issued twice. Each `File.read` is one read of
//! at most `max` bytes: a regular file's runs inline on the program
//! thread, as `read_bytes` does; a special file's (a FIFO, a device) runs
//! on the blocking pool, so a slow writer never blocks other tasks
//! (`std-fs.special-file`). `File.dispose`, which a `using!` binding
//! calls, drops the file, which closes it (`std-fs.open.dispose`); the
//! number is dead from then on, so a later call through an alias is
//! `Closed`. A read in flight holds the file too, so the descriptor closes
//! when both are done.
//!
//! Cost: one host call and one `read` system call per chunk, and
//! O(min(max, the bytes left)) memory; nothing reads the whole file.

use std::io::Read as _;
use std::sync::Arc;

use wasmtime::{Caller, Val};

use crate::abi::{File, FsError, Id, IoError};
use crate::codec::{Enc, encode};
use crate::host::offload::{finished, not_completed, pending};
use crate::host::{Host, Ret, Stop, count_arg, int_arg};
use crate::runtime::reactor::Done;

/// The handle registry interface of an open file: a host number above
/// every contract's interfaces, which count from 0.
pub(crate) const FILE: u32 = u32::MAX;

/// The most bytes one read of a special file asks of the OS: the buffer
/// a pipe or device read fills at once, as `net::CHUNK` is for a socket.
/// A shorter chunk is a valid `read!` result (`std-io.read.at-most`).
const SPECIAL_CHUNK: usize = crate::net::CHUNK;

/// An open file and whether it is special.
struct Open {
    file: Arc<std::fs::File>,
    special: bool,
}

/// The `Result[File, FsError]` of an `open`: `file` gets its number.
pub(crate) fn issue(host: &mut Host, opened: Result<std::fs::File, Enc>, special: bool) -> Enc {
    let file = match opened {
        Ok(file) => file,
        Err(encoded) => return encoded,
    };
    let value = Box::new(Open {
        file: Arc::new(file),
        special,
    });
    match host.handles.insert_host(FILE, value) {
        Ok(number) => encode(&Ok::<File, FsError>(crate::codec::Handle(u64::from(
            number,
        )))),
        Err(error) => encode(&Err::<File, _>(FsError::Other { message: error.0 })),
    }
}

/// A `File.read` or `File.dispose` `.start`, or a read's `.finish`.
pub(crate) fn call(
    id: Id,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    if finish {
        let Done::Read(read) = finished(caller, params)? else {
            return Err(not_completed());
        };
        return Ok(Ret::encoded(chunk(read)));
    }
    let number = u32::try_from(int_arg(params, 0)?).unwrap_or(0);
    let host = caller.data_mut();
    // A number that is not live was disposed: only unchecked code makes
    // one this instance never issued. Disposing it again does nothing.
    let open = host.handles.host::<Open>(number, FILE);
    if id == Id::FsReadFileDispose {
        if open.is_some() {
            host.handles
                .release(number, FILE)
                .map_err(|error| Stop::Contract(error.0))?;
        }
        return Ret::ready(Enc::default());
    }
    let Some(open) = open else {
        return Ret::ready(encode(&Err::<Vec<u8>, _>(IoError::Closed)));
    };
    let max = count_arg(params, 1)?;
    let (file, special) = (Arc::clone(&open.file), open.special);
    if !special {
        return Ret::ready(chunk(read_regular(&file, max)));
    }
    let work = async move {
        let read = tokio::task::spawn_blocking(move || read_once(&file, max)).await;
        Done::Read(read.unwrap_or_else(|error| Err(std::io::Error::other(error.to_string()))))
    };
    pending(host.reactor.offload(work), |error| {
        encode(&Err::<Vec<u8>, _>(IoError::Other {
            message: error.to_owned(),
        }))
    })
}

/// A regular file's next bytes, at most `max`: all that are left when
/// fewer, so a short chunk means the end is next. The buffer grows with
/// what is read, never to `max` ahead of it.
fn read_regular(file: &std::fs::File, max: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(max.min(SPECIAL_CHUNK));
    let limit = u64::try_from(max).unwrap_or(u64::MAX);
    file.take(limit).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// One read of a special file: the bytes it has now, at most `max`, or
/// none at its end.
fn read_once(file: &std::fs::File, max: usize) -> std::io::Result<Vec<u8>> {
    let mut bytes = vec![0; max.min(SPECIAL_CHUNK)];
    let n = loop {
        match (&*file).read(&mut bytes) {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            result => break result?,
        }
    };
    bytes.truncate(n);
    Ok(bytes)
}

/// `Result[List[u8], IoError]` of a read.
fn chunk(read: std::io::Result<Vec<u8>>) -> Enc {
    encode(&read.map_err(|error| IoError::Other {
        message: format!("read: {error}"),
    }))
}
