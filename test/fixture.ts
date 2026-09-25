import { readFileSync } from "node:fs";
import { resolve } from "node:path";

const fixtureRoot = resolve(import.meta.dirname, "fixtures");

export function fixture(path: string): string {
  return readFileSync(resolve(fixtureRoot, `${path}.hd`), "utf8");
}

export function fixtureBody(path: string): string {
  return fixture(path).replace(/^(?:# (?:test|expect(?:-result)?):[^\n]*\n)+\n?/, "");
}
