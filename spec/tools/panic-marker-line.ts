// The panic-marker line check (#1948): a `# panic:` marker names the line
// that raises the panic (conformance/README.md, Comment Directives), so it
// must not sit on a top-level call of a function the fixture declares, whose
// body raises it. spec/check-fixture-index.ts runs it over each panic case.
//
// A call whose receiver and arguments are literals and names evaluates
// nothing that can panic before the callee runs. The call itself can still
// raise three kinds of panic, and the check leaves those lines alone:
// `borrow-conflict` (lending a pointer argument, ref.excl.alias.call-loan and
// ref.convert.lend.loan), `stack-exhausted` (its own frame), and the
// `suspension-*` categories (a bang call polls its callee's suspension). A
// `@track_caller` function reports its panics at its call site, so a call of
// one is left alone too.
//
// Cost: one scan of the fixture's lines collects the names of the functions
// it declares into a Set; the marked statement is then parsed once. Both are
// linear in the fixture's bytes.

import { CommentScanner } from "./fixture-duplicates.ts";

const fnHeader = /^\s*(?:pub\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)!?/;
const markerLine = /#\s*panic:\s*([a-z0-9-]+)\s*$/;
// The categories a call raises itself, whatever its callee's body does.
const callRaised = (code: string): boolean =>
  code === "borrow-conflict" || code === "stack-exhausted" || code.startsWith("suspension-");

/** Net bracket depth of one line of code, strings and characters skipped. */
function depth(code: string): number {
  let net = 0;
  for (const token of tokens(code)) {
    if (token === "(" || token === "[" || token === "{") {
      net += 1;
    } else if (token === ")" || token === "]" || token === "}") {
      net -= 1;
    }
  }
  return net;
}

/**
 * The tokens of a code fragment: names, numbers, punctuation, and literals.
 * A string literal is one token; one with an interpolation is `"${"`, which no
 * inert test accepts, since an interpolation evaluates an expression.
 */
function tokens(code: string): string[] {
  const out: string[] = [];
  // One sticky match per token, so a line is read once, left to right.
  const token =
    /\s+|("""[\s\S]*?"""|"(?:\\.|[^"\\])*"|'(?:\\.[^']*|[^'\\])')|([A-Za-z_][A-Za-z0-9_]*|[0-9][0-9_.]*(?:[eE][+-]?[0-9]+)?[A-Za-z0-9_]*|::|:=|==|!=|<=|>=|->|=>|\*\*|&&|\|\||[\s\S])/y;
  for (let match = token.exec(code); match !== null; match = token.exec(code)) {
    if (match[1] !== undefined) {
      out.push(match[1].includes("${") ? '"${' : match[1]);
    } else if (match[2] !== undefined) {
      out.push(match[2]);
    }
  }
  return out;
}

const isName = (token: string | undefined): boolean =>
  token !== undefined && /^[A-Za-z_$][A-Za-z0-9_]*$/.test(token);
const isLiteral = (token: string | undefined): boolean =>
  token !== undefined && (/^[0-9]/.test(token) || /^["']/.test(token)) && token !== '"${';

/** Parses `items` from `at` to a closing `close` at depth 0, split at depth-0 commas. */
function group(items: string[], at: number, close: string): [string[][], number] | undefined {
  const parts: string[][] = [[]];
  let nest = 0;
  for (let i = at; i < items.length; i += 1) {
    const token = items[i]!;
    if (nest === 0 && token === close) {
      return [parts.at(-1)!.length === 0 ? parts.slice(0, -1) : parts, i + 1];
    }
    if (token === "(" || token === "[" || token === "{") {
      nest += 1;
    } else if (token === ")" || token === "]" || token === "}") {
      nest -= 1;
    }
    if (nest === 0 && token === ",") {
      parts.push([]);
    } else {
      parts.at(-1)!.push(token);
    }
  }
  return undefined;
}

/**
 * Whether evaluating `expr` cannot panic: a literal, a signed number, a name,
 * a bare variant, or a list or tuple of those. A named argument's value
 * counts. Anything else (an operator, an index, a call, a reference, a
 * closure) may raise the panic on the call's own line. One left-to-right pass
 * with no recursion: a value must follow an opening bracket, a comma or the
 * start, and a closing bracket or a comma must follow a value.
 */
function inert(expr: string[]): boolean {
  const items = expr.length >= 2 && isName(expr[0]) && expr[1] === "=" ? expr.slice(2) : expr;
  let wantValue = true;
  for (let i = 0; i < items.length; i += 1) {
    if (wantValue) {
      const used = valueStart(items, i);
      if (used < 0) {
        return false;
      }
      i += Math.max(used - 1, 0);
      wantValue = used === 0;
    } else if (items[i] === ",") {
      wantValue = true;
    } else if (items[i] !== "]" && items[i] !== ")") {
      return false;
    }
  }
  return !wantValue;
}

/**
 * The tokens a value at `i` takes: 0 for an opening bracket (a list or a
 * tuple starts, and a value still follows), 1 for a name, a literal or the
 * close of an empty or comma-ended bracket, 2 for a signed number or a bare
 * variant; -1 for anything else.
 */
function valueStart(items: string[], i: number): number {
  const [prev, token, next] = [items[i - 1], items[i]!, items[i + 1]];
  if (token === "[" || token === "(") {
    return 0;
  }
  if ((token === "]" || token === ")") && (prev === "[" || prev === "(" || prev === ",")) {
    return 1;
  }
  if (((token === "+" || token === "-") && isLiteral(next)) || (token === "." && isName(next))) {
    return 2;
  }
  return isName(token) || isLiteral(token) ? 1 : -1;
}

/** The binding or `return` before a statement's expression, as a token count. */
function bindingLength(items: string[]): number {
  if (items[0] === "return") {
    return 1;
  }
  if (items[0] === "let") {
    const equals = items.indexOf("=");
    return equals < 0 ? 0 : equals + 1;
  }
  return items[1] === ":=" || (items[0] === "_" && items[1] === "=") ? 2 : 0;
}

/**
 * The callee of a statement that is one call with inert receiver and
 * arguments, as `f(a)`, `f!(a)`, `f::[T](a)`, `Type::f(a)` or `recv.f(a)`.
 */
function inertCallee(items: string[]): string | undefined {
  let at = 0;
  let callee: string | undefined;
  // The path or receiver before the callee: names joined by `::` or `.`.
  while (isName(items[at])) {
    callee = items[at];
    at += 1;
    if (items[at] === "!") {
      at += 1;
    }
    if (items[at] === "::" && items[at + 1] === "[") {
      const generic = group(items, at + 2, "]");
      if (generic === undefined) {
        return undefined;
      }
      at = generic[1];
    }
    if (items[at] === "::" || items[at] === ".") {
      at += 1;
      continue;
    }
    break;
  }
  if (callee === undefined || items[at] !== "(") {
    return undefined;
  }
  const args = group(items, at + 1, ")");
  if (args === undefined || args[1] !== items.length || !args[0].every(inert)) {
    return undefined;
  }
  return callee;
}

/** Whether a function header's tokens end in a body: a `:` after the parameters. */
function hasBody(items: string[]): boolean {
  const params = items.indexOf("(");
  const closed = params < 0 ? undefined : group(items, params + 1, ")");
  if (closed === undefined) {
    return false;
  }
  let nest = 0;
  for (const token of items.slice(closed[1])) {
    if (nest === 0 && token === ":") {
      return true;
    }
    if (token === "[" || token === "(") {
      nest += 1;
    } else if (token === "]" || token === ")") {
      nest -= 1;
    }
  }
  return false;
}

/** Names the fixture's bodied, non-`@track_caller` functions declare. */
function bodiedFunctions(lines: string[]): Set<string> {
  const names = new Set<string>();
  let previous = "";
  let i = 0;
  while (i < lines.length) {
    const line = lines[i]!;
    const header = fnHeader.exec(line);
    // A header runs until its brackets close; its lines hold no other header,
    // so the scan resumes after them and reads each line once.
    const parts = [line];
    i += 1;
    for (let open = header ? depth(line) : 0; open > 0 && i < lines.length; i += 1) {
      parts.push(lines[i]!);
      open += depth(lines[i]!);
    }
    if (header && previous !== "@track_caller" && hasBody(tokens(parts.join(" ")))) {
      names.add(header[1]!);
    }
    if (line.trim() !== "") {
      previous = line.trim();
    }
  }
  return names;
}

/**
 * The function whose top-level call the fixture's panic marker sits on, when
 * the panic comes from that function's body; otherwise undefined.
 */
export function panicMarkerOnCall(text: string): string | undefined {
  const scanner = new CommentScanner();
  const lines: string[] = [];
  let marked = -1;
  let code = "";
  for (const physical of text.split("\n")) {
    const line = physical.endsWith("\r") ? physical.slice(0, -1) : physical;
    // A doc-test line (`## code`) is code of its doc test.
    const doc = /^\s*## ?/.exec(line);
    const scanned = doc ? scanner.scan(line.slice(doc[0].length)) : scanner.scan(line);
    const found = markerLine.exec(line);
    if (found && marked < 0) {
      marked = lines.length;
      code = found[1]!;
    }
    lines.push(scanned.replace(/\s*#(?:diagnostic|warning|panic):[a-z0-9-]+$/, ""));
  }
  if (marked < 0 || callRaised(code)) {
    return undefined;
  }
  // A marker on a line that closes brackets names the statement they close.
  const statement = [lines[marked]!.trim().replace(/,$/, "")];
  for (let open = depth(statement[0]!), start = marked - 1; open < 0 && start >= 0; start -= 1) {
    statement.push(lines[start]!.trim());
    open += depth(lines[start]!);
  }
  const items = tokens(statement.reverse().join(" "));
  const callee = inertCallee(items.slice(bindingLength(items)));
  return callee !== undefined && bodiedFunctions(lines).has(callee) ? callee : undefined;
}
