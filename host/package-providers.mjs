// Helpers used by signature-generated NAME.host.mjs modules. Providers are
// trusted JS selected from package manifests after the root grants access.
// Guest contract data is never interpreted as module code or import paths.
import { Dec, Enc, HostContract, decodeUtf8 } from "./wire.mts";
export { readPackageContract } from "./package-contract.mjs";
import { readPackageContract } from "./package-contract.mjs";

const fail = (message) => {
  throw new HostContract(`package host: ${message}`);
};
const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
const equalBytes = (a, b) => a.length === b.length && a.every((v, i) => v === b[i]);
const own = (v, k) => {
  if (v === null || typeof v !== "object" || !Object.hasOwn(v, k)) {
    fail(`missing field ${k}`);
  }
  return v[k];
};
const object = (fields) => Object.fromEntries(fields);
const named = (i) => `${i.identity.package}@${i.identity.version}/${i.identity.trait_path}`;

// Typed Serde transport is internal. Provider methods receive JS values:
// bigint integers, number floats, and ordinary arrays/objects. Using a flat
// stream preserves numeric kind and avoids multiplying container depth.
function encodePrivate(value) {
  const tokens = [];
  const pending = [{ value }];
  const active = new Set();
  while (pending.length) {
    const job = pending.pop();
    if (job.leave) {
      active.delete(job.leave);
      continue;
    }
    const v = job.value;
    if (v === null) {
      tokens.push(["n"]);
    } else if (typeof v === "boolean") {
      tokens.push(["b", v]);
    } else if (typeof v === "string") {
      tokens.push(["s", v]);
    } else if (typeof v === "bigint") {
      if (v < -(1n << 63n) || v >= 1n << 64n) {
        fail("private integer outside 64-bit range");
      }
      tokens.push([v < 0n ? "i" : "u", String(v)]);
    } else if (typeof v === "number") {
      if (!Number.isFinite(v)) {
        tokens.push(["n"]);
      } else {
        const bits = new DataView(new ArrayBuffer(8));
        bits.setFloat64(0, v, true);
        tokens.push(["f", bits.getBigUint64(0, true).toString(16).padStart(16, "0")]);
      }
    } else if (typeof v === "object") {
      if (active.has(v)) {
        fail("cyclic private value");
      }
      active.add(v);
      pending.push({ leave: v });
      if (Array.isArray(v)) {
        tokens.push(["a", v.length]);
        for (let i = v.length - 1; i >= 0; --i) {
          pending.push({ value: v[i] });
        }
      } else {
        if (![Object.prototype, null].includes(Object.getPrototypeOf(v))) {
          fail("private value must be a plain object");
        }
        const entries = Object.entries(v);
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

function decodePrivate(text) {
  let tokens;
  try {
    tokens = JSON.parse(text);
  } catch {
    fail("invalid private value transport");
  }
  if (!Array.isArray(tokens)) {
    fail("invalid private value transport");
  }
  const values = [];
  for (let i = tokens.length - 1; i >= 0; --i) {
    const token = tokens[i];
    if (!Array.isArray(token)) {
      fail("invalid private value token");
    }
    const [tag, v] = token;
    if (token.length !== (tag === "n" ? 1 : 2)) {
      fail("invalid private value token");
    }
    if (tag === "n") {
      values.push(null);
    } else if (tag === "b" && typeof v === "boolean") {
      values.push(v);
    } else if (tag === "s" && typeof v === "string") {
      values.push(v);
    } else if (["i", "u", "f"].includes(tag)) {
      if (
        typeof v !== "string" ||
        !(tag === "f" ? /^[0-9a-f]{16}$/ : /^(?:0|-?[1-9][0-9]*)$/).test(v)
      ) {
        fail("invalid private numeric token");
      }
      const n = BigInt(tag === "f" ? "0x" + v : v);
      if (tag === "i") {
        if (n < -(1n << 63n) || n >= 1n << 63n) {
          fail("private signed integer out of range");
        }
        values.push(n);
      } else {
        if (n < 0n || n >= 1n << 64n) {
          fail("private unsigned integer out of range");
        }
        if (tag === "u") {
          values.push(n);
        } else {
          const bits = new DataView(new ArrayBuffer(8));
          bits.setBigUint64(0, n, true);
          const number = bits.getFloat64(0, true);
          if (!Number.isFinite(number)) {
            fail("invalid private float");
          }
          values.push(number);
        }
      }
    } else if (tag === "a" || tag === "o") {
      if (!Number.isSafeInteger(v) || v < 0 || v > values.length / (tag === "o" ? 2 : 1)) {
        fail("invalid private container length");
      }
      if (tag === "a") {
        values.push(Array.from({ length: v }, () => values.pop()));
      } else {
        const entries = [];
        const keys = new Set();
        for (let n = 0; n < v; ++n) {
          const key = values.pop();
          if (typeof key !== "string" || keys.has(key)) {
            fail("invalid private object key");
          }
          keys.add(key);
          entries.push([key, values.pop()]);
        }
        values.push(Object.fromEntries(entries));
      }
    } else {
      fail("invalid private value token");
    }
  }
  if (values.length !== 1) {
    fail("invalid private value framing");
  }
  return values[0];
}

// Each codec carries its canonical shape, checked against the data section
// before an instance starts. Encoding validates before the wire writer can
// coerce values (notably a negative unsigned LEB value).
export function createPackageRuntime(io, { module, grants = {} } = {}) {
  const handles = new Map();
  const handleMethods = new Map();
  let nextHandle = 1;
  const lifetime = new AbortController();
  const codec = (shape, enc, dec) => ({ shape, enc, dec });
  const unit = codec(
    [0],
    (_e, v) => {
      if (v !== undefined) {
        fail("void result has a value");
      }
    },
    () => undefined,
  );
  const bool = codec(
    [1],
    (e, v) => {
      if (typeof v !== "boolean") {
        fail("expected bool");
      }
      e.leb(v ? 1 : 0);
    },
    (d) => d.tag(2) === 1,
  );
  const int = (bits, signed) => {
    const low = signed ? -(1n << BigInt(bits - 1)) : 0n;
    const high = (1n << BigInt(bits - (signed ? 1 : 0))) - 1n;
    const check = (v) => {
      if (typeof v !== "bigint" && !(typeof v === "number" && Number.isSafeInteger(v))) {
        fail("expected integer");
      }
      const n = BigInt(v);
      if (n < low || n > high) {
        fail("integer is out of range");
      }
      return n;
    };
    return codec(
      [3, bits, signed],
      (e, v) => {
        const n = check(v);
        signed ? e.zz(n) : e.leb(n);
      },
      (d) => {
        const n = check(signed ? d.zz() : d.leb());
        return bits === 64 ? n : Number(n);
      },
    );
  };
  const char = codec(
    [2],
    (e, v) => {
      if (typeof v !== "string" || [...v].length !== 1) {
        fail("expected char");
      }
      const n = v.codePointAt(0);
      if (n >= 0xd800 && n <= 0xdfff) {
        fail("invalid char");
      }
      e.leb(n);
    },
    (d) => {
      const n = d.num();
      if (n > 0x10ffff || (n >= 0xd800 && n <= 0xdfff)) {
        fail("invalid char");
      }
      return String.fromCodePoint(n);
    },
  );
  const float = (tag, narrow) =>
    codec(
      [tag],
      (e, v) => {
        if (typeof v !== "number") {
          fail("expected float");
        }
        const n = narrow ? 4 : 8;
        e.room(n);
        const view = new DataView(e.buf.buffer, e.buf.byteOffset + e.len, n);
        if (narrow) {
          view.setFloat32(0, v, true);
        } else {
          view.setFloat64(0, v, true);
        }
        e.len += n;
      },
      (d) => {
        const n = narrow ? 4 : 8;
        if (d.at + n > d.b.length) {
          fail("truncated float");
        }
        const view = new DataView(d.b.buffer, d.b.byteOffset + d.at, n);
        const v = narrow ? view.getFloat32(0, true) : view.getFloat64(0, true);
        d.at += n;
        return v;
      },
    );
  const str = codec(
    [6],
    (e, v) => {
      if (typeof v !== "string") {
        fail("expected string");
      }
      e.str(v);
    },
    (d) => d.str(),
  );
  const bytes = codec(
    [7],
    (e, v) => {
      if (!(v instanceof Uint8Array)) {
        fail("expected Uint8Array");
      }
      e.bytes(v);
    },
    (d) => d.bytes().slice(),
  );
  const json = codec(
    [14],
    (e, v) => e.str(encodePrivate(v)),
    (d) => decodePrivate(d.str()),
  );
  const array = (xs, n = null) => {
    if (!Array.isArray(xs) || (n !== null && xs.length !== n)) {
      fail("invalid array");
    }
  };
  const list = (t) =>
    codec(
      [8, t.shape],
      (e, xs, tx) => {
        array(xs);
        e.leb(xs.length);
        for (const x of xs) {
          t.enc(e, x, tx);
        }
      },
      (d) => {
        const n = d.num();
        if (!Number.isSafeInteger(n) || n > 0xffffffff) {
          fail("invalid list length");
        }
        return Array.from({ length: n }, () => t.dec(d));
      },
    );
  const option = (t) =>
    codec(
      [9, t.shape],
      (e, v, tx) => {
        if (v === null) {
          e.leb(0);
        } else {
          t.enc(e.leb(1), v, tx);
        }
      },
      (d) => (d.tag(2) ? t.dec(d) : null),
    );
  const tuple = (ts) =>
    codec(
      [10, ts.map((t) => t.shape)],
      (e, xs, tx) => {
        array(xs, ts.length);
        ts.forEach((t, n) => t.enc(e, xs[n], tx));
      },
      (d) => ts.map((t) => t.dec(d)),
    );
  const fieldShape = (fs) => fs.map(([n, t]) => [n, t.shape]);
  const encFields = (e, v, fs, tx) => {
    for (const [n, t] of fs) {
      t.enc(e, own(v, n), tx);
    }
  };
  const decFields = (d, fs) => object(fs.map(([n, t]) => [n, t.dec(d)]));
  const record = (fs) =>
    codec(
      [11, fieldShape(fs)],
      (e, v, tx) => encFields(e, v, fs, tx),
      (d) => decFields(d, fs),
    );
  const enumeration = (vs) =>
    codec(
      [12, vs.map(([n, fs]) => [n, fieldShape(fs)])],
      (e, v, tx) => {
        const tag = own(v, "tag"),
          index = vs.findIndex(([n]) => n === tag);
        if (index < 0) {
          fail("unknown enum variant");
        }
        e.leb(index);
        encFields(e, own(v, "fields"), vs[index][1], tx);
      },
      (d) => {
        const [tag, fs] = vs[d.tag(vs.length)];
        return { tag, fields: decFields(d, fs) };
      },
    );
  const result = (ok, err) =>
    codec(
      [13, ok.shape, err.shape],
      (e, v, tx) => {
        const tag = own(v, "ok");
        if (typeof tag !== "boolean") {
          fail("invalid Result tag");
        }
        (tag ? ok : err).enc(e.leb(tag ? 0 : 1), own(v, tag ? "value" : "error"), tx);
      },
      (d) => (d.tag(2) ? { ok: false, error: err.dec(d) } : { ok: true, value: ok.dec(d) }),
    );
  const handle = (namespace) =>
    codec(
      [15, namespace],
      (e, v, tx) => {
        if ((typeof v !== "object" && typeof v !== "function") || v === null) {
          fail("expected provider handle object");
        }
        const methods = handleMethods.get(namespace);
        if (!methods || methods.some((name) => typeof v[name] !== "function")) {
          fail("returned handle is missing a declared method");
        }
        if (nextHandle > 0x7fffffff) {
          fail("handle space exhausted");
        }
        const id = nextHandle++;
        handles.set(id, { namespace, value: v });
        tx.push(id);
        e.leb(id);
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
  const bind = (contract, generated, providers) => {
    const descriptors = generated.map((i) => ({
      ...i,
      methods: i.methods.map((m) => ({ ...m, params: [...m.params] })),
    }));
    const required = readPackageContract(contract);
    const sections = module && WebAssembly.Module.customSections(module, "hd.capabilities");
    if (!sections || sections.length !== 1 || !equalBytes(contract, new Uint8Array(sections[0]))) {
      fail("generated glue does not match the guest contract");
    }
    const shapes = descriptors.map((i) => ({
      ...i,
      methods: i.methods.map((m) => ({
        ...m,
        params: m.params.map((p) => p.shape),
        result: m.result.shape,
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
        i.methods.map((m) => m.member),
      );
    }
    if (!Array.isArray(providers)) {
      fail("providers must be explicit registrations");
    }
    const registrations = new Map();
    for (const provider of providers) {
      const expected = readPackageContract(provider.contract);
      const root = expected.find(
        (i) => !i.handle && i.module === provider.module && same(i.identity, provider.identity),
      );
      if (!root || registrations.has(provider.module)) {
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
    const checkExpected = (needed, expected, seen = new Set()) => {
      if (seen.has(needed.module)) {
        return;
      }
      seen.add(needed.module);
      const i = expected.find(
        (i) =>
          i.module === needed.module &&
          i.handle === needed.handle &&
          same(i.identity, needed.identity),
      );
      if (!i) {
        fail(`provider identity/schema mismatch for ${named(needed)}`);
      }
      for (const m of needed.methods) {
        if (!i.methods.some((other) => same(m, other))) {
          fail(`provider method/schema mismatch for ${named(needed)}.${m.member}`);
        }
      }
      const visit = (v) => {
        if (v[0] === 15) {
          checkExpected(
            required.find((i) => i.handle && i.module === v[1]),
            expected,
            seen,
          );
        }
        if (v[0] === 8 || v[0] === 9) {
          visit(v[1]);
        }
        if (v[0] === 10) {
          v[1].forEach(visit);
        }
        if (v[0] === 11) {
          v[1].forEach(([, t]) => visit(t));
        }
        if (v[0] === 12) {
          v[1].forEach(([, fs]) => fs.forEach(([, t]) => visit(t)));
        }
        if (v[0] === 13) {
          visit(v[1]);
          visit(v[2]);
        }
      };
      for (const m of needed.methods) {
        m.params.forEach(visit);
        visit(m.result);
      }
    };
    for (const i of required.filter((i) => !i.handle)) {
      const registration = registrations.get(i.module);
      if (!registration) {
        fail(`missing provider for ${named(i)}; register it explicitly`);
      }
      checkExpected(i, registration.expected);
      if (i.selectors.some((s) => Object.hasOwn(grants, s) && grants[s] === false)) {
        fail(`capability denied: ${named(i)}`);
      }
      for (const m of i.methods) {
        if (
          !Object.hasOwn(registration.implementation, m.member) ||
          typeof registration.implementation[m.member] !== "function"
        ) {
          fail(`missing provider method ${named(i)}.${m.member}`);
        }
      }
    }
    const imports = Object.create(null);
    for (const i of descriptors) {
      const declared = required.find((r) => r.module === i.module);
      const interfaceIdentity = Object.freeze({ ...declared.identity });
      const out = Object.create(null);
      if (i.handle) {
        out.$release = (id) => {
          const h = handles.get(id);
          if (!h || h.namespace !== i.module) {
            fail("release of an invalid handle");
          }
          handles.delete(id);
        };
      }
      for (const m of i.methods) {
        const key = `${i.module} ${m.name}`;
        const label = `${named(declared)}.${m.member}`;
        const write = (value) => {
          const tx = [];
          try {
            if (m.result === unit) {
              unit.enc(null, value);
              return m.wait === "may" ? 0 : undefined;
            }
            const e = new Enc();
            m.result.enc(e, value, tx);
            return io.put(e);
          } catch (error) {
            for (const id of tx) {
              handles.delete(id);
            }
            throw new HostContract(`${label}: ${error.message}`);
          }
        };
        const invoke = (raw, signal) => {
          if (lifetime.signal.aborted) {
            fail("instance is closed");
          }
          let receiver = registrations.get(i.module)?.implementation;
          if (i.handle) {
            const h = handles.get(raw.shift());
            if (!h || h.namespace !== i.module) {
              fail("call of an invalid handle");
            }
            receiver = h.value;
          }
          let args = [];
          if (m.params.length) {
            const len = raw.shift();
            if (!Number.isInteger(len) || len < 0) {
              fail("invalid argument buffer length");
            }
            const bytes = Uint8Array.from(io.view(len));
            if (!i.handle && m.params.length === 1 && m.params[0] === str) {
              args = [decodeUtf8(bytes)];
            } else {
              const d = new Dec(bytes);
              args = m.params.map((t) => t.dec(d));
              if (d.at !== bytes.length) {
                fail("trailing argument bytes");
              }
            }
          }
          if (raw.length) {
            fail("wrong import arity");
          }
          const f = receiver?.[m.member];
          if (typeof f !== "function") {
            fail(`missing provider method ${m.member}`);
          }
          try {
            const context = {
              signal,
              instanceSignal: lifetime.signal,
              pending(poll, done, abort = null, finish = null) {
                if (m.wait !== "may") {
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
            return f.call(receiver, ...args, context);
          } catch (error) {
            throw new HostContract(`${label}: provider threw ${String(error)}`);
          }
        };
        if (m.wait === "never") {
          out[m.name] = (...raw) => {
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
          out[`${m.name}.start`] = (...raw) => {
            const operation = new AbortController();
            const value = invoke(raw, operation.signal);
            const prepared = io.prepared?.(value);
            if (prepared) {
              const call = (hook) => {
                try {
                  return hook?.();
                } catch (error) {
                  throw new HostContract(`${label}: provider callback threw ${String(error)}`);
                }
              };
              return io.pending(
                prepared.at,
                () => call(prepared.done),
                key,
                () => {
                  const ready = call(prepared.poll);
                  if (typeof ready !== "boolean") {
                    fail(`readiness probe for ${label} did not return a boolean`);
                  }
                  return ready;
                },
                () => {
                  operation.abort();
                  call(prepared.abort);
                },
                prepared.finish ? () => call(prepared.finish) : null,
              );
            }
            if (!value || typeof value.then !== "function") {
              return [0, write(value)];
            }
            let live = true,
              ready = false,
              rejected = false,
              kept,
              failure;
            Promise.resolve(value).then(
              (v) => {
                if (live) {
                  kept = v;
                  ready = true;
                }
              },
              (e) => {
                if (live) {
                  failure = e;
                  rejected = true;
                  ready = true;
                }
              },
            );
            return io.pending(
              performance.now(),
              () => {
                if (rejected) {
                  fail(`provider ${label} rejected: ${String(failure)}`);
                }
                return kept;
              },
              key,
              () => ready,
              () => {
                live = false;
                kept = undefined;
                operation.abort();
              },
            );
          };
          out[`${m.name}.finish`] = (id) => write(io.finished(id, key));
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
