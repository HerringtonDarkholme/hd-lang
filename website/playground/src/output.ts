// The output panel: console lines, the outcome line, and diagnostics that
// jump to their source position when clicked.

import type { RunDiagnostic, RunMode, RunResult } from "./runner.ts";

type Outcome = RunResult | "stopped" | "timeout";

export function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text = "",
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  node.textContent = text;
  return node;
}

export class OutputPanel {
  private readonly root: HTMLElement;
  private stdout: HTMLElement | undefined;
  private readonly onJump: (diagnostic: RunDiagnostic) => void;

  constructor(root: HTMLElement, onJump: (diagnostic: RunDiagnostic) => void) {
    this.root = root;
    this.onJump = onJump;
  }

  clear(): void {
    this.root.replaceChildren();
    this.stdout = undefined;
  }

  begin(label: string): void {
    this.clear();
    this.root.append(element("div", "outcome running", label));
  }

  line(text: string): void {
    if (!this.stdout) {
      this.stdout = element("pre", "stdout");
      this.root.append(this.stdout);
    }
    this.stdout.append(`${text}\n`);
  }

  message(text: string, kind: "info" | "error" = "info"): void {
    this.clear();
    this.root.append(element("div", `outcome ${kind === "error" ? "failed" : "running"}`, text));
  }

  finish(outcome: Outcome, mode: RunMode): void {
    this.root.querySelector(".outcome.running")?.remove();
    const header = this.outcomeLine(outcome, mode);
    this.root.prepend(header);
    if (typeof outcome === "string") return;
    if (outcome.stdout.length === 0 && mode !== "check" && outcome.status !== "compile-error")
      this.root.append(element("div", "empty", "(no console output)"));
    if (outcome.diagnostics.length > 0)
      this.root.append(diagnosticList(outcome.diagnostics, this.onJump));
  }

  private outcomeLine(outcome: Outcome, mode: RunMode): HTMLElement {
    if (outcome === "stopped") return element("div", "outcome failed", "Stopped.");
    if (outcome === "timeout")
      return element(
        "div",
        "outcome failed",
        "Stopped: the program ran longer than 15 seconds. Is there an endless loop?",
      );
    const errors = outcome.diagnostics.filter(({ severity }) => severity === "error").length;
    const time = `${outcome.milliseconds} ms`;
    if (outcome.status === "compile-error")
      return element(
        "div",
        "outcome failed",
        `✗ ${errors} ${errors === 1 ? "error" : "errors"} · ${time}`,
      );
    if (outcome.status === "panic") return element("div", "outcome panic", `✗ ${outcome.summary}`);
    if (outcome.status === "failure")
      return element("div", "outcome failed", `✗ ${outcome.summary}`);
    const label = mode === "check" ? "✓ No errors" : `✓ ${outcome.summary}`;
    return element("div", "outcome passed", `${label} · ${time}`);
  }
}

/** Diagnostics as buttons that jump to their source position. */
export function diagnosticList(
  diagnostics: readonly RunDiagnostic[],
  onJump: (diagnostic: RunDiagnostic) => void,
): HTMLElement {
  const list = element("ul", "diagnostics");
  for (const diagnostic of diagnostics) {
    const item = element("li", `diagnostic ${diagnostic.severity}`);
    const button = element("button", "jump");
    button.type = "button";
    button.title = "Show in editor";
    button.append(
      element("span", "location", `${diagnostic.path}:${diagnostic.line}:${diagnostic.column}`),
      element("span", "severity", diagnostic.severity),
      element("span", "code", diagnostic.code),
      element("span", "message", diagnostic.message),
    );
    for (const note of diagnostic.notes) button.append(element("span", "note", `note: ${note}`));
    button.addEventListener("click", () => onJump(diagnostic));
    item.append(button);
    list.append(item);
  }
  return list;
}
