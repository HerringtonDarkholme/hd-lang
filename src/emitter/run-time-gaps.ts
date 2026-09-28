import { DiagnosticError, type Diagnostic } from "../diagnostics.ts";
import type { HirProgram } from "../hir.ts";

// Checked features the prototype does not run yet; `hd run`, `hd test`, and
// `hd build` report them before emission.
//
// `Console` is a prelude trait (spec/10-modules.md#console), but the prototype
// runs only the host console, an opaque `externref` that `println` writes
// through. Before emission every `trait:Console` type becomes the opaque
// `provider:Console`. A program-defined `Console` implementation, or a direct
// `write_line!` call, has no run-time lowering yet (MHP-1): the spec does not
// say how the non-suspending `println` drives a suspending `write_line!`.
const CONSOLE_TYPE = /trait:Console(?![\w.])/g;

function rewrite<T>(value: T): T {
  if (typeof value === "string") return value.replace(CONSOLE_TYPE, "provider:Console") as T;
  if (Array.isArray(value)) return value.map(rewrite) as T;
  if (!value || typeof value !== "object") return value;
  const result: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(value))
    result[key] = key === "span" ? child : rewrite(child);
  return result as T;
}

function consoleCalls(value: unknown, consoleIndex: number, found: Diagnostic[]): void {
  if (Array.isArray(value)) {
    for (const child of value) consoleCalls(child, consoleIndex, found);
    return;
  }
  if (!value || typeof value !== "object") return;
  const node = value as { kind?: unknown; traitIndex?: unknown; span?: Diagnostic["span"] };
  if (
    (node.kind === "trait-suspend-construct" || node.kind === "trait-call") &&
    node.traitIndex === consoleIndex &&
    node.span
  )
    found.push({
      code: "unsupported-console-call",
      message:
        "the prototype does not run a direct Console.write_line! call; println writes through the host Console",
      span: node.span,
    });
  for (const [key, child] of Object.entries(value))
    if (key !== "span") consoleCalls(child, consoleIndex, found);
}

// `debug(value)` (spec/09-traits.md#debug-trait): the spec leaves the builder
// calls of `DebugWriter` and the text layout to the standard library, so the
// prototype renders no `debug` text. `snapshot_file(text)`
// (spec/10-modules.md#snapshots): the spec does not say where the runner keeps
// the snapshot file or what a missing one means.
const UNSUPPORTED: Readonly<Record<string, readonly [string, string]>> = {
  "debug-render": [
    "unsupported-debug-render",
    "the prototype does not render debug text; the DebugWriter layout is unspecified",
  ],
  "snapshot-file": [
    "unsupported-snapshot-file",
    "the prototype does not run snapshot_file; the snapshot file's location is unspecified",
  ],
};

function unsupportedCalls(value: unknown, found: Diagnostic[]): void {
  if (Array.isArray(value)) {
    for (const child of value) unsupportedCalls(child, found);
    return;
  }
  if (!value || typeof value !== "object") return;
  const node = value as { kind?: unknown; span?: Diagnostic["span"] };
  const unsupported = typeof node.kind === "string" ? UNSUPPORTED[node.kind] : undefined;
  if (unsupported && node.span)
    found.push({ code: unsupported[0], message: unsupported[1], span: node.span });
  for (const [key, child] of Object.entries(value))
    if (key !== "span") unsupportedCalls(child, found);
}

/** The program with Console providers made opaque; throws when it cannot run yet. */
export function lowerRunTimeGaps(program: HirProgram): HirProgram {
  const unsupported: Diagnostic[] = [];
  unsupportedCalls([program.functions, program.closures], unsupported);
  if (unsupported.length > 0) throw new DiagnosticError(unsupported);
  const console = program.traits.find((trait) => trait.name === "Console");
  if (!console) return program;
  const diagnostics: Diagnostic[] = program.implementations
    .filter((implementation) => implementation.traitIndex === console.index)
    .map((implementation) => ({
      code: "unsupported-console-provider",
      message:
        "the prototype runs only the host Console; a program-defined Console provider is not supported at run time",
      span: implementation.span,
    }));
  consoleCalls([program.functions, program.closures], console.index, diagnostics);
  if (diagnostics.length > 0) throw new DiagnosticError(diagnostics);
  // With no implementation or call left, the trait itself is not emitted; it
  // is the last built-in trait, so no other trait moves in `program.traits`.
  const traits = program.traits.filter((trait) => trait !== console);
  return rewrite({ ...program, traits });
}
