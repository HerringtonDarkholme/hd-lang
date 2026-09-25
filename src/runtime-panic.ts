export const RUNTIME_PANIC_NAMES = [
  "explicit-panic",
  "integer-division-by-zero",
  "integer-overflow",
  "suspension-nested-driver",
  "suspension-invalid-state",
  "suspension-reentrant-poll",
  "suspension-competing-driver",
  "assertion-failed",
  "iterator-invalidated",
  "invalid-shift",
  "index-out-of-bounds",
] as const;

export type RuntimePanicName = (typeof RUNTIME_PANIC_NAMES)[number];

export class RuntimePanicError extends Error {
  readonly code: RuntimePanicName;

  constructor(code: RuntimePanicName) {
    super(`${code}: runtime panic`);
    this.name = "RuntimePanicError";
    this.code = code;
  }
}

export function runtimePanicCode(name: RuntimePanicName): number {
  return RUNTIME_PANIC_NAMES.indexOf(name);
}

export function runtimePanicName(code: number): RuntimePanicName {
  return RUNTIME_PANIC_NAMES[code] ?? "explicit-panic";
}
