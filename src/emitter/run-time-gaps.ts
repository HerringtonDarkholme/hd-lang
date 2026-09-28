import type { HirProgram } from "../hir.ts";

// Prepares a checked program for emission.
//
// `Console` is a prelude trait (spec/10-modules.md#console). The host console
// is a `Console` trait value whose receiver boxes the host's `externref`
// (emitter/host-providers.ts); its `write_line!` writes the line and is ready
// on its first poll with `.Ok()`. `println` drives the covering provider's
// `write_line!`, host or program-defined (MHP-1). `debug` and
// `snapshot_file` are `lib/std` code, so no checked call is left unrun.
const CONSOLE_NAME = /(?<![\w.])Console(?![\w.])/;

/** The program without an unused `Console` trait. */
export function lowerRunTimeGaps(program: HirProgram): HirProgram {
  const console = program.traits.find((trait) => trait.name === "Console");
  if (!console) return program;
  if (CONSOLE_NAME.test(JSON.stringify({ ...program, traits: [], hostCapabilities: [] })))
    return program;
  // An unused `Console` trait is not emitted. It is the last built-in trait,
  // so no other trait moves in `program.traits`.
  return { ...program, traits: program.traits.filter((trait) => trait !== console) };
}
