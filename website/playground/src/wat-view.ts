// The WAT view: the WebAssembly text of the module the project compiles to,
// highlighted with `tokenizeWat`, or the diagnostics that stopped it.

import { diagnosticList, element } from "./output.ts";
import type { RunDiagnostic, WatResult } from "./runner.ts";
import { tokenizeWat } from "./wat.ts";

/** The view renders at most this many lines; Copy and Download give the full text. */
const MAX_WAT_LINES = 5000;

const escapeHtml = (text: string): string =>
  text.replace(/[&<>]/g, (char) => (char === "&" ? "&amp;" : char === "<" ? "&lt;" : "&gt;"));

/** Highlighted HTML for the first `maxLines` lines of `wat`, one `.wat-line` each. */
function watHtml(wat: string, maxLines = MAX_WAT_LINES): string {
  const lines: string[] = [];
  let line = "";
  for (const { text, kind } of tokenizeWat(wat)) {
    const parts = text.split("\n");
    parts.forEach((part, index) => {
      if (index > 0) {
        lines.push(line);
        line = "";
      }
      if (part === "") return;
      line +=
        kind === "plain"
          ? escapeHtml(part)
          : `<span class="wat-${kind}">${escapeHtml(part)}</span>`;
    });
    if (lines.length >= maxLines) break;
  }
  if (lines.length < maxLines && line !== "") lines.push(line);
  return lines
    .slice(0, maxLines)
    .map((html) => `<span class="wat-line">${html}</span>`)
    .join("");
}

const lineCount = (text: string): number => {
  let count = 1;
  for (let index = text.indexOf("\n"); index !== -1; index = text.indexOf("\n", index + 1))
    count += 1;
  return text.endsWith("\n") ? count - 1 : count;
};

export class WatPanel {
  private readonly root: HTMLElement;
  private readonly onJump: (diagnostic: RunDiagnostic) => void;
  private readonly onDownload: () => void;
  /** The full text of the module shown, for Copy and Download. */
  text: string | undefined;

  constructor(
    root: HTMLElement,
    onJump: (diagnostic: RunDiagnostic) => void,
    onDownload: () => void,
  ) {
    this.root = root;
    this.onJump = onJump;
    this.onDownload = onDownload;
  }

  message(text: string, kind: "info" | "error" = "info"): void {
    this.text = undefined;
    this.root.replaceChildren(
      element("div", `outcome ${kind === "error" ? "failed" : "running"}`, text),
    );
  }

  show(result: WatResult): void {
    const { module } = result;
    if (result.status !== "ok" || !module) {
      this.text = undefined;
      if (result.status === "not-run") return this.message(result.summary);
      const errors = result.diagnostics.filter(({ severity }) => severity === "error").length;
      const heading =
        result.status === "compile-error"
          ? `✗ ${errors} ${errors === 1 ? "error" : "errors"}: no module to show`
          : `✗ ${result.summary}`;
      const children: HTMLElement[] = [element("div", "outcome failed", heading)];
      if (result.diagnostics.length > 0)
        children.push(diagnosticList(result.diagnostics, this.onJump));
      this.root.replaceChildren(...children);
      return;
    }
    this.text = module.wat;
    const lines = lineCount(module.wat);
    const code = element("pre", "wat-code");
    code.setAttribute("aria-label", "WebAssembly text");
    code.innerHTML = watHtml(module.wat);
    const children: HTMLElement[] = [code];
    if (lines > MAX_WAT_LINES) {
      const more = element(
        "div",
        "wat-truncated",
        `Showing the first ${MAX_WAT_LINES.toLocaleString("en-US")} of ${lines.toLocaleString("en-US")} lines. `,
      );
      const download = element("button", "quiet", "Download the full .wat");
      download.type = "button";
      download.addEventListener("click", this.onDownload);
      more.append(download);
      children.push(more);
    }
    this.root.replaceChildren(...children);
  }
}
