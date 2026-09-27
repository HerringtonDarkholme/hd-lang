// The site's bottom REPL panel. It docks at the bottom of every page and
// opens over the content, so trying code never navigates away. Input is read
// with the REPL's own rules (src/repl-input.ts) and highlighted with the
// repository highlighter; the session itself runs in the playground's
// compiler worker, which loads only when the panel first opens.
//
// website/build.ts bundles this file to assets/repl.js when the playground
// build exists; the layout then includes it and names the worker on <body>.

import { CompilerClient, type Interrupted } from "../../playground/src/compiler-client.ts";
import { classify } from "../../src/highlight.ts";
import { needsMoreInput, splitInputs } from "../../src/repl-input.ts";
import type { ReplEntry, ReplReply } from "../../src/repl.ts";

const HISTORY_KEY = "hd-repl-history";
const HISTORY_LIMIT = 200;
const INDENT = "    ";
const WELCOME =
  "hd REPL. Enter evaluates; a line ending in ':' or an open bracket continues, and an empty line ends a block. Shift+Enter adds a line, Up and Down recall history, Ctrl+` toggles this panel. Type :help for commands.";

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

/** Appends `code` to `parent` as spans with the site's `hl-*` token classes. */
function appendHighlighted(parent: HTMLElement, code: string): void {
  code.split("\n").forEach((line, index) => {
    if (index > 0) parent.append("\n");
    for (const { text, kind } of classify(line))
      parent.append(kind === "plain" ? text : element("span", `hl-${kind}`, text));
  });
}

function loadHistory(): string[] {
  try {
    const stored = JSON.parse(localStorage.getItem(HISTORY_KEY) ?? "[]") as unknown;
    return Array.isArray(stored) ? stored.filter((item) => typeof item === "string") : [];
  } catch {
    return [];
  }
}

function saveHistory(history: readonly string[]): void {
  try {
    localStorage.setItem(HISTORY_KEY, JSON.stringify(history.slice(-HISTORY_LIMIT)));
  } catch {
    // History is a convenience; the panel works without storage.
  }
}

class ReplPanel {
  private readonly client: CompilerClient;
  private readonly toggle = element("button", "repl-toggle");
  private readonly panel = element("section", "repl-panel");
  private readonly status = element("span", "repl-status");
  private readonly stopButton = element("button", "repl-stop", "Stop");
  private readonly log = element("div", "repl-log");
  private readonly prompt = element("pre", "repl-prompt");
  private readonly shadow = element("pre", "repl-highlight");
  private readonly input = element("textarea", "repl-input");
  private readonly history = loadHistory();
  /** Position in `history` while recalling; `history.length` means the draft. */
  private recall = this.history.length;
  private draft = "";
  private pending = 0;
  private ready = false;
  private welcomed = false;

  constructor(workerUrl: string) {
    this.client = new CompilerClient({
      workerUrl,
      lazy: true,
      onReady: () => {
        this.ready = true;
        this.setStatus(this.pending > 0 ? "Running…" : "Ready");
      },
    });
    this.build();
  }

  private build(): void {
    const { toggle, panel, input } = this;
    toggle.type = "button";
    toggle.id = "repl-toggle";
    toggle.setAttribute("aria-controls", "repl-panel");
    toggle.setAttribute("aria-expanded", "false");
    toggle.title = "Open the hd REPL (Ctrl+`)";
    toggle.append(element("span", "repl-toggle-mark", "›_"), " REPL");
    toggle.addEventListener("click", () => this.setOpen(panel.hidden !== false));

    panel.id = "repl-panel";
    panel.hidden = true;
    panel.setAttribute("aria-label", "hd REPL");
    const header = element("header", "repl-header");
    const title = element("strong", "repl-title", "hd REPL");
    this.status.setAttribute("role", "status");
    const actions = element("div", "repl-actions");
    const button = (label: string, title: string, action: () => void): HTMLButtonElement => {
      const node = element("button", "", label);
      node.type = "button";
      node.title = title;
      node.addEventListener("click", action);
      return node;
    };
    this.stopButton.type = "button";
    this.stopButton.hidden = true;
    this.stopButton.title = "Stop the running input; the session keeps what it had accepted";
    this.stopButton.addEventListener("click", () => this.client.stop());
    actions.append(
      this.stopButton,
      button(
        "Reset",
        "Forget every declaration and binding (:reset)",
        () => void this.submit(":reset"),
      ),
      button("Clear", "Clear the output (Ctrl+L)", () => this.log.replaceChildren()),
      button("×", "Close (Esc)", () => this.setOpen(false)),
    );
    actions.lastElementChild!.setAttribute("aria-label", "Close the REPL");
    header.append(title, this.status, actions);

    this.log.setAttribute("role", "log");
    this.log.setAttribute("aria-live", "polite");
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
    panel.append(header, this.log, row);
    panel.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        this.setOpen(false);
        toggle.focus();
      }
    });
    document.body.append(toggle, panel);
    document.body.classList.add("repl-ready");
    this.render();

    document.addEventListener("keydown", (event) => {
      if ((event.ctrlKey || event.metaKey) && (event.key === "`" || event.code === "Backquote")) {
        event.preventDefault();
        this.setOpen(panel.hidden !== false);
      }
    });
    document.addEventListener("click", (event) => {
      const target = event.target instanceof Element ? event.target.closest(".try-repl") : null;
      const code = target?.closest(".code-block")?.querySelector("pre code")?.textContent;
      if (code) void this.trySnippet(code);
    });
  }

  private setStatus(text: string): void {
    this.status.textContent = text;
  }

  setOpen(open: boolean): void {
    this.panel.hidden = !open;
    this.toggle.setAttribute("aria-expanded", String(open));
    this.toggle.title = `${open ? "Close" : "Open"} the hd REPL (Ctrl+\`)`;
    document.body.classList.toggle("repl-open", open);
    if (!open) return;
    if (!this.client.started) {
      this.setStatus("Loading the compiler…");
      this.client.start();
    }
    if (!this.welcomed) {
      this.welcomed = true;
      this.show({ kind: "info", text: WELCOME });
    }
    this.input.focus();
  }

  /** Redraws the highlighted copy of the input and the prompt column. */
  private render(): void {
    const value = this.input.value;
    const lines = value.split("\n");
    this.prompt.textContent = lines.map((_, index) => (index === 0 ? "hd>" : "...")).join("\n");
    this.shadow.replaceChildren();
    if (value.trimStart().startsWith(":")) this.shadow.append(value);
    else appendHighlighted(this.shadow, value);
    // A trailing newline in a <pre> collapses; keep the last empty line visible.
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
      const lines = value.split("\n");
      const command = lines.length === 1 && value.trimStart().startsWith(":");
      const editing = !atEnd && lines.length > 1;
      if (event.shiftKey || (!command && (editing || needsMoreInput(lines)))) {
        const line = value.slice(0, selectionStart).split("\n").at(-1) ?? "";
        const indent = /^\s*/.exec(line)![0];
        this.insert(`\n${indent}${/:\s*$/.test(line) ? INDENT : ""}`);
        return;
      }
      this.setInput("");
      void this.submit(value.replace(/\s+$/, ""));
      return;
    }
    if (event.key === "Tab" && !event.shiftKey && !event.ctrlKey && !event.metaKey) {
      event.preventDefault();
      this.insert(INDENT);
      return;
    }
    if (event.key === "Backspace" && selectionStart === selectionEnd) {
      // In leading spaces, delete back to the previous indentation stop.
      const before = value.slice(0, selectionStart).split("\n").at(-1) ?? "";
      if (before.length > 0 && /^ +$/.test(before)) {
        event.preventDefault();
        const remove = before.length % INDENT.length || INDENT.length;
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
      this.log.replaceChildren();
      return;
    }
    if (event.ctrlKey && event.key === "d" && value === "") {
      event.preventDefault();
      this.setOpen(false);
      this.toggle.focus();
    }
  }

  private remember(text: string): void {
    if (text.trim() === "" || this.history.at(-1) === text) {
      this.recall = this.history.length;
      return;
    }
    this.history.push(text);
    if (this.history.length > HISTORY_LIMIT)
      this.history.splice(0, this.history.length - HISTORY_LIMIT);
    this.recall = this.history.length;
    this.draft = "";
    saveHistory(this.history);
  }

  private echo(text: string): void {
    const node = element("pre", "repl-echo");
    text.split("\n").forEach((line, index) => {
      if (index > 0) node.append("\n");
      node.append(element("span", "repl-echo-prompt", index === 0 ? "hd> " : "... "));
      if (text.trimStart().startsWith(":")) node.append(line);
      else appendHighlighted(node, line);
    });
    this.log.append(node);
  }

  private show(entry: ReplEntry): void {
    const node = element("pre", `repl-${entry.kind}`);
    if (entry.kind === "value") {
      appendHighlighted(node, entry.text);
      node.append(element("span", "repl-type", ` : ${entry.type ?? ""}`));
    } else if (entry.kind === "code") appendHighlighted(node, entry.text);
    else node.textContent = entry.text;
    this.log.append(node);
  }

  private scroll(): void {
    this.log.scrollTop = this.log.scrollHeight;
  }

  /** Echoes and evaluates one input or command; inputs run in submission order. */
  async submit(text: string): Promise<void> {
    this.remember(text);
    this.echo(text);
    this.scroll();
    this.pending += 1;
    this.stopButton.hidden = false;
    if (this.ready) this.setStatus("Running…");
    let reply: ReplReply | Interrupted;
    try {
      reply = await this.client.repl(text);
    } catch (error) {
      this.setStatus("Compiler unavailable");
      reply = {
        entries: [{ kind: "error", text: `The compiler could not start: ${String(error)}` }],
        kept: false,
      };
    }
    this.pending -= 1;
    if (reply === "stopped" || reply === "timeout")
      this.show({
        kind: "error",
        text:
          reply === "stopped"
            ? "stopped"
            : "stopped: the input ran longer than 15 seconds. Is there an endless loop?",
      });
    else {
      for (const entry of reply.entries) this.show(entry);
      if (reply.command === "quit") this.setOpen(false);
    }
    this.scroll();
    if (this.pending === 0) {
      this.stopButton.hidden = true;
      if (this.ready) this.setStatus("Ready");
    }
  }

  /** Opens the panel and evaluates a code block's inputs in order. */
  async trySnippet(code: string): Promise<void> {
    this.setOpen(true);
    for (const input of splitInputs(code)) await this.submit(input.text);
  }
}

const workerUrl = document.body.dataset.replWorker;
if (workerUrl) {
  const panel = new ReplPanel(workerUrl);
  // Tests and the console can drive the panel.
  (window as unknown as { hdRepl?: ReplPanel }).hdRepl = panel;
}
