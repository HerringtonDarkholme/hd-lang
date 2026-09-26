import { argumentOrderDiagnostics, contextDiagnostics } from "./contextual.ts";
import { earleyAccepts } from "./grammar.ts";
import { lexSource } from "./lexer.ts";
import type { Diagnostic } from "./types.ts";

export function parseSource(source: string): Diagnostic[] {
  const { diagnostics, tokens } = lexSource(source);
  diagnostics.push(...argumentOrderDiagnostics(source), ...contextDiagnostics(source));
  const parsed = earleyAccepts(tokens);
  if (!parsed.accepted) {
    const line = tokens[Math.min(parsed.farthest, tokens.length - 1)]!.line;
    // A lexical or contextual diagnostic on the same or an earlier line
    // already explains the failure; the grammar error would only cascade.
    if (!diagnostics.some((item) => item.line <= line))
      diagnostics.push({ code: "syntax-error", line });
  }
  const unique = new Map(diagnostics.map((item) => [`${item.code}\u0000${item.line}`, item]));
  return [...unique.values()].sort(
    (left, right) => left.line - right.line || left.code.localeCompare(right.code),
  );
}

export function parserSelfTest(): string[] {
  const probes = new Map<string, boolean>([
    ["x := 1 2 3 4\n", false],
    ["+ + + * / % if else while\n", false],
    ["fn f(type: i32) -> void: pass\n", false],
    ["ok := check(a < b, c > d)\n", true],
    ["flags := [x == 1, y != 2]\n", true],
    ['fn f() -> void: log("a;b")\n', true],
    ['fn f() -> void: log("a < b > c")\n', true],
    [
      "fn f(flag: bool) -> void:\n    let total: i32 = 1\n    if flag: total = 2\n    else: total = 0\n",
      true,
    ],
  ]);
  const failures: string[] = [];
  for (const [source, expected] of probes) {
    const accepted = parseSource(source).length === 0;
    if (accepted !== expected)
      failures.push(
        `parser probe ${JSON.stringify(source.trim())}: expected ${expected}, got ${accepted}`,
      );
  }
  return failures;
}
