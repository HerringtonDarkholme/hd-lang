// The execution journal of a REPL session in the JS host, byte for byte as
// `hd_run` writes and reads it (crates/hd_run/src/journal.rs and
// replay.rs; docs/future-work/live-execution.md §4 and §5), so a journal the wasmtime
// session recorded replays here and one recorded here replays there.
//
// `journal = Header Row*`; `Row = kind: u8, index: uleb, len: uleb,
// payload, crc32: u32 LE`. The payload of each kind, the method naming of
// `Call` rows, the request (scalar parameters as uleb of their two's
// complement bits or 8 float bytes, then the exchange buffer bytes the call
// read, kept when 8 bytes or fewer and else as their digest) and the reply
// (a tag byte, the value, result bytes) are journal.rs's.
//
// Hashes are xxh3-128 with the default secret, as `hd_base::hash128`: an
// `Input` row holds the hash of the input's source text and of its module,
// and a request digest is the low half of the hash of the `StableHasher`
// tag `req` (its length as a u32, then its bytes) followed by the request.
//
// Values: integers that may pass 2^53 are bigints. A reply value is
// `{ tag: 0 }` (void), `{ tag: 1, v }` (an i64), `{ tag: 2, v }` (the bits
// of an f64) or `{ tag: 3, v, w }` (a status and a value, i32s). An end is
// `{ kind: "exit", status }`, or `{ kind }` with `trapped`, `stopped`,
// `cancelled` or `failed`.

export type Value =
  | { tag: 0 }
  | { tag: 1; v: bigint }
  | { tag: 2; v: bigint }
  | { tag: 3; v: number; w: number };

export type End =
  | { kind: "exit"; status: number }
  | { kind: "trapped" | "stopped" | "cancelled" | "failed"; status?: undefined };

export type Request = { args: Uint8Array } | { len: number; digest: bigint };

export type Event =
  | { kind: "header"; format: bigint; execution: Uint8Array }
  | { kind: "input"; number: number; source: bigint; module: bigint }
  | {
      kind: "call";
      module: string;
      name: string;
      request: Request;
      value: Value;
      result: Uint8Array | null;
    }
  | { kind: "finish"; handle: number; value: Value; result: Uint8Array | null }
  | { kind: "wake"; poll: bigint; handles: number[] }
  | { kind: "block"; handles: number[] }
  | { kind: "abort"; handle: number }
  | { kind: "cancel"; poll: bigint }
  | { kind: "end"; end: End };

export interface Row {
  kind: number;
  index: number;
  payload: Uint8Array;
}

export interface RecordedInput {
  number: number;
  source: bigint;
  module: bigint;
  events: Event[];
  end: End | null;
}

export interface ImportType {
  params: string[];
  results: string[];
}

const M64 = (1n << 64n) - 1n;
const utf8 = new TextEncoder();

// --- xxh3-128 (the scalar reference, seed 0) -----------------------------

const P32_1 = 0x9e3779b1n;
const P32_2 = 0x85ebca77n;
const P32_3 = 0xc2b2ae3dn;
const P64_1 = 0x9e3779b185ebca87n;
const P64_2 = 0xc2b2ae3d27d4eb4fn;
const P64_3 = 0x165667b19e3779f9n;
const P64_4 = 0x85ebca77c2b2ae63n;
const P64_5 = 0x27d4eb2f165667c5n;

// prettier-ignore
const SECRET = Uint8Array.of(
  0xb8, 0xfe, 0x6c, 0x39, 0x23, 0xa4, 0x4b, 0xbe, 0x7c, 0x01, 0x81, 0x2c, 0xf7, 0x21, 0xad, 0x1c,
  0xde, 0xd4, 0x6d, 0xe9, 0x83, 0x90, 0x97, 0xdb, 0x72, 0x40, 0xa4, 0xa4, 0xb7, 0xb3, 0x67, 0x1f,
  0xcb, 0x79, 0xe6, 0x4e, 0xcc, 0xc0, 0xe5, 0x78, 0x82, 0x5a, 0xd0, 0x7d, 0xcc, 0xff, 0x72, 0x21,
  0xb8, 0x08, 0x46, 0x74, 0xf7, 0x43, 0x24, 0x8e, 0xe0, 0x35, 0x90, 0xe6, 0x81, 0x3a, 0x26, 0x4c,
  0x3c, 0x28, 0x52, 0xbb, 0x91, 0xc3, 0x00, 0xcb, 0x88, 0xd0, 0x65, 0x8b, 0x1b, 0x53, 0x2e, 0xa3,
  0x71, 0x64, 0x48, 0x97, 0xa2, 0x0d, 0xf9, 0x4e, 0x38, 0x19, 0xef, 0x46, 0xa9, 0xde, 0xac, 0xd8,
  0xa8, 0xfa, 0x76, 0x3f, 0xe3, 0x9c, 0x34, 0x3f, 0xf9, 0xdc, 0xbb, 0xc7, 0xc7, 0x0b, 0x4f, 0x1d,
  0x8a, 0x51, 0xe0, 0x4b, 0xcd, 0xb4, 0x59, 0x31, 0xc8, 0x9f, 0x7e, 0xc9, 0xd9, 0x78, 0x73, 0x64,
  0xea, 0xc5, 0xac, 0x83, 0x34, 0xd3, 0xeb, 0xc3, 0xc5, 0x81, 0xa0, 0xff, 0xfa, 0x13, 0x63, 0xeb,
  0x17, 0x0d, 0xdd, 0x51, 0xb7, 0xf0, 0xda, 0x49, 0xd3, 0x16, 0x55, 0x26, 0x29, 0xd4, 0x68, 0x9e,
  0x2b, 0x16, 0xbe, 0x58, 0x7d, 0x47, 0xa1, 0xfc, 0x8f, 0xf8, 0xb8, 0xd1, 0x7a, 0xd0, 0x31, 0xce,
  0x45, 0xcb, 0x3a, 0x8f, 0x95, 0x16, 0x04, 0x28, 0xaf, 0xd7, 0xfb, 0xca, 0xbb, 0x4b, 0x40, 0x7e,
);
const SV = new DataView(SECRET.buffer);

const add = (a: bigint, b: bigint): bigint => (a + b) & M64;
const mul = (a: bigint, b: bigint): bigint => (a * b) & M64;
const neg = (a: bigint): bigint => -a & M64;
const xorshift = (v: bigint, s: bigint): bigint => v ^ (v >> s);
const mul128 = (a: bigint, b: bigint): [bigint, bigint] => {
  const p = a * b;
  return [p & M64, p >> 64n];
};
const fold = (a: bigint, b: bigint): bigint => {
  const p = a * b;
  return (p & M64) ^ (p >> 64n);
};
const bswap64 = (v: bigint): bigint => {
  let r = 0n;
  for (let i = 0; i < 8; i++) {
    r = (r << 8n) | ((v >> BigInt(8 * i)) & 0xffn);
  }
  return r;
};
const bswap32 = (v: number): number =>
  ((v & 0xff) << 24) | ((v & 0xff00) << 8) | ((v >>> 8) & 0xff00) | (v >>> 24);
const avalanche = (v: bigint): bigint => xorshift(mul(xorshift(v, 37n), 0x165667919e3779f9n), 32n);
const avalanche64 = (v: bigint): bigint => {
  v = mul(xorshift(v, 33n), P64_2);
  v = mul(xorshift(v, 29n), P64_3);
  return xorshift(v, 32n);
};
const s64 = (at: number): bigint => SV.getBigUint64(at, true);
const s32 = (at: number): bigint => BigInt(SV.getUint32(at, true));
const join128 = (lo: bigint, hi: bigint): bigint => lo | (hi << 64n);

function short128(b: Uint8Array, dv: DataView): bigint {
  const len = b.length;
  const r64 = (at: number): bigint => dv.getBigUint64(at, true);
  if (len > 8) {
    const lo = r64(0);
    let hi = r64(len - 8);
    let [ml, mh] = mul128(lo ^ hi ^ (s64(32) ^ s64(40)), P64_1);
    ml = add(ml, BigInt(len - 1) << 54n);
    hi ^= s64(48) ^ s64(56);
    mh = add(mh, add(hi, (hi & 0xffffffffn) * (P32_2 - 1n)));
    ml ^= bswap64(mh);
    const [rl, rh] = mul128(ml, P64_2);
    return join128(avalanche(rl), avalanche(add(rh, mul(mh, P64_2))));
  }
  if (len >= 4) {
    const input = BigInt(dv.getUint32(0, true)) + (BigInt(dv.getUint32(len - 4, true)) << 32n);
    let [lo, hi] = mul128(input ^ (s64(16) ^ s64(24)), add(P64_1, BigInt(len) << 2n));
    hi = add(hi, (lo << 1n) & M64);
    lo ^= hi >> 3n;
    lo = xorshift(mul(xorshift(lo, 35n), 0x9fb21c651e98df25n), 28n);
    return join128(lo, avalanche(hi));
  }
  if (len > 0) {
    const lo32 = ((b[0]! << 16) | (b[len >> 1]! << 24) | b[len - 1]! | (len << 8)) >>> 0;
    const swapped = bswap32(lo32) >>> 0;
    const hi32 = ((swapped << 13) | (swapped >>> 19)) >>> 0;
    const lo = avalanche64(BigInt(lo32) ^ (s32(0) ^ s32(4)));
    const hi = avalanche64(BigInt(hi32) ^ (s32(8) ^ s32(12)));
    return join128(lo, hi);
  }
  return join128(avalanche64(s64(64) ^ s64(72)), avalanche64(s64(80) ^ s64(88)));
}

function mid128(b: Uint8Array, dv: DataView): bigint {
  const len = b.length;
  const r64 = (at: number): bigint => dv.getBigUint64(at, true);
  const mix16 = (at: number, sat: number): bigint =>
    fold(r64(at) ^ s64(sat), r64(at + 8) ^ s64(sat + 8));
  let a0 = mul(BigInt(len), P64_1);
  let a1 = 0n;
  const mix32 = (at1: number, at2: number, sat: number): void => {
    a0 = add(a0, mix16(at1, sat)) ^ add(r64(at2), r64(at2 + 8));
    a1 = add(a1, mix16(at2, sat + 16)) ^ add(r64(at1), r64(at1 + 8));
  };
  if (len <= 128) {
    if (len > 32) {
      if (len > 64) {
        if (len > 96) {
          mix32(48, len - 64, 96);
        }
        mix32(32, len - 48, 64);
      }
      mix32(16, len - 32, 32);
    }
    mix32(0, len - 16, 0);
  } else {
    for (let i = 0; i < 4; i++) {
      mix32(32 * i, 32 * i + 16, 32 * i);
    }
    a0 = avalanche(a0);
    a1 = avalanche(a1);
    const rounds = Math.floor(len / 32);
    for (let i = 4; i < rounds; i++) {
      mix32(32 * i, 32 * i + 16, 3 + 32 * (i - 4));
    }
    mix32(len - 16, len - 32, 136 - 17 - 16);
  }
  const lo = add(a0, a1);
  const hi = add(add(mul(a0, P64_1), mul(a1, P64_4)), mul(BigInt(len), P64_2));
  return join128(avalanche(lo), neg(avalanche(hi)));
}

function long128(b: Uint8Array, dv: DataView): bigint {
  const len = b.length;
  const acc = [P32_3, P64_1, P64_2, P64_3, P64_4, P32_2, P64_5, P32_1];
  const stripe = (at: number, sat: number): void => {
    for (let i = 0; i < 8; i++) {
      const data = dv.getBigUint64(at + 8 * i, true);
      const key = data ^ s64(sat + 8 * i);
      acc[i ^ 1] = add(acc[i ^ 1]!, data);
      acc[i] = add(acc[i]!, (key & 0xffffffffn) * (key >> 32n));
    }
  };
  const blockLen = 64 * 16;
  const blocks = Math.floor((len - 1) / blockLen);
  for (let n = 0; n < blocks; n++) {
    for (let s = 0; s < 16; s++) {
      stripe(n * blockLen + 64 * s, 8 * s);
    }
    for (let i = 0; i < 8; i++) {
      acc[i] = mul(xorshift(acc[i]!, 47n) ^ s64(128 + 8 * i), P32_1);
    }
  }
  const stripes = Math.floor((len - 1 - blockLen * blocks) / 64);
  for (let s = 0; s < stripes; s++) {
    stripe(blocks * blockLen + 64 * s, 8 * s);
  }
  stripe(len - 64, 192 - 64 - 7);
  const merge = (sat: number, start: bigint): bigint => {
    let r = start;
    for (let i = 0; i < 4; i++) {
      r = add(r, fold(acc[2 * i]! ^ s64(sat + 16 * i), acc[2 * i + 1]! ^ s64(sat + 16 * i + 8)));
    }
    return avalanche(r);
  };
  return join128(merge(11, mul(BigInt(len), P64_1)), merge(117, ~mul(BigInt(len), P64_2) & M64));
}

// The xxh3-128 hash of `bytes`, as a bigint (`hd_base::hash128`).
export function xxh3_128(bytes: Uint8Array): bigint {
  const dv = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.length <= 16) {
    return short128(bytes, dv);
  }
  if (bytes.length <= 240) {
    return mid128(bytes, dv);
  }
  return long128(bytes, dv);
}

// --- framing ---------------------------------------------------------------

const CRC_TABLE = Array.from({ length: 256 }, (_, i) => {
  let c = i;
  for (let bit = 0; bit < 8; bit++) {
    c = c & 1 ? (c >>> 1) ^ 0xedb88320 : c >>> 1;
  }
  return c >>> 0;
});

// CRC-32 (IEEE), the journal checksum.
export function crc32(bytes: Uint8Array): number {
  let c = 0xffffffff;
  for (const b of bytes) {
    c = CRC_TABLE[(c ^ b) & 0xff]! ^ (c >>> 8);
  }
  return ~c >>> 0;
}

// A growable byte string that rows and payloads are written into.
class Bytes {
  buf: Uint8Array;
  len: number;

  constructor() {
    this.buf = new Uint8Array(256);
    this.len = 0;
  }
  room(n: number): void {
    if (this.len + n <= this.buf.length) {
      return;
    }
    const next = new Uint8Array(Math.max(this.buf.length * 2, this.len + n));
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
  }
  byte(b: number): void {
    this.room(1);
    this.buf[this.len++] = b;
  }
  raw(bytes: Uint8Array): void {
    this.room(bytes.length);
    this.buf.set(bytes, this.len);
    this.len += bytes.length;
  }
  uleb(v: number | bigint): void {
    let x = BigInt(v);
    this.room(10);
    do {
      let b = Number(x & 0x7fn);
      x >>= 7n;
      if (x) {
        b |= 0x80;
      }
      this.buf[this.len++] = b;
    } while (x);
  }
  // A u64 or u128 as `bytes` little-endian bytes.
  le(v: number | bigint, bytes: number): void {
    for (let i = 0; i < bytes; i++) {
      this.byte(Number((BigInt(v) >> BigInt(8 * i)) & 0xffn));
    }
  }
  view(): Uint8Array {
    return this.buf.subarray(0, this.len);
  }
  clear(): void {
    this.len = 0;
  }
}

const ulebLen = (v: number | bigint): number => {
  let n = 1;
  for (let x = BigInt(v) >> 7n; x; x >>= 7n) {
    n++;
  }
  return n;
};

// Row kinds, as journal.rs numbers them.
export const KIND = {
  header: 0,
  input: 1,
  call: 2,
  finish: 3,
  wake: 4,
  block: 5,
  abort: 6,
  cancel: 7,
  end: 8,
} as const;
const KIND_NAMES = Object.keys(KIND) as (keyof typeof KIND)[];

// The format number of a `Header` row.
export const FORMAT = 1;
// A REPL session's journal cap: past it, recording stops.
export const SESSION_CAP = 64 << 20;

function appendRow(out: Bytes, kind: number, index: number, payload: Uint8Array): void {
  const start = out.len;
  out.byte(kind);
  out.uleb(index);
  out.uleb(payload.length);
  out.raw(payload);
  out.le(crc32(out.buf.subarray(start, out.len)), 4);
}

// A reader over a payload; every read answers `undefined` past its end.
class Reader {
  b: Uint8Array;
  at: number;

  constructor(b: Uint8Array) {
    this.b = b;
    this.at = 0;
  }
  uleb(): bigint | undefined {
    let v = 0n;
    for (let shift = 0n; shift < 64n; shift += 7n) {
      const byte = this.b[this.at];
      if (byte === undefined) {
        return undefined;
      }
      this.at++;
      v |= BigInt(byte & 0x7f) << shift;
      if (!(byte & 0x80)) {
        return v;
      }
    }
    return undefined;
  }
  num(): number | undefined {
    const v = this.uleb();
    return v === undefined || v > 0xffffffffn ? undefined : Number(v);
  }
  bytes(n: number | undefined): Uint8Array | undefined {
    if (n === undefined || this.at + n > this.b.length) {
      return undefined;
    }
    const out = this.b.slice(this.at, this.at + n);
    this.at += n;
    return out;
  }
  le(n: number): bigint | undefined {
    const b = this.bytes(n);
    if (b === undefined) {
      return undefined;
    }
    let v = 0n;
    for (let i = n - 1; i >= 0; i--) {
      v = (v << 8n) | BigInt(b[i]!);
    }
    return v;
  }
  text(): string | undefined {
    const b = this.bytes(this.num());
    if (b === undefined) {
      return undefined;
    }
    try {
      return new TextDecoder("utf-8", { fatal: true }).decode(b);
    } catch {
      return undefined;
    }
  }
  handles(): number[] | undefined {
    const n = this.num();
    if (n === undefined) {
      return undefined;
    }
    const out: number[] = [];
    for (let i = 0; i < n; i++) {
      const h = this.num();
      if (h === undefined) {
        return undefined;
      }
      out.push(h);
    }
    return out;
  }
}

// Reads rows until the end or the first torn or corrupt row: the rows,
// `{ kind, index, payload }`, and the length of the valid prefix.
export function readRows(b: Uint8Array): { rows: Row[]; valid: number } {
  const rows: Row[] = [];
  let at = 0;
  for (;;) {
    const start = at;
    if (at >= b.length) {
      return { rows, valid: start };
    }
    const r = new Reader(b);
    r.at = at + 1;
    const kind = b[at]!;
    const index = r.uleb();
    const len = r.num();
    const payload = r.bytes(len);
    const crc = r.le(4);
    if (
      kind >= KIND_NAMES.length ||
      payload === undefined ||
      crc === undefined ||
      Number(crc) !== crc32(b.subarray(start, r.at - 4)) ||
      index !== BigInt(rows.length)
    ) {
      return { rows, valid: start };
    }
    rows.push({ kind, index: Number(index), payload });
    at = r.at;
  }
}

// The digest of a request's argument bytes (`journal::request_digest`).
export function requestDigest(args: Uint8Array): bigint {
  const tagged = new Uint8Array(7 + args.length);
  tagged.set([3, 0, 0, 0, 0x72, 0x65, 0x71]);
  tagged.set(args, 7);
  return xxh3_128(tagged) & M64;
}

// The request a `Call` row holds for the arguments `args`.
export const requestOf = (args: Uint8Array): Request =>
  args.length <= 8
    ? { args: Uint8Array.from(args) }
    : { len: args.length, digest: requestDigest(args) };

const sameRequest = (a: Request, b: Request): boolean =>
  "args" in a && "args" in b
    ? a.args.length === b.args.length && a.args.every((x, i) => x === b.args[i])
    : "len" in a && "len" in b && a.len === b.len && a.digest === b.digest;

// Appends an integer parameter to a call's arguments.
export function putIntArg(out: Bytes, v: number | bigint): void {
  out.uleb(BigInt.asUintN(64, BigInt(v)));
}

// A host call's arguments as a journal holds them: each scalar parameter
// in `values` by its type in `types` (integers as `putIntArg`, floats as
// their IEEE 754 bits, an `f32` widened to 8 bytes), then `tail`, the
// exchange buffer bytes the call reads, when not null.
export function callArgs(
  values: readonly (number | bigint)[],
  types: readonly string[],
  tail: Uint8Array | null,
): Uint8Array {
  const out = new Bytes();
  const dv = new DataView(new ArrayBuffer(8));
  values.forEach((v, i) => {
    const type = types[i];
    if (type === "i32" || type === "i64") {
      putIntArg(out, v);
    } else if (type === "f64") {
      dv.setFloat64(0, Number(v), true);
      out.raw(new Uint8Array(dv.buffer));
    } else if (type === "f32") {
      dv.setFloat32(0, Number(v), true);
      out.le(BigInt(dv.getUint32(0, true)), 8);
    }
  });
  if (tail) {
    out.raw(tail);
  }
  return out.view().slice();
}

function putReply(out: Bytes, value: Value, result: Uint8Array | null): void {
  out.byte(value.tag | (result ? 4 : 0));
  if (value.tag === 1) {
    out.uleb(BigInt.asUintN(64, BigInt(value.v)));
  } else if (value.tag === 2) {
    out.le(value.v, 8);
  } else if (value.tag === 3) {
    out.uleb(value.v >>> 0);
    out.uleb(value.w >>> 0);
  }
  if (result) {
    out.uleb(result.length);
    out.raw(result);
  }
}

function getReply(r: Reader): { value: Value; result: Uint8Array | null } | undefined {
  const tag = r.bytes(1)?.[0];
  if (tag === undefined) {
    return undefined;
  }
  const low = (v: bigint | undefined): number | undefined =>
    v === undefined ? undefined : Number(BigInt.asIntN(32, v));
  let value: Value;
  switch (tag & 3) {
    case 0:
      value = { tag: 0 };
      break;
    case 1: {
      const v = r.uleb();
      if (v === undefined) {
        return undefined;
      }
      value = { tag: 1, v: BigInt.asIntN(64, v) };
      break;
    }
    case 2: {
      const v = r.le(8);
      if (v === undefined) {
        return undefined;
      }
      value = { tag: 2, v };
      break;
    }
    default: {
      const v = low(r.uleb());
      const w = low(r.uleb());
      if (v === undefined || w === undefined) {
        return undefined;
      }
      value = { tag: 3, v, w };
    }
  }
  let result: Uint8Array | null = null;
  if (tag >> 2 === 1) {
    const bytes = r.bytes(r.num());
    if (bytes === undefined) {
      return undefined;
    }
    result = bytes;
  } else if (tag >> 2 !== 0) {
    return undefined;
  }
  return { value, result };
}

const END_KINDS: End["kind"][] = ["exit", "trapped", "stopped", "cancelled", "failed"];

function getRequest(r: Reader, len: bigint): Request | undefined {
  if (len <= 8n) {
    const args = r.bytes(Number(len));
    return args === undefined ? undefined : { args };
  }
  const digest = r.le(8);
  return digest === undefined ? undefined : { len: Number(len), digest };
}

function decodeCall(
  r: Reader,
  methods: (string | undefined)[][],
): Record<string, unknown> | undefined {
  const method = r.num();
  if (method === methods.length) {
    methods.push([r.text(), r.text()]);
  }
  if (method === undefined) {
    return undefined;
  }
  const named = methods[method];
  if (!named || named.includes(undefined)) {
    return undefined;
  }
  const len = r.uleb();
  if (len === undefined) {
    return undefined;
  }
  const request = getRequest(r, len);
  const reply = getReply(r);
  if (!request || !reply) {
    return undefined;
  }
  return { kind: "call", module: named[0]!, name: named[1]!, request, ...reply };
}

function decodeFinish(r: Reader): Record<string, unknown> | undefined {
  const handle = r.num();
  const reply = getReply(r);
  return reply ? { kind: "finish", handle, ...reply } : undefined;
}

function decodeEnd(r: Reader): Record<string, unknown> | undefined {
  const tag = r.bytes(1)?.[0];
  if (tag === undefined || tag >= END_KINDS.length) {
    return undefined;
  }
  if (tag === 0) {
    const status = r.bytes(1)?.[0];
    return status === undefined ? undefined : { kind: "end", end: { kind: "exit", status } };
  }
  return {
    kind: "end",
    end: { kind: END_KINDS[tag]! as Exclude<End["kind"], "exit"> },
  };
}

function decodeRow(row: Row, methods: (string | undefined)[][]): Event | undefined {
  const r = new Reader(row.payload);
  let event: Record<string, unknown> | undefined;
  switch (row.kind) {
    case KIND.header:
      event = { kind: "header", format: r.uleb(), execution: r.bytes(16) };
      break;
    case KIND.input:
      event = { kind: "input", number: r.num(), source: r.le(16), module: r.le(16) };
      break;
    case KIND.call:
      event = decodeCall(r, methods);
      break;
    case KIND.finish:
      event = decodeFinish(r);
      break;
    case KIND.wake:
      event = { kind: "wake", poll: r.uleb(), handles: r.handles() };
      break;
    case KIND.block:
      event = { kind: "block", handles: r.handles() };
      break;
    case KIND.abort:
      event = { kind: "abort", handle: r.num() };
      break;
    case KIND.cancel:
      event = { kind: "cancel", poll: r.uleb() };
      break;
    default:
      event = decodeEnd(r);
  }
  if (!event || Object.values(event).includes(undefined) || r.at !== row.payload.length) {
    return undefined;
  }
  return event as unknown as Event;
}

// Decodes the rows of one journal, in order, or throws naming the first
// row whose payload does not decode.
export function decodeRows(rows: Row[]): Event[] {
  const methods: (string | undefined)[][] = [];
  return rows.map((row) => {
    const event = decodeRow(row, methods);
    if (!event) {
      const kind = KIND_NAMES[row.kind]!;
      throw new Error(
        `row ${row.index} (${kind[0].toUpperCase()}${kind.slice(1)}) does not decode`,
      );
    }
    return event;
  });
}

// --- the recorder ------------------------------------------------------------

// Writes one execution's journal in memory, as `journal::Recorder` does:
// the host's import call sites call `call`, `finish`, `block` or `abort`
// per host call; the driver `poll`, `wake` and `cancel`; a session `input`
// and `end` around each input. Past its cap, recording stops for good and
// the journal is dropped; `takeNotice` then answers true once.
export class Recorder {
  bytes: Bytes;
  capBytes: number;
  next: number;
  methods: Map<string, number>;
  row: Bytes;
  polls: number;
  state: "on" | "off" | "told";

  // `execution` is the 16 bytes of the execution id.
  constructor(cap: number, execution: Uint8Array) {
    this.bytes = new Bytes();
    this.capBytes = cap;
    this.next = 0;
    this.methods = new Map();
    this.row = new Bytes();
    this.polls = 0;
    this.state = "on";
    this.row.uleb(FORMAT);
    this.row.raw(execution);
    this.push(KIND.header);
  }
  // The journal so far, or `null` once recording stopped at the cap.
  journal(): Uint8Array | null {
    return this.state === "on" ? this.bytes.view() : null;
  }
  get cap(): number {
    return this.capBytes;
  }
  setCap(cap: number): void {
    this.capBytes = cap;
  }
  takeNotice(): boolean {
    if (this.state !== "off") {
      return false;
    }
    this.state = "told";
    return true;
  }
  // A host call of the import `module` `name` with the arguments
  // `request` that returned `value` and, when not null, `result` bytes.
  call({
    module,
    name,
    request,
    value,
    result,
  }: {
    module: string;
    name: string;
    request: Uint8Array;
    value: Value;
    result: Uint8Array | null;
  }): void {
    if (this.state !== "on") {
      return;
    }
    const key = `${module}\0${name}`;
    const known = this.methods.get(key);
    if (known === undefined) {
      this.row.uleb(this.methods.size);
      for (const text of [module, name]) {
        const b = utf8.encode(text);
        this.row.uleb(b.length);
        this.row.raw(b);
      }
      this.methods.set(key, this.methods.size);
    } else {
      this.row.uleb(known);
    }
    this.row.uleb(request.length);
    if (request.length <= 8) {
      this.row.raw(request);
    } else {
      this.row.le(requestDigest(request), 8);
    }
    putReply(this.row, value, result);
    this.push(KIND.call);
  }
  finish(handle: number, value: Value, result: Uint8Array | null): void {
    if (this.state !== "on") {
      return;
    }
    this.row.uleb(handle);
    putReply(this.row, value, result);
    this.push(KIND.finish);
  }
  block(handles: readonly number[]): void {
    if (this.state !== "on") {
      return;
    }
    this.handles(handles);
    this.push(KIND.block);
  }
  abort(handle: number): void {
    if (this.state !== "on") {
      return;
    }
    this.row.uleb(handle);
    this.push(KIND.abort);
  }
  poll(): void {
    this.polls += 1;
  }
  wake(handles: readonly number[]): void {
    if (this.state !== "on") {
      return;
    }
    this.row.uleb(this.polls);
    this.handles(handles);
    this.push(KIND.wake);
  }
  cancel(): void {
    if (this.state !== "on") {
      return;
    }
    this.row.uleb(this.polls);
    this.push(KIND.cancel);
  }
  // A REPL input numbered `number` starts, with its source text (a
  // string) and module bytes.
  input(number: number, source: string, module: Uint8Array): void {
    this.polls = 0;
    if (this.state !== "on") {
      return;
    }
    this.row.uleb(number);
    this.row.le(xxh3_128(utf8.encode(source)), 16);
    this.row.le(xxh3_128(module), 16);
    this.push(KIND.input);
  }
  end(end: End): void {
    if (this.state !== "on") {
      return;
    }
    this.row.byte(END_KINDS.indexOf(end.kind));
    if (end.kind === "exit") {
      this.row.byte(end.status);
    }
    this.push(KIND.end);
  }
  private handles(handles: readonly number[]): void {
    this.row.uleb(handles.length);
    for (const h of handles) {
      this.row.uleb(h);
    }
  }
  private push(kind: number): void {
    const len = this.row.len;
    const size = 1 + ulebLen(this.next) + ulebLen(len) + len + 4;
    if (this.bytes.len + size > this.capBytes) {
      this.state = "off";
      this.bytes = new Bytes();
      this.methods = new Map();
      this.row = new Bytes();
      return;
    }
    appendRow(this.bytes, kind, this.next, this.row.view());
    this.next += 1;
    this.row.clear();
  }
}

// --- replay ------------------------------------------------------------------

// A row, as a divergence message names what the record has.
function describe(event: Event | undefined): string {
  switch (event?.kind) {
    case undefined:
      return "nothing more";
    case "header":
      return "a header";
    case "input":
      return `input ${event.number}`;
    case "call":
      return `a call of ${event.module} ${event.name}`;
    case "finish":
      return `the finish of operation ${event.handle}`;
    case "wake":
      return `a wake after ${event.poll} polls`;
    case "block":
      return "a block";
    case "abort":
      return `the abort of operation ${event.handle}`;
    case "cancel":
      return "a cancel";
    case "end":
      return `its end: it ${ended(event.end)}`;
  }
}

// How an input ended, as a sentence's verb phrase.
export function ended(end: End): string {
  switch (end.kind) {
    case "exit":
      return end.status === 0 ? "ended" : `ended with status ${end.status}`;
    case "trapped":
      return "panicked";
    case "stopped":
      return "was stopped";
    case "cancelled":
      return "was cancelled";
    default:
      return "failed to run";
  }
}

const sameEnd = (a: End, b: End): boolean => a.kind === b.kind && a.status === b.status;

// Reads a session's journal into its execution id (16 bytes) and its
// inputs, `{ number, source, module, events, end }` with `end` null when
// the journal ends inside it. A torn tail is cut.
export function recorded(bytes: Uint8Array): {
  execution: Uint8Array;
  inputs: RecordedInput[];
} {
  const events = decodeRows(readRows(bytes).rows);
  const header = events.shift();
  if (header?.kind !== "header") {
    throw new Error("the journal has no header");
  }
  if (header.format !== BigInt(FORMAT)) {
    throw new Error(`the journal has format ${header.format}, not ${FORMAT}`);
  }
  const inputs: RecordedInput[] = [];
  for (const event of events) {
    if (event.kind === "input") {
      const { number, source, module } = event;
      inputs.push({ number, source, module, events: [], end: null });
      continue;
    }
    const input = inputs.at(-1);
    if (!input || input.end) {
      throw new Error(`the journal has ${describe(event)} outside an input`);
    }
    if (event.kind === "end") {
      input.end = event.end;
    } else {
      input.events.push(event);
    }
  }
  return { execution: header.execution, inputs };
}

function archivedCall(
  event: Extract<Event, { kind: "call" }>,
  methods: Map<string, number>,
): Uint8Array {
  const write = new Bytes();
  const key = `${event.module}\0${event.name}`;
  const known = methods.get(key);
  write.uleb(known ?? methods.size);
  if (known === undefined) {
    for (const text of [event.module, event.name]) {
      const encoded = utf8.encode(text);
      write.uleb(encoded.length);
      write.raw(encoded);
    }
    methods.set(key, methods.size);
  }
  if ("args" in event.request) {
    write.uleb(event.request.args.length);
    write.raw(event.request.args);
  } else {
    write.uleb(event.request.len);
    write.le(event.request.digest, 8);
  }
  putReply(write, event.value, event.result);
  return write.view();
}

// Materializes archived, completed epochs as one ordinary journal. Method
// numbers and row indices are local to each chunk and must be reassigned;
// stored request digests and reply bytes are copied without running HD.
export function joinJournals(chunks: Iterable<Uint8Array>): Uint8Array {
  const out = new Bytes();
  const methods = new Map<string, number>();
  let execution: Uint8Array | null = null;
  let next = 0;
  let last = -1;
  for (const bytes of chunks) {
    const read = readRows(bytes);
    if (read.valid !== bytes.byteLength) {
      throw new Error("an archived journal is cut short");
    }
    const record = recorded(bytes);
    if (record.inputs.some((input) => input.end === null)) {
      throw new Error("an archived journal has an unfinished input");
    }
    if (execution && !execution.every((b, i) => b === record.execution[i])) {
      throw new Error("archived journals belong to different executions");
    }
    const events = decodeRows(read.rows);
    for (const [index, event] of events.entries()) {
      const row = read.rows[index]!;
      if (event.kind === "header") {
        if (execution) {
          continue;
        }
        execution = record.execution;
      }
      if (event.kind === "input") {
        if (event.number <= last) {
          throw new Error("archived inputs are not in increasing order");
        }
        last = event.number;
      }
      let payload = row.payload;
      if (event.kind === "call") {
        payload = archivedCall(event, methods);
      }
      appendRow(out, row.kind, next++, payload);
    }
  }
  if (!execution) {
    throw new Error("no archived journal header");
  }
  return out.view();
}

// Whether `source` is the text a recorded input ran, by its hash.
export const ran = (input: RecordedInput, source: string): boolean =>
  xxh3_128(utf8.encode(source)) === input.source;

// The divergence of a replayed input: the message, as `Replay` words it.
export class Diverged extends Error {}

// Serves one recorded input's host results while it runs again
// (`replay::Replay`): each method throws `Diverged` at the first request
// that is not the record's next row, and keeps its message.
export class Replay {
  readonly number: number;
  private events: Event[];
  private polls: number;
  diverged: string | null;

  constructor(input: RecordedInput) {
    this.number = input.number;
    this.events = [...input.events];
    this.polls = 0;
    this.diverged = null;
  }
  // The reply `{ value, result }` the record holds for a call of `module`
  // `name` with the arguments `args`.
  call(
    module: string,
    name: string,
    args: Uint8Array,
  ): { value: Value; result: Uint8Array | null } {
    const asked = `called ${module} ${name}`;
    const front = this.events[0];
    if (front?.kind === "call" && front.module === module && front.name === name) {
      if (!sameRequest(front.request, requestOf(args))) {
        this.diverge(`input ${this.number} ${asked} with other arguments than before`);
      }
      const event = this.events.shift()! as Extract<Event, { kind: "call" }>;
      return { value: event.value, result: event.result };
    }
    return this.unexpected(asked);
  }
  finish(handle: number): { value: Value; result: Uint8Array | null } {
    const front = this.events[0];
    if (front?.kind === "finish" && front.handle === handle) {
      this.events.shift();
      return { value: front.value, result: front.result };
    }
    return this.unexpected(`finished operation ${handle}`);
  }
  // The handles of a recorded block, or null when the record has nothing
  // more (the block finds nothing pending, as the first run's did).
  block(): number[] | null {
    const front = this.events[0];
    if (front === undefined) {
      return null;
    }
    if (front.kind === "block") {
      this.events.shift();
      return front.handles;
    }
    return this.unexpected("blocked");
  }
  abort(handle: number): void {
    const front = this.events[0];
    if (front?.kind === "abort" && front.handle === handle) {
      this.events.shift();
    } else {
      this.unexpected(`aborted operation ${handle}`);
    }
  }
  poll(): void {
    this.polls += 1;
  }
  // Whether the record's next row is a wake: the input waits for one.
  wakeNext(): boolean {
    return this.events[0]?.kind === "wake";
  }
  // The handles the record wakes the input with after its polls so far.
  wake(): number[] {
    const front = this.events[0];
    if (front?.kind === "wake" && front.poll === BigInt(this.polls)) {
      this.events.shift();
      return front.handles;
    }
    return this.unexpected(`waited on the host after ${this.polls} polls`);
  }
  // The input ended with `end`: a divergence unless the record ended it
  // so, `recordedEnd`, after every row was served.
  end(end: End, recordedEnd: End | null): void {
    if (this.diverged !== null) {
      throw new Diverged(this.diverged);
    }
    if (this.events.length) {
      this.unexpected(`${ended(end)} early`);
    }
    if (recordedEnd === null) {
      this.diverge(`input ${this.number} ${ended(end)}, but the record of it is cut short`);
    }
    if (!sameEnd(end, recordedEnd)) {
      this.diverge(`input ${this.number} ${ended(end)}, but before it ${ended(recordedEnd)}`);
    }
  }
  private unexpected(asked: string): never {
    return this.diverge(
      `input ${this.number} ${asked}, but the record has ${describe(this.events[0])}`,
    );
  }
  private diverge(message: string): never {
    this.diverged ??= message;
    throw new Diverged(this.diverged);
  }
}

// --- import types --------------------------------------------------------------

// The parameter and result types of each function a module imports, by
// `module\0name`: "i32", "i64", "f32", "f64" or "ref". A recorder encodes
// parameters by them and a replay converts served values to them.
export function importTypes(bytes: Uint8Array | ArrayBuffer): Map<string, ImportType | null> {
  const b = bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes);
  const r = new Reader(b);
  r.at = 8;
  const types: ImportType[] = [];
  const out = new Map<string, ImportType | null>();
  const NUM: Record<number, string> = {
    0x7f: "i32",
    0x7e: "i64",
    0x7d: "f32",
    0x7c: "f64",
    0x7b: "v128",
  };
  const sleb = (): number => {
    let shift = 0;
    let byte: number;
    do {
      byte = b[r.at++]!;
      shift += 7;
    } while (byte & 0x80);
    return shift;
  };
  const valtype = (): string => {
    const t = b[r.at++]!;
    if (NUM[t]) {
      return NUM[t];
    }
    if (t === 0x63 || t === 0x64) {
      sleb();
    }
    return "ref";
  };
  const vec = <T,>(each: () => T): T[] => Array.from({ length: r.num() ?? 0 }, each);
  // A memory's or table's limits: flags, the minimum, and the maximum
  // when flag bit 0 is set.
  const limits = (): void => {
    const flags = r.num() ?? 0;
    r.num();
    if (flags & 1) {
      r.num();
    }
  };
  const functionType = (): void => {
    if (b[r.at++] !== 0x60) {
      throw new Error("a module type is not a function");
    }
    types.push({ params: vec(valtype), results: vec(valtype) });
  };
  while (r.at < b.length) {
    const id = b[r.at++]!;
    const size = r.num() ?? 0;
    const end = r.at + size;
    if (id === 1) {
      vec(functionType);
    } else if (id === 2) {
      vec(() => {
        const module = r.text()!;
        const name = r.text()!;
        const kind = b[r.at++]!;
        if (kind === 0x00) {
          out.set(`${module}\0${name}`, types[r.num()!] ?? null);
        } else if (kind === 0x02) {
          limits();
        } else if (kind === 0x01) {
          valtype();
          limits();
        } else if (kind === 0x03) {
          valtype();
          r.at++;
        } else {
          r.at++;
          r.num();
        }
      });
      return out;
    }
    r.at = end;
  }
  return out;
}
