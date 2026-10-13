// The REPL panel (repl-plan.md job W5): an input line with continuation,
// the transcript, history in memory, and the REPL commands. The playground
// shows it as its REPL view, over the editor's files; the website docks it
// at the bottom of its pages (website/client/repl.ts), where code blocks
// get Try in REPL. The session runs in a compiler worker (repl-client.ts).

import { classify } from "../../src/highlight.ts";
import { ReplClient, type ReplStopped } from "./repl-client.ts";
import type { ReplFiles, ReplRan, ReplRebuilt } from "./repl-host.ts";
import {
  backspaceWidth,
  continuationIndent,
  INDENT_UNIT,
  needsMoreInput,
  splitInputs,
} from "./repl-input.ts";

export interface ReplPanelOptions {
  /** The worker script; defaults to `worker.js` beside this bundle. */
  readonly workerUrl?: string | URL;
  /** The session's package: the playground's files; none on the website. */
  readonly files?: () => ReplFiles;
  /** What `:reload` does here, for `:help`. */
  readonly reloadHelp?: string;
  /** The CSS class of a highlighted token of `kind`. */
  readonly tokenClass: (kind: string) => string;
  /** Called when an input starts and stops running, for a Stop button. */
  readonly onBusy?: (busy: boolean) => void;
  readonly onStatus?: (text: string) => void;
  /** Called after `:quit` or Ctrl+D on an empty line ended the session. */
  readonly onQuit?: () => void;
}

const PROMPT = "hd> ";
const CONTINUE = "... ";
const WELCOME =
  "hd REPL. Enter runs an input; a line ending in ':' or an open bracket continues it, and an empty line ends a block. Shift+Enter adds a line; Up and Down recall earlier inputs. :help lists the commands.";

/** `count` and `noun`, plural unless it is one. */
const counted = (count: number, noun: string): string =>
  `${count} ${noun}${count === 1 ? "" : "s"}`;

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className = "",
  text = "",
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text) node.textContent = text;
  return node;
}

/** Hands `each` the complete lines of a stream written in pieces. */
function lineStream(each: (line: string) => void): { write(text: string): void; end(): void } {
  let rest = "";
  return {
    write(text) {
      const lines = (rest + text).split("\n");
      rest = lines.pop()!;
      lines.forEach(each);
    },
    end() {
      if (rest !== "") each(rest);
      rest = "";
    },
  };
}

export class ReplPanel {
  readonly element = element("div", "repl");
  private readonly client: ReplClient;
  private readonly options: ReplPanelOptions;
  private readonly log = element("div", "repl-log");
  private readonly prompt = element("pre", "repl-prompt");
  private readonly shadow = element("pre", "repl-highlight");
  private readonly input = element("textarea", "repl-input");
  private readonly history: string[] = [];
  /** Position in `history` while recalling; `history.length` is the draft. */
  private recall = 0;
  private draft = "";
  /** Inputs run one after another, in the order they were entered. */
  private queue: Promise<void> = Promise.resolve();
  private waiting = 0;

  constructor(parent: HTMLElement, options: ReplPanelOptions) {
    this.options = options;
    this.client = new ReplClient({
      workerUrl: options.workerUrl,
      onReady: () => this.status(this.client.busy ? "Running…" : "REPL ready"),
    });
    const { input, log } = this;
    log.setAttribute("role", "log");
    log.setAttribute("aria-live", "polite");
    const row = element("div", "repl-input-row");
    this.prompt.setAttribute("aria-hidden", "true");
    const editor = element("div", "repl-editor");
    this.shadow.setAttribute("aria-hidden", "true");
    input.rows = 1;
    input.spellcheck = false;
    input.autocapitalize = "off";
    input.setAttribute("autocomplete", "off");
    input.setAttribute("aria-label", "REPL input");
    input.placeholder = "x := 21";
    input.addEventListener("input", () => this.render());
    input.addEventListener("keydown", (event) => this.onKey(event));
    editor.append(this.shadow, input);
    row.append(this.prompt, editor);
    this.element.append(log, row);
    parent.append(this.element);
    this.note(WELCOME);
    this.render();
  }

  /** Loads the worker, so the first input does not wait for the compiler. */
  load(): void {
    if (!this.client.started) this.status("Loading the compiler…");
    this.client.load();
  }

  focus(): void {
    this.input.focus();
  }

  clear(): void {
    this.log.replaceChildren();
  }

  /** Stops the input that runs (the Stop button). */
  stop(): void {
    if (!this.client.busy) return;
    this.status("Stopping…");
    this.client.stop();
  }

  private status(text: string): void {
    this.options.onStatus?.(text);
  }

  private busy(busy: boolean): void {
    this.options.onBusy?.(busy);
  }

  /** Appends `code` to `parent` as spans with the host's token classes. */
  private highlighted(parent: HTMLElement, code: string): void {
    code.split("\n").forEach((line, index) => {
      if (index > 0) parent.append("\n");
      for (const { text, kind } of classify(line))
        parent.append(
          kind === "plain" ? text : element("span", this.options.tokenClass(kind), text),
        );
    });
  }

  /** Redraws the highlighted copy of the input and the prompt column. */
  private render(): void {
    const value = this.input.value;
    const lines = value.split("\n");
    this.prompt.textContent = lines.map((_, i) => (i === 0 ? PROMPT : CONTINUE).trim()).join("\n");
    this.shadow.replaceChildren();
    if (value.trimStart().startsWith(":")) this.shadow.append(value);
    else this.highlighted(this.shadow, value);
    // A trailing newline in a <pre> collapses; keep the last line visible.
    this.shadow.append("\n");
    this.input.rows = Math.max(1, lines.length);
  }

  private setInput(value: string, caret = value.length): void {
    this.input.value = value;
    this.input.setSelectionRange(caret, caret);
    this.render();
  }

  private insert(text: string): void {
    const { selectionStart, selectionEnd, value } = this.input;
    this.setInput(
      value.slice(0, selectionStart) + text + value.slice(selectionEnd),
      selectionStart + text.length,
    );
  }

  private onKey(event: KeyboardEvent): void {
    const { input } = this;
    const { value, selectionStart, selectionEnd } = input;
    if (event.key === "Enter" && !event.isComposing) {
      event.preventDefault();
      const atEnd = selectionStart === value.length;
      const command = !value.includes("\n") && value.trimStart().startsWith(":");
      const editing = !atEnd && value.includes("\n");
      if (event.shiftKey || (!command && (editing || needsMoreInput(value)))) {
        const line = value.slice(0, selectionStart).split("\n").at(-1) ?? "";
        this.insert(`\n${continuationIndent(line)}`);
        return;
      }
      this.setInput("");
      if (value.trim() !== "") void this.submit(value.replace(/\s+$/, ""));
      return;
    }
    if (event.key === "Tab" && !event.shiftKey && !event.ctrlKey && !event.metaKey) {
      event.preventDefault();
      this.insert(INDENT_UNIT);
      return;
    }
    if (event.key === "Backspace" && selectionStart === selectionEnd) {
      const before = value.slice(0, selectionStart).split("\n").at(-1) ?? "";
      const remove = backspaceWidth(before);
      if (remove > 1) {
        event.preventDefault();
        input.setSelectionRange(selectionStart - remove, selectionStart);
        this.insert("");
      }
      return;
    }
    if (event.key === "ArrowUp" && !value.slice(0, selectionStart).includes("\n")) {
      if (this.recall === 0) return;
      event.preventDefault();
      if (this.recall === this.history.length) this.draft = value;
      this.recall -= 1;
      this.setInput(this.history[this.recall]!);
      return;
    }
    if (event.key === "ArrowDown" && !value.slice(selectionEnd).includes("\n")) {
      if (this.recall >= this.history.length) return;
      event.preventDefault();
      this.recall += 1;
      this.setInput(this.recall === this.history.length ? this.draft : this.history[this.recall]!);
      return;
    }
    if (event.ctrlKey && event.key === "l") {
      event.preventDefault();
      this.clear();
      return;
    }
    if (event.ctrlKey && event.key === "d" && value === "") {
      event.preventDefault();
      void this.submit(":quit");
    }
  }

  private remember(text: string): void {
    if (this.history.at(-1) !== text) this.history.push(text);
    this.recall = this.history.length;
    this.draft = "";
  }

  private line(className: string, text: string): HTMLPreElement {
    const node = element("pre", className, text);
    this.log.append(node);
    return node;
  }

  private note(text: string): void {
    this.line("repl-note", text);
  }

  private echo(text: string, number?: number): void {
    const node = element("pre", "repl-echo");
    const command = text.trimStart().startsWith(":");
    text.split("\n").forEach((line, index) => {
      if (index > 0) node.append("\n");
      node.append(element("span", "repl-echo-prompt", index === 0 ? PROMPT : CONTINUE));
      if (command) node.append(line);
      else this.highlighted(node, line);
    });
    if (number !== undefined) node.append(element("span", "repl-number", `repl#${number}`));
    this.log.append(node);
  }

  private scroll(): void {
    this.log.scrollTop = this.log.scrollHeight;
  }

  /** Echoes and runs one input or command, after the ones before it. */
  submit(text: string): Promise<void> {
    this.remember(text);
    this.waiting += 1;
    const run = this.queue.then(() => this.run(text));
    this.queue = run.catch((error: unknown) => {
      this.line("repl-error", `error: ${error instanceof Error ? error.message : String(error)}`);
      this.status("REPL unavailable");
    });
    return this.queue;
  }

  /** Runs a code block's top-level items as inputs, one after another. */
  async trySnippet(code: string): Promise<void> {
    for (const text of splitInputs(code)) await this.submit(text.replace(/\n$/, ""));
  }

  private async run(text: string): Promise<void> {
    try {
      const command = text.trim();
      if (command.startsWith(":") && !command.includes("\n")) {
        this.echo(command);
        await this.command(command);
      } else await this.evaluate(text);
    } finally {
      this.waiting -= 1;
      this.scroll();
    }
  }

  private async command(command: string): Promise<void> {
    switch (command) {
      case ":help":
        this.note(
          [
            "Type a declaration, a statement or an expression; an expression shows its value and type.",
            "An input continues while it is incomplete, after a `:` header or an open bracket;",
            "an empty line ends it. Shift+Enter adds a line.",
            "  :help         this text",
            `  :reload       ${this.options.reloadHelp ?? "rebuild the session from its record"}`,
            "  :quit, :q     end the session; the next input starts a new one (also Ctrl+D on an empty line)",
            "Stop cancels an input that waits; an input that computes is stopped, and the session is rebuilt without it.",
          ].join("\n"),
        );
        return;
      case ":quit":
      case ":q":
      case ":exit":
        await this.client.quit();
        this.note("session ended; the next input starts a new one");
        this.options.onQuit?.();
        return;
      case ":reload":
        if (!this.client.started) {
          this.note("no session yet; the next input starts one");
          return;
        }
        this.status("Rebuilding the session…");
        this.rebuilt(await this.client.rebuild(this.options.files?.() ?? {}), "reloaded");
        this.status("REPL ready");
        return;
      default:
        this.line("repl-error", `error: unknown command \`${command}\`; :help lists the commands`);
    }
  }

  /** Starts the session when none runs; false when it could not start. */
  private async ensureSession(): Promise<boolean> {
    if (this.client.needsRebuild) {
      this.status("Restoring the session…");
      this.rebuilt(await this.client.rebuild(), "restored");
    }
    if (this.client.started) return true;
    this.status(this.client.busy ? "Starting the session…" : "Loading the compiler…");
    const started = await this.client.start(this.options.files?.() ?? {});
    for (const line of started.diagnostics) this.line(this.severity(line), line);
    if (!started.ok)
      this.line("repl-error", `error: ${started.error ?? "the session did not start"}`);
    return started.ok;
  }

  private severity(line: string): string {
    return line.startsWith("warning:") ? "repl-warning" : "repl-error";
  }

  private async evaluate(text: string): Promise<void> {
    if (!(await this.ensureSession())) {
      this.echo(text);
      return;
    }
    const number = this.client.nextNumber;
    this.echo(text, number);
    this.scroll();
    this.busy(true);
    this.status(`Running repl#${number}…`);
    const out = lineStream((line) => this.line("repl-out", line));
    const err = lineStream((line) =>
      this.line(
        /^(?:panic|internal error): |^cancelled$/.test(line) ? "repl-error" : "repl-debug",
        line,
      ),
    );
    let answer: ReplRan | ReplStopped;
    try {
      answer = await this.client.input(`${text}\n`, {
        out: (chunk) => {
          out.write(chunk);
          this.scroll();
        },
        err: (chunk) => {
          err.write(chunk);
          this.scroll();
        },
      });
    } finally {
      out.end();
      err.end();
      this.busy(false);
    }
    if (answer.type === "stopped") {
      this.line(
        "repl-error",
        `stopped: repl#${number} kept computing, so the page ended its worker; the session is rebuilt without it, and its changes are not restored`,
      );
      this.status("Restoring the session…");
      this.rebuilt(await this.client.rebuild(), "restored");
      this.status("REPL ready");
      return;
    }
    for (const line of answer.diagnostics) this.line(this.severity(line), line);
    if (answer.error) this.line("repl-error", `error: ${answer.error}`);
    if (answer.journalFull)
      this.note(
        "note: the session's record of host calls is full, so this session can no longer be rebuilt",
      );
    this.status(this.waiting > 1 ? "Running…" : "REPL ready");
  }

  /** Reports a rebuild as `hd` reports `:reload`. */
  private rebuilt(rebuilt: ReplRebuilt, done: "reloaded" | "restored"): void {
    for (const n of rebuilt.skipped)
      this.note(
        `note: repl#${n} was stopped, so the rebuild skipped it; its changes are not restored`,
      );
    const [first] = rebuilt.dropped;
    if (first !== undefined && rebuilt.reason !== null) {
      if (rebuilt.diagnostics.length > 0)
        for (const line of rebuilt.diagnostics) this.line(this.severity(line), line);
      else this.line("repl-error", `error: repl#${first} does not replay: ${rebuilt.reason}`);
      const later = rebuilt.dropped.length - 1;
      this.note(
        `note: the rebuild dropped repl#${first} and the ${counted(later, "input")} after it`,
      );
    }
    if (!rebuilt.ok) {
      if (first === undefined)
        for (const line of rebuilt.diagnostics) this.line(this.severity(line), line);
      this.line("repl-error", `error: ${rebuilt.error ?? "the rebuild failed"}`);
      this.note("note: the session starts again empty with the next input");
      return;
    }
    const kept = rebuilt.kept.filter((n) => n !== 0).length;
    this.note(`${done}: ${counted(kept, "input")} replayed`);
  }
}
