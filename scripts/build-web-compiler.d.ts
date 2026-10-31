import type { PathLike } from "node:fs";

/** The compiler Wasm artifact served by the playground. */
export const COMPILER_WASM: string;
export const WAT_WASM: string;
/** Raw and gzip (level 9) byte counts of a file. */
export function sizes(path: PathLike | number): { raw: number; gzip: number };
/** Format a byte count with its decimal megabyte equivalent. */
export function formatBytes(bytes: number): string;
