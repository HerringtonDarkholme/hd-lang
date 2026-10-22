// The host ABI's JS codecs and method descriptors, generated from the
// ABI model (host/gen/abi.json) by crates/hd_host_abi/tests/abi_model.rs.
// Do not edit: `HD_BLESS=1 cargo test -p hd_host_abi --test
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
export const Path = newtype(str);
export const Command = data({
  program: str,
  args: list(str),
  env: list(tuple(str, str)),
  current_dir: option(Path),
  stdin: str,
});
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
export const EntryKind = variants({
  File: {},
  Directory: {},
  Symlink: {},
});
export const Timestamp = data({
  millis: signed64,
});
export const Entry = data({
  path: Path,
  kind: EntryKind,
  size: unsigned64,
  modified: option(Timestamp),
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
export const HostError = variants({
  NotGranted: { subject: str },
  Unsupported: { name: str },
  Other: { message: str },
  InvalidValue: { name: str, reason: str },
});
export const Response = data({
  status: unsigned,
  headers: list(tuple(str, str)),
  body: bytes,
  url: str,
});
export const JsonError = variants({
  UnexpectedEnd: { position: unsigned },
  UnexpectedCharacter: { position: unsigned },
  InvalidEscape: { position: unsigned },
  LoneSurrogate: { position: unsigned },
  ControlCharacter: { position: unsigned },
  NumberOutOfRange: { position: unsigned },
  NestingTooDeep: { position: unsigned },
  WrongType: { path: str, expected: str },
  MissingField: { path: str },
  UnknownVariant: { path: str, name: str },
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
  Status: { response: Response },
  Json: { error: JsonError },
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
  Closed: {},
  Other: { message: str },
});
export const ProcessOutput = data({
  stdout: str,
  stderr: str,
  status: signed,
  program: str,
});
export const ProcessError = variants({
  NotFound: { program: str },
  PermissionDenied: { program: str },
  NotGranted: {},
  Other: { message: str },
  Exited: { output: ProcessOutput },
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
  query: list(tuple(str, str)),
  headers: list(tuple(str, str)),
  body: bytes,
  redirects: unsigned,
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
    write_error_line: {
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
    read_all: {
      wait: "may",
      resource: null,
      params: [],
      result: result(str, ConsoleError),
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
    copy: {
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
      result: result(bytes, NetError),
    },
    "TcpStream.write": {
      wait: "may",
      resource: null,
      params: ["i32", bytes],
      result: result(unit, NetError),
    },
    "TcpStream.set_timeout": {
      wait: "never",
      resource: null,
      params: ["i32", option(Duration)],
      result: result(unit, NetError),
    },
    "TcpStream.set_nodelay": {
      wait: "never",
      resource: null,
      params: ["i32", "i32"],
      result: result(unit, NetError),
    },
    "TcpStream.set_keepalive": {
      wait: "never",
      resource: null,
      params: ["i32", option(Duration)],
      result: result(unit, NetError),
    },
    "TcpStream.close": {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: result(unit, NetError),
    },
    "TcpStream.dispose": {
      wait: "may",
      resource: null,
      params: ["i32"],
      result: "void",
    },
    "TcpListener.accept": {
      wait: "may",
      resource: null,
      params: ["i32"],
      result: result(TcpStream, NetError),
    },
    "TcpListener.set_timeout": {
      wait: "never",
      resource: null,
      params: ["i32", option(Duration)],
      result: result(unit, NetError),
    },
    "TcpListener.local_address": {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: result(tuple(str, unsigned), NetError),
    },
    "TcpListener.close": {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: result(unit, NetError),
    },
    "TcpListener.dispose": {
      wait: "may",
      resource: null,
      params: ["i32"],
      result: "void",
    },
    "UdpSocket.send_to": {
      wait: "may",
      resource: "addr",
      params: ["i32", str, "i32", bytes],
      result: result(unit, NetError),
    },
    "UdpSocket.receive": {
      wait: "may",
      resource: null,
      params: ["i32", "i32"],
      result: result(Datagram, NetError),
    },
    "UdpSocket.set_timeout": {
      wait: "never",
      resource: null,
      params: ["i32", option(Duration)],
      result: result(unit, NetError),
    },
    "UdpSocket.local_address": {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: result(tuple(str, unsigned), NetError),
    },
    "UdpSocket.close": {
      wait: "never",
      resource: null,
      params: ["i32"],
      result: result(unit, NetError),
    },
    "UdpSocket.dispose": {
      wait: "may",
      resource: null,
      params: ["i32"],
      result: "void",
    },
  },
  "hd:Process": {
    run: {
      wait: "may",
      resource: "program",
      params: [Command],
      result: result(ProcessOutput, ProcessError),
    },
  },
  "hd:Sys": {
    os: {
      wait: "never",
      resource: "sys-name",
      params: [],
      result: result(str, HostError),
    },
    arch: {
      wait: "never",
      resource: "sys-name",
      params: [],
      result: result(str, HostError),
    },
    hostname: {
      wait: "never",
      resource: "sys-name",
      params: [],
      result: result(str, HostError),
    },
    cpu_count: {
      wait: "never",
      resource: "sys-name",
      params: [],
      result: result(unsigned, HostError),
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
