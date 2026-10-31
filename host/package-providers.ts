// Helpers used by signature-generated NAME.host.ts modules. Providers are
// trusted JS selected from package manifests after the root grants access.
// Guest contract data is never interpreted as module code or import paths.
import { Dec, Enc, HostContract, decodeUtf8 } from "./wire.ts";
export { readPackageContract } from "./package-contract.ts";
import { readPackageContract } from "./package-contract.ts";
import type {
  ContractField,
  ContractIdentity,
  ContractInterface,
  ContractValue,
} from "./package-contract.ts";

type Dynamic = ReturnType<typeof JSON.parse>;
type DynamicRecord = Record<string, Dynamic>;
type RuntimeField = [string, RuntimeCodec];
type RuntimeVariant = [string, RuntimeField[]];

interface RuntimeCodec {
  shape: ContractValue;
  enc(encoder: Enc, value: Dynamic, transaction: number[]): Dynamic;
  dec(decoder: Dec): Dynamic;
}

interface RuntimeMethod {
  name: string;
  member: string;
  wait: "never" | "may";
  params: RuntimeCodec[];
  result: RuntimeCodec;
}

interface RuntimeInterface {
  module: string;
  handle: boolean;
  methods: RuntimeMethod[];
}

interface ProviderRegistration {
  contract: Uint8Array;
  module: string;
  identity: ContractIdentity;
  implementation: DynamicRecord;
}

export interface PackageHostIO extends DynamicRecord {}

export interface PackageRuntimeOptions {
  module?: WebAssembly.Module;
  grants?: Record<string, boolean>;
}

export interface PackageRuntime extends DynamicRecord {
  bind(
    contract: Uint8Array,
    generated: RuntimeInterface[],
    providers: ProviderRegistration[],
  ): DynamicRecord;
  close(): void;
  handleCount(): number;
}

const fail = (message: string): never => {
  throw new HostContract(`package host: ${message}`);
};
const same = (left: Dynamic, right: Dynamic): boolean =>
  JSON.stringify(left) === JSON.stringify(right);
const equalBytes = (left: Uint8Array, right: Uint8Array): boolean =>
  left.length === right.length && left.every((item, i) => item === right[i]);
const own = (value: Dynamic, k: string): Dynamic => {
  if (value === null || typeof value !== "object" || !Object.hasOwn(value, k)) {
    fail(`missing field ${k}`);
  }
  return value[k];
};
const object = (fields: [string, Dynamic][]): DynamicRecord => Object.fromEntries(fields);
const named = (i: ContractInterface): string =>
  `${i.identity.package}@${i.identity.version}/${i.identity.trait_path}`;

function encodePrimitive(value: Dynamic, tokens: Dynamic[]): boolean {
  if (value === null) {
    tokens.push(["n"]);
  } else if (typeof value === "boolean") {
    tokens.push(["b", value]);
  } else if (typeof value === "string") {
    tokens.push(["s", value]);
  } else if (typeof value === "bigint") {
    if (value < -(1n << 63n) || value >= 1n << 64n) {
      fail("private integer outside 64-bit range");
    }
    tokens.push([value < 0n ? "i" : "u", String(value)]);
  } else if (typeof value === "number") {
    if (!Number.isFinite(value)) {
      tokens.push(["n"]);
    } else {
      const bits = new DataView(new ArrayBuffer(8));
      bits.setFloat64(0, value, true);
      tokens.push(["f", bits.getBigUint64(0, true).toString(16).padStart(16, "0")]);
    }
  } else {
    return false;
  }
  return true;
}

// Typed Serde transport is internal. Provider methods receive JS values:
// bigint integers, number floats, and ordinary arrays/objects. Using a flat
// stream preserves numeric kind and avoids multiplying container depth.
function encodePrivate(value: Dynamic): string {
  const tokens: Dynamic[] = [];
  const pending: ({ value: Dynamic } | { leave: object })[] = [{ value }];
  const active = new Set<object>();
  while (pending.length) {
    const job = pending.pop()!;
    if ("leave" in job) {
      active.delete(job.leave);
      continue;
    }
    const value = job.value;
    if (encodePrimitive(value, tokens)) {
      continue;
    }
    if (typeof value === "object") {
      if (active.has(value)) {
        fail("cyclic private value");
      }
      active.add(value);
      pending.push({ leave: value });
      if (Array.isArray(value)) {
        tokens.push(["a", value.length]);
        for (let i = value.length - 1; i >= 0; --i) {
          pending.push({ value: value[i] });
        }
      } else {
        if (![Object.prototype, null].includes(Object.getPrototypeOf(value))) {
          fail("private value must be a plain object");
        }
        const entries = Object.entries(value);
        tokens.push(["o", entries.length]);
        for (let i = entries.length - 1; i >= 0; --i) {
          pending.push({ value: entries[i][1] }, { value: entries[i][0] });
        }
      }
    } else {
      fail("unsupported private value");
    }
  }
  return JSON.stringify(tokens);
}

function decodePrivateNumber(tag: Dynamic, value: Dynamic): Dynamic {
  if (
    typeof value !== "string" ||
    !(tag === "f" ? /^[0-9a-f]{16}$/ : /^(?:0|-?[1-9][0-9]*)$/).test(value)
  ) {
    fail("invalid private numeric token");
  }
  const number = BigInt(tag === "f" ? "0x" + value : value);
  if (tag === "i") {
    if (number < -(1n << 63n) || number >= 1n << 63n) {
      fail("private signed integer out of range");
    }
    return number;
  }
  if (number < 0n || number >= 1n << 64n) {
    fail("private unsigned integer out of range");
  }
  if (tag === "u") {
    return number;
  }
  const bits = new DataView(new ArrayBuffer(8));
  bits.setBigUint64(0, number, true);
  const float = bits.getFloat64(0, true);
  if (!Number.isFinite(float)) {
    fail("invalid private float");
  }
  return float;
}

function decodePrivateContainer(tag: Dynamic, length: Dynamic, values: Dynamic[]): Dynamic {
  if (
    !Number.isSafeInteger(length) ||
    length < 0 ||
    length > values.length / (tag === "o" ? 2 : 1)
  ) {
    fail("invalid private container length");
  }
  if (tag === "a") {
    return Array.from({ length }, () => values.pop());
  }
  const entries = [];
  const keys = new Set();
  for (let count = 0; count < length; ++count) {
    const key = values.pop();
    if (typeof key !== "string" || keys.has(key)) {
      fail("invalid private object key");
    }
    keys.add(key);
    entries.push([key, values.pop()]);
  }
  return Object.fromEntries(entries);
}

function decodePrivateToken(token: Dynamic, values: Dynamic[]): void {
  if (!Array.isArray(token)) {
    fail("invalid private value token");
  }
  const [tag, value] = token;
  if (token.length !== (tag === "n" ? 1 : 2)) {
    fail("invalid private value token");
  }
  if (tag === "n") {
    values.push(null);
  } else if (tag === "b" && typeof value === "boolean") {
    values.push(value);
  } else if (tag === "s" && typeof value === "string") {
    values.push(value);
  } else if (["i", "u", "f"].includes(tag)) {
    values.push(decodePrivateNumber(tag, value));
  } else if (tag === "a" || tag === "o") {
    values.push(decodePrivateContainer(tag, value, values));
  } else {
    fail("invalid private value token");
  }
}

function decodePrivate(text: string): Dynamic {
  let tokens: Dynamic;
  try {
    tokens = JSON.parse(text);
  } catch {
    fail("invalid private value transport");
  }
  if (!Array.isArray(tokens)) {
    fail("invalid private value transport");
  }
  const values: Dynamic[] = [];
  for (let i = tokens.length - 1; i >= 0; --i) {
    decodePrivateToken(tokens[i], values);
  }
  if (values.length !== 1) {
    fail("invalid private value framing");
  }
  return values[0];
}

// Each codec carries its canonical shape, checked against the data section
// before an instance starts. Encoding validates before the wire writer can
// coerce values (notably a negative unsigned LEB value).
/* oxlint-disable max-lines-per-function -- codecs and bind share one instance lifetime. */
export function createPackageRuntime(
  io: PackageHostIO,
  { module, grants = {} }: PackageRuntimeOptions = {},
): PackageRuntime {
  const handles = new Map<number, { namespace: string; value: Dynamic }>();
  const handleMethods = new Map<string, string[]>();
  let nextHandle = 1;
  const lifetime = new AbortController();
  const codec = (
    shape: ContractValue,
    enc: RuntimeCodec["enc"],
    dec: RuntimeCodec["dec"],
  ): RuntimeCodec => ({ shape, enc, dec });
  const unit = codec(
    [0],
    (_e, value) => {
      if (value !== undefined) {
        fail("void result has a value");
      }
    },
    () => undefined,
  );
  const bool = codec(
    [1],
    (error, value) => {
      if (typeof value !== "boolean") {
        fail("expected bool");
      }
      error.leb(value ? 1 : 0);
    },
    (data) => data.tag(2) === 1,
  );
  const int = (bits: number, signed: boolean): RuntimeCodec => {
    const low = signed ? -(1n << BigInt(bits - 1)) : 0n;
    const high = (1n << BigInt(bits - (signed ? 1 : 0))) - 1n;
    const check = (value: Dynamic): bigint => {
      if (
        typeof value !== "bigint" &&
        !(typeof value === "number" && Number.isSafeInteger(value))
      ) {
        fail("expected integer");
      }
      const bigInt = BigInt(value);
      if (bigInt < low || bigInt > high) {
        fail("integer is out of range");
      }
      return bigInt;
    };
    return codec(
      [3, bits, signed],
      (error, value) => {
        const checked = check(value);
        if (signed) {
          error.zz(checked);
        } else {
          error.leb(checked);
        }
      },
      (data) => {
        const checked = check(signed ? data.zz() : data.leb());
        return bits === 64 ? checked : Number(checked);
      },
    );
  };
  const char = codec(
    [2],
    (error, value) => {
      if (typeof value !== "string" || [...value].length !== 1) {
        fail("expected char");
      }
      const codePoint = value.codePointAt(0);
      if (codePoint >= 0xd800 && codePoint <= 0xdfff) {
        fail("invalid char");
      }
      error.leb(codePoint);
    },
    (data) => {
      const codePoint = data.num();
      if (codePoint > 0x10ffff || (codePoint >= 0xd800 && codePoint <= 0xdfff)) {
        fail("invalid char");
      }
      return String.fromCodePoint(codePoint);
    },
  );
  const float = (tag: 4 | 5, narrow: boolean): RuntimeCodec =>
    codec(
      [tag],
      (error, value) => {
        if (typeof value !== "number") {
          fail("expected float");
        }
        const count = narrow ? 4 : 8;
        error.room(count);
        const view = new DataView(error.buf.buffer, error.buf.byteOffset + error.len, count);
        if (narrow) {
          view.setFloat32(0, value, true);
        } else {
          view.setFloat64(0, value, true);
        }
        error.len += count;
      },
      (data) => {
        const count = narrow ? 4 : 8;
        if (data.at + count > data.input.length) {
          fail("truncated float");
        }
        const view = new DataView(data.input.buffer, data.input.byteOffset + data.at, count);
        const value = narrow ? view.getFloat32(0, true) : view.getFloat64(0, true);
        data.at += count;
        return value;
      },
    );
  const str = codec(
    [6],
    (error, value) => {
      if (typeof value !== "string") {
        fail("expected string");
      }
      error.str(value);
    },
    (data) => data.str(),
  );
  const bytes = codec(
    [7],
    (error, value) => {
      if (!(value instanceof Uint8Array)) {
        fail("expected Uint8Array");
      }
      error.bytes(value);
    },
    (data) => data.bytes().slice(),
  );
  const json = codec(
    [14],
    (error, value) => error.str(encodePrivate(value)),
    (data) => decodePrivate(data.str()),
  );
  const array = (xs: Dynamic, value: number | null = null): void => {
    if (!Array.isArray(xs) || (value !== null && xs.length !== value)) {
      fail("invalid array");
    }
  };
  const list = (text: RuntimeCodec): RuntimeCodec =>
    codec(
      [8, text.shape],
      (error, xs, tx) => {
        array(xs);
        error.leb(xs.length);
        for (const item of xs) {
          text.enc(error, item, tx);
        }
      },
      (data) => {
        const length = data.num();
        if (!Number.isSafeInteger(length) || length > 0xffffffff) {
          fail("invalid list length");
        }
        return Array.from({ length }, () => text.dec(data));
      },
    );
  const option = (text: RuntimeCodec): RuntimeCodec =>
    codec(
      [9, text.shape],
      (error, value, tx) => {
        if (value === null) {
          error.leb(0);
        } else {
          text.enc(error.leb(1), value, tx);
        }
      },
      (data) => (data.tag(2) ? text.dec(data) : null),
    );
  const tuple = (ts: RuntimeCodec[]): RuntimeCodec =>
    codec(
      [10, ts.map((item) => item.shape)],
      (error, xs, tx) => {
        array(xs, ts.length);
        ts.forEach((item, index) => item.enc(error, xs[index], tx));
      },
      (data) => ts.map((item) => item.dec(data)),
    );
  const fieldShape = (fs: RuntimeField[]): ContractField[] =>
    fs.map(([count, text]) => [count, text.shape]);
  const encFields = (error: Enc, value: Dynamic, fs: RuntimeField[], tx: number[]): void => {
    for (const [count, text] of fs) {
      text.enc(error, own(value, count), tx);
    }
  };
  const decFields = (data: Dec, fs: RuntimeField[]): DynamicRecord =>
    object(fs.map(([count, text]) => [count, text.dec(data)]));
  const record = (fs: RuntimeField[]): RuntimeCodec =>
    codec(
      [11, fieldShape(fs)],
      (error, value, tx) => encFields(error, value, fs, tx),
      (data) => decFields(data, fs),
    );
  const enumeration = (vs: RuntimeVariant[]): RuntimeCodec =>
    codec(
      [12, vs.map(([count, fs]) => [count, fieldShape(fs)])],
      (error, value, tx) => {
        const tag = own(value, "tag"),
          index = vs.findIndex(([count]) => count === tag);
        if (index < 0) {
          fail("unknown enum variant");
        }
        error.leb(index);
        encFields(error, own(value, "fields"), vs[index][1], tx);
      },
      (data) => {
        const [tag, fs] = vs[data.tag(vs.length)];
        return { tag, fields: decFields(data, fs) };
      },
    );
  const result = (ok: RuntimeCodec, err: RuntimeCodec): RuntimeCodec =>
    codec(
      [13, ok.shape, err.shape],
      (error, value, tx) => {
        const tag = own(value, "ok");
        if (typeof tag !== "boolean") {
          fail("invalid Result tag");
        }
        (tag ? ok : err).enc(error.leb(tag ? 0 : 1), own(value, tag ? "value" : "error"), tx);
      },
      (data) =>
        data.tag(2) ? { ok: false, error: err.dec(data) } : { ok: true, value: ok.dec(data) },
    );
  const handle = (namespace: string): RuntimeCodec =>
    codec(
      [15, namespace],
      (error, value, tx) => {
        if ((typeof value !== "object" && typeof value !== "function") || value === null) {
          fail("expected provider handle object");
        }
        const methods = handleMethods.get(namespace);
        if (!methods || methods.some((name) => typeof value[name] !== "function")) {
          fail("returned handle is missing a declared method");
        }
        if (nextHandle > 0x7fffffff) {
          fail("handle space exhausted");
        }
        const id = nextHandle++;
        handles.set(id, { namespace, value: value });
        tx.push(id);
        error.leb(id);
      },
      () => fail("a handle cannot be passed as a data argument"),
    );
  const runtime = {
    unit,
    bool,
    char,
    int,
    f32: float(4, true),
    f64: float(5, false),
    str,
    bytes,
    json,
    list,
    option,
    tuple,
    record,
    enumeration,
    result,
    handle,
  };
  const registrationsFor = (providers: ProviderRegistration[]) => {
    if (!Array.isArray(providers)) {
      fail("providers must be explicit registrations");
    }
    const registrations = new Map<
      string,
      { expected: ContractInterface[]; root: ContractInterface; implementation: DynamicRecord }
    >();
    for (const provider of providers) {
      const expected = readPackageContract(provider.contract);
      const root =
        expected.find(
          (item) =>
            !item.handle &&
            item.module === provider.module &&
            same(item.identity, provider.identity),
        ) ?? fail("invalid or duplicate provider registration");
      if (registrations.has(provider.module)) {
        fail("invalid or duplicate provider registration");
      }
      const implementation = provider.implementation;
      if (
        implementation === null ||
        typeof implementation !== "object" ||
        ![Object.prototype, null].includes(Object.getPrototypeOf(implementation))
      ) {
        fail(`provider for ${named(root)} must default-export a plain object of methods`);
      }
      registrations.set(provider.module, { expected, root, implementation });
    }
    return registrations;
  };
  const validateRegistrations = (
    required: ContractInterface[],
    registrations: ReturnType<typeof registrationsFor>,
  ): void => {
    const checkExpected = (
      needed: ContractInterface,
      expected: ContractInterface[],
      seen = new Set<string>(),
    ): void => {
      if (seen.has(needed.module)) {
        return;
      }
      seen.add(needed.module);
      const item =
        expected.find(
          (candidate) =>
            candidate.module === needed.module &&
            candidate.handle === needed.handle &&
            same(candidate.identity, needed.identity),
        ) ?? fail(`provider identity/schema mismatch for ${named(needed)}`);
      for (const method of needed.methods) {
        if (!item.methods.some((other) => same(method, other))) {
          fail(`provider method/schema mismatch for ${named(needed)}.${method.member}`);
        }
      }
      const visit = (value: ContractValue): void => {
        if (value[0] === 15) {
          const nested =
            required.find((entry) => entry.handle && entry.module === value[1]) ??
            fail("missing handle interface");
          checkExpected(nested, expected, seen);
        }
        if (value[0] === 8 || value[0] === 9) {
          visit(value[1]);
        }
        if (value[0] === 10) {
          value[1].forEach(visit);
        }
        if (value[0] === 11) {
          value[1].forEach(([, type]) => visit(type));
        }
        if (value[0] === 12) {
          value[1].forEach(([, fields]) => fields.forEach(([, type]) => visit(type)));
        }
        if (value[0] === 13) {
          visit(value[1]);
          visit(value[2]);
        }
      };
      for (const method of needed.methods) {
        method.params.forEach(visit);
        visit(method.result);
      }
    };
    for (const item of required.filter((entry) => !entry.handle)) {
      const registration =
        registrations.get(item.module) ??
        fail(`missing provider for ${named(item)}; register it explicitly`);
      checkExpected(item, registration.expected);
      if (
        item.selectors.some(
          (selector) => Object.hasOwn(grants, selector) && grants[selector] === false,
        )
      ) {
        fail(`capability denied: ${named(item)}`);
      }
      for (const method of item.methods) {
        if (
          !Object.hasOwn(registration.implementation, method.member) ||
          typeof registration.implementation[method.member] !== "function"
        ) {
          fail(`missing provider method ${named(item)}.${method.member}`);
        }
      }
    }
  };
  const argumentsFor = (method: RuntimeMethod, isHandle: boolean, raw: Dynamic[]): Dynamic[] => {
    if (!method.params.length) {
      return [];
    }
    const length = raw.shift();
    if (!Number.isInteger(length) || (length as number) < 0) {
      fail("invalid argument buffer length");
    }
    const bytes = Uint8Array.from(io.view(length as number));
    if (!isHandle && method.params.length === 1 && method.params[0] === str) {
      return [decodeUtf8(bytes)];
    }
    const decoder = new Dec(bytes);
    const args = method.params.map((type) => type.dec(decoder));
    if (decoder.at !== bytes.length) {
      fail("trailing argument bytes");
    }
    return args;
  };
  const bind = (
    contract: Uint8Array,
    generated: RuntimeInterface[],
    providers: ProviderRegistration[],
  ): DynamicRecord => {
    const descriptors = generated.map((i) => ({
      ...i,
      methods: i.methods.map((item) => ({ ...item, params: [...item.params] })),
    }));
    const required = readPackageContract(contract);
    const sections = module && WebAssembly.Module.customSections(module, "hd.capabilities");
    if (!sections || sections.length !== 1 || !equalBytes(contract, new Uint8Array(sections[0]))) {
      fail("generated glue does not match the guest contract");
    }
    const shapes = descriptors.map((i) => ({
      ...i,
      methods: i.methods.map((item) => ({
        ...item,
        params: item.params.map((item) => item.shape),
        result: item.result.shape,
      })),
    }));
    if (
      !same(
        shapes,
        required.map(({ module, handle, methods }) => ({ module, handle, methods })),
      )
    ) {
      fail("generated method shapes do not match the contract");
    }
    for (const i of descriptors.filter((i) => i.handle)) {
      handleMethods.set(
        i.module,
        i.methods.map((item) => item.member),
      );
    }
    const registrations = registrationsFor(providers);
    validateRegistrations(required, registrations);
    const imports: DynamicRecord = Object.create(null);
    for (const i of descriptors) {
      const declared =
        required.find((item) => item.module === i.module) ??
        fail("generated interface is absent from the contract");
      const interfaceIdentity = Object.freeze({ ...declared.identity });
      const out: DynamicRecord = Object.create(null);
      if (i.handle) {
        out.$release = (id: number) => {
          const host = handles.get(id);
          if (!host || host.namespace !== i.module) {
            fail("release of an invalid handle");
          }
          handles.delete(id);
        };
      }
      for (const method of i.methods) {
        const key = `${i.module} ${method.name}`;
        const label = `${named(declared)}.${method.member}`;
        const write = (value: Dynamic): Dynamic => {
          const tx: number[] = [];
          try {
            if (method.result === unit) {
              unit.enc(null as never, value, tx);
              return method.wait === "may" ? 0 : undefined;
            }
            const encoder = new Enc();
            method.result.enc(encoder, value, tx);
            return io.put(encoder);
          } catch (error) {
            for (const id of tx) {
              handles.delete(id);
            }
            throw new HostContract(
              `${label}: ${error instanceof Error ? error.message : String(error)}`,
            );
          }
        };
        const invoke = (raw: Dynamic[], signal: AbortSignal): Dynamic => {
          if (lifetime.signal.aborted) {
            fail("instance is closed");
          }
          let receiver = registrations.get(i.module)?.implementation;
          if (i.handle) {
            const host = handles.get(raw.shift()) ?? fail("call of an invalid handle");
            if (host.namespace !== i.module) {
              fail("call of an invalid handle");
            }
            receiver = host.value;
          }
          const args = argumentsFor(method, i.handle, raw);
          if (raw.length) {
            fail("wrong import arity");
          }
          const file = receiver?.[method.member];
          if (typeof file !== "function") {
            fail(`missing provider method ${method.member}`);
          }
          try {
            const context = {
              signal,
              instanceSignal: lifetime.signal,
              pending(
                poll: () => boolean,
                done: () => Dynamic,
                abort: (() => void) | null = null,
                finish: (() => void) | null = null,
              ): Dynamic {
                if (method.wait !== "may") {
                  fail(`non-suspending method ${label} cannot create a pending operation`);
                }
                if (typeof poll !== "function" || typeof done !== "function") {
                  fail("a pending operation needs readiness and result functions");
                }
                if (
                  (abort !== null && typeof abort !== "function") ||
                  (finish !== null && typeof finish !== "function")
                ) {
                  fail("pending cancellation and delivery hooks must be functions");
                }
                if (!io.prepare) {
                  fail("the host cannot create pending operations");
                }
                return io.prepare(poll, done, abort, finish);
              },
            };
            Object.defineProperty(context, "interfaceIdentity", {
              enumerable: true,
              value: interfaceIdentity,
            });
            Object.defineProperty(context, "http", {
              enumerable: true,
              get() {
                if (!io.http) {
                  fail("the host does not provide HTTP policy services");
                }
                return io.http;
              },
            });
            Object.defineProperty(context, "console", {
              enumerable: true,
              get() {
                if (!io.console) {
                  fail("the host does not provide Console policy services");
                }
                return io.console;
              },
            });
            return file.call(receiver, ...args, context);
          } catch (error) {
            throw new HostContract(`${label}: provider threw ${String(error)}`);
          }
        };
        if (method.wait === "never") {
          out[method.name] = (...raw: Dynamic[]) => {
            const value = invoke(raw, lifetime.signal);
            if (value && typeof value.then === "function") {
              Promise.resolve(value).catch(() => {});
              fail(`non-suspending method ${label} returned a Promise`);
            }
            const prepared = io.prepared?.(value);
            if (prepared) {
              try {
                prepared.abort?.();
              } finally {
                fail(`non-suspending method ${label} returned a pending operation`);
              }
            }
            return write(value);
          };
        } else {
          out[`${method.name}.start`] = (...raw: Dynamic[]) => {
            const operation = new AbortController();
            const value = invoke(raw, operation.signal);
            const prepared = io.prepared?.(value);
            if (prepared) {
              const call = (hook: (() => Dynamic) | null | undefined): Dynamic => {
                try {
                  return hook?.();
                } catch (error) {
                  throw new HostContract(`${label}: provider callback threw ${String(error)}`);
                }
              };
              return io.pending({
                at: prepared.at,
                done: () => call(prepared.done),
                method: key,
                poll: () => {
                  const ready = call(prepared.poll);
                  if (typeof ready !== "boolean") {
                    fail(`readiness probe for ${label} did not return a boolean`);
                  }
                  return ready;
                },
                abort: () => {
                  operation.abort();
                  call(prepared.abort);
                },
                finish: prepared.finish ? () => call(prepared.finish) : null,
              });
            }
            if (!value || typeof value.then !== "function") {
              return [0, write(value)];
            }
            let live = true,
              ready = false,
              rejected = false,
              kept: Dynamic = undefined,
              failure: Dynamic = undefined;
            Promise.resolve(value).then(
              (value) => {
                if (live) {
                  kept = value;
                  ready = true;
                }
              },
              (value) => {
                if (live) {
                  failure = value;
                  rejected = true;
                  ready = true;
                }
              },
            );
            return io.pending({
              at: performance.now(),
              done: () => {
                if (rejected) {
                  fail(`provider ${label} rejected: ${String(failure)}`);
                }
                return kept;
              },
              method: key,
              poll: () => ready,
              abort: () => {
                live = false;
                kept = undefined;
                operation.abort();
              },
            });
          };
          out[`${method.name}.finish`] = (id: number) => write(io.finished(id, key));
        }
      }
      imports[i.module] = out;
    }
    return imports;
  };
  return Object.assign(runtime, {
    bind,
    close: () => {
      lifetime.abort();
      handles.clear();
    },
    handleCount: () => handles.size,
  });
}
/* oxlint-enable max-lines-per-function */
