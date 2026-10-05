import type { Expression, FunctionDecl, ModuleScope, Program } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import type { HirData, HirEnum, HirProgram, ValueType } from "../hir.ts";
import {
  CURSOR_TYPE,
  displayType,
  functionParts,
  nominalGenericParts,
  optionalInner,
  readonlyType,
  restInner,
  resultParts,
  storedSuspensionParts,
  suspensionParts,
  traitSuspensionParts,
  tupleParts,
} from "../types.ts";
import { Source_ } from "./generated-source.ts";
import { substituteGenericType } from "./shared.ts";

// `dbg` (spec/lang/10-modules.md#debug-printing) is an ordinary declaration,
// `fn dbg[Args < Tuple](values...: Args) -> void`, so every call is checked
// by the plain call rules. Only its body is intrinsic, and the printing body
// of the user's own code depends on each argument's static type, so a program
// that calls it checks in two passes:
//
// 1. The first pass checks each call as that ordinary call and records the
//    static type of each element that the call collected into `Args`. Where a
//    type implements `Debug`, the value prints through it; elsewhere the pass
//    walks the type's parts and records which of them implement `Debug`.
// 2. Between the passes, `debugPrinters` writes one printer function per
//    type that prints structurally, over `std.format`'s `DebugWriter`, and a
//    plan per call. The second pass checks each call as `dbg_done` of calls
//    of `std.format`'s `dbg_one`, `dbg_shown`, or `dbg_opaque`, with its
//    location and source text, and the generated printers.
//
// A call in a fetched dependency stays the ordinary call, whose body prints
// nothing (spec/lang/10-modules.md#r-module.dbg.dependency).
//
// The REPL's value display (spec/cli/command-line.md#r-cli.repl.value) is a
// call of the hidden `dbg_text`, which takes the same two passes. A program
// without either call checks once, and links no printer.

/** The `@intrinsic` names of `std.format`'s `dbg` and of the REPL's `dbg_text`. */
export const DBG_INTRINSIC = "dbg";
export const DBG_TEXT_INTRINSIC = "dbg_text";

/** The hidden names of `std.format`'s support functions (lib/std/format.hd). */
const SUPPORT = {
  one: "__std_format_dbg_one",
  shown: "__std_format_dbg_shown",
  opaque: "__std_format_dbg_opaque",
  here: "__std_format_dbg_here",
  done: "__std_format_dbg_done",
  nested: "__std_format_dbg_nested",
  render: "__std_format_dbg_render",
  renderShown: "__std_format_dbg_render_shown",
} as const;

/** How one argument prints. */
export type DebugPrinter =
  | { readonly kind: "debug" }
  | { readonly kind: "structural"; readonly type: ValueType }
  | { readonly kind: "opaque"; readonly text: string };

/** A checked `dbg` or `dbg_text` call of the first pass. */
interface DebugSite {
  readonly span: SourceSpan;
  /** `dbg_text`: the REPL's value display, which returns the text. */
  readonly text: boolean;
  readonly printers: readonly DebugPrinter[];
}

/** What the second pass checks a call as. */
export interface DebugSitePlan {
  /** Each argument's printer, with structural ones named by their function. */
  readonly printers: readonly (DebugPrinter & { readonly function?: string })[];
}

/** The options of a compile that concern `dbg`. */
export interface DebugPrintOptions {
  /** A release build: `dbg` in the user's own code is `dbg-in-release`. */
  readonly release?: boolean;
  /**
   * The `FILE:LINE:COLUMN` that a `dbg` line names for a call at `span`
   * (spec/lang/10-modules.md#r-module.dbg.location); `LINE:COLUMN` without it.
   */
  readonly debugLocation?: (span: SourceSpan) => string;
  /** The checked source, for each argument's text and the fix-it. */
  readonly sourceText?: string;
}

/** The state that every function check of one pass shares. */
export interface DebugPrintState {
  readonly release: boolean;
  readonly locate: (span: SourceSpan) => string;
  readonly text: (span: SourceSpan) => string;
  /** The fetched package whose code holds `span`, as messages name it (module.dbg.dependency). */
  readonly fetchedPackage: (span: SourceSpan) => string | undefined;
  /** The second pass's plans, by call span; absent in the first pass. */
  readonly plans?: ReadonlyMap<string, DebugSitePlan>;
  /** The first pass's calls, by call span. */
  readonly sites: Map<string, DebugSite>;
  /** Whether each type a structural printer reaches implements `Debug`. */
  readonly hasDebug: Map<ValueType, boolean>;
  /** Each fetched package with a `dbg` call, and its first call. */
  readonly quiet: Map<string, SourceSpan>;
}

const registered = new WeakMap<object, DebugPrintState>();

/** Records a pass's state under `key`, an object every function check of the program shares. */
export function registerDebugPrint(key: object, state: DebugPrintState): void {
  registered.set(key, state);
}

export function registeredDebugPrint(key: object): DebugPrintState | undefined {
  return registered.get(key);
}

/** A pass's state for `program`, with the second pass's `plans`. */
export function debugPrintState(
  program: Program,
  options: DebugPrintOptions,
  plans?: ReadonlyMap<string, DebugSitePlan>,
): DebugPrintState {
  const source = options.sourceText ?? "";
  const scopes = program.packageScopes?.scopes ?? [];
  return {
    release: options.release === true,
    locate: options.debugLocation ?? ((span) => `${span.start.line}:${span.start.column}`),
    text: (span) =>
      source
        .slice(span.start.offset, span.end.offset)
        .replace(/\s*\n\s*/g, " ")
        .trim(),
    fetchedPackage: (span) => scopeAt(scopes, span.start.line)?.fetched,
    ...(plans ? { plans } : {}),
    sites: new Map(),
    hasDebug: new Map(),
    quiet: new Map(),
  };
}

function scopeAt(scopes: readonly ModuleScope[], line: number): ModuleScope | undefined {
  return scopes.find(({ firstLine, lastLine }) => firstLine <= line && line <= lastLine);
}

/** What the checker answers about types, for the walk below. */
export interface DebugTypeOracle {
  /** Whether `type` implements `Debug` where the call is, its bounds included. */
  implementsDebug(type: ValueType): boolean;
  readonly dataTypes: ReadonlyMap<string, HirData>;
  readonly enumTypes: ReadonlyMap<string, HirEnum>;
}

/** How a value of a type prints, apart from `Debug`: its parts, or text. */
type DebugShape =
  | { readonly kind: "text"; readonly text: string }
  | { readonly kind: "option"; readonly inner: ValueType }
  | { readonly kind: "result"; readonly ok: ValueType; readonly error: ValueType }
  | { readonly kind: "list"; readonly element: ValueType }
  | { readonly kind: "map"; readonly key: ValueType; readonly value: ValueType }
  | { readonly kind: "tuple"; readonly elements: readonly ValueType[] }
  | {
      readonly kind: "data";
      readonly data: HirData;
      readonly fields: readonly { readonly name: string; readonly type: ValueType }[];
    }
  | {
      readonly kind: "enum";
      readonly name: string;
      readonly variants: readonly {
        readonly name: string;
        readonly fields: readonly { readonly name: string; readonly type: ValueType }[];
      }[];
    };

const HANDLE = "<handle>";

/** The shape of a value of `type` (spec/lang/10-modules.md#debug-values). */
function debugShape(
  type: ValueType,
  dataTypes: ReadonlyMap<string, HirData>,
  enumTypes: ReadonlyMap<string, HirEnum>,
): DebugShape {
  if (type === "void" || type === "()") return { kind: "text", text: "()" };
  if (type === "never") return { kind: "text", text: "never" };
  if (
    type === CURSOR_TYPE ||
    suspensionParts(type) ||
    traitSuspensionParts(type) ||
    storedSuspensionParts(type)
  )
    return { kind: "text", text: HANDLE };
  if (["Any", "AnyRef", "AnyVal"].includes(type)) return { kind: "text", text: `<${type}>` };
  if (type.startsWith("trait:")) return { kind: "text", text: `<${displayType(type)}>` };
  if (functionParts(type)) return { kind: "text", text: `<${displayType(type)}>` };
  const optional = optionalInner(type);
  if (optional !== undefined) return { kind: "option", inner: readonlyType(optional) };
  const result = resultParts(type);
  if (result)
    return { kind: "result", ok: readonlyType(result.ok), error: readonlyType(result.error) };
  const tuple = tupleParts(type);
  if (tuple)
    return {
      kind: "tuple",
      elements: tuple.map((element) => readonlyType(restInner(element) ?? element)),
    };
  const nominal = nominalGenericParts(type);
  const name = nominal?.name ?? type;
  const typeArguments = nominal?.arguments ?? [];
  if (name === "List" && typeArguments.length === 1)
    return { kind: "list", element: readonlyType(typeArguments[0]!) };
  if (name === "Map" && typeArguments.length === 2)
    return {
      kind: "map",
      key: readonlyType(typeArguments[0]!),
      value: readonlyType(typeArguments[1]!),
    };
  const substitute = (parameters: readonly string[]) => {
    const map = new Map(parameters.map((parameter, index) => [parameter, typeArguments[index]!]));
    return (field: ValueType): ValueType => readonlyType(substituteGenericType(field, map));
  };
  const data = dataTypes.get(name);
  if (data) {
    const field = substitute(data.genericParameters);
    return {
      kind: "data",
      data,
      fields: data.fields.map((member) => ({ name: member.name, type: field(member.type) })),
    };
  }
  const declaration = enumTypes.get(name);
  if (declaration) {
    const field = substitute(declaration.genericParameters);
    return {
      kind: "enum",
      name: declaration.name,
      variants: declaration.variants.map((variant) => ({
        name: variant.name,
        fields: variant.fields.map((member) => ({ name: member.name, type: field(member.type) })),
      })),
    };
  }
  return { kind: "text", text: `<${displayType(type)}>` };
}

function shapeParts(shape: DebugShape): readonly ValueType[] {
  switch (shape.kind) {
    case "text":
      return [];
    case "option":
      return [shape.inner];
    case "result":
      return [shape.ok, shape.error];
    case "list":
      return [shape.element];
    case "map":
      return [shape.key, shape.value];
    case "tuple":
      return shape.elements;
    case "data":
      return shape.fields.map(({ type }) => type);
    case "enum":
      return shape.variants.flatMap(({ fields }) => fields.map(({ type }) => type));
  }
}

/** Whether a type mentions a type parameter, so no printer can be generated for it. */
function mentionsTypeParameter(type: ValueType): boolean {
  return /(?<![A-Za-z0-9_])generic:/.test(type);
}

/**
 * How an argument of static type `type` prints, recording in `hasDebug`
 * whether each part a structural printer reaches implements `Debug`.
 * `functionName` names a function that the argument names
 * (spec/lang/10-modules.md#r-module.dbg.value.function).
 */
export function debugPrinter(
  type: ValueType,
  oracle: DebugTypeOracle,
  hasDebug: Map<ValueType, boolean>,
  functionName?: string,
): DebugPrinter {
  const shown = readonlyType(type.startsWith("cell:") ? type.slice("cell:".length) : type);
  if (mentionsTypeParameter(shown)) {
    // Inside generic code a type prints through `Debug` when its bounds give
    // it one; otherwise its text is implementation-defined
    // (spec/lang/10-modules.md#r-module.dbg.value.generic).
    return oracle.implementsDebug(shown)
      ? { kind: "debug" }
      : { kind: "opaque", text: `<${displayType(shown)}>` };
  }
  if (functionName !== undefined && functionParts(shown))
    return { kind: "opaque", text: `<fn ${functionName}${displayType(shown).slice("fn".length)}>` };
  if (oracle.implementsDebug(shown)) return { kind: "debug" };
  const shape = debugShape(shown, oracle.dataTypes, oracle.enumTypes);
  if (shape.kind === "text") return { kind: "opaque", text: shape.text };
  const pending = [...shapeParts(shape)];
  while (pending.length > 0) {
    const part = pending.pop()!;
    if (hasDebug.has(part)) continue;
    const implemented = oracle.implementsDebug(part);
    hasDebug.set(part, implemented);
    if (!implemented)
      pending.push(...shapeParts(debugShape(part, oracle.dataTypes, oracle.enumTypes)));
  }
  return { kind: "structural", type: shown };
}

/** Records a first-pass call. */
export function recordDebugSite(
  state: DebugPrintState,
  span: SourceSpan,
  key: string,
  text: boolean,
  printers: readonly DebugPrinter[],
): void {
  state.sites.set(key, { span, text, printers });
}

/** The first pass found a call that prints, so the program needs the second pass. */
export function needsDebugPrinters(state: DebugPrintState): boolean {
  return state.sites.size > 0;
}

/**
 * The printer functions and the per-call plans of the second pass, from the
 * first pass's calls and checked program.
 */
export function debugPrinters(
  state: DebugPrintState,
  hir: HirProgram,
  span: SourceSpan,
): { readonly functions: readonly FunctionDecl[]; readonly plans: Map<string, DebugSitePlan> } {
  const dataTypes = new Map(hir.data.map((data) => [data.name, data] as const));
  const enumTypes = new Map(
    hir.enums.map((declaration) => [declaration.name, declaration] as const),
  );
  const writer = hir.data.find(({ standardName }) => standardName === "std.format.DebugWriter");
  if (!writer) throw new Error("std.format.DebugWriter is not declared");
  const out = new Source_();
  const names = new Map<ValueType, string>();
  const queue: ValueType[] = [];
  const printerName = (type: ValueType): string => {
    let name = names.get(type);
    if (name === undefined) {
      name = `hd__dbg_show_${names.size}`;
      names.set(type, name);
      queue.push(type);
    }
    return name;
  };
  // The statement that writes `value`, a part of type `type`.
  const part = (type: ValueType, value: string): string => {
    if (state.hasDebug.get(type) === true) return `${SUPPORT.nested}(${value}, hd__out)`;
    const shape = debugShape(type, dataTypes, enumTypes);
    if (shape.kind === "text") return `hd__out.write(${out.string(shape.text)})`;
    return `${printerName(type)}(${value}, hd__out)`;
  };
  const plans = new Map<string, DebugSitePlan>();
  for (const [key, site] of state.sites)
    plans.set(key, {
      printers: site.printers.map((printer) =>
        printer.kind === "structural"
          ? { ...printer, function: printerName(printer.type) }
          : printer,
      ),
    });
  const writerType = out.type(`mut:${writer.name}`);
  while (queue.length > 0) {
    const type = queue.shift()!;
    const shape = debugShape(type, dataTypes, enumTypes);
    const body: string[] = [];
    const add = (indent: number, line: string): void => {
      body.push(`${"    ".repeat(indent + 1)}${line}`);
    };
    // A part opened one level deeper, as a builder opens one: its name, then
    // each item's writes, then its close (lib/std/format.hd, DebugWriter).
    const opened = (
      indent: number,
      name: string | undefined,
      items: readonly {
        /** A named argument of a call: `name=value`. */
        readonly name?: string;
        /** A named field of a data value: `name: value`. */
        readonly field?: string;
        readonly type: ValueType;
        readonly value: string;
      }[],
      close: string,
    ): void => {
      add(indent, "if hd__out.open():");
      if (name !== undefined) add(indent + 1, `hd__out.write(${out.string(name)})`);
      items.forEach((item, index) => {
        add(
          indent + 1,
          item.field === undefined
            ? item.name === undefined
              ? `hd__out.tuple_item(${index})`
              : `hd__out.named_item(${out.string(item.name)}, ${index})`
            : `hd__out.struct_item(${out.string(item.field)}, ${index})`,
        );
        add(indent + 1, part(item.type, item.value));
        add(indent + 1, "hd__out.end_item()");
      });
      add(indent + 1, close.replace("COUNT", String(items.length)));
    };
    // A variant, written as hd source constructs it: its qualified name
    // alone, or a call with its positional fields first and its named ones
    // as `name=value` (std-format.debug.derive-builders.source).
    const variant = (
      indent: number,
      name: string,
      fields: readonly {
        readonly name: string;
        readonly type: ValueType;
        readonly value: string;
      }[],
    ): void => {
      if (fields.length === 0) {
        add(indent, `hd__out.write(${out.string(name)})`);
        return;
      }
      opened(
        indent,
        name,
        fields.map((field) => ({
          type: field.type,
          value: field.value,
          ...(/^\d+$/.test(field.name) ? {} : { name: field.name }),
        })),
        "hd__out.tuple_close(COUNT, false)",
      );
    };
    // A `void` payload binds no value, so its pattern is `_`; it prints `()`.
    const binder = (type: ValueType, name: string): string =>
      type === "void" || type === "()" ? "_" : name;
    switch (shape.kind) {
      case "text":
        add(0, `hd__out.write(${out.string(shape.text)})`);
        break;
      case "option":
        add(0, "match hd__value:");
        add(1, `.Some(${binder(shape.inner, "hd__inner")}) =>`);
        variant(2, "Option.Some", [{ name: "0", type: shape.inner, value: "hd__inner" }]);
        add(1, `.None => hd__out.write(${out.string("Option.None")})`);
        break;
      case "result":
        add(0, "match hd__value:");
        add(1, `.Ok(${binder(shape.ok, "hd__inner")}) =>`);
        variant(2, "Result.Ok", [{ name: "0", type: shape.ok, value: "hd__inner" }]);
        add(1, `.Err(${binder(shape.error, "hd__inner")}) =>`);
        variant(2, "Result.Err", [{ name: "0", type: shape.error, value: "hd__inner" }]);
        break;
      case "list":
      case "map": {
        const [opening, closing] = shape.kind === "list" ? ["[", "]"] : ["{", "}"];
        add(0, "if hd__out.open():");
        add(1, "let hd__count: usize = 0");
        add(
          1,
          shape.kind === "list"
            ? "for hd__item in hd__value:"
            : "for (hd__key, hd__item) in hd__value:",
        );
        add(2, `if hd__out.entry_item(hd__count, ${out.string(opening)}):`);
        if (shape.kind === "map") {
          add(3, part(shape.key, "hd__key"));
          add(3, `hd__out.write(${out.string(": ")})`);
          add(3, part(shape.value, "hd__item"));
        } else add(3, part(shape.element, "hd__item"));
        add(3, "hd__out.end_item()");
        add(2, "hd__count = hd__count + 1");
        add(1, `hd__out.entry_close(hd__count, ${out.string(opening)}, ${out.string(closing)})`);
        break;
      }
      case "tuple":
        opened(
          0,
          undefined,
          shape.elements.map((element, index) => ({ type: element, value: `hd__value._${index}` })),
          "hd__out.tuple_close(COUNT, true)",
        );
        break;
      case "data": {
        const { data } = shape;
        const shown = displayType(data.name);
        // A local data type is not inspectable, so it converts to no `Any`
        // and takes no part in finding a cycle.
        const tracked = data.local !== true;
        const indent = tracked ? 1 : 0;
        if (tracked) add(0, "if hd__out.enter(hd__value):");
        if (data.newtype) {
          // A newtype shows its name around its base value, which the base
          // type's constructor unwraps (types.newtype.construct).
          const base = shape.fields[0]!;
          if (/^[A-Za-z_][A-Za-z0-9_]*$/.test(base.type))
            opened(
              indent,
              shown,
              [{ type: base.type, value: `${base.type}(hd__value)` }],
              "hd__out.tuple_close(COUNT, false)",
            );
          else add(indent, `hd__out.write(${out.string(`${shown}(…)`)})`);
        } else
          opened(
            indent,
            shown,
            shape.fields.map((field) => ({
              type: field.type,
              field: field.name,
              value: `hd__value.${field.name}`,
            })),
            "hd__out.struct_close(COUNT)",
          );
        if (tracked) add(1, "hd__out.leave()");
        break;
      }
      case "enum":
        add(0, "if hd__out.enter(hd__value):");
        add(1, "match hd__value:");
        const owner = displayType(shape.name);
        for (const each of shape.variants) {
          const qualified = `${owner}.${each.name}`;
          const binders = each.fields.map((field, index) =>
            binder(field.type, `hd__field${index}`),
          );
          if (binders.length === 0) {
            add(2, `.${each.name} => hd__out.write(${out.string(qualified)})`);
            continue;
          }
          add(2, `.${each.name}(${binders.join(", ")}) =>`);
          variant(
            3,
            qualified,
            each.fields.map((field, index) => ({ ...field, value: binders[index]! })),
          );
        }
        add(1, "hd__out.leave()");
        break;
    }
    out.add(`fn ${names.get(type)}(hd__value: ${out.type(type)}, hd__out: ${writerType}) -> void:`);
    for (const line of body) out.add(line);
    out.add("");
  }
  const functions =
    names.size === 0
      ? []
      : out.program(span).functions.map((declaration): FunctionDecl => ({
          ...declaration,
          compilerGenerated: true,
          privateAccess: true,
        }));
  return { functions, plans };
}

/**
 * The expression the second pass checks a call as: each argument through
 * its printer, with its location and source text (r-module.dbg.line).
 */
export function debugPrintingCall(
  state: DebugPrintState,
  call: Extract<Expression, { kind: "call" }>,
  plan: DebugSitePlan,
  text: boolean,
): Expression {
  const span = call.span;
  const name = (value: string): Expression => ({ kind: "name", name: value, span });
  const string = (value: string): Expression => ({ kind: "string", value, span });
  const invoke = (callee: string, arguments_: readonly Expression[]): Expression => ({
    kind: "call",
    callee: name(callee),
    arguments: arguments_,
    span,
  });
  const location = state.locate(span);
  if (text) {
    const [value, width] = call.arguments;
    const printer = plan.printers[0]!;
    if (printer.kind === "debug") return invoke(SUPPORT.renderShown, [width!, value!]);
    if (printer.kind === "opaque") return string(printer.text);
    return invoke(SUPPORT.render, [width!, value!, name(printer.function!)]);
  }
  if (call.arguments.length === 0) return invoke(SUPPORT.here, [string(location)]);
  const printed = call.arguments.map((argument, index) => {
    const head = string(`${location}: ${state.text(argument.span)}`);
    const printer = plan.printers[index]!;
    if (printer.kind === "debug") return invoke(SUPPORT.shown, [head, argument]);
    if (printer.kind === "opaque")
      return invoke(SUPPORT.opaque, [head, argument, string(printer.text)]);
    return invoke(SUPPORT.one, [head, argument, name(printer.function!)]);
  });
  return invoke(SUPPORT.done, [
    printed.length === 1 ? printed[0]! : { kind: "tuple", elements: printed, span },
  ]);
}
