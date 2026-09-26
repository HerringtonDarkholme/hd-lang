import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const fixtureRoot = resolve(import.meta.dirname, "fixtures");
const conformanceRoot = resolve(import.meta.dirname, "../spec/conformance");

export function fixture(path: string): string {
  return readFileSync(resolve(fixtureRoot, `${path}.hd`), "utf8");
}

export function fixtureBody(path: string): string {
  return fixture(path).replace(/^(?:# (?:test|expect(?:-result)?):[^\n]*\n)+\n?/, "");
}

// Reads a conformance fixture by its path under spec/conformance, without `.hd`.
export function conformance(path: string): string {
  return readFileSync(resolve(conformanceRoot, `${path}.hd`), "utf8");
}

export function conformanceBody(path: string): string {
  return conformance(path).replace(/^(?:# (?:test|expect(?:-result)?):[^\n]*\n)+\n?/, "");
}
