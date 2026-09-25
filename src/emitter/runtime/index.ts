import { readFileSync } from "node:fs";

function readWat(name: string): string {
  return readFileSync(new URL(name, import.meta.url), "utf8");
}

export const RUNTIME_WAT = readWat("runtime.wat");
export const MAP_RUNTIME_WAT = readWat("map.wat");
export const CONSOLE_RUNTIME_WAT = readWat("console.wat");
