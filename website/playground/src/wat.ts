// A small tokenizer for the WebAssembly text format, for the WAT view. It
// splits text into runs that concatenate back to the input, so rendering the
// runs shows every character. It does not check that the text is valid WAT.

export type WatTokenKind =
  | "keyword"
  | "instruction"
  | "type"
  | "name"
  | "number"
  | "string"
  | "comment"
  | "paren"
  | "plain";

interface WatToken {
  readonly text: string;
  readonly kind: WatTokenKind;
}

/** Words that give a module its structure, rather than instructions. */
const KEYWORDS = new Set([
  "module",
  "rec",
  "type",
  "sub",
  "final",
  "func",
  "struct",
  "array",
  "field",
  "mut",
  "param",
  "result",
  "local",
  "global",
  "import",
  "export",
  "memory",
  "table",
  "elem",
  "data",
  "start",
  "declare",
  "item",
  "offset",
  "tag",
  "then",
  "else",
  "end",
]);

/** Value, storage, reference, and heap types. */
const TYPES = new Set([
  "i8",
  "i16",
  "i32",
  "i64",
  "f32",
  "f64",
  "v128",
  "ref",
  "null",
  "any",
  "eq",
  "i31",
  "none",
  "extern",
  "noextern",
  "nofunc",
  "exn",
  "noexn",
  "anyref",
  "eqref",
  "i31ref",
  "structref",
  "arrayref",
  "nullref",
  "externref",
  "nullexternref",
  "funcref",
  "nullfuncref",
  "exnref",
  "nullexnref",
]);

/** Characters of an identifier or keyword (the spec's `idchar`). */
const ID_CHAR = /[0-9A-Za-z!#$%&'*+\-./:<=>?@\\^_`|~]/;
const NUMBER =
  /^[+-]?(?:inf|nan(?::0x[0-9A-Fa-f](?:_?[0-9A-Fa-f])*)?|0x[0-9A-Fa-f](?:_?[0-9A-Fa-f])*(?:\.(?:[0-9A-Fa-f](?:_?[0-9A-Fa-f])*)?)?(?:[Pp][+-]?\d(?:_?\d)*)?|\d(?:_?\d)*(?:\.(?:\d(?:_?\d)*)?)?(?:[Ee][+-]?\d(?:_?\d)*)?)$/;

function wordKind(word: string): WatTokenKind {
  if (word.startsWith("$")) {
    return "name";
  }
  if (NUMBER.test(word)) {
    return "number";
  }
  if (TYPES.has(word)) {
    return "type";
  }
  if (KEYWORDS.has(word)) {
    return "keyword";
  }
  if (/^[a-z]/.test(word)) {
    return "instruction";
  }
  return "plain";
}

function scanBlockComment(text: string, start: number): number {
  let depth = 0;
  let index = start;
  while (index < text.length) {
    if (text.startsWith("(;", index)) {
      depth += 1;
      index += 2;
    } else if (text.startsWith(";)", index)) {
      depth -= 1;
      index += 2;
      if (depth === 0) {
        break;
      }
    } else {
      index += 1;
    }
  }
  return index;
}

function scanQuoted(text: string, quote: number): number {
  let index = quote + 1;
  while (index < text.length && text[index] !== '"' && text[index] !== "\n") {
    index += text[index] === "\\" ? 2 : 1;
  }
  return text[index] === '"' ? index + 1 : index;
}

function pushWord(push: (kind: WatTokenKind, value: string) => void, word: string): void {
  const equals = word.indexOf("=");
  if (equals > 0 && equals < word.length - 1 && !word.startsWith("$")) {
    push("keyword", word.slice(0, equals + 1));
    push(wordKind(word.slice(equals + 1)), word.slice(equals + 1));
  } else {
    push(wordKind(word), word);
  }
}

function plainEnd(text: string, start: number): number {
  let index = start;
  while (index < text.length && !ID_CHAR.test(text[index]!) && !'()";'.includes(text[index]!)) {
    index += 1;
  }
  return index === start ? start + 1 : index;
}

/** Splits WAT text into highlighted runs; joining their text gives back `text`. */
export function tokenizeWat(text: string): WatToken[] {
  const tokens: WatToken[] = [];
  const push = (kind: WatTokenKind, value: string): void => {
    const last = tokens.at(-1);
    if (last && last.kind === kind && (kind === "plain" || kind === "paren")) {
      tokens[tokens.length - 1] = { kind, text: last.text + value };
    } else {
      tokens.push({ kind, text: value });
    }
  };
  let index = 0;
  while (index < text.length) {
    const char = text[index]!;
    const start = index;
    if (text.startsWith(";;", index)) {
      const end = text.indexOf("\n", index);
      index = end === -1 ? text.length : end;
      push("comment", text.slice(start, index));
    } else if (text.startsWith("(;", index)) {
      // Block comments nest.
      index = scanBlockComment(text, index);
      push("comment", text.slice(start, index));
    } else if (char === "(" || char === ")") {
      index += 1;
      push("paren", char);
    } else if (char === '"') {
      index = scanQuoted(text, index);
      push("string", text.slice(start, index));
    } else if (char === "$" && text[index + 1] === '"') {
      // A quoted name, `$"std/format/impl Display for usize.to_string#0"`,
      // which wasmprinter writes for names that are not plain identifiers.
      index = scanQuoted(text, index + 1);
      push("name", text.slice(start, index));
    } else if (ID_CHAR.test(char)) {
      while (index < text.length && ID_CHAR.test(text[index]!)) {
        index += 1;
      }
      const word = text.slice(start, index);
      // `offset=4` and `align=8` are a keyword and a number.
      pushWord(push, word);
    } else {
      index = plainEnd(text, start);
      push("plain", text.slice(start, index));
    }
  }
  return tokens;
}
