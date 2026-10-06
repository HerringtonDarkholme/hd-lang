import type { ForeignName, Program, UseDecl } from "./ast.ts";
import { isStandardModulePath } from "./checker/standard-uses.ts";

// How a linked package's module texts join into one source (src/package.ts).
//
// Std uses join once for every module: the joined source keeps a std use
// line the first time a module writes it. Its names still belong to the
// module that writes the use (03-names-and-scopes.md#r-names.module.use-own-module):
// another module that names one reports it (`standardForeign`), and two
// modules may bind one local name to different std declarations, the later
// one under a hidden spelling (`standardSpellings`).

export function isStandardUse(declaration: UseDecl): boolean {
  return declaration.module.split(".")[0] === "std";
}

/** What a diagnostic says about `alias ?? name`, which a use of std `module` binds. */
export function standardForeign(
  module: string,
  name: string,
  alias: string | undefined,
): ForeignName {
  const path = module === "std" ? name : `${module.slice("std.".length)}.${name}`;
  const as = alias === undefined ? "" : ` as ${alias}`;
  // A use that names a std module binds a namespace (module.use.single).
  if (isStandardModulePath(path)) return { hint: `import it with \`use std.${path}${as}\`` };
  const standard = { module: module.slice("std.".length), name };
  return { standard, hint: `import it with \`use ${module}.{${name}${as}}\`` };
}

/**
 * The joined spelling of each local name that a module's std uses bind. The
 * first module in `order` that binds a name keeps it, as does a later one
 * that binds it to the same std declaration; any other gets `hide`'s
 * spelling, which its module scope maps back.
 */
export function standardSpellings<M extends { readonly program?: Program }>(
  order: readonly M[],
  hide: (module: M, name: string) => string,
): (module: M, local: string) => string {
  const first = new Map<string, string>();
  const spelled = new Map<M, Map<string, string>>();
  for (const module of order) {
    const own = new Map<string, string>();
    spelled.set(module, own);
    for (const use of module.program?.uses ?? []) {
      if (!isStandardUse(use)) continue;
      for (const { name, alias } of use.names) {
        const local = alias ?? name;
        const std = `${use.module}.${name}`;
        const bound = first.get(local);
        if (bound === undefined) first.set(local, std);
        own.set(local, bound === undefined || bound === std ? local : hide(module, local));
      }
    }
  }
  return (module, local) => spelled.get(module)?.get(local) ?? local;
}

/**
 * A module's text in the joined source: package uses are dropped, and a
 * standard use keeps only the names that no earlier module imported under
 * the same spelling (`importedStd`, which this adds to), each under its
 * joined spelling (`spelling`). Every other line stays in place.
 */
export function joinedText(
  program: Program,
  source: string,
  importedStd: Set<string>,
  spelling: (local: string) => string,
): string {
  let text = source;
  const edits: { readonly start: number; readonly end: number; readonly text: string }[] = [];
  const moduleStd = new Set<string>();
  // A module's documentation is its file's first block, so in the joined
  // source it would attach to nothing: its lines stay, blank
  // (spec/lang/01-lexical-structure.md#r-lex.doc.module).
  const moduleDoc = program.moduleDoc?.span;
  if (moduleDoc) {
    const { offset: start } = moduleDoc.start;
    const end = moduleDoc.end.offset;
    edits.push({ start, end, text: text.slice(start, end).replace(/[^\n]/g, "") });
  }
  for (const declaration of program.uses) {
    // A `pub use` span starts at `use`; the edit covers the `pub` too.
    const end = declaration.span.end.offset;
    let start = declaration.span.start.offset;
    if (declaration.public) start = text.lastIndexOf("pub", start);
    const newlines = text.slice(start, end).replace(/[^\n]/g, "");
    if (!isStandardUse(declaration)) {
      edits.push({ start, end, text: newlines });
      continue;
    }
    const kept = declaration.names
      .map(({ name, alias }) => ({ name, local: alias ?? name, joined: spelling(alias ?? name) }))
      .filter(({ name, joined }) => {
        const key = `${joined}=${declaration.module}.${name}`;
        moduleStd.add(key);
        return !importedStd.has(key);
      });
    const same = kept.every(({ local, joined }) => local === joined);
    if (kept.length === declaration.names.length && same) continue;
    const names = kept.map(({ name, joined }) => (joined === name ? name : `${name} as ${joined}`));
    const pub = declaration.public ? "pub " : "";
    edits.push({
      start,
      end,
      text:
        kept.length === 0
          ? newlines
          : `${pub}use ${declaration.module}.{${names.join(", ")}}${newlines}`,
    });
  }
  for (const key of moduleStd) importedStd.add(key);
  for (const edit of edits.toReversed())
    text = text.slice(0, edit.start) + edit.text + text.slice(edit.end);
  return text;
}

/**
 * Wraps a unit test module's text as a `tests:` block by re-indenting its
 * lines and deleting top-level `pub`. Integration test modules never pass
 * through here: they join as ordinary top-level source from their own AST.
 */
export function wrapTestModule(text: string, deleted: Map<number, number>): string {
  const lines = text.split("\n").map((lineText, index) => {
    const pub = /^pub\s+(?=(?:fn|data|enum|trait|type|use)\b)/.exec(lineText);
    if (!pub) return lineText;
    deleted.set(index, pub[0].length);
    return lineText.slice(pub[0].length);
  });
  return `tests:\n${lines.join("\n").replace(/^(?=.)/gm, "    ")}`;
}
