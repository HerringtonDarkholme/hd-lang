// The exchange-buffer encoding of crates/hd_run_wasmtime/DESIGN.md §17.4:
// the byte writer and reader (`Enc`, `Dec`), and the codecs of the
// language's own types, from which host/gen/abi.ts, generated from the ABI
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
// Keep a local binding as well as the public re-export: the decoder throws it.
import { HostContract } from "./run-support.ts";
export { HostContract };

export type Integer = number | bigint;
export type Result<OkValue, ErrorValue> =
  | { ok: true; value: OkValue }
  | { ok: false; error: ErrorValue };
export interface Codec<Value> {
  enc(encoder: Enc, value: Value): void;
  dec(decoder: Dec): Value;
}
export interface TaggedValue extends Record<string, unknown> {
  tag: string;
}
export interface VariantCodec extends Codec<TaggedValue> {
  fields: Map<string, string[]>;
}

const utf8 = new TextEncoder();
// Lossy UTF-8 text that keeps a leading byte-order mark, as Node's UTF-8
// decoding does.
const lossy = new TextDecoder("utf-8", { ignoreBOM: true });
export const decodeUtf8 = (bytes: Uint8Array): string => lossy.decode(bytes);

// The encoder of §17.4: integers as LEB128 (zigzag when signed), strings
// and byte lists as their length then their bytes, lists as their count
// then their elements, an optional as 0, or 1 and the value, an enum as
// its variant index then its payload, data as its fields in order.
export class Enc {
  buf: Uint8Array;
  len: number;

  constructor() {
    this.buf = new Uint8Array(64);
    this.len = 0;
  }
  room(value: number): void {
    if (this.len + value <= this.buf.length) {
      return;
    }
    const next = new Uint8Array(Math.max(this.buf.length * 2, this.len + value));
    next.set(this.buf.subarray(0, this.len));
    this.buf = next;
  }
  leb(value: Integer): this {
    let bigInt = BigInt(value);
    this.room(10);
    do {
      let number = Number(bigInt & 0x7fn);
      bigInt >>= 7n;
      if (bigInt) {
        number |= 0x80;
      }
      this.buf[this.len++] = number;
    } while (bigInt);
    return this;
  }
  zz(value: Integer): this {
    const bigInt = BigInt(value);
    return this.leb(bigInt >= 0n ? bigInt << 1n : (-bigInt << 1n) - 1n);
  }
  bytes(u8: Uint8Array): this {
    this.leb(u8.length);
    this.room(u8.length);
    this.buf.set(u8, this.len);
    this.len += u8.length;
    return this;
  }
  str(text: string): this {
    return this.bytes(utf8.encode(text));
  }
  // An `f64` as its raw IEEE 754 bits, little-endian.
  f64(value: number): this {
    this.room(8);
    new DataView(this.buf.buffer, this.buf.byteOffset).setFloat64(this.len, value, true);
    this.len += 8;
    return this;
  }
  done(): Uint8Array {
    return this.buf.subarray(0, this.len);
  }
}

// The decoder of §17.4, over a copy of the argument bytes. A byte past the
// end, or a tag no value has, is the `host-contract` panic.
export class Dec {
  input: Uint8Array;
  at: number;

  constructor(bytes: Uint8Array) {
    this.input = bytes;
    this.at = 0;
  }
  leb(): bigint {
    let result = 0n;
    let shift = 0n;
    let byte: number | undefined;
    do {
      byte = this.input[this.at++];
      if (byte === undefined) {
        throw new HostContract("an argument does not decode");
      }
      result |= BigInt(byte & 0x7f) << shift;
      shift += 7n;
    } while (byte & 0x80);
    return result;
  }
  num(): number {
    return Number(this.leb());
  }
  zz(): bigint {
    const encoded = this.leb();
    return (encoded >> 1n) ^ -(encoded & 1n);
  }
  bytes(): Uint8Array {
    const length = this.num();
    if (this.at + length > this.input.length) {
      throw new HostContract("an argument does not decode");
    }
    const out = this.input.subarray(this.at, this.at + length);
    this.at += length;
    return out;
  }
  str(): string {
    return decodeUtf8(this.bytes());
  }
  // A tag below `count`: an option's, a result's or an enum's.
  tag(count: number): number {
    const tag = this.num();
    if (tag >= count) {
      throw new HostContract("an argument does not decode");
    }
    return tag;
  }
}

export const Ok = <Value>(value: Value): Result<Value, never> => ({
  ok: true,
  value,
});
export const Err = <ErrorValue>(error: ErrorValue): Result<never, ErrorValue> => ({
  ok: false,
  error,
});

// `void`, as a result's `T`: no bytes.
export const unit: Codec<void> = { enc: () => {}, dec: () => undefined };
export const str: Codec<string> = {
  enc: (encoder, value) => encoder.str(value),
  dec: (decoder) => decoder.str(),
};
// `List[u8]`: its length, then its bytes; decoded as a copy.
export const bytes: Codec<Uint8Array> = {
  enc: (encoder, value) => encoder.bytes(value),
  dec: (decoder) => decoder.bytes().slice(),
};
// `u8` to `u32` and `usize`; `u64`, a `BigInt`.
export const unsigned: Codec<number> = {
  enc: (encoder, value) => encoder.leb(value),
  dec: (decoder) => decoder.num(),
};
export const unsigned64: Codec<bigint> = {
  enc: (encoder, value) => encoder.leb(value),
  dec: (decoder) => decoder.leb(),
};
// `i8` to `i32` and `isize`; `i64`, a `BigInt`.
export const signed: Codec<number> = {
  enc: (encoder, value) => encoder.zz(value),
  dec: (decoder) => Number(decoder.zz()),
};
export const signed64: Codec<bigint> = {
  enc: (encoder, value) => encoder.zz(value),
  dec: (decoder) => decoder.zz(),
};
// A host handle: its number.
export const handle: Codec<number> = {
  enc: (encoder, value) => encoder.leb(value),
  dec: (decoder) => decoder.num(),
};

export const list = <Item>(codec: Codec<Item>): Codec<Item[]> => ({
  enc: (encoder, xs) => {
    encoder.leb(xs.length);
    for (const item of xs) {
      codec.enc(encoder, item);
    }
  },
  dec: (decoder) => {
    const length = decoder.num();
    const out: Item[] = [];
    for (let i = 0; i < length; i++) {
      out.push(codec.dec(decoder));
    }
    return out;
  },
});

export const option = <Value>(codec: Codec<Value>): Codec<Value | null> => ({
  enc: (encoder, value) => {
    if (value === null) {
      encoder.leb(0);
    } else {
      codec.enc(encoder.leb(1), value);
    }
  },
  dec: (decoder) => (decoder.tag(2) === 0 ? null : codec.dec(decoder)),
});

export const result = <OkValue, ErrorValue>(
  ok: Codec<OkValue>,
  err: Codec<ErrorValue>,
): Codec<Result<OkValue, ErrorValue>> => ({
  enc: (encoder, result) =>
    result.ok ? ok.enc(encoder.leb(0), result.value) : err.enc(encoder.leb(1), result.error),
  dec: (decoder) => (decoder.tag(2) === 0 ? Ok(ok.dec(decoder)) : Err(err.dec(decoder))),
});

export const tuple = <Values extends unknown[]>(
  ...ts: { [Key in keyof Values]: Codec<Values[Key]> }
): Codec<Values> => ({
  enc: (encoder, value) => ts.forEach((item, index) => item.enc(encoder, value[index])),
  dec: (decoder) => ts.map((item) => item.dec(decoder)) as Values,
});

// A newtype crosses as its field: the same bytes, a codec of its own.
export const newtype = <Value>(codec: Codec<Value>): Codec<Value> => ({
  enc: codec.enc,
  dec: codec.dec,
});

// A data type of `fields`, its field codecs by name in declaration order.
export const data = <Value extends Record<string, unknown>>(fields: {
  [Key in keyof Value]: Codec<Value[Key]>;
}): Codec<Value> => {
  const all = Object.entries(fields);
  return {
    enc: (encoder, value) => all.forEach(([name, codec]) => codec.enc(encoder, value[name])),
    dec: (decoder) =>
      Object.fromEntries(all.map(([name, codec]) => [name, codec.dec(decoder)])) as Value,
  };
};

// An enum of `cases`: each variant's payload fields, as `data` takes them,
// by variant name in declaration order.
export const variants = (cases: Record<string, Record<string, Codec<unknown>>>): VariantCodec => {
  const all: [string, [string, Codec<unknown>][]][] = Object.entries(cases).map(([tag, fields]) => [
    tag,
    Object.entries(fields),
  ]);
  const index = new Map(all.map(([tag], i) => [tag, i]));
  return {
    // The payload field names of each variant, for `variant`.
    fields: new Map(all.map(([tag, fs]) => [tag, fs.map(([name]) => name)])),
    enc: (encoder, value) => {
      const i = index.get(value.tag);
      if (i === undefined) {
        throw new Error(`no variant \`${value.tag}\``);
      }
      encoder.leb(i);
      for (const [name, codec] of all[i][1]) {
        codec.enc(encoder, value[name]);
      }
    },
    dec: (decoder) => {
      const [tag, fs] = all[decoder.tag(all.length)];
      return Object.fromEntries([
        ["tag", tag],
        ...fs.map(([name, codec]) => [name, codec.dec(decoder)]),
      ]) as TaggedValue;
    },
  };
};

// The value of `codec`'s variant `tag` with its payload `values` in order;
// values past the payload are dropped.
export const variant = (codec: VariantCodec, tag: string, ...values: unknown[]): TaggedValue => {
  const names = codec.fields.get(tag);
  if (names === undefined) {
    throw new Error(`no variant \`${tag}\``);
  }
  return Object.fromEntries([["tag", tag], ...names.map((name, i) => [name, values[i]])]);
};
