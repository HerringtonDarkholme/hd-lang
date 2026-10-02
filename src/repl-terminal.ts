// The terminal front end of the REPL, `hd repl`. The session and the
// command handling live in repl.ts, which runs in Node and in a browser
// worker; this file adds Node's readline, prompts, and ANSI colors.

import { clearLine, createInterface, cursorTo } from "node:readline";

import type { CompileOptions } from "./compiler.ts";
import { highlight, highlightLines } from "./highlight.ts";
import {
  backspaceWidth,
  continuationIndent,
  isReplCommand,
  needsMoreInput,
  ReplSession,
  respond,
  type ReplEntry,
  type ReplReply,
} from "./repl.ts";

export interface ReplIo {
  readonly input: NodeJS.ReadableStream;
  readonly output: NodeJS.WritableStream;
  readonly terminal?: boolean;
  /** Syntax coloring; defaults to on for a terminal unless NO_COLOR is set. */
  readonly color?: boolean;
}

const RED = "\u001b[31m";
const YELLOW = "\u001b[33m";
const DIM = "\u001b[2m";
const RESET = "\u001b[0m";
/** Ask the terminal to mark pasted text, so pasted lines are not indented again. */
const BRACKETED_PASTE_ON = "\u001b[?2004h";
const BRACKETED_PASTE_OFF = "\u001b[?2004l";

function colorEnabled(terminal: boolean): boolean {
  return terminal && process.env.NO_COLOR === undefined && process.env.TERM !== "dumb";
}

/** Runs an interactive session until end of input or `:quit`. */
export async function runRepl(io: ReplIo, options: CompileOptions = {}): Promise<number> {
  const session = new ReplSession(options);
  const terminal = io.terminal ?? Boolean((io.output as { isTTY?: boolean }).isTTY);
  const reader = createInterface({ input: io.input, output: io.output, terminal });
  const write = (text: string): void => {
    io.output.write(`${text}\n`);
  };
  let closed = false;
  reader.on("close", () => {
    closed = true;
  });
  let pending: string[] = [];
  const promptText = (): string => (pending.length === 0 ? "hd> " : "... ");
  const prompt = (): void => {
    if (!terminal || closed) return;
    reader.setPrompt(promptText());
    // Keep the cursor where it is in any text typed or pasted ahead.
    reader.prompt(true);
  };
  // Auto-indent: a continuation line starts with the indentation
  // `continuationIndent` gives, and Backspace in leading spaces removes a
  // whole level. Pasted lines carry their own indentation, so a line that
  // arrived in a paste gets none: inside a bracketed paste, or in an input
  // chunk that holds more text after a line break.
  let pasting = false;
  let chunkPaste = false;
  /** For each line readline has emitted and the loop has not read: pasted? */
  const pastedLines: boolean[] = [];
  if (terminal) {
    io.output.write(BRACKETED_PASTE_ON);
    io.input.prependListener("data", (chunk: Buffer | string) => {
      chunkPaste = /[\r\n][^]/.test(chunk.toString().replace(/\r\n/g, "\n"));
    });
    reader.on("line", () => pastedLines.push(pasting || chunkPaste));
    io.input.prependListener("keypress", (_text: string, key?: { name?: string }) => {
      if (key?.name === "paste-start") pasting = true;
      else if (key?.name === "paste-end") pasting = false;
      else if (key?.name === "backspace" && !pasting && !closed) {
        // Readline deletes one space itself after this listener.
        const width = backspaceWidth(reader.line.slice(0, reader.cursor));
        for (let count = 1; count < width; count += 1) reader.write(null, { name: "backspace" });
      }
    });
  }
  const indent = (pasted: boolean): void => {
    if (!terminal || closed || pasted || pasting || reader.line !== "") return;
    const text = continuationIndent(pending.at(-1) ?? "");
    if (text !== "") reader.write(text);
  };
  const color = io.color ?? colorEnabled(terminal);
  const paint = (code: string, text: string): string => (color ? `${code}${text}${RESET}` : text);
  if (color) {
    // Readline echoes plain text; after it handles a key, redraw the edited
    // line in color. The return key is handled first, so the submitted line
    // stays colored after readline moves to the next line.
    const redraw = (): void => {
      if (closed) return;
      const position = reader.getCursorPos();
      const columns = (io.output as { columns?: number }).columns || 80;
      if (position.rows > 0 || promptText().length + reader.line.length >= columns) return;
      cursorTo(io.output, 0);
      const line = isReplCommand(reader.line) ? reader.line : highlight(reader.line);
      io.output.write(promptText() + line);
      clearLine(io.output, 1);
      cursorTo(io.output, position.cols);
    };
    const isReturn = (key?: { name?: string }): boolean =>
      key?.name === "return" || key?.name === "enter";
    io.input.prependListener("keypress", (_text: string, key?: { name?: string }) => {
      if (isReturn(key)) redraw();
    });
    io.input.on("keypress", (_text: string, key?: { name?: string }) => {
      if (!isReturn(key)) redraw();
    });
  }
  const show = (entry: ReplEntry): string => {
    if (entry.kind === "warning") return paint(YELLOW, entry.text);
    if (entry.kind === "error") return paint(RED, entry.text);
    if (entry.kind === "code") return color ? highlightLines(entry.text) : entry.text;
    if (entry.kind === "value")
      return color
        ? `${highlight(entry.text)}${paint(DIM, ` : ${entry.type}`)}`
        : `${entry.text} : ${entry.type}`;
    return entry.text;
  };
  const report = (reply: ReplReply): void => {
    for (const entry of reply.entries) write(show(entry));
  };
  if (terminal) write("hd repl. Type :help for commands, :quit to leave.");
  prompt();
  for await (const line of reader) {
    const pasted = pastedLines.shift() ?? false;
    if (pending.length === 0 && isReplCommand(line)) {
      const reply = await respond(session, line);
      if (reply.command === "quit") break;
      report(reply);
      prompt();
      continue;
    }
    pending.push(line);
    if (needsMoreInput(pending)) {
      prompt();
      indent(pasted);
      continue;
    }
    const input = pending.join("\n");
    pending = [];
    report(await respond(session, input));
    prompt();
  }
  if (pending.length > 0) report(await respond(session, pending.join("\n")));
  reader.close();
  if (terminal) io.output.write(BRACKETED_PASTE_OFF);
  if (terminal) write("");
  return 0;
}
