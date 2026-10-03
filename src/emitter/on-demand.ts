// Emits module code on demand (task #146): a function is emitted only when
// code already emitted names it, starting from the functions the host calls.
// So a program gets the std functions, function-value twins (`$fv`),
// dictionary adapters (`$tadapt`), and runtime functions it reaches, not
// every one the checker declared.

import type { HirFunction, HirProgram } from "../hir.ts";
import { indent } from "./shared.ts";
import { emitStoredSuspensionFunctionAdapters } from "./stored-suspension.ts";

/** A WAT identifier, such as `$f12` or `$tadapt3_0`. */
const SYMBOL = /\$[\w.-]+/g;

class OnDemandCode {
  /** Emitted text by unit key; empty while a unit waits in the queue. */
  private readonly texts = new Map<string, string>();
  private readonly queue: string[] = [];
  private readonly unitOf: (symbol: string) => string | undefined;
  private readonly emitUnit: (key: string) => string;

  /**
   * @param unitOf The key of the unit that defines `symbol`, or undefined
   *   for a symbol that names no on-demand code.
   * @param emitUnit The text of the unit with that key.
   */
  constructor(unitOf: (symbol: string) => string | undefined, emitUnit: (key: string) => string) {
    this.unitOf = unitOf;
    this.emitUnit = emitUnit;
  }

  /** Asks for every unit that `text` names. */
  scan(text: string): void {
    for (const [symbol] of text.matchAll(SYMBOL)) {
      const key = this.unitOf(symbol);
      if (key !== undefined) this.request(key);
    }
  }

  /** Asks for the unit with key `key`. */
  request(key: string): void {
    if (this.texts.has(key)) return;
    this.texts.set(key, "");
    this.queue.push(key);
  }

  /** Emits every unit asked for, and the units they name; true when it emitted any. */
  drain(): boolean {
    const emitted = this.queue.length > 0;
    for (let key = this.queue.shift(); key !== undefined; key = this.queue.shift()) {
      const text = this.emitUnit(key);
      this.texts.set(key, text);
      this.scan(text);
    }
    return emitted;
  }

  /** The texts of the units with these keys that were asked for, in order. */
  joined(keys: readonly string[]): string {
    return keys
      .map((key) => this.texts.get(key))
      .filter(Boolean)
      .join("\n\n");
  }

  has(key: string): boolean {
    return this.texts.has(key);
  }
}

/** A top-level form of fixed module text, with the comments and space before it. */
interface FixedForm {
  readonly text: string;
  /** The function the form defines, such as `$hd.map_get`; undefined for any other form. */
  readonly function?: string;
  /** Whether the form is a function the host calls by its export. */
  readonly exported: boolean;
}

/**
 * Splits fixed module text, such as the runtime, into its top-level forms,
 * so that its functions too are emitted only on demand.
 */
function fixedForms(text: string): FixedForm[] {
  const forms: FixedForm[] = [];
  let depth = 0;
  let start = 0;
  let index = 0;
  while (index < text.length) {
    const character = text[index]!;
    if (character === ";" && text[index + 1] === ";") {
      const end = text.indexOf("\n", index);
      index = end < 0 ? text.length : end;
    } else if (character === "(" && text[index + 1] === ";") {
      index = text.indexOf(";)", index) + 2;
    } else if (character === '"') {
      index += 1;
      while (index < text.length && text[index] !== '"') index += text[index] === "\\" ? 2 : 1;
      index += 1;
    } else {
      if (character === "(") depth += 1;
      if (character === ")") {
        depth -= 1;
        if (depth === 0) {
          const form = text.slice(start, index + 1);
          const header = /^\s*(?:;;.*\n\s*)*\(func (\$[\w.-]+)( \(export )?/.exec(form);
          forms.push({ text: form, function: header?.[1], exported: Boolean(header?.[2]) });
          start = index + 1;
        }
      }
      index += 1;
    }
  }
  const rest = text.slice(start);
  if (rest.trim()) forms.push({ text: rest, exported: false });
  return forms;
}

/** The functions that emitted code takes a reference to, which the module must declare. */
function referencedFunctions(text: string): string[] {
  return [...new Set([...text.matchAll(/\(ref\.func (\$[\w.-]+)\)/g)].map((match) => match[1]!))];
}

/** The emitter methods that produce on-demand code (emitter.ts `FunctionEmitter`). */
export interface ReachableEmitter {
  /** Whether the host calls the function, which makes it a root. */
  calledByHost(declaration: HirFunction): boolean;
  /** A function's code, with its suspension support when it suspends. */
  emitDeclaration(declaration: HirFunction): string;
  /** A function's value twin, `$fv`, or a suspending closure's value, `$c`. */
  emitFunctionValueWrapper(declaration: HirFunction): string;
  /** The dictionary adapter `$tadapt` of one implementation method. */
  emitTraitAdapter(implementationIndex: number, methodIndex: number): string;
  emitCallableAdapters(): string;
  emitBuiltinTraitAdapters(): string;
  emitForwardingAdapters(): string;
  emitKeyEqualities(): string;
}

export interface ReachableCode {
  /** The fixed text's forms, without the functions nothing reaches. */
  readonly fixed: string;
  /** The declared function references, `(elem declare func ...)`, or "". */
  readonly declarations: string;
  /** The functions, adapters, and helpers that are reached, after the fixed text. */
  readonly code: string;
}

/**
 * The module's functions that the host reaches: from the functions it calls,
 * the exported functions of `fixed`, and the code in `roots`, each function,
 * closure, twin, and adapter that emitted code names, to a fixed point.
 */
export function emitReachableCode(
  program: HirProgram,
  emitter: ReachableEmitter,
  /** Fixed module text: the runtime and per-type and per-trait helpers. */
  fixed: string,
  /** Code that is always emitted, such as globals and host provider functions. */
  roots: string,
): ReachableCode {
  const suspensionIndex = (declaration: HirFunction): number =>
    declaration.suspensionIndex ?? declaration.index;
  const forms = fixedForms(fixed);
  const fixedFunctions = new Map(
    forms.flatMap((form) => (form.function ? [[form.function, form] as const] : [])),
  );
  // Functions and suspending closures by suspension index, as `$f` names them.
  const bySuspension = new Map(
    [...program.functions, ...program.closures]
      .filter((declaration) => !declaration.closure || declaration.suspending)
      .map((declaration) => [suspensionIndex(declaration), declaration] as const),
  );
  const functionsByIndex = new Map(
    program.functions.map((declaration) => [declaration.index, declaration] as const),
  );
  const closuresByIndex = new Map(
    program.closures.map((closure) => [closure.index, closure] as const),
  );
  const unitOf = (symbol: string): string | undefined => {
    if (fixedFunctions.has(symbol)) return `fixed:${symbol}`;
    const match = /^\$([a-z]+)(\d+)(?:_(\d+))?$/.exec(symbol);
    if (!match) return undefined;
    const [, prefix, first, second] = match;
    const index = Number(first);
    if (second !== undefined)
      return ["tadapt", "tspolladapt", "tscanceladapt", "tsresultadapt"].includes(prefix!)
        ? `t:${index}_${second}`
        : undefined;
    if (["f", "body", "poll", "drive", "cancel"].includes(prefix!))
      return bySuspension.has(index) ? `f:${index}` : undefined;
    if (["swpoll", "swcancel", "swresult"].includes(prefix!))
      return bySuspension.has(index) ? `sw:${index}` : undefined;
    if (prefix === "fv") return functionsByIndex.has(index) ? `fv:${index}` : undefined;
    if (prefix === "c") return closuresByIndex.has(index) ? `c:${index}` : undefined;
    return undefined;
  };
  const emitUnit = (key: string): string => {
    const [kind, id] = key.split(":") as [string, string];
    if (kind === "fixed") return fixedFunctions.get(id)!.text;
    if (kind === "f") return emitter.emitDeclaration(bySuspension.get(Number(id))!);
    if (kind === "c") {
      const closure = closuresByIndex.get(Number(id))!;
      return closure.suspending
        ? indent(emitter.emitFunctionValueWrapper(closure))
        : emitter.emitDeclaration(closure);
    }
    if (kind === "fv")
      return indent(emitter.emitFunctionValueWrapper(functionsByIndex.get(Number(id))!));
    if (kind === "sw") return emitStoredSuspensionFunctionAdapters(bySuspension.get(Number(id))!);
    const [implementation, method] = id.split("_").map(Number) as [number, number];
    return emitter.emitTraitAdapter(implementation, method);
  };

  const code = new OnDemandCode(unitOf, emitUnit);
  for (const declaration of program.functions)
    if (emitter.calledByHost(declaration)) code.request(`f:${suspensionIndex(declaration)}`);
  for (const form of forms)
    if (!form.function) code.scan(form.text);
    else if (form.exported) code.request(`fixed:${form.function}`);
  code.scan(roots);
  // The emitter adds callable, builtin, forwarding, and key-equality
  // adapters as it emits the code that needs them, and they may name more
  // on-demand code, so they are emitted again until no new code is named.
  let adapters: string;
  let builtinAdapters: string;
  let keyEqualities: string;
  do {
    code.drain();
    adapters = emitter.emitCallableAdapters();
    builtinAdapters = [emitter.emitBuiltinTraitAdapters(), emitter.emitForwardingAdapters()]
      .filter(Boolean)
      .join("\n\n");
    keyEqualities = emitter.emitKeyEqualities();
    code.scan([adapters, builtinAdapters, keyEqualities].join("\n"));
  } while (code.drain());

  const reachedFixed = forms
    .filter((form) => !form.function || code.has(`fixed:${form.function}`))
    .map((form) => form.text)
    .join("");
  const functions = code.joined(
    [...program.functions, ...program.closures].flatMap((declaration) =>
      declaration.closure
        ? [
            ...(declaration.suspending ? [`f:${suspensionIndex(declaration)}`] : []),
            `c:${declaration.index}`,
          ]
        : [`f:${suspensionIndex(declaration)}`, `fv:${declaration.index}`],
    ),
  );
  const storedSuspensionAdapters = code.joined(
    [...program.functions, ...program.closures]
      .filter((declaration) => declaration.suspending)
      .map((declaration) => `sw:${suspensionIndex(declaration)}`),
  );
  const traitAdapters = [
    code.joined(
      program.implementations.flatMap((implementation) =>
        implementation.methodFunctions.map(
          (mapping) => `t:${implementation.index}_${mapping.methodIndex}`,
        ),
      ),
    ),
    builtinAdapters,
  ]
    .filter(Boolean)
    .join("\n\n");
  const reached = [functions, keyEqualities, storedSuspensionAdapters, adapters, traitAdapters]
    .map((part, index) => (part && index > 0 ? indent(part) : part))
    .filter(Boolean)
    .join("\n\n");
  // A function that code takes a reference to must be declared.
  const referenced = referencedFunctions(`${reachedFixed}\n${reached}\n${roots}`);
  return {
    fixed: reachedFixed,
    declarations: referenced.length > 0 ? `\n  (elem declare func ${referenced.join(" ")})\n` : "",
    code: reached,
  };
}
