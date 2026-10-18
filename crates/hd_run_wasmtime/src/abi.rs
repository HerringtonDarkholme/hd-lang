//! The host ABI's wasmtime codecs and method descriptors, generated from
//! the ABI model (`host/gen/abi.json`) by
//! `crates/hd_host_abi/tests/abi_model.rs`. Do not edit: `HD_UPDATE_ABI=1
//! cargo test -p hd_host_abi --test abi_model` rewrites it. `codec.rs`
//! says how each value crosses.
//!
//! Each std type a host method crosses is a Rust type of the same name
//! and shape: a data type a struct, an enum an enum, a newtype a tuple
//! struct, a host handle a `Handle`; `T?` is `Option`, `List[T]` a
//! `Vec` and `string` a `String`.

use hd_host_abi::{Codec, ResourceArg, Scalar, Wait};

use crate::codec::{Dec, Enc, Handle, Wire};

/// `ConsoleError` of `lib/std/console.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsoleError {
    Closed,
}

impl Wire for ConsoleError {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::Closed => {
                e.leb(0);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        d.tag(1)?;
        Ok(Self::Closed)
    }
}

/// `Datagram` of `lib/std/net.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Datagram {
    pub bytes: Vec<u8>,
    pub host: String,
    pub port: u16,
}

impl Wire for Datagram {
    fn enc(&self, e: &mut Enc) {
        self.bytes.enc(e);
        self.host.enc(e);
        self.port.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self {
            bytes: Wire::dec(d)?,
            host: Wire::dec(d)?,
            port: Wire::dec(d)?,
        })
    }
}

/// `Duration` of `lib/std/time.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Duration {
    pub millis: i64,
}

impl Wire for Duration {
    fn enc(&self, e: &mut Enc) {
        self.millis.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self {
            millis: Wire::dec(d)?,
        })
    }
}

/// `Entry` of `lib/std/fs.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: Path,
    pub kind: EntryKind,
    pub size: u64,
}

impl Wire for Entry {
    fn enc(&self, e: &mut Enc) {
        self.path.enc(e);
        self.kind.enc(e);
        self.size.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self {
            path: Wire::dec(d)?,
            kind: Wire::dec(d)?,
            size: Wire::dec(d)?,
        })
    }
}

/// `EntryKind` of `lib/std/fs.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryKind {
    File,
    Directory,
    Symlink,
}

impl Wire for EntryKind {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::File => {
                e.leb(0);
            }
            Self::Directory => {
                e.leb(1);
            }
            Self::Symlink => {
                e.leb(2);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(3)? {
            0 => Self::File,
            1 => Self::Directory,
            _ => Self::Symlink,
        })
    }
}

/// `FsError` of `lib/std/fs.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsError {
    NotFound { path: Path },
    PermissionDenied { path: Path },
    NotGranted { path: Path },
    AlreadyExists { path: Path },
    NotADirectory { path: Path },
    IsADirectory { path: Path },
    InvalidUtf8 { path: Path },
    Other { message: String },
}

impl Wire for FsError {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::NotFound { path } => {
                e.leb(0);
                path.enc(e);
            }
            Self::PermissionDenied { path } => {
                e.leb(1);
                path.enc(e);
            }
            Self::NotGranted { path } => {
                e.leb(2);
                path.enc(e);
            }
            Self::AlreadyExists { path } => {
                e.leb(3);
                path.enc(e);
            }
            Self::NotADirectory { path } => {
                e.leb(4);
                path.enc(e);
            }
            Self::IsADirectory { path } => {
                e.leb(5);
                path.enc(e);
            }
            Self::InvalidUtf8 { path } => {
                e.leb(6);
                path.enc(e);
            }
            Self::Other { message } => {
                e.leb(7);
                message.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(8)? {
            0 => Self::NotFound {
                path: Wire::dec(d)?,
            },
            1 => Self::PermissionDenied {
                path: Wire::dec(d)?,
            },
            2 => Self::NotGranted {
                path: Wire::dec(d)?,
            },
            3 => Self::AlreadyExists {
                path: Wire::dec(d)?,
            },
            4 => Self::NotADirectory {
                path: Wire::dec(d)?,
            },
            5 => Self::IsADirectory {
                path: Wire::dec(d)?,
            },
            6 => Self::InvalidUtf8 {
                path: Wire::dec(d)?,
            },
            _ => Self::Other {
                message: Wire::dec(d)?,
            },
        })
    }
}

/// `HttpError` of `lib/std/http.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HttpError {
    NotGranted { host: String },
    InvalidUrl { url: String },
    Dns { host: String },
    Connect { message: String },
    Tls { message: String },
    Timeout,
    TooManyRedirects { url: String },
    Other { message: String },
    Status { status: u16, url: String },
    Json { error: JsonError },
}

impl Wire for HttpError {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::NotGranted { host } => {
                e.leb(0);
                host.enc(e);
            }
            Self::InvalidUrl { url } => {
                e.leb(1);
                url.enc(e);
            }
            Self::Dns { host } => {
                e.leb(2);
                host.enc(e);
            }
            Self::Connect { message } => {
                e.leb(3);
                message.enc(e);
            }
            Self::Tls { message } => {
                e.leb(4);
                message.enc(e);
            }
            Self::Timeout => {
                e.leb(5);
            }
            Self::TooManyRedirects { url } => {
                e.leb(6);
                url.enc(e);
            }
            Self::Other { message } => {
                e.leb(7);
                message.enc(e);
            }
            Self::Status { status, url } => {
                e.leb(8);
                status.enc(e);
                url.enc(e);
            }
            Self::Json { error } => {
                e.leb(9);
                error.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(10)? {
            0 => Self::NotGranted {
                host: Wire::dec(d)?,
            },
            1 => Self::InvalidUrl { url: Wire::dec(d)? },
            2 => Self::Dns {
                host: Wire::dec(d)?,
            },
            3 => Self::Connect {
                message: Wire::dec(d)?,
            },
            4 => Self::Tls {
                message: Wire::dec(d)?,
            },
            5 => Self::Timeout,
            6 => Self::TooManyRedirects { url: Wire::dec(d)? },
            7 => Self::Other {
                message: Wire::dec(d)?,
            },
            8 => Self::Status {
                status: Wire::dec(d)?,
                url: Wire::dec(d)?,
            },
            _ => Self::Json {
                error: Wire::dec(d)?,
            },
        })
    }
}

/// `JsonError` of `lib/std/json.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JsonError {
    UnexpectedEnd { position: usize },
    UnexpectedCharacter { position: usize },
    InvalidEscape { position: usize },
    LoneSurrogate { position: usize },
    ControlCharacter { position: usize },
    NumberOutOfRange { position: usize },
    NestingTooDeep { position: usize },
    WrongType { path: String, expected: String },
    MissingField { path: String },
    UnknownVariant { path: String, name: String },
}

impl Wire for JsonError {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::UnexpectedEnd { position } => {
                e.leb(0);
                position.enc(e);
            }
            Self::UnexpectedCharacter { position } => {
                e.leb(1);
                position.enc(e);
            }
            Self::InvalidEscape { position } => {
                e.leb(2);
                position.enc(e);
            }
            Self::LoneSurrogate { position } => {
                e.leb(3);
                position.enc(e);
            }
            Self::ControlCharacter { position } => {
                e.leb(4);
                position.enc(e);
            }
            Self::NumberOutOfRange { position } => {
                e.leb(5);
                position.enc(e);
            }
            Self::NestingTooDeep { position } => {
                e.leb(6);
                position.enc(e);
            }
            Self::WrongType { path, expected } => {
                e.leb(7);
                path.enc(e);
                expected.enc(e);
            }
            Self::MissingField { path } => {
                e.leb(8);
                path.enc(e);
            }
            Self::UnknownVariant { path, name } => {
                e.leb(9);
                path.enc(e);
                name.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(10)? {
            0 => Self::UnexpectedEnd {
                position: Wire::dec(d)?,
            },
            1 => Self::UnexpectedCharacter {
                position: Wire::dec(d)?,
            },
            2 => Self::InvalidEscape {
                position: Wire::dec(d)?,
            },
            3 => Self::LoneSurrogate {
                position: Wire::dec(d)?,
            },
            4 => Self::ControlCharacter {
                position: Wire::dec(d)?,
            },
            5 => Self::NumberOutOfRange {
                position: Wire::dec(d)?,
            },
            6 => Self::NestingTooDeep {
                position: Wire::dec(d)?,
            },
            7 => Self::WrongType {
                path: Wire::dec(d)?,
                expected: Wire::dec(d)?,
            },
            8 => Self::MissingField {
                path: Wire::dec(d)?,
            },
            _ => Self::UnknownVariant {
                path: Wire::dec(d)?,
                name: Wire::dec(d)?,
            },
        })
    }
}

/// `Method` of `lib/std/http.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Method {
    Get,
    Head,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Other { name: String },
}

impl Wire for Method {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::Get => {
                e.leb(0);
            }
            Self::Head => {
                e.leb(1);
            }
            Self::Post => {
                e.leb(2);
            }
            Self::Put => {
                e.leb(3);
            }
            Self::Patch => {
                e.leb(4);
            }
            Self::Delete => {
                e.leb(5);
            }
            Self::Options => {
                e.leb(6);
            }
            Self::Other { name } => {
                e.leb(7);
                name.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(8)? {
            0 => Self::Get,
            1 => Self::Head,
            2 => Self::Post,
            3 => Self::Put,
            4 => Self::Patch,
            5 => Self::Delete,
            6 => Self::Options,
            _ => Self::Other {
                name: Wire::dec(d)?,
            },
        })
    }
}

/// `NetError` of `lib/std/net.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetError {
    NotGranted { address: String },
    InvalidAddress { address: String },
    Dns { host: String },
    Refused { address: String },
    Timeout,
    Closed,
    Other { message: String },
}

impl Wire for NetError {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::NotGranted { address } => {
                e.leb(0);
                address.enc(e);
            }
            Self::InvalidAddress { address } => {
                e.leb(1);
                address.enc(e);
            }
            Self::Dns { host } => {
                e.leb(2);
                host.enc(e);
            }
            Self::Refused { address } => {
                e.leb(3);
                address.enc(e);
            }
            Self::Timeout => {
                e.leb(4);
            }
            Self::Closed => {
                e.leb(5);
            }
            Self::Other { message } => {
                e.leb(6);
                message.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(7)? {
            0 => Self::NotGranted {
                address: Wire::dec(d)?,
            },
            1 => Self::InvalidAddress {
                address: Wire::dec(d)?,
            },
            2 => Self::Dns {
                host: Wire::dec(d)?,
            },
            3 => Self::Refused {
                address: Wire::dec(d)?,
            },
            4 => Self::Timeout,
            5 => Self::Closed,
            _ => Self::Other {
                message: Wire::dec(d)?,
            },
        })
    }
}

/// `Path` of `lib/std/path.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Path(pub String);

impl Wire for Path {
    fn enc(&self, e: &mut Enc) {
        self.0.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self(Wire::dec(d)?))
    }
}

/// `ProcessError` of `lib/std/process.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessError {
    NotFound { program: String },
    PermissionDenied { program: String },
    NotGranted,
    Other { message: String },
    Exited { status: i32, stderr: String },
}

impl Wire for ProcessError {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::NotFound { program } => {
                e.leb(0);
                program.enc(e);
            }
            Self::PermissionDenied { program } => {
                e.leb(1);
                program.enc(e);
            }
            Self::NotGranted => {
                e.leb(2);
            }
            Self::Other { message } => {
                e.leb(3);
                message.enc(e);
            }
            Self::Exited { status, stderr } => {
                e.leb(4);
                status.enc(e);
                stderr.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(5)? {
            0 => Self::NotFound {
                program: Wire::dec(d)?,
            },
            1 => Self::PermissionDenied {
                program: Wire::dec(d)?,
            },
            2 => Self::NotGranted,
            3 => Self::Other {
                message: Wire::dec(d)?,
            },
            _ => Self::Exited {
                status: Wire::dec(d)?,
                stderr: Wire::dec(d)?,
            },
        })
    }
}

/// `ProcessOutput` of `lib/std/process.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProcessOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: i32,
}

impl Wire for ProcessOutput {
    fn enc(&self, e: &mut Enc) {
        self.stdout.enc(e);
        self.stderr.enc(e);
        self.status.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self {
            stdout: Wire::dec(d)?,
            stderr: Wire::dec(d)?,
            status: Wire::dec(d)?,
        })
    }
}

/// `PropertyCase` of `lib/std/testing.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PropertyCase {
    pub example: Option<usize>,
    pub seed: i64,
    pub size: i32,
    pub replay: Vec<i64>,
}

impl Wire for PropertyCase {
    fn enc(&self, e: &mut Enc) {
        self.example.enc(e);
        self.seed.enc(e);
        self.size.enc(e);
        self.replay.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self {
            example: Wire::dec(d)?,
            seed: Wire::dec(d)?,
            size: Wire::dec(d)?,
            replay: Wire::dec(d)?,
        })
    }
}

/// `Request` of `lib/std/http.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    pub method: Method,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub timeout: Option<Duration>,
}

impl Wire for Request {
    fn enc(&self, e: &mut Enc) {
        self.method.enc(e);
        self.url.enc(e);
        self.headers.enc(e);
        self.body.enc(e);
        self.timeout.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self {
            method: Wire::dec(d)?,
            url: Wire::dec(d)?,
            headers: Wire::dec(d)?,
            body: Wire::dec(d)?,
            timeout: Wire::dec(d)?,
        })
    }
}

/// `Response` of `lib/std/http.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    pub url: String,
}

impl Wire for Response {
    fn enc(&self, e: &mut Enc) {
        self.status.enc(e);
        self.headers.enc(e);
        self.body.enc(e);
        self.url.enc(e);
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(Self {
            status: Wire::dec(d)?,
            headers: Wire::dec(d)?,
            body: Wire::dec(d)?,
            url: Wire::dec(d)?,
        })
    }
}

/// `SysError` of `lib/std/sys.hd`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SysError {
    NotGranted { name: String },
    Unsupported { name: String },
}

impl Wire for SysError {
    fn enc(&self, e: &mut Enc) {
        match self {
            Self::NotGranted { name } => {
                e.leb(0);
                name.enc(e);
            }
            Self::Unsupported { name } => {
                e.leb(1);
                name.enc(e);
            }
        }
    }
    fn dec(d: &mut Dec<'_>) -> wasmtime::Result<Self> {
        Ok(match d.tag(2)? {
            0 => Self::NotGranted {
                name: Wire::dec(d)?,
            },
            _ => Self::Unsupported {
                name: Wire::dec(d)?,
            },
        })
    }
}

/// `TcpListener`, a host handle: its handle number.
pub type TcpListener = Handle;

/// `TcpStream`, a host handle: its handle number.
pub type TcpStream = Handle;

/// `UdpSocket`, a host handle: its handle number.
pub type UdpSocket = Handle;

/// A host method of the ABI model, by its trait key and name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Id {
    ConsoleWriteLine,
    ConsoleInputReadLine,
    ConsoleInputReadAll,
    ClockNow,
    ClockMonotonic,
    ClockSleep,
    FsReadReadBytes,
    FsReadReadText,
    FsReadListDir,
    FsReadStat,
    FsWriteWriteBytes,
    FsWriteWriteText,
    FsWriteAppendText,
    FsWriteCreateDirAll,
    FsWriteRemove,
    FsWriteRename,
    RandomNextU64,
    RandomFill,
    HttpSend,
    NetLookup,
    NetConnect,
    NetConnectTimeout,
    NetListen,
    NetBindUdp,
    ProcessRun,
    SysOs,
    SysArch,
    SysHostname,
    SysCpuCount,
    EnvGet,
    EnvNames,
    ArgsProgram,
    ArgsList,
    TestRunnerRow,
    TestRunnerReportTimeout,
    TestRunnerSnapshotCheck,
    TestRunnerTempDir,
    PropertyRunnerStart,
    PropertyRunnerRecord,
    PropertyRunnerShow,
    NetTcpStreamRead,
    NetTcpStreamWrite,
    NetTcpStreamSetTimeout,
    NetTcpStreamSetNodelay,
    NetTcpStreamSetKeepalive,
    NetTcpStreamClose,
    NetTcpStreamDispose,
    NetTcpListenerAccept,
    NetTcpListenerSetTimeout,
    NetTcpListenerLocalAddress,
    NetTcpListenerClose,
    NetTcpListenerDispose,
    NetUdpSocketSendTo,
    NetUdpSocketReceive,
    NetUdpSocketSetTimeout,
    NetUdpSocketLocalAddress,
    NetUdpSocketClose,
    NetUdpSocketDispose,
}

/// A host method (`crates/hd_run_wasmtime/DESIGN.md` §17.2): `wait` is
/// `Never` for one import or `May` for `.start` and `.finish`;
/// `resource` names the argument a grant checks; each parameter and the
/// result is a scalar or a value of the std type it names in the
/// exchange buffer.
#[derive(Debug)]
pub struct HostMethod {
    pub id: Id,
    pub module: &'static str,
    pub name: &'static str,
    pub wait: Wait,
    pub resource: Option<ResourceArg>,
    pub params: &'static [Codec],
    pub result: Codec,
}

/// Each host method by import module and name.
pub static METHODS: &[HostMethod] = &[
    HostMethod {
        id: Id::ConsoleWriteLine,
        module: "hd:Console",
        name: "write_line",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Buffer("string")],
        result: Codec::Buffer("Result[void, ConsoleError]"),
    },
    HostMethod {
        id: Id::ConsoleInputReadLine,
        module: "hd:ConsoleInput",
        name: "read_line",
        wait: Wait::May,
        resource: None,
        params: &[],
        result: Codec::Buffer("Result[string?, ConsoleError]"),
    },
    HostMethod {
        id: Id::ConsoleInputReadAll,
        module: "hd:ConsoleInput",
        name: "read_all",
        wait: Wait::May,
        resource: None,
        params: &[],
        result: Codec::Buffer("Result[string, ConsoleError]"),
    },
    HostMethod {
        id: Id::ClockNow,
        module: "hd:Clock",
        name: "now",
        wait: Wait::Never,
        resource: None,
        params: &[],
        result: Codec::Scalar(Scalar::I64),
    },
    HostMethod {
        id: Id::ClockMonotonic,
        module: "hd:Clock",
        name: "monotonic",
        wait: Wait::Never,
        resource: None,
        params: &[],
        result: Codec::Scalar(Scalar::I64),
    },
    HostMethod {
        id: Id::ClockSleep,
        module: "hd:Clock",
        name: "sleep",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Scalar(Scalar::I64)],
        result: Codec::Void,
    },
    HostMethod {
        id: Id::FsReadReadBytes,
        module: "hd:FsRead",
        name: "read_bytes",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path")],
        result: Codec::Buffer("Result[List[u8], FsError]"),
    },
    HostMethod {
        id: Id::FsReadReadText,
        module: "hd:FsRead",
        name: "read_text",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path")],
        result: Codec::Buffer("Result[string, FsError]"),
    },
    HostMethod {
        id: Id::FsReadListDir,
        module: "hd:FsRead",
        name: "list_dir",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path")],
        result: Codec::Buffer("Result[List[Entry], FsError]"),
    },
    HostMethod {
        id: Id::FsReadStat,
        module: "hd:FsRead",
        name: "stat",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path")],
        result: Codec::Buffer("Result[Entry?, FsError]"),
    },
    HostMethod {
        id: Id::FsWriteWriteBytes,
        module: "hd:FsWrite",
        name: "write_bytes",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path"), Codec::Buffer("List[u8]")],
        result: Codec::Buffer("Result[void, FsError]"),
    },
    HostMethod {
        id: Id::FsWriteWriteText,
        module: "hd:FsWrite",
        name: "write_text",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path"), Codec::Buffer("string")],
        result: Codec::Buffer("Result[void, FsError]"),
    },
    HostMethod {
        id: Id::FsWriteAppendText,
        module: "hd:FsWrite",
        name: "append_text",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path"), Codec::Buffer("string")],
        result: Codec::Buffer("Result[void, FsError]"),
    },
    HostMethod {
        id: Id::FsWriteCreateDirAll,
        module: "hd:FsWrite",
        name: "create_dir_all",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path")],
        result: Codec::Buffer("Result[void, FsError]"),
    },
    HostMethod {
        id: Id::FsWriteRemove,
        module: "hd:FsWrite",
        name: "remove",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path")],
        result: Codec::Buffer("Result[void, FsError]"),
    },
    HostMethod {
        id: Id::FsWriteRename,
        module: "hd:FsWrite",
        name: "rename",
        wait: Wait::May,
        resource: Some(ResourceArg::Path),
        params: &[Codec::Buffer("Path"), Codec::Buffer("Path")],
        result: Codec::Buffer("Result[void, FsError]"),
    },
    HostMethod {
        id: Id::RandomNextU64,
        module: "hd:Random",
        name: "next_u64",
        wait: Wait::Never,
        resource: None,
        params: &[],
        result: Codec::Scalar(Scalar::I64),
    },
    HostMethod {
        id: Id::RandomFill,
        module: "hd:Random",
        name: "fill",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("List[u8]"),
    },
    HostMethod {
        id: Id::HttpSend,
        module: "hd:Http",
        name: "send",
        wait: Wait::May,
        resource: Some(ResourceArg::Host),
        params: &[Codec::Buffer("Request")],
        result: Codec::Buffer("Result[Response, HttpError]"),
    },
    HostMethod {
        id: Id::NetLookup,
        module: "hd:Net",
        name: "lookup",
        wait: Wait::May,
        resource: Some(ResourceArg::Host),
        params: &[Codec::Buffer("string")],
        result: Codec::Buffer("Result[List[string], NetError]"),
    },
    HostMethod {
        id: Id::NetConnect,
        module: "hd:Net",
        name: "connect",
        wait: Wait::May,
        resource: Some(ResourceArg::Addr),
        params: &[Codec::Buffer("string"), Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[TcpStream, NetError]"),
    },
    HostMethod {
        id: Id::NetConnectTimeout,
        module: "hd:Net",
        name: "connect_timeout",
        wait: Wait::May,
        resource: Some(ResourceArg::Addr),
        params: &[
            Codec::Buffer("string"),
            Codec::Scalar(Scalar::I32),
            Codec::Scalar(Scalar::I64),
        ],
        result: Codec::Buffer("Result[TcpStream, NetError]"),
    },
    HostMethod {
        id: Id::NetListen,
        module: "hd:Net",
        name: "listen",
        wait: Wait::May,
        resource: Some(ResourceArg::Addr),
        params: &[Codec::Buffer("string"), Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[TcpListener, NetError]"),
    },
    HostMethod {
        id: Id::NetBindUdp,
        module: "hd:Net",
        name: "bind_udp",
        wait: Wait::May,
        resource: Some(ResourceArg::Addr),
        params: &[Codec::Buffer("string"), Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[UdpSocket, NetError]"),
    },
    HostMethod {
        id: Id::ProcessRun,
        module: "hd:Process",
        name: "run",
        wait: Wait::May,
        resource: Some(ResourceArg::Program),
        params: &[
            Codec::Buffer("string"),
            Codec::Buffer("List[string]"),
            Codec::Buffer("string"),
        ],
        result: Codec::Buffer("Result[ProcessOutput, ProcessError]"),
    },
    HostMethod {
        id: Id::SysOs,
        module: "hd:Sys",
        name: "os",
        wait: Wait::Never,
        resource: Some(ResourceArg::SysName),
        params: &[],
        result: Codec::Buffer("Result[string, SysError]"),
    },
    HostMethod {
        id: Id::SysArch,
        module: "hd:Sys",
        name: "arch",
        wait: Wait::Never,
        resource: Some(ResourceArg::SysName),
        params: &[],
        result: Codec::Buffer("Result[string, SysError]"),
    },
    HostMethod {
        id: Id::SysHostname,
        module: "hd:Sys",
        name: "hostname",
        wait: Wait::Never,
        resource: Some(ResourceArg::SysName),
        params: &[],
        result: Codec::Buffer("Result[string, SysError]"),
    },
    HostMethod {
        id: Id::SysCpuCount,
        module: "hd:Sys",
        name: "cpu_count",
        wait: Wait::Never,
        resource: Some(ResourceArg::SysName),
        params: &[],
        result: Codec::Buffer("Result[u32, SysError]"),
    },
    HostMethod {
        id: Id::EnvGet,
        module: "hd:Env",
        name: "get",
        wait: Wait::Never,
        resource: Some(ResourceArg::EnvName),
        params: &[Codec::Buffer("string")],
        result: Codec::Buffer("string?"),
    },
    HostMethod {
        id: Id::EnvNames,
        module: "hd:Env",
        name: "names",
        wait: Wait::Never,
        resource: None,
        params: &[],
        result: Codec::Buffer("List[string]"),
    },
    HostMethod {
        id: Id::ArgsProgram,
        module: "hd:Args",
        name: "program",
        wait: Wait::Never,
        resource: None,
        params: &[],
        result: Codec::Buffer("string"),
    },
    HostMethod {
        id: Id::ArgsList,
        module: "hd:Args",
        name: "list",
        wait: Wait::Never,
        resource: None,
        params: &[],
        result: Codec::Buffer("List[string]"),
    },
    HostMethod {
        id: Id::TestRunnerRow,
        module: "hd:TestRunner",
        name: "row",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Scalar(Scalar::I32),
    },
    HostMethod {
        id: Id::TestRunnerReportTimeout,
        module: "hd:TestRunner",
        name: "report_timeout",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I64)],
        result: Codec::Void,
    },
    HostMethod {
        id: Id::TestRunnerSnapshotCheck,
        module: "hd:TestRunner",
        name: "snapshot_check",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Buffer("string")],
        result: Codec::Buffer("string"),
    },
    HostMethod {
        id: Id::TestRunnerTempDir,
        module: "hd:TestRunner",
        name: "temp_dir",
        wait: Wait::Never,
        resource: None,
        params: &[],
        result: Codec::Buffer("string"),
    },
    HostMethod {
        id: Id::PropertyRunnerStart,
        module: "hd:PropertyRunner",
        name: "start",
        wait: Wait::Never,
        resource: None,
        params: &[
            Codec::Scalar(Scalar::I32),
            Codec::Scalar(Scalar::I32),
            Codec::Scalar(Scalar::I32),
        ],
        result: Codec::Buffer("PropertyCase"),
    },
    HostMethod {
        id: Id::PropertyRunnerRecord,
        module: "hd:PropertyRunner",
        name: "record",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I64)],
        result: Codec::Void,
    },
    HostMethod {
        id: Id::PropertyRunnerShow,
        module: "hd:PropertyRunner",
        name: "show",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Buffer("string")],
        result: Codec::Void,
    },
    HostMethod {
        id: Id::NetTcpStreamRead,
        module: "hd:Net",
        name: "TcpStream.read",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32), Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[List[u8], NetError]"),
    },
    HostMethod {
        id: Id::NetTcpStreamWrite,
        module: "hd:Net",
        name: "TcpStream.write",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32), Codec::Buffer("List[u8]")],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetTcpStreamSetTimeout,
        module: "hd:Net",
        name: "TcpStream.set_timeout",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32), Codec::Buffer("Duration?")],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetTcpStreamSetNodelay,
        module: "hd:Net",
        name: "TcpStream.set_nodelay",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32), Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetTcpStreamSetKeepalive,
        module: "hd:Net",
        name: "TcpStream.set_keepalive",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32), Codec::Buffer("Duration?")],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetTcpStreamClose,
        module: "hd:Net",
        name: "TcpStream.close",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetTcpStreamDispose,
        module: "hd:Net",
        name: "TcpStream.dispose",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Void,
    },
    HostMethod {
        id: Id::NetTcpListenerAccept,
        module: "hd:Net",
        name: "TcpListener.accept",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[TcpStream, NetError]"),
    },
    HostMethod {
        id: Id::NetTcpListenerSetTimeout,
        module: "hd:Net",
        name: "TcpListener.set_timeout",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32), Codec::Buffer("Duration?")],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetTcpListenerLocalAddress,
        module: "hd:Net",
        name: "TcpListener.local_address",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[(string, u16), NetError]"),
    },
    HostMethod {
        id: Id::NetTcpListenerClose,
        module: "hd:Net",
        name: "TcpListener.close",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetTcpListenerDispose,
        module: "hd:Net",
        name: "TcpListener.dispose",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Void,
    },
    HostMethod {
        id: Id::NetUdpSocketSendTo,
        module: "hd:Net",
        name: "UdpSocket.send_to",
        wait: Wait::May,
        resource: Some(ResourceArg::Addr),
        params: &[
            Codec::Scalar(Scalar::I32),
            Codec::Buffer("string"),
            Codec::Scalar(Scalar::I32),
            Codec::Buffer("List[u8]"),
        ],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetUdpSocketReceive,
        module: "hd:Net",
        name: "UdpSocket.receive",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32), Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[Datagram, NetError]"),
    },
    HostMethod {
        id: Id::NetUdpSocketSetTimeout,
        module: "hd:Net",
        name: "UdpSocket.set_timeout",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32), Codec::Buffer("Duration?")],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetUdpSocketLocalAddress,
        module: "hd:Net",
        name: "UdpSocket.local_address",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[(string, u16), NetError]"),
    },
    HostMethod {
        id: Id::NetUdpSocketClose,
        module: "hd:Net",
        name: "UdpSocket.close",
        wait: Wait::Never,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Buffer("Result[void, NetError]"),
    },
    HostMethod {
        id: Id::NetUdpSocketDispose,
        module: "hd:Net",
        name: "UdpSocket.dispose",
        wait: Wait::May,
        resource: None,
        params: &[Codec::Scalar(Scalar::I32)],
        result: Codec::Void,
    },
];
