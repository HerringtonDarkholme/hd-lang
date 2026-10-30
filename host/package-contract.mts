// Decode the compiler's package-host data section without loading provider
// codecs. Ordinary programs need this validation but never instantiate a
// package runtime.
import { HostContract } from "./wire.mts";

export type ContractValue =
  | [0 | 1 | 2 | 4 | 5 | 6 | 7 | 14]
  | [3, number, boolean]
  | [8 | 9, ContractValue]
  | [10, ContractValue[]]
  | [11, ContractField[]]
  | [12, [string, ContractField[]][]]
  | [13, ContractValue, ContractValue]
  | [15, string];
export type ContractField = [string, ContractValue];
export interface ContractIdentity {
  package: string;
  version: string;
  trait_path: string;
}
export interface ContractMethod {
  name: string;
  member: string;
  wait: "never" | "may";
  params: ContractValue[];
  result: ContractValue;
}
export interface ContractInterface {
  identity: ContractIdentity;
  module: string;
  selectors: string[];
  handle: boolean;
  methods: ContractMethod[];
}

const fail = (message: string): never => {
  throw new HostContract(`package host: ${message}`);
};

function validSelector(item: ContractInterface, selector: string): boolean {
  const parts = selector.split(".");
  return (
    !item.handle && parts.length >= 2 && ["dep", "pkg"].includes(parts[0]!) && parts.every(Boolean)
  );
}

function validateContract(interfaces: ContractInterface[]): void {
  const identities = new Set<string>();
  const modules = new Set<string>();
  const selectors = new Set<string>();
  const unique = (set: Set<string>, key: string): void => {
    if (!key || set.has(key)) {
      fail("empty or duplicate contract name");
    }
    set.add(key);
  };
  const checkFields = (fields: ContractField[]): void => {
    const names = new Set<string>();
    for (const [name, value] of fields) {
      unique(names, name);
      checkValue(value);
    }
  };
  const checkValue = (value: ContractValue): void => {
    const [tag, a, b] = value;
    if (tag === 15 && !interfaces.some((item) => item.handle && item.module === a)) {
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
      const names = new Set<string>();
      for (const [name, fields] of a) {
        unique(names, name);
        checkFields(fields);
      }
    }
    if (tag === 13) {
      checkValue(a);
      checkValue(b);
    }
  };
  for (const item of interfaces) {
    if (
      !item.identity.package ||
      !item.identity.trait_path ||
      !item.module.startsWith("hd:pkg:") ||
      item.module.length === 7
    ) {
      fail("invalid nominal identity");
    }
    unique(identities, JSON.stringify([item.identity, item.handle]));
    unique(modules, item.module);
    for (const selector of item.selectors) {
      if (!validSelector(item, selector)) {
        fail("invalid source selector");
      }
      unique(selectors, selector);
    }
    const methods = new Set<string>();
    const imports = new Set<string>(item.handle ? ["$release"] : []);
    for (const method of item.methods) {
      unique(methods, method.name);
      if (!method.member) {
        fail("empty method member");
      }
      for (const name of method.wait === "may"
        ? [`${method.name}.start`, `${method.name}.finish`]
        : [method.name]) {
        unique(imports, name);
      }
      method.params.forEach(checkValue);
      checkValue(method.result);
    }
  }
}

export function readPackageContract(bytes: Uint8Array): ContractInterface[] {
  if (!(bytes instanceof Uint8Array)) {
    fail("contract is not bytes");
  }
  let at = 0;
  const byte = (): number => {
    if (at >= bytes.length) {
      fail("truncated contract");
    }
    return bytes[at++]!;
  };
  const boolean = (): boolean => {
    const v = byte();
    if (v > 1) {
      fail("invalid contract boolean");
    }
    return v === 1;
  };
  const count = (): number => {
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
    return fail("invalid contract length");
  };
  const string = (): string => {
    const n = count();
    const b = bytes.subarray(at, at + n);
    at += n;
    try {
      return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(b);
    } catch {
      return fail("invalid contract UTF-8");
    }
  };
  const vector = <T,>(read: () => T): T[] => Array.from({ length: count() }, read);
  const fields = (depth: number): ContractField[] =>
    vector<ContractField>(() => [string(), value(depth + 1)]);
  const value = (depth = 0): ContractValue => {
    if (depth > 32) {
      fail("contract nesting exceeds the boundary limit");
    }
    const tag = byte();
    if ([0, 1, 2, 4, 5, 6, 7, 14].includes(tag)) {
      return [tag as 0 | 1 | 2 | 4 | 5 | 6 | 7 | 14];
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
      return [tag, vector<[string, ContractField[]]>(() => [string(), fields(depth)])];
    }
    if (tag === 13) {
      return [tag, value(depth + 1), value(depth + 1)];
    }
    if (tag === 15) {
      return [tag, string()];
    }
    return fail("unknown contract value tag");
  };
  if ([byte(), byte(), byte(), byte()].join(",") !== "1,0,0,0") {
    fail("unsupported ABI version");
  }
  const interfaces = vector<ContractInterface>(() => ({
    identity: { package: string(), version: string(), trait_path: string() },
    module: string(),
    selectors: vector(string),
    handle: boolean(),
    methods: vector(() => ({
      name: string(),
      member: string(),
      wait: boolean() ? ("may" as const) : ("never" as const),
      params: vector(() => value()),
      result: value(),
    })),
  }));
  if (at !== bytes.length) {
    fail("trailing contract bytes");
  }
  validateContract(interfaces);
  return interfaces;
}
