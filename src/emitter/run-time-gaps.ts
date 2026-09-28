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
// prototype renders no `debug` text.
function debugCalls(value: unknown, found: Diagnostic[]): void {
  if (Array.isArray(value)) {
    for (const child of value) debugCalls(child, found);
    return;
  }
  if (!value || typeof value !== "object") return;
  const node = value as { kind?: unknown; span?: Diagnostic["span"] };
  if (node.kind === "debug-render" && node.span)
    found.push({
      code: "unsupported-debug-render",
      message: "the prototype does not render debug text; the DebugWriter layout is unspecified",
      span: node.span,
    });
  for (const [key, child] of Object.entries(value)) if (key !== "span") debugCalls(child, found);
}

/** The program with Console providers made opaque; throws when it cannot run yet. */
export function lowerRunTimeGaps(program: HirProgram): HirProgram {
  const debugDiagnostics: Diagnostic[] = [];
  debugCalls([program.functions, program.closures], debugDiagnostics);
  if (debugDiagnostics.length > 0) throw new DiagnosticError(debugDiagnostics);
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
