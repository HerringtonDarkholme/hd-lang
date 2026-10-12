// The highlighter lives with the website (website/src/highlight.ts); the
// terminal REPL and the tests under test/ import it from here.
export { classify, highlight, highlightLines, type TokenClass } from "../website/src/highlight.ts";
