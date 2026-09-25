import { closeToOpen, openToClose } from "./lexer.ts";
import type { Diagnostic } from "./types.ts";

const reserved = new Set(
  "Self and annotate as break continue data defer else enum false fn for if impl in is let match mut nil not or pass pub reified return self shape super trait true type use where while".split(
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

function maskStringLiterals(source: string): string {
  return source.replaceAll(/"(?:\\.|[^"\\])*"/g, '""').replaceAll(/'(?:\\.|[^'\\])*'/g, "''");
}

function lineRecords(source: string): LineRecord[] {
  const lines = source.split(/\r?\n/);
  if (lines.at(-1) === "") lines.pop();
  return lines.map((original, index) => ({
    clean: maskStringLiterals(original).replace(/#.*$/, "").trim(),
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
  const masked = maskStringLiterals(source);
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
      let namedSeen = false;
      for (const part of splitTopLevel(content)) {
        if (!part) continue;
        const named = /^[^=,:]+=(?!=)/.test(part);
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
  if (clean.startsWith("test ")) return "test";
  return undefined;
}

function lineDiagnostics(record: LineRecord, parent: string): Diagnostic[] {
  const { clean, line } = record;
  const diagnostics: Diagnostic[] = [];
  if (clean.includes(";")) diagnostics.push(diagnostic("reserved-semicolon", line));
  if (/^struct\b/.test(clean)) diagnostics.push(diagnostic("old-struct-declaration", line));
  if (/^import\b/.test(clean)) diagnostics.push(diagnostic("old-import-declaration", line));
  if (/^export\b/.test(clean)) diagnostics.push(diagnostic("old-export-declaration", line));
  if (/^use\b.*\{[^}]*\.[A-Za-z_]/.test(clean))
    diagnostics.push(diagnostic("direct-variant-use", line));
  if (/\bfn\s+\w+[^\n]*\([^)]*\bshape\s*:/u.test(clean))
    diagnostics.push(diagnostic("reserved-name", line));
  if (/\b[\w.]+\s*(?:<=|>=|==|!=|<|>)\s*[\w.]+\s*(?:<=|>=|==|!=|<|>)\s*[\w.]+/u.test(clean))
    diagnostics.push(diagnostic("comparison-chaining", line));
  if (/\[[^\]]*,\s*[^\],]+\s*:=/.test(clean))
    diagnostics.push(diagnostic("multi-binding-needs-parentheses", line));
  if (clean === ":" || /\[[^\]]*\w+\s*:\s*$/u.test(clean))
    diagnostics.push(diagnostic("trailing-block-position", line));
  if (clean.startsWith("@") && !new Set(["module", "data", "enum"]).has(parent))
    diagnostics.push(diagnostic("decorator-not-top-level", line));
  if (parent === "data" && /^mut\s+[A-Z]\w*$/u.test(clean))
    diagnostics.push(diagnostic("mutable-embedded-field", line));
  if (parent === "data" && /^mut\s+\w+\s*:/u.test(clean))
    diagnostics.push(diagnostic("mutable-field-modifier", line));
  if (parent === "trait" && /^pub\s+fn\b/.test(clean))
    diagnostics.push(diagnostic("trait-method-visibility", line));
  const firstWord = clean.split(/\s+/, 1)[0]!.replace(/:$/, "");
  if (
    !new Set(["data", "enum", "trait", "impl"]).has(parent) &&
    !reserved.has(firstWord) &&
    /^[\p{L}_][\p{L}\p{N}_]*\s*:\s*[^=]+\s*=/u.test(clean)
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
    const declaration = /^(?:pub\s+)?(?:data|enum|trait|impl|type|fn)\b|^@|^\w+\s*(?::|\()/u.test(
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
