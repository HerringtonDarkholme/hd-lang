// How the REPL panel reads an input (`f1c55020:future-work/compiler/repl-plan.md` §3.3, as `hd` does at
// the terminal): an input continues while it is incomplete, inside a
// bracket or a `"""` string, after a line that ends in `:`, or while its
// last line is a line of an indented block, which an empty line ends.
// The terminal asks the parser; the browser has no parser outside the
// worker, so this scans brackets, strings and comments, which covers the
// same inputs.

/** One indentation level. */
export const INDENT_UNIT = "    ";

interface Scan {
  /** Open brackets at the end, outside strings and comments. */
  readonly depth: number;
  /** Whether the end is inside a `"""` string. */
  readonly inTriple: boolean;
}

/**
 * Scans `text` from the state `from`: brackets, `"..."` and `"""..."""`
 * strings with `\` escapes, and `#` comments to the end of the line.
 */
function scan(text: string, from: Scan = { depth: 0, inTriple: false }): Scan {
  let { depth, inTriple } = from;
  let i = 0;
  while (i < text.length) {
    if (inTriple) {
      const end = text.indexOf('"""', i);
      if (end < 0) {
        return { depth, inTriple };
      }
      inTriple = false;
      i = end + 3;
      continue;
    }
    const character = text[i]!;
    if (character === "#") {
      const end = text.indexOf("\n", i);
      if (end < 0) {
        break;
      }
      i = end;
    } else if (text.startsWith('"""', i)) {
      inTriple = true;
      i += 3;
      continue;
    } else if (character === '"') {
      // A one-line string: up to its closing quote or the line's end.
      i += 1;
      while (i < text.length && text[i] !== '"' && text[i] !== "\n") {
        i += text[i] === "\\" ? 2 : 1;
      }
    } else if ("([{".includes(character)) {
      depth += 1;
    } else if (")]}".includes(character)) {
      depth = Math.max(0, depth - 1);
    }
    i += 1;
  }
  return { depth, inTriple };
}

/** `line` without its comment and trailing spaces. */
function codeOf(line: string): string {
  let i = 0;
  while (i < line.length) {
    const character = line[i]!;
    if (character === "#") {
      return line.slice(0, i).trimEnd();
    }
    if (character === '"') {
      i += 1;
      while (i < line.length && line[i] !== '"') {
        i += line[i] === "\\" ? 2 : 1;
      }
    }
    i += 1;
  }
  return line.trimEnd();
}

/** Whether `text`, an input's lines so far joined with `\n`, needs more lines. */
export function needsMoreInput(text: string): boolean {
  const { depth, inTriple } = scan(text);
  if (depth > 0 || inTriple) {
    return true;
  }
  const lines = text.split("\n");
  const last = lines.at(-1)!;
  // An empty line ends a block.
  if (last.trim() === "") {
    return false;
  }
  if (codeOf(last).endsWith(":")) {
    return true;
  }
  // A line of an indented block may be followed by more of the block.
  return lines.length > 1 && /^\s/.test(last);
}

/** The indentation a new line after `line` starts with: one level deeper after a `:`. */
export function continuationIndent(line: string): string {
  const indent = /^\s*/.exec(line)![0];
  return codeOf(line).endsWith(":") ? indent + INDENT_UNIT : indent;
}

/** How many characters Backspace deletes after `before`, the line's text up to the caret. */
export function backspaceWidth(before: string): number {
  if (before === "" || before.trim() !== "" || before.includes("\t")) {
    return 1;
  }
  return before.length % INDENT_UNIT.length || INDENT_UNIT.length;
}

// A line at the left margin that carries on the input before it.
const CARRIES_ON = /^(?:else\b|elif\b|\|>|\.|[)\]}])/;

/**
 * Splits a code block into REPL inputs, one per top-level item, so each
 * expression shows its value: a new input starts at a line at the left
 * margin, outside brackets and strings. Comments and annotations (`@...`)
 * stay with the item after them; empty and comment-only inputs are left out.
 */
export function splitInputs(code: string): string[] {
  const inputs: string[] = [];
  let current: string[] = [];
  let state: Scan = { depth: 0, inTriple: false };
  // Whether `current` holds code that ends an item: not only comments or
  // annotations.
  let holdsItem = false;
  const flush = (): void => {
    const text = current.join("\n").trimEnd();
    if (holdsItem && text !== "") {
      inputs.push(`${text}\n`);
    }
    current = [];
    holdsItem = false;
  };
  for (const line of code.split("\n")) {
    const atMargin = state.depth === 0 && !state.inTriple && /^\S/.test(line);
    if (atMargin && holdsItem && !CARRIES_ON.test(line)) {
      flush();
    }
    current.push(line);
    const code = codeOf(line).trim();
    if (atMargin && code !== "") {
      holdsItem = !code.startsWith("@");
    }
    state = scan(`${line}\n`, state);
  }
  flush();
  return inputs;
}
