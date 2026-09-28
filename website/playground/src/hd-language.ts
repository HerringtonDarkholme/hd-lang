// hd syntax support for CodeMirror. Token classes come from the repository's
// own highlighter (src/highlight.ts, also used by the REPL): the stream
// parser only replays `classify(line)` spans, so both agree by construction.

import { EditorSelection, Prec, type Extension } from "@codemirror/state";
import {
  indentUnit,
  LanguageSupport,
  StreamLanguage,
  syntaxHighlighting,
  type StringStream,
} from "@codemirror/language";
import { keymap, type Command } from "@codemirror/view";
import { tagHighlighter, tags, type Tag } from "@lezer/highlight";

import { classify, type TokenClass } from "../../../src/highlight.ts";

export type StyledClass = Exclude<TokenClass, "plain">;

/** The highlight tag for each class; `plain` text gets no token. */
export const TOKEN_TAGS: Readonly<Record<StyledClass, Tag>> = {
  keyword: tags.keyword,
  literal: tags.atom,
  type: tags.typeName,
  function: tags.function(tags.variableName),
  number: tags.number,
  string: tags.string,
  interpolation: tags.special(tags.string),
  comment: tags.lineComment,
  operator: tags.operator,
};

/** The CSS class the editor puts on text of each token class. */
export const cssClass = (kind: StyledClass): string => `hd-${kind}`;

export const hdHighlighter = tagHighlighter(
  Object.entries(TOKEN_TAGS).map(([kind, tag]) => ({ tag, class: cssClass(kind as StyledClass) })),
);

interface ClassifiedLine {
  readonly text: string;
  readonly spans: readonly { readonly end: number; readonly kind: TokenClass }[];
}

let lastLine: ClassifiedLine | undefined;

function classifiedLine(text: string): ClassifiedLine {
  if (lastLine?.text === text) return lastLine;
  let end = 0;
  const spans = classify(text).map(({ text: part, kind }) => ({ end: (end += part.length), kind }));
  lastLine = { text, spans };
  return lastLine;
}

export const hdStreamLanguage = StreamLanguage.define<null>({
  name: "hd",
  startState: () => null,
  token(stream: StringStream): string | null {
    const span = classifiedLine(stream.string).spans.find(({ end }) => end > stream.pos);
    if (!span) {
      stream.skipToEnd();
      return null;
    }
    stream.pos = span.end;
    return span.kind === "plain" ? null : span.kind;
  },
  tokenTable: TOKEN_TAGS,
  languageData: { commentTokens: { line: "#" } },
});

/** The source of `line` without its trailing comment. */
function codeOf(line: string): string {
  return classify(line)
    .filter(({ kind }) => kind !== "comment")
    .map(({ text }) => text)
    .join("");
}

/** Enter keeps the line's indentation and opens a block after a trailing `:`. */
export const newlineAndIndent: Command = (view) => {
  const { state } = view;
  view.dispatch(
    state.changeByRange((range) => {
      const line = state.doc.lineAt(range.from);
      const before = line.text.slice(0, range.from - line.from);
      let indent = /^ */.exec(line.text)![0].length;
      if (before.trim() === "") indent = before.length;
      else if (codeOf(before).trimEnd().endsWith(":")) indent += 4;
      const insert = `\n${" ".repeat(indent)}`;
      return {
        changes: { from: range.from, to: range.to, insert },
        range: EditorSelection.cursor(range.from + insert.length),
      };
    }),
    { scrollIntoView: true, userEvent: "input" },
  );
  return true;
};

export function hd(): Extension {
  return [
    new LanguageSupport(hdStreamLanguage),
    syntaxHighlighting(hdHighlighter),
    indentUnit.of("    "),
    Prec.high(keymap.of([{ key: "Enter", run: newlineAndIndent }])),
  ];
}
