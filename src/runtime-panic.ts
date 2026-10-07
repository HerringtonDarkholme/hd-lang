import type { SourceSpan } from "./diagnostics.ts";

export const RUNTIME_PANIC_NAMES = [
  "explicit-panic",
  "integer-division-by-zero",
  "integer-overflow",
  "suspension-invalid-state",
  "suspension-reentrant-poll",
  "suspension-competing-driver",
  "assertion-failed",
  "iterator-invalidated",
  "invalid-shift",
  "index-out-of-bounds",
  "structure-variant-mismatch",
  // Appended to preserve the prototype's existing numeric Wasm panic codes.
  "host-contract",
  "stack-exhausted",
  // Stable categories the prototype never raises, listed so that
  // `expect_panic` accepts them (06-control-flow.md#r-flow.panic.stable-categories).
  "heap-exhausted",
  "time-limit",
  "suspension-deadlock",
  "suspension-forbidden-context",
] as const;

export type RuntimePanicName = (typeof RUNTIME_PANIC_NAMES)[number];

/**
 * A binding whose type came from a bare literal's `usize` default
 * (spec/lang/04-type-system.md#r-types.literal.local.default).
 */
export interface FallbackBinding {
  readonly name: string;
  /** The bare literal that gave the type, and where it is. */
  readonly literal: string;
  readonly span: SourceSpan;
  /** A collection or range of literals, or a loop over one. */
  readonly kind?: "structure" | "loop";
}

/**
 * A source operation that may panic: the span its report names
 * (spec/lang/06-control-flow.md#r-flow.panic.report). The emitter collects
 * them, and the host maps a panicking Wasm frame back to one (src/panic-locator.ts).
 */
export interface PanicSite {
  readonly span: SourceSpan;
  /**
   * The operation's type came from the `usize` default
   * (spec/lang/06-control-flow.md#r-flow.panic.report.fallback): the binding
   * it reads, or else the first bare literal of its own group.
   */
  readonly fallback?:
    | { readonly binding: FallbackBinding }
    | { readonly literal: string; readonly span: SourceSpan };
}

export class RuntimePanicError extends Error {
  readonly code: RuntimePanicName;
  /** What the panic says, such as a failed `assert_equal`'s values. */
  readonly detail?: string;
  /** The operation that panicked, when the host found it (`locate`). */
  site?: PanicSite;
  /** The site's `file:line:col`, as the host shows it. */
  location?: string;
  /** Lines the report adds, such as the `usize` fallback's fix. */
  notes: readonly string[] = [];

  constructor(code: RuntimePanicName, detail?: string) {
    super(`${code}: ${detail ?? "runtime panic"}`);
    this.name = "RuntimePanicError";
    this.code = code;
    if (detail !== undefined) this.detail = detail;
  }

  /**
   * Records where the panic happened: `location` is the site's `file:line:col`
   * text, which the message then starts with, as a located diagnostic does.
   */
  locate(site: PanicSite, location: string, notes: readonly string[]): void {
    this.site = site;
    this.location = location;
    this.notes = notes;
    this.message = `${location}: ${this.code}: ${this.detail ?? "runtime panic"}`;
  }

  /** The message and its notes, one `note:` line each, as a report prints them. */
  get report(): string {
    return `${this.message}${this.notes.map((note) => `\n  note: ${note}`).join("")}`;
  }
}

/**
 * The note of an `integer-overflow` whose type fell back to `usize`
 * (spec/lang/06-control-flow.md#r-flow.panic.report.fallback.fix): it says so,
 * names the binding, and suggests the signed literal and an annotation.
 * `where` shows a span as `file:line:col`.
 */
export function fallbackNote(
  fallback: NonNullable<PanicSite["fallback"]>,
  where: (span: SourceSpan) => string,
): string {
  if (!("binding" in fallback))
    return `this operation's literals fell back to usize because none has a sign ('${fallback.literal}' at ${where(fallback.span)}); write '+${fallback.literal}' to make them i32, or annotate the type of its result`;
  const { name, literal, span, kind } = fallback.binding;
  const at = where(span);
  if (kind === "loop")
    return `'${name}' fell back to usize because the literal '${literal}' it loops over (${at}) has no sign; write '+${literal}' there for i32`;
  if (kind === "structure")
    return `'${name}' fell back to usize in its type because its literal '${literal}' (${at}) has no sign; write '+${literal}' there for i32, or annotate '${name}'`;
  return `'${name}' fell back to usize because its literal '${literal}' (${at}) has no sign; write '+${literal}' or 'let ${name}: i32 = ${literal}'`;
}

/**
 * A checked program stopped at a feature the prototype does not run; `code`
 * is a prototype diagnostic code.
 */
export class UnsupportedAtRunTimeError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "UnsupportedAtRunTimeError";
    this.code = code;
  }
}

/**
 * Whether `error` is the engine's report that the call stack ran out, as deep
 * recursion causes: V8 and JavaScriptCore throw a `RangeError`, SpiderMonkey
 * an `InternalError`.
 */
export function isStackExhaustion(error: unknown): boolean {
  if (!(error instanceof Error)) return false;
  if (error instanceof RangeError) return /call stack size/i.test(error.message);
  return error.name === "InternalError" && /too much recursion/i.test(error.message);
}

export function runtimePanicCode(name: RuntimePanicName): number {
  return RUNTIME_PANIC_NAMES.indexOf(name);
}

/** The stable panic category `name`, which `lib/std`'s panic primitive passes. */
export function runtimePanicCategory(name: string): RuntimePanicName {
  const category = RUNTIME_PANIC_NAMES.find((known) => known === name);
  if (!category) throw new Error(`'${name}' is not a panic category`);
  return category;
}

export function runtimePanicName(code: number): RuntimePanicName {
  return RUNTIME_PANIC_NAMES[code] ?? "explicit-panic";
}
