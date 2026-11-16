// The shapes of the `scale` metric (#1880): one generated package per
// grammar construct, repeated or nested k times, from the templates
// fixtures/scale/NAME.hd.tmpl.
//
// A shape's template sees `items`, one entry per copy, numbered from 0000
// with fixed-width numbers so every copy has the same bytes: `i` the
// entry's number, `next` the next one's, `pad` its indentation one level
// per entry (from one level), `pad2` two levels per entry (from two).
// `reversed` is `items` backwards, for the closing half of a nest.
// `last`, `lastPad` and `lastPad2` are those of entry k. `{{rep P}}` is P
// once per entry, with `@` the entry's number and `~` a space, for a
// construct repeated inside one line.
//
// Every shape is a correct program: it checks with no diagnostic at every
// size (check-fixtures.ts checks it at k = 2), and `builds` shapes have a
// `main` that `hd build` builds. Diagnostics are not scaled (owner,
// 2026-11-14: "don't optimize diagnostic for now, optimize correct code").
//
// Usage: node --experimental-strip-types bench/metrics/lib/scale.ts SHAPE K
// prints the package's files, for reading or for trying a shape by hand.

import { manifest, renderFixture, type Files, type TemplateScope } from "./fixtures.ts";

export type Form = "repeat" | "nest";

export interface Shape {
  readonly name: string;
  /** The grammar construct the shape stresses. */
  readonly construct: string;
  readonly form: Form;
  /** The smaller size; the larger is 4n. */
  readonly n: number;
  /** Whether `hd build` builds the package. */
  readonly builds: boolean;
  /** The package's files at size k. */
  readonly files: (k: number) => Files;
}

/** The size of a repeated shape's smaller run. */
export const REPEAT_N = 500;
/** The size of a nested shape's smaller run: 4n stays below the nesting limit. */
export const NEST_N = 16;

const LEVEL = "    ";
const number = (index: number): string => String(index).padStart(4, "0");
const range = (count: number): number[] => Array.from({ length: count }, (_, index) => index);

/** The template scope of size k. */
export function scope(k: number): TemplateScope {
  const entry = (index: number) => ({
    i: number(index),
    next: number(index + 1),
    pad: LEVEL.repeat(index + 1),
    pad2: LEVEL.repeat(2 * index + 2),
  });
  const items = range(k).map(entry);
  const last = entry(k);
  return {
    items,
    reversed: [...items].reverse(),
    last: last.i,
    lastPad: last.pad,
    lastPad2: last.pad2,
    rep: (pattern: string) =>
      range(k)
        .map((index) => pattern.replaceAll("@", number(index)).replaceAll("~", " "))
        .join(""),
  };
}

const MANIFEST = manifest("scale", { capabilities: ["Console"] });

/** A package of one main module, from fixtures/scale/NAME.hd.tmpl. */
const single =
  (name: string) =>
  (k: number): Files =>
    new Map([
      ["hd.toml", MANIFEST],
      ["src/main.hd", renderFixture(`scale/${name}.hd.tmpl`, scope(k))],
    ]);

interface Entry {
  readonly construct: string;
  readonly form?: Form;
  readonly n?: number;
  readonly builds?: boolean;
  readonly files?: (k: number) => Files;
}

const ENTRIES: Readonly<Record<string, Entry>> = {
  // Items.
  "fn-items": { construct: "functions" },
  "struct-items": { construct: "structs and struct literals" },
  "enum-items": { construct: "enums and payload patterns" },
  "trait-items": { construct: "traits, impls and method calls" },
  "impl-methods": { construct: "methods of one impl" },
  "traits-of-one-struct": { construct: "impls of many traits for one struct, each method called" },
  "impls-of-one-trait": { construct: "impls of one trait and their generic instances" },
  "operator-impls": { construct: "operator impls with an associated type" },
  "derive-items": { construct: "derives (`Eq`, `Debug`)" },
  "dyn-values": { construct: "trait values (`*dyn`)" },
  consts: { construct: "top-level bindings" },
  "local-items": { construct: "local structs and functions" },
  "syntax-items": {
    construct: "the remaining item, type, statement, expression and pattern forms",
    n: 100,
  },
  "struct-chain": { construct: "structs holding the next by value", builds: false },
  modules: {
    construct: "modules and `use`",
    files: (k) => {
      const files = new Map(single("modules")(k));
      for (const index of range(k)) {
        const i = number(index);
        files.set(`src/m${i}.hd`, renderFixture("scale/modules-module.hd.tmpl", { i }));
      }
      return files;
    },
  },
  "use-names": {
    construct: "`use` of many names from one module",
    files: (k) =>
      new Map([
        ...single("use-names")(k),
        ["src/items.hd", renderFixture("scale/use-names-items.hd.tmpl", scope(k))],
      ]),
  },
  // Generics and rows.
  "generic-fns": { construct: "generic functions and instances" },
  "generic-params": { construct: "type parameters of one function", n: 100 },
  "param-bounds": { construct: "bounds of one type parameter, each method called", n: 100 },
  "type-defaults": { construct: "defaulted type parameters" },
  "row-chain": { construct: "requirement rows along a call chain" },
  "row-traits": { construct: "traits of one requirement row and `$.use`", n: 100 },
  // Statements and expressions.
  closures: { construct: "closures" },
  "let-statements": { construct: "`let` statements of one body" },
  matches: { construct: "`match` expressions with guards" },
  "enum-arms": { construct: "arms of one `match`" },
  "tuple-patterns": { construct: "tuple patterns" },
  "range-patterns": { construct: "range patterns" },
  "wide-struct": { construct: "fields of one struct and literal" },
  "call-args": { construct: "parameters and arguments of one call", n: 100 },
  "binary-chain": { construct: "terms of one binary expression" },
  interpolation: { construct: "interpolations of one string" },
  "list-literal": { construct: "elements of one list literal" },
  "method-chain": { construct: "calls of one method chain" },
  "pipe-chain": { construct: "stages of one `|>` chain" },
  "question-chain": { construct: "`?` propagations of one body" },
  // Nesting.
  "if-nest": { construct: "nested `if`", form: "nest" },
  "block-nest": { construct: "nested `else` branches", form: "nest" },
  "for-nest": { construct: "nested `for`", form: "nest" },
  "while-nest": { construct: "nested `while`", form: "nest" },
  "match-nest": { construct: "nested `match` arms", form: "nest" },
  "closure-nest": {
    construct: "nested closures capturing the outermost",
    form: "nest",
    // Each level is about four nesting levels: 4n stays below the limit.
    n: 10,
  },
  "option-nest": { construct: "nested option types, values and patterns", form: "nest" },
  "list-type-nest": { construct: "nested list types and literals", form: "nest" },
  // 4n stays below the instantiation depth limit of 32.
  "generic-nest": { construct: "nested generic instances", form: "nest", n: 7 },
  "paren-nest": { construct: "nested parentheses", form: "nest" },
  // arith-, bool- and if-expr-nest take two nesting levels per entry: n = 8
  // keeps 4n below the limit.
  "arith-nest": { construct: "nested arithmetic and unary operators", form: "nest", n: 8 },
  "bool-nest": { construct: "nested comparisons and logic", form: "nest", n: 8 },
  "bit-nest": { construct: "nested bitwise operators and shifts", form: "nest" },
  "tuple-nest": { construct: "nested tuple types, values and patterns", form: "nest" },
  "if-expr-nest": { construct: "nested inline `if` expressions", form: "nest", n: 8 },
  "call-nest": { construct: "nested calls", form: "nest" },
  "index-nest": { construct: "nested index expressions", form: "nest" },
  "fn-nest": { construct: "nested local functions and `return`", form: "nest" },
  "module-path-nest": {
    construct: "a module path one folder deeper per level",
    form: "nest",
    files: (k) =>
      new Map([
        ...single("module-path-nest")(k),
        [
          `src/${range(k)
            .map((index) => `d${number(index)}/`)
            .join("")}leaf.hd`,
          renderFixture("scale/module-path-nest-leaf.hd.tmpl", {}),
        ],
      ]),
  },
};

export const SHAPES: readonly Shape[] = Object.entries(ENTRIES).map(([name, entry]) => {
  const form = entry.form ?? "repeat";
  return {
    name,
    construct: entry.construct,
    form,
    n: entry.n ?? (form === "nest" ? NEST_N : REPEAT_N),
    builds: entry.builds ?? true,
    files: entry.files ?? single(name),
  };
});

/** Every template a shape renders, for check-fixtures.ts's use check. */
export const TEMPLATES: readonly string[] = [
  ...SHAPES.map((shape) => `scale/${shape.name}.hd.tmpl`),
  "scale/modules-module.hd.tmpl",
  "scale/use-names-items.hd.tmpl",
  "scale/module-path-nest-leaf.hd.tmpl",
];

if (import.meta.main) {
  const [name, k] = process.argv.slice(2);
  const found = SHAPES.find((shape) => shape.name === name);
  if (!found || k === undefined) {
    console.error(
      `usage: scale.ts SHAPE K\nshapes: ${SHAPES.map((shape) => shape.name).join(", ")}`,
    );
    process.exit(2);
  }
  for (const [path, text] of found.files(Number(k))) {
    process.stdout.write(`== ${path}\n${text}\n`);
  }
}
