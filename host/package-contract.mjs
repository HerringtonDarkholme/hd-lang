// Decode the compiler's package-host data section without loading provider
// codecs. Ordinary programs need this validation but never instantiate a
// package runtime.
import { HostContract } from "./wire.mts";

const fail = (message) => {
  throw new HostContract(`package host: ${message}`);
};

export function readPackageContract(bytes) {
  if (!(bytes instanceof Uint8Array)) {
    fail("contract is not bytes");
  }
  let at = 0;
  const byte = () => {
    if (at >= bytes.length) {
      fail("truncated contract");
    }
    return bytes[at++];
  };
  const boolean = () => {
    const v = byte();
    if (v > 1) {
      fail("invalid contract boolean");
    }
    return v === 1;
  };
  const count = () => {
    let n = 0;
    for (let shift = 0; shift < 35; shift += 7) {
      const b = byte();
      if (shift === 28 && b > 15) {
        fail("oversized contract length");
      }
      n += (b & 127) * 2 ** shift;
      if (b < 128) {
        if ((shift && b === 0) || n > bytes.length - at) {
          fail("invalid contract length");
        }
        return n;
      }
    }
    fail("invalid contract length");
  };
  const string = () => {
    const n = count();
    const b = bytes.subarray(at, at + n);
    at += n;
    try {
      return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(b);
    } catch {
      fail("invalid contract UTF-8");
    }
  };
  const vector = (read) => Array.from({ length: count() }, read);
  const fields = (depth) => vector(() => [string(), value(depth + 1)]);
  const value = (depth = 0) => {
    if (depth > 32) {
      fail("contract nesting exceeds the boundary limit");
    }
    const tag = byte();
    if ([0, 1, 2, 4, 5, 6, 7, 14].includes(tag)) {
      return [tag];
    }
    if (tag === 3) {
      const bits = byte();
      const signed = boolean();
      if (![8, 16, 32, 64].includes(bits)) {
        fail("invalid integer width");
      }
      return [tag, bits, signed];
    }
    if (tag === 8 || tag === 9) {
      return [tag, value(depth + 1)];
    }
    if (tag === 10) {
      return [tag, vector(() => value(depth + 1))];
    }
    if (tag === 11) {
      return [tag, fields(depth)];
    }
    if (tag === 12) {
      return [tag, vector(() => [string(), fields(depth)])];
    }
    if (tag === 13) {
      return [tag, value(depth + 1), value(depth + 1)];
    }
    if (tag === 15) {
      return [tag, string()];
    }
    fail("unknown contract value tag");
  };
  if ([byte(), byte(), byte(), byte()].join(",") !== "1,0,0,0") {
    fail("unsupported ABI version");
  }
  const interfaces = vector(() => ({
    identity: { package: string(), version: string(), trait_path: string() },
    module: string(),
    selectors: vector(string),
    handle: boolean(),
    methods: vector(() => ({
      name: string(),
      member: string(),
      wait: boolean() ? "may" : "never",
      params: vector(() => value()),
      result: value(),
    })),
  }));
  if (at !== bytes.length) {
    fail("trailing contract bytes");
  }
  const identities = new Set(),
    modules = new Set(),
    selectors = new Set();
  const unique = (set, k) => {
    if (!k || set.has(k)) {
      fail("empty or duplicate contract name");
    }
    set.add(k);
  };
  const checkValue = (v) => {
    const [tag, a, b] = v;
    if (tag === 15 && !interfaces.some((i) => i.handle && i.module === a)) {
      fail("missing handle interface");
    }
    if (tag === 8 || tag === 9) {
      checkValue(a);
    }
    if (tag === 10) {
      a.forEach(checkValue);
    }
    if (tag === 11) {
      checkFields(a);
    }
    if (tag === 12) {
      const names = new Set();
      for (const [name, fs] of a) {
        unique(names, name);
        checkFields(fs);
      }
    }
    if (tag === 13) {
      checkValue(a);
      checkValue(b);
    }
  };
  const checkFields = (fs) => {
    const names = new Set();
    for (const [name, v] of fs) {
      unique(names, name);
      checkValue(v);
    }
  };
  for (const i of interfaces) {
    if (
      !i.identity.package ||
      !i.identity.trait_path ||
      !i.module.startsWith("hd:pkg:") ||
      i.module.length === 7
    ) {
      fail("invalid nominal identity");
    }
    unique(identities, JSON.stringify([i.identity, i.handle]));
    unique(modules, i.module);
    for (const selector of i.selectors) {
      const parts = selector.split(".");
      if (
        i.handle ||
        parts.length < 2 ||
        !["dep", "pkg"].includes(parts[0]) ||
        parts.some((p) => !p)
      ) {
        fail("invalid source selector");
      }
      unique(selectors, selector);
    }
    const methods = new Set(),
      imports = new Set(i.handle ? ["$release"] : []);
    for (const m of i.methods) {
      unique(methods, m.name);
      if (!m.member) {
        fail("empty method member");
      }
      for (const name of m.wait === "may" ? [`${m.name}.start`, `${m.name}.finish`] : [m.name]) {
        unique(imports, name);
      }
      m.params.forEach(checkValue);
      checkValue(m.result);
    }
  }
  return interfaces;
}
