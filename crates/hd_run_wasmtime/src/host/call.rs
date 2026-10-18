//! The one dispatch from an import to its provider. Each host trait's
//! provider has one entry point in its own module; this file only routes
//! to it, and checks the deadline before and after every call. Host
//! methods are matched on their `Id`, so a method the ABI model adds does
//! not compile until it is routed here.

use wasmtime::{Caller, Val};

use super::kind::Kind;
use super::reply::Ret;
use super::{Host, rt};
use crate::abi::{HostMethod, Id};
use crate::{console, fs, http, net, process, profiles, sys, testing};

pub(crate) fn call(
    kind: Kind,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    caller.data().check_deadline()?;
    let result = match kind {
        Kind::Host { method, finish } => host(method, finish, caller, params)?,
        Kind::Fixture { method, finish } => profiles::call(method, finish, caller, params)?,
        Kind::Rt(import) => rt::rt(import, caller, params)?,
        Kind::Prim(import) => rt::prim(import, caller, params)?,
    };
    caller.data().check_deadline()?;
    Ok(result)
}

/// A host method of the ABI model: its one import, or its `.start`, or its
/// `.finish` when `finish`.
fn host(
    method: &'static HostMethod,
    finish: bool,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    match method.id {
        Id::ConsoleWriteLine | Id::ConsoleInputReadLine | Id::ConsoleInputReadAll => {
            console::call(method, finish, caller, params)
        }
        Id::ClockNow
        | Id::ClockMonotonic
        | Id::ClockSleep
        | Id::SysOs
        | Id::SysArch
        | Id::SysHostname
        | Id::SysCpuCount
        | Id::RandomNextU64
        | Id::RandomFill
        | Id::ArgsProgram
        | Id::ArgsList
        | Id::EnvGet
        | Id::EnvNames => sys::call(method, finish, caller, params),
        Id::FsReadReadBytes
        | Id::FsReadReadText
        | Id::FsReadListDir
        | Id::FsReadStat
        | Id::FsWriteWriteBytes
        | Id::FsWriteWriteText
        | Id::FsWriteAppendText
        | Id::FsWriteCreateDirAll
        | Id::FsWriteRemove
        | Id::FsWriteRename => fs::call(method, finish, caller, params),
        Id::ProcessRun => process::call(method, finish, caller, params),
        Id::HttpSend => http::call(method, finish, caller, params),
        Id::NetLookup
        | Id::NetConnect
        | Id::NetConnectTimeout
        | Id::NetListen
        | Id::NetBindUdp
        | Id::NetTcpStreamRead
        | Id::NetTcpStreamWrite
        | Id::NetTcpStreamSetTimeout
        | Id::NetTcpStreamSetNodelay
        | Id::NetTcpStreamSetKeepalive
        | Id::NetTcpStreamClose
        | Id::NetTcpStreamDispose
        | Id::NetTcpListenerAccept
        | Id::NetTcpListenerSetTimeout
        | Id::NetTcpListenerLocalAddress
        | Id::NetTcpListenerClose
        | Id::NetTcpListenerDispose
        | Id::NetUdpSocketSendTo
        | Id::NetUdpSocketReceive
        | Id::NetUdpSocketSetTimeout
        | Id::NetUdpSocketLocalAddress
        | Id::NetUdpSocketClose
        | Id::NetUdpSocketDispose => {
            let input = (Kind::Host { method, finish }).input();
            net::call(method, finish, input, caller, params)
        }
        Id::TestRunnerRow
        | Id::TestRunnerReportTimeout
        | Id::TestRunnerSnapshotCheck
        | Id::TestRunnerTempDir
        | Id::PropertyRunnerStart
        | Id::PropertyRunnerRecord
        | Id::PropertyRunnerShow => testing::provider::call(method, caller, params),
    }
}
