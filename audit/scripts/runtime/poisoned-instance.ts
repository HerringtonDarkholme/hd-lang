// Phase 4.2: does a panic poison the instance, and is a second instance clean?
// Usage: node --experimental-strip-types audit/scripts/runtime/poisoned-instance.ts
import { instantiate } from "../../../src/compiler.ts";

const source = `let counter: i32 = 0

pub fn bump() -> i32:
    counter = counter + 1
    counter

pub fn fail() -> i32:
    counter = counter + 100
    panic("poison")
`;

function call(instance: WebAssembly.Instance, name: string): string {
  const entry = instance.exports[name];
  if (typeof entry !== "function") return `${name}: no export`;
  try {
    return `${name}() = ${String(entry())}`;
  } catch (error) {
    return `${name}() threw ${(error as Error).name}: ${(error as Error).message}`;
  }
}

const first = await instantiate(source);
console.log("instance 1:", call(first.instance, "bump"));
console.log("instance 1:", call(first.instance, "fail"));
console.log("instance 1 after panic:", call(first.instance, "bump"));
console.log("instance 1 after panic:", call(first.instance, "bump"));
const second = await instantiate(source);
console.log("instance 2 (fresh):", call(second.instance, "bump"));
