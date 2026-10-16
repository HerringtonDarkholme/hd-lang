// The host ABI's JS codecs and method descriptors, generated from the
// ABI model (host/gen/abi.json) by crates/hd_host_abi/tests/abi_model.rs.
// Do not edit: `HD_UPDATE_ABI=1 cargo test -p hd_host_abi --test
// abi_model` rewrites it. host/wire.mjs says how each value crosses.
import {
  bytes,
  data,
  handle,
  list,
  newtype,
  option,
  result,
  signed,
  signed64,
  str,
  tuple,
  unit,
  unsigned,
  unsigned64,
  variants,
} from "../wire.mjs";

// Each std type a host method crosses, from its declaration in lib/std.
export const ConsoleError = variants({
  Closed: {},
});
export const Datagram = data({
  bytes: bytes,
  host: str,
  port: unsigned,
});
export const Duration = data({
  millis: signed64,
});
export const Path = newtype(str);
export const EntryKind = variants({
  File: {},
  Directory: {},
  Symlink: {},
});
export const Entry = data({
  path: Path,
  kind: EntryKind,
  size: unsigned64,
});
export const FsError = variants({
  NotFound: { path: Path },
  PermissionDenied: { path: Path },
  NotGranted: { path: Path },
  AlreadyExists: { path: Path },
  NotADirectory: { path: Path },
  IsADirectory: { path: Path },
  InvalidUtf8: { path: Path },
  Other: { message: str },
});
export const HttpError = variants({
  NotGranted: { host: str },
  InvalidUrl: { url: str },
  Dns: { host: str },
  Connect: { message: str },
  Tls: { message: str },
  Timeout: {},
  TooManyRedirects: { url: str },
  Other: { message: str },
});
export const Method = variants({
  Get: {},
  Head: {},
  Post: {},
  Put: {},
  Patch: {},
  Delete: {},
  Options: {},
  Other: { name: str },
});
export const NetError = variants({
  NotGranted: { address: str },
  InvalidAddress: { address: str },
  Dns: { host: str },
  Refused: { address: str },
  Timeout: {},
  Other: { message: str },
});
export const ProcessError = variants({
  NotFound: {},
  PermissionDenied: {},
  NotGranted: {},
  Other: { message: str },
});
export const ProcessOutput = data({
  stdout: str,
  stderr: str,
  status: signed,
});
export const PropertyCase = data({
  example: option(unsigned),
  seed: signed64,
  size: signed,
  replay: list(signed64),
});
export const Request = data({
  method: Method,
  url: str,
  headers: list(tuple(str, str)),
  body: bytes,
  timeout: option(Duration),
});
export const ResourceError = (E) =>
  variants({
    Operation: { error: E },
    Disposed: {},
  });
export const Response = data({
  status: unsigned,
  headers: list(tuple(str, str)),
  body: bytes,
});
export const SysError = variants({
  NotGranted: { name: str },
  Unsupported: { name: str },
});
export const TcpListener = handle;
export const TcpStream = handle;
export const UdpSocket = handle;

// Each host method by import module and name (crates/hd_run_wasmtime/DESIGN.md
// §17.2): `wait` is "never" for one import or "may" for `.start` and
// `.finish`; `resource` names the argument a grant checks; a scalar
// parameter or result is its Wasm type, any other a codec over the
// exchange buffer.
export const METHODS = {
  "hd:Console": {
    write_line: {
      wait: "may",
      resource: null,
      params: [str],
      result: result(unit, ConsoleError),
    },
  },
  "hd:ConsoleInput": {
    read_line: {
      wait: "may",
      resource: null,
      params: [],
      result: result(option(str), ConsoleError),
    },
  },
  "hd:Clock": {
    now: {
      wait: "never",
      resource: null,
      params: [],
      result: "i64",
    },
    monotonic: {
      wait: "never",
      resource: null,
      params: [],
      result: "i64",
    },
    sleep: {
      wait: "may",
      resource: null,
      params: ["i64"],
      result: "void",
    },
  },
  "hd:FsRead": {
    read_bytes: {
      wait: "may",
      resource: "path",
      params: [Path],
      result: result(bytes, FsError),
    },
    read_text: {
      wait: "may",
      resource: "path",
      params: [Path],
      result: result(str, FsError),
    },
    list_dir: {
      wait: "may",
      resource: "path",
      params: [Path],
      result: result(list(Entry), FsError),
    },
    stat: {
      wait: "may",
      resource: "path",
      params: [Path],
      result: result(option(Entry), FsError),
    },
  },
  "hd:FsWrite": {
    write_bytes: {
      wait: "may",
      resource: "path",
      params: [Path, bytes],
      result: result(unit, FsError),
    },
    write_text: {
      wait: "may",
      resource: "path",
      params: [Path, str],
      result: result(unit, FsError),
    },
    append_text: {
      wait: "may",
      resource: "path",
      params: [Path, str],
      result: result(unit, FsError),
    },
    create_dir_all: {
      wait: "may",
      resource: "path",
      params: [Path],
      result: result(unit, FsError),
    },
    remove: {
      wait: "may",
      resource: "path",
      params: [Path],
      result: result(unit, FsError),
    },
    rename: {
      wait: "may",
      resource: "path",
      params: [Path, Path],
      result: result(unit, FsError),
    },
  },
  "hd:Random": {
    next_u64: {
      wait: "never",
      resource: null,
      params: [],
      result: "i64",
    },
    fill: {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: bytes,
    },
  },
  "hd:Http": {
    send: {
      wait: "may",
      resource: "host",
      params: [Request],
      result: result(Response, HttpError),
    },
  },
  "hd:Net": {
    lookup: {
      wait: "may",
      resource: "host",
      params: [str],
      result: result(list(str), NetError),
    },
    connect: {
      wait: "may",
      resource: "addr",
      params: [str, "i32"],
      result: result(TcpStream, NetError),
    },
    connect_timeout: {
      wait: "may",
      resource: "addr",
      params: [str, "i32", "i64"],
      result: result(TcpStream, NetError),
    },
    listen: {
      wait: "may",
      resource: "addr",
      params: [str, "i32"],
      result: result(TcpListener, NetError),
    },
    bind_udp: {
      wait: "may",
      resource: "addr",
      params: [str, "i32"],
      result: result(UdpSocket, NetError),
    },
    "TcpStream.read": {
      wait: "may",
      resource: null,
      params: ["i32", "i32"],
      result: result(bytes, ResourceError(NetError)),
    },
    "TcpStream.write": {
      wait: "may",
      resource: null,
      params: ["i32", bytes],
      result: result(unit, ResourceError(NetError)),
    },
    "TcpStream.set_timeout": {
      wait: "never",
      resource: null,
      params: ["i32", option(Duration)],
      result: result(unit, ResourceError(NetError)),
    },
    "TcpStream.close": {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: result(unit, ResourceError(NetError)),
    },
    "TcpListener.accept": {
      wait: "may",
      resource: null,
      params: ["i32"],
      result: result(TcpStream, ResourceError(NetError)),
    },
    "TcpListener.set_timeout": {
      wait: "never",
      resource: null,
      params: ["i32", option(Duration)],
      result: result(unit, ResourceError(NetError)),
    },
    "TcpListener.close": {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: result(unit, ResourceError(NetError)),
    },
    "UdpSocket.send_to": {
      wait: "may",
      resource: "addr",
      params: ["i32", str, "i32", bytes],
      result: result(unit, ResourceError(NetError)),
    },
    "UdpSocket.receive": {
      wait: "may",
      resource: null,
      params: ["i32", "i32"],
      result: result(Datagram, ResourceError(NetError)),
    },
    "UdpSocket.set_timeout": {
      wait: "never",
      resource: null,
      params: ["i32", option(Duration)],
      result: result(unit, ResourceError(NetError)),
    },
    "UdpSocket.close": {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: result(unit, ResourceError(NetError)),
    },
  },
  "hd:Process": {
    run: {
      wait: "may",
      resource: "program",
      params: [str, list(str), str],
      result: result(ProcessOutput, ProcessError),
    },
  },
  "hd:Sys": {
    os: {
      wait: "never",
      resource: "sys-name",
      params: [],
      result: result(str, SysError),
    },
    arch: {
      wait: "never",
      resource: "sys-name",
      params: [],
      result: result(str, SysError),
    },
    hostname: {
      wait: "never",
      resource: "sys-name",
      params: [],
      result: result(str, SysError),
    },
    cpu_count: {
      wait: "never",
      resource: "sys-name",
      params: [],
      result: result(unsigned, SysError),
    },
  },
  "hd:Env": {
    get: {
      wait: "never",
      resource: "env-name",
      params: [str],
      result: option(str),
    },
    names: {
      wait: "never",
      resource: null,
      params: [],
      result: list(str),
    },
  },
  "hd:Args": {
    program: {
      wait: "never",
      resource: null,
      params: [],
      result: str,
    },
    list: {
      wait: "never",
      resource: null,
      params: [],
      result: list(str),
    },
  },
  "hd:TestRunner": {
    row: {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: "i32",
    },
    report_timeout: {
      wait: "never",
      resource: null,
      params: ["i64"],
      result: "void",
    },
    snapshot_check: {
      wait: "never",
      resource: null,
      params: [str],
      result: str,
    },
    temp_dir: {
      wait: "never",
      resource: null,
      params: [],
      result: str,
    },
  },
  "hd:PropertyRunner": {
    start: {
      wait: "never",
      resource: null,
      params: ["i32", "i32", "i32"],
      result: PropertyCase,
    },
    record: {
      wait: "never",
      resource: null,
      params: ["i64"],
      result: "void",
    },
    show: {
      wait: "never",
      resource: null,
      params: [str],
      result: "void",
    },
  },
};
