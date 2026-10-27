// The exchange-buffer encoding of crates/hd_run_wasmtime/DESIGN.md §17.4:
// the byte writer and reader (`Enc`, `Dec`), and the codecs of the
// language's own types, from which host/gen/abi.mjs, generated from the ABI
// model, builds the codec of every std type a host method crosses.
//
// A codec is `{ enc(e, v), dec(d) }`. A crossing value is a plain JS value:
// - an integer is a number, or a `BigInt` when wider than 32 bits (the
//   encoder takes either); a string is a string, a `List[u8]` a
//   `Uint8Array`, and a list an array;
// - `T?` is `null` or the value; a tuple is an array;
// - `Result[T, E]` is `Ok(value)` or `Err(error)`: `{ ok, value }` or
//   `{ ok, error }`;
// - a data value is an object of its fields by name, and a newtype its
//   field's value;
// - an enum value is `{ tag, ...fields }`, `tag` its variant's name;
//   `variant(codec, tag, ...values)` makes one from the payload in order;
// - a host handle is its number.

// A host result or argument that breaks the ABI: the `host-contract` panic.
export { HostContract } from "./run-support.mjs";

const utf8 = new TextEncoder();
// Lossy UTF-8 text that keeps a leading byte-order mark, as Node's UTF-8
// decoding does.
const lossy = new TextDecoder("utf-8", { ignoreBOM: true });
export const decodeUtf8 = (bytes) => lossy.decode(bytes);

// The encoder of §17.4: integers as LEB128 (zigzag when signed), strings
// and byte lists as their length then their bytes, lists as their count
// then their elements, an optional as 0, or 1 and the value, an enum as
// its variant index then its payload, data as its fields in order.
export class Enc {
  constructor() {
    this.buf = new Uint8Array(64);
    this.len = 0;
  }
  room(n) {
    if (this.len + n <= this.buf.length) return;
    const next = new Uint8Array(Math.max(this.buf.length * 2, this.len + n));
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
  }
  leb(v) {
    let x = BigInt(v);
    this.room(10);
    do {
      let b = Number(x & 0x7fn);
      x >>= 7n;
      if (x) b |= 0x80;
      this.buf[this.len++] = b;
    } while (x);
    return this;
  }
  zz(v) {
    const x = BigInt(v);
    return this.leb(x >= 0n ? x << 1n : (-x << 1n) - 1n);
  }
  bytes(u8) {
    this.leb(u8.length);
    this.room(u8.length);
    this.buf.set(u8, this.len);
    this.len += u8.length;
    return this;
  }
  str(s) {
    return this.bytes(utf8.encode(s));
  }
  // An `f64` as its raw IEEE 754 bits, little-endian.
  f64(v) {
    this.room(8);
    new DataView(this.buf.buffer, this.buf.byteOffset).setFloat64(this.len, v, true);
    this.len += 8;
    return this;
  }
  done() {
    return this.buf.subarray(0, this.len);
  }
}

// The decoder of §17.4, over a copy of the argument bytes. A byte past the
// end, or a tag no value has, is the `host-contract` panic.
export class Dec {
  constructor(bytes) {
    this.b = bytes;
    this.at = 0;
  }
  leb() {
    let r = 0n;
    let s = 0n;
    let b;
    do {
      b = this.b[this.at++];
      if (b === undefined) throw new HostContract("an argument does not decode");
      r |= BigInt(b & 0x7f) << s;
      s += 7n;
    } while (b & 0x80);
    return r;
  }
  num() {
    return Number(this.leb());
  }
  zz() {
    const x = this.leb();
    return (x >> 1n) ^ -(x & 1n);
  }
  bytes() {
    const n = this.num();
    if (this.at + n > this.b.length) throw new HostContract("an argument does not decode");
    const out = this.b.subarray(this.at, this.at + n);
    this.at += n;
    return out;
  }
  str() {
    return decodeUtf8(this.bytes());
  }
  // A tag below `count`: an option's, a result's or an enum's.
  tag(count) {
    const t = this.num();
    if (t >= count) throw new HostContract("an argument does not decode");
    return t;
  }
}

export const Ok = (value) => ({ ok: true, value });
export const Err = (error) => ({ ok: false, error });

// `void`, as a result's `T`: no bytes.
export const unit = { enc: () => {}, dec: () => undefined };
export const str = { enc: (e, v) => e.str(v), dec: (d) => d.str() };
// `List[u8]`: its length, then its bytes; decoded as a copy.
export const bytes = { enc: (e, v) => e.bytes(v), dec: (d) => d.bytes().slice() };
// `u8` to `u32` and `usize`; `u64`, a `BigInt`.
export const unsigned = { enc: (e, v) => e.leb(v), dec: (d) => d.num() };
export const unsigned64 = { enc: (e, v) => e.leb(v), dec: (d) => d.leb() };
// `i8` to `i32` and `isize`; `i64`, a `BigInt`.
export const signed = { enc: (e, v) => e.zz(v), dec: (d) => Number(d.zz()) };
export const signed64 = { enc: (e, v) => e.zz(v), dec: (d) => d.zz() };
// A host handle: its number.
export const handle = { enc: (e, v) => e.leb(v), dec: (d) => d.num() };

export const list = (t) => ({
  enc: (e, xs) => {
    e.leb(xs.length);
    for (const x of xs) t.enc(e, x);
  },
  dec: (d) => {
    const n = d.num();
    const out = [];
    for (let i = 0; i < n; i++) out.push(t.dec(d));
    return out;
  },
});

export const option = (t) => ({
  enc: (e, v) => {
    if (v === null) e.leb(0);
    else t.enc(e.leb(1), v);
  },
  dec: (d) => (d.tag(2) === 0 ? null : t.dec(d)),
});

export const result = (ok, err) => ({
  enc: (e, r) => (r.ok ? ok.enc(e.leb(0), r.value) : err.enc(e.leb(1), r.error)),
  dec: (d) => (d.tag(2) === 0 ? Ok(ok.dec(d)) : Err(err.dec(d))),
});

export const tuple = (...ts) => ({
  enc: (e, v) => ts.forEach((t, i) => t.enc(e, v[i])),
  dec: (d) => ts.map((t) => t.dec(d)),
});

// A newtype crosses as its field: the same bytes, a codec of its own.
export const newtype = (t) => ({ enc: t.enc, dec: t.dec });

// A data type of `fields`, its field codecs by name in declaration order.
export const data = (fields) => {
  const all = Object.entries(fields);
  return {
    enc: (e, v) => all.forEach(([name, t]) => t.enc(e, v[name])),
    dec: (d) => Object.fromEntries(all.map(([name, t]) => [name, t.dec(d)])),
  };
};

// An enum of `cases`: each variant's payload fields, as `data` takes them,
// by variant name in declaration order.
export const variants = (cases) => {
  const all = Object.entries(cases).map(([tag, fields]) => [tag, Object.entries(fields)]);
  const index = new Map(all.map(([tag], i) => [tag, i]));
  return {
    // The payload field names of each variant, for `variant`.
    fields: new Map(all.map(([tag, fs]) => [tag, fs.map(([name]) => name)])),
    enc: (e, v) => {
      const i = index.get(v.tag);
      if (i === undefined) throw new Error(`no variant \`${v.tag}\``);
      e.leb(i);
      for (const [name, t] of all[i][1]) t.enc(e, v[name]);
    },
    dec: (d) => {
      const [tag, fs] = all[d.tag(all.length)];
      return Object.fromEntries([["tag", tag], ...fs.map(([name, t]) => [name, t.dec(d)])]);
    },
  };
};

// The value of `codec`'s variant `tag` with its payload `values` in order;
// values past the payload are dropped.
export const variant = (codec, tag, ...values) => {
  const names = codec.fields.get(tag);
  if (names === undefined) throw new Error(`no variant \`${tag}\``);
  return Object.fromEntries([["tag", tag], ...names.map((name, i) => [name, values[i]])]);
};
