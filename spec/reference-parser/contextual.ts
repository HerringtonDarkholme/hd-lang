import { closeToOpen, maskLiterals, openToClose } from "./lexer.ts";
import type { Diagnostic } from "./types.ts";

const reserved = new Set(
  "Self break continue data defer else enum false fn for if impl in is let match mut pass pub return self trait true type while".split(
    " ",
  ),
);

interface LineRecord {
  readonly clean: string;
  readonly indent: number;
  readonly line: number;
  readonly original: string;
}

interface ContextEntry {
  readonly indent: number;
  readonly kind: string;
}

interface ParenthesisEntry {
  readonly index: number;
  readonly line: number;
}

function diagnostic(code: string, line: number): Diagnostic {
  return { code, line };
}

function lineRecords(source: string): LineRecord[] {
  const lines = source.split(/\r?\n/);
  const masked = maskLiterals(source).split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  return lines.map((original, index) => ({
    clean: (masked[index] ?? "").trim(),
    indent: /^ */.exec(original)![0].length,
    line: index + 1,
    original,
  }));
}

function splitTopLevel(text: string): string[] {
  const parts: string[] = [];
  const stack: string[] = [];
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index]!;
    if (openToClose.has(character)) stack.push(character);
    else if (closeToOpen.has(character) && stack.at(-1) === closeToOpen.get(character)) stack.pop();
    else if (character === "," && stack.length === 0) {
      parts.push(text.slice(start, index).trim());
      start = index + 1;
    }
  }
  parts.push(text.slice(start).trim());
  return parts;
}

export function argumentOrderDiagnostics(source: string): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const masked = maskLiterals(source);
  const lines = masked.split(/\r?\n/);
  const stack: ParenthesisEntry[] = [];
  let line = 1;
  for (let index = 0; index < masked.length; index += 1) {
    const character = masked[index]!;
    if (character === "\n") line += 1;
    else if (character === "(") stack.push({ index, line });
    else if (character === ")" && stack.length > 0) {
      const start = stack.pop()!;
      const content = masked.slice(start.index + 1, index);
      if (
        /\$\s*\.\s*(?:with|context)\s*$/u.test(
          masked.slice(Math.max(0, start.index - 64), start.index),
        )
      )
        continue;
      let namedSeen = false;
      for (const part of splitTopLevel(content)) {
        if (!part) continue;
        const named = /^(?:[\p{L}_][\p{L}\p{N}_]*|`[\p{L}_][\p{L}\p{N}_]*`)\s*=(?!=)/u.test(part);
        if (named) namedSeen = true;
        else if (namedSeen && !part.startsWith("fn ") && !part.startsWith("mut fn ")) {
          const code = lines[start.line - 1]?.includes("=>") ? "pattern-order" : "argument-order";
          diagnostics.push(diagnostic(code, start.line));
          break;
        }
      }
    }
  }
  return diagnostics;
}

function declarationContext(clean: string): string | undefined {
  if (/^(?:pub\s+)?data\b/.test(clean)) return "data";
  if (/^(?:pub\s+)?enum\b/.test(clean)) return "enum";
  if (/^(?:pub\s+)?trait\b/.test(clean)) return "trait";
  if (/^(?:pub\s+)?impl\b/.test(clean)) return "impl";
  if (/^(?:pub\s+)?fn\b/.test(clean)) return "function";
  if (clean === "tests:") return "tests";
  return undefined;
}

// Chapter 01: an unbackticked `reified` first in a generic parameter is always
// the modifier, so it must be followed by the parameter name; `[reified]`
// is a syntax error. Checks the generic parameters of a function, method,
// implementation, or enum variant header written on one line.
function loneReifiedParameter(clean: string, parent: string): boolean {
  const name = String.raw`(?:[\p{L}_][\p{L}\p{N}_]*|\x60[\p{L}_][\p{L}\p{N}_]*\x60)`;
  const header = new RegExp(
    parent === "enum"
      ? String.raw`^(?:@\S+\s+)*${name}\s*\[`
      : String.raw`^(?:pub\s+)?(?:fn\s+${name}!?|impl)\s*\[`,
    "u",
  ).exec(clean);
  if (!header) return false;
  let depth = 1;
  let end = header[0].length;
  while (end < clean.length && depth > 0) {
    const character = clean[end]!;
    if (openToClose.has(character)) depth += 1;
    else if (closeToOpen.has(character)) depth -= 1;
    end += 1;
  }
  const parameters = splitTopLevel(clean.slice(header[0].length, end - 1));
  return parameters.some((parameter) => /^reified\s*(?:\.\.\.|<|:|$)/u.test(parameter));
}

// Chapter 02: a requirement row is a comma list of keys. A `+` or `-`
// between keys, as in `$ A + B`, `$ (R - K)`, or `$.Context[A + B]`, is
// the removed row union or subtraction. A row type argument such as
// `Job[A + B]` is matched only between capitalized names, so a list or
// index expression like `[a + b]` stays an expression.
const rowKey = String.raw`(?:mut\s+)?[\p{L}_][\p{L}\p{N}_.]*(?:\[(?:[^[\]]|\[[^[\]]*\])*\])?`;
const oldRowOperator = new RegExp(
  String.raw`\$\s*(?:\.\s*Context\s*\[)?\s*\(?\s*${rowKey}\s*[+-]\s*\(?\s*(?:mut\s+)?[\p{L}_]`,
  "u",
);
const oldRowTypeArgument = new RegExp(
  String.raw`\b\p{Lu}[\p{L}\p{N}_]*\[(?:[^[\]]*,\s*)?\p{Lu}[\p{L}\p{N}_]*\s*\+\s*\p{Lu}`,
  "u",
);

function lineDiagnostics(record: LineRecord, parent: string): Diagnostic[] {
  const { clean, line } = record;
  const diagnostics: Diagnostic[] = [];
  if (clean.includes(";")) diagnostics.push(diagnostic("reserved-semicolon", line));
  if (/^struct\b/.test(clean)) diagnostics.push(diagnostic("old-struct-declaration", line));
  if (/^import\b/.test(clean)) diagnostics.push(diagnostic("old-import-declaration", line));
  if (/^export\b/.test(clean)) diagnostics.push(diagnostic("old-export-declaration", line));
  if (oldRowOperator.test(clean) || oldRowTypeArgument.test(clean))
    diagnostics.push(diagnostic("old-row-operator", line));
  if (/^(?:pub\s+)?use\s+(?:pkg|std|dep|self|super)\b.*\{[^}]*\.[A-Za-z_]/.test(clean))
    diagnostics.push(diagnostic("direct-variant-use", line));
  if (/\b[\w.]+\s*(?:<=|>=|==|!=|<|>)\s*[\w.]+\s*(?:<=|>=|==|!=|<|>)\s*[\w.]+/u.test(clean))
    diagnostics.push(diagnostic("comparison-chaining", line));
  // `Type::name` or `x::name` without a call is reserved for method values.
  // Associated type projections such as `I::Item` are capitalized by
  // convention, so only a lowercase member is checked.
  // The type-argument list may nest brackets two deep, as in `::of[Box[List[i32]]]`.
  if (
    /::[a-z_][\p{L}\p{N}_]*(?:\[(?:[^[\]]|\[(?:[^[\]]|\[[^[\]]*\])*\])*\])?(?![\p{L}\p{N}_[(!])/u.test(
      clean,
    )
  )
    diagnostics.push(diagnostic("deferred-method-value", line));
  if (/\[[^\]]*,\s*[^\],]+\s*:=/.test(clean))
    diagnostics.push(diagnostic("multi-binding-needs-parentheses", line));
  // A bracketed control-flow expression or closure may end its header line
  // in `:`; only a call colon inside brackets is a misplaced trailing block.
  const bracketTail = /\[([^\]]*\w+)\s*:\s*$/u.exec(clean)?.[1];
  if (
    clean === ":" ||
    (bracketTail !== undefined && !/\b(?:for|if|else|while|match|fn|with)\b/u.test(bracketTail))
  )
    diagnostics.push(diagnostic("trailing-block-position", line));
  if (clean.startsWith("@") && !new Set(["module", "data", "enum"]).has(parent))
    diagnostics.push(diagnostic("decorator-not-top-level", line));
  if (parent === "data" && /^mut\s+[A-Z]\w*$/u.test(clean))
    diagnostics.push(diagnostic("mutable-embedded-field", line));
  if (parent === "data" && /^mut\s+\w+\s*:/u.test(clean))
    diagnostics.push(diagnostic("mutable-field-modifier", line));
  if (parent === "trait" && /^pub\s+fn\b/.test(clean))
    diagnostics.push(diagnostic("trait-method-visibility", line));
  if (loneReifiedParameter(clean, parent)) diagnostics.push(diagnostic("syntax-error", line));
  const firstWord = clean.split(/\s+/, 1)[0]!.replace(/:$/, "");
  if (
    !new Set(["data", "enum", "trait", "impl"]).has(parent) &&
    !reserved.has(firstWord) &&
    /^[\p{L}_][\p{L}\p{N}_]*\s*:\s*[^=]+(?<![!<>])=(?!=)/u.test(clean)
  )
    diagnostics.push(diagnostic("missing-let", line));
  return diagnostics;
}

function docCommentDiagnostics(records: readonly LineRecord[]): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  for (let index = 0; index < records.length; index += 1) {
    const record = records[index]!;
    if (!record.original.trimStart().startsWith("##")) continue;
    let follower = index + 1;
    while (records[follower]?.original.trimStart().startsWith("##")) follower += 1;
    const next = records[follower];
    if (!next) {
      diagnostics.push(diagnostic("doc-comment-without-target", record.line));
      continue;
    }
    const declaration =
      /^(?:pub\s+)?(?:data|enum|trait|impl|type|fn)\b|^@|^(?:\w+|`\w+`)\s*(?::|\()/u.test(
        next.clean,
      );
    if (next.indent !== record.indent || !declaration || next.clean.includes(":="))
      diagnostics.push(diagnostic("doc-comment-without-target", record.line));
  }
  return diagnostics;
}

export function contextDiagnostics(source: string): Diagnostic[] {
  const diagnostics: Diagnostic[] = [];
  const records = lineRecords(source);
  const context: ContextEntry[] = [];
  for (const record of records) {
    if (!record.clean) continue;
    while (context.length > 0 && record.indent <= context.at(-1)!.indent) context.pop();
    diagnostics.push(...lineDiagnostics(record, context.at(-1)?.kind ?? "module"));
    const kind = declarationContext(record.clean);
    if (kind && record.clean.endsWith(":")) context.push({ indent: record.indent, kind });
  }
  diagnostics.push(...docCommentDiagnostics(records));
  return diagnostics;
}
