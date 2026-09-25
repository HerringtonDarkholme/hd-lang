import { readFileSync } from "node:fs";

function readWat(name: string): string {
  return readFileSync(new URL(name, import.meta.url), "utf8");
}

export const RUNTIME_WAT = readWat("runtime.wat");
export const MAP_RUNTIME_WAT = readWat("map.wat");
export const CONSOLE_RUNTIME_WAT = readWat("console.wat");
export const FLOAT_RUNTIME_WAT = readWat("float.wat");
export const STRING_TRANSFORM_RUNTIME_WAT = readWat("string-transform.wat");
export const STRING_SPLIT_RUNTIME_WAT = readWat("string-split.wat");
