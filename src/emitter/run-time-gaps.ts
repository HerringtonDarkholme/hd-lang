import { DiagnosticError, type Diagnostic } from "../diagnostics.ts";
import type { HirProgram } from "../hir.ts";

// Checked features the prototype does not run yet; `hd run`, `hd test`, and
// `hd build` report them before emission.
//
// `Console` is a prelude trait (spec/10-modules.md#console). The host console
// is a `Console` trait value whose receiver boxes the host's `externref`
// (emitter/host-providers.ts); its `write_line!` writes the line and is ready
// on its first poll with `.Ok()`. `println` writes through the host console
// only: the spec does not say how the non-suspending `println` drives a
// suspending `write_line!` (MHP-1), so `println` through a program-defined
// provider stops the run with `unsupported-console-provider`.
const CONSOLE_NAME = /(?<![\w.])Console(?![\w.])/;

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

/** The program without an unused `Console` trait; throws when it cannot run yet. */
export function lowerRunTimeGaps(program: HirProgram): HirProgram {
  const unsupported: Diagnostic[] = [];
  unsupportedCalls([program.functions, program.closures], unsupported);
  if (unsupported.length > 0) throw new DiagnosticError(unsupported);
  const console = program.traits.find((trait) => trait.name === "Console");
  if (!console) return program;
  if (CONSOLE_NAME.test(JSON.stringify({ ...program, traits: [], hostCapabilities: [] })))
    return program;
  // An unused `Console` trait is not emitted. It is the last built-in trait,
  // so no other trait moves in `program.traits`.
  return { ...program, traits: program.traits.filter((trait) => trait !== console) };
}
