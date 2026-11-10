// Decode the compiler's package-host data section without loading provider
// codecs. Ordinary programs need this validation but never instantiate a
// package runtime.
import { HostContract } from "./wire.ts";

export type ContractValue =
  | [0 | 1 | 2 | 4 | 5 | 6 | 7 | 14 | 19]
  | [3, number, boolean]
  | [8 | 9, ContractValue]
  | [10, ContractValue[]]
  | [11, ContractField[]]
  | [12, [string, ContractField[]][]]
  | [13 | 18, ContractValue, ContractValue]
  | [15, string]
  | [16, number, ContractValue]
  | [17, number];
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
  // The recursive types around the value being checked, outermost first,
  // and how many of them a list, option, result or enum of more than one
  // variant guards (hd_host_abi schema.rs `Binders`).
  const binders: number[] = [];
  let guard = 0;
  const checkValue = (value: ContractValue): void => {
    const outer = guard;
    if (
      [8, 9, 13, 18].includes(value[0]) ||
      (value[0] === 12 && (value[1] as unknown[]).length > 1)
    ) {
      guard = binders.length;
    }
    checkShape(value);
    guard = outer;
  };
  const checkShape = (value: ContractValue): void => {
    const [tag, left, right] = value;
    // `never` is a method's whole result only (module.boundary.never-result).
    if (tag === 19) {
      fail("never outside a method's whole result");
    }
    if (tag === 15 && !interfaces.some((item) => item.handle && item.module === left)) {
      fail("missing handle interface");
    }
    if (tag === 8 || tag === 9) {
      checkValue(left);
    }
    if (tag === 10) {
      left.forEach(checkValue);
    }
    if (tag === 11) {
      checkFields(left);
    }
    if (tag === 12) {
      const names = new Set<string>();
      for (const [name, fields] of left) {
        unique(names, name);
        checkFields(fields);
      }
    }
    if (tag === 13 || tag === 18) {
      checkValue(left);
      checkValue(right);
    }
    if (tag === 16 || tag === 17) {
      checkRecursive(value);
    }
  };
  // `[16, id, body]` binds `id` over a struct or enum body; `[17, id]`
  // names a binder around it, guarded since.
  const checkRecursive = (value: ContractValue): void => {
    if (value[0] === 16) {
      const [, id, body] = value;
      if (binders.includes(id) || ![11, 12].includes(body[0])) {
        fail("invalid recursive type");
      }
      binders.push(id);
      checkValue(body);
      binders.pop();
    } else if (value[0] === 17) {
      const at = binders.lastIndexOf(value[1]);
      if (at < 0 || at >= guard) {
        fail("unbound or unguarded recursive type");
      }
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
      if (method.result[0] !== 19) {
        checkValue(method.result);
      }
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
    const byteValue = byte();
    if (byteValue > 1) {
      fail("invalid contract boolean");
    }
    return byteValue === 1;
  };
  // A canonical unsigned LEB128 number below 2^32.
  const number = (): number => {
    let count = 0;
    for (let shift = 0; shift < 35; shift += 7) {
      const byteValue = byte();
      if (shift === 28 && byteValue > 15) {
        fail("oversized contract length");
      }
      count += (byteValue & 127) * 2 ** shift;
      if (byteValue < 128) {
        if (shift && byteValue === 0) {
          fail("invalid contract length");
        }
        return count;
      }
    }
    return fail("invalid contract length");
  };
  // A length, which the remaining bytes must hold.
  const count = (): number => {
    const count = number();
    if (count > bytes.length - at) {
      fail("invalid contract length");
    }
    return count;
  };
  const string = (): string => {
    const length = count();
    const chunk = bytes.subarray(at, at + length);
    at += length;
    try {
      return new TextDecoder("utf-8", { fatal: true, ignoreBOM: true }).decode(chunk);
    } catch {
      return fail("invalid contract UTF-8");
    }
  };
  const vector = <Item>(read: () => Item): Item[] => Array.from({ length: count() }, read);
  const fields = (depth: number): ContractField[] =>
    vector<ContractField>(() => [string(), value(depth + 1)]);
  const value = (depth = 0): ContractValue => {
    if (depth > 32) {
      fail("contract nesting exceeds the boundary limit");
    }
    const tag = byte();
    if ([0, 1, 2, 4, 5, 6, 7, 14, 19].includes(tag)) {
      return [tag as 0 | 1 | 2 | 4 | 5 | 6 | 7 | 14 | 19];
    }
    if (tag === 3) {
      const bits = byte();
      const signed = boolean();
      if (![8, 16, 32, 64].includes(bits)) {
        fail("invalid integer width");
      }
      return [tag, bits, signed];
    }
    if ([8, 9].includes(tag)) {
      return [tag as 8 | 9, value(depth + 1)];
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
    if ([13, 18].includes(tag)) {
      return [tag as 13 | 18, value(depth + 1), value(depth + 1)];
    }
    if (tag === 15) {
      return [tag, string()];
    }
    if (tag === 16) {
      return [tag, number(), value(depth + 1)];
    }
    if (tag === 17) {
      return [tag, number()];
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
