// The site's bottom REPL panel. It docks at the bottom of every page and
// opens over the content, so trying code never navigates away. The panel
// itself, its input rules and its session are the playground's
// (playground/src/repl-panel.ts); its compiler worker is the playground's
// worker, which loads only when the panel first opens. The session has no
// package: its inputs see the standard library.
//
// website/build.ts bundles this file to assets/repl.js when the playground
// build exists; the layout then includes it and names the worker on <body>.

import { ReplPanel } from "../playground/src/repl-panel.ts";

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

class DockedRepl {
  private readonly toggle = element("button", "repl-toggle");
  private readonly panel = element("section", "repl-panel");
  private readonly status = element("span", "repl-status");
  private readonly stopButton = element("button", "repl-stop", "Stop");
  readonly repl: ReplPanel;

  constructor(workerUrl: string) {
    const { toggle, panel } = this;
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
    this.stopButton.title = "Stop the input that runs";
    this.stopButton.addEventListener("click", () => this.repl.stop());
    const close = button("×", "Close (Esc)", () => this.setOpen(false));
    close.setAttribute("aria-label", "Close the REPL");
    actions.append(
      this.stopButton,
      button("Clear", "Clear the output (Ctrl+L)", () => this.repl.clear()),
      close,
    );
    header.append(element("strong", "repl-title", "hd REPL"), this.status, actions);
    panel.append(header);
    this.repl = new ReplPanel(panel, {
      workerUrl,
      tokenClass: (kind) => `hl-${kind}`,
      reloadHelp: "rebuild the session from its record",
      onBusy: (busy) => {
        this.stopButton.hidden = !busy;
      },
      onStatus: (text) => {
        this.status.textContent = text;
      },
      onQuit: () => this.setOpen(false),
    });
    panel.addEventListener("keydown", (event) => {
      if (event.key === "Escape") {
        event.preventDefault();
        this.setOpen(false);
        toggle.focus();
      }
    });
    document.body.append(toggle, panel);
    document.body.classList.add("repl-ready");

    document.addEventListener("keydown", (event) => {
      if ((event.ctrlKey || event.metaKey) && (event.key === "`" || event.code === "Backquote")) {
        event.preventDefault();
        this.setOpen(panel.hidden !== false);
      }
    });
    document.addEventListener("click", (event) => {
      const target = event.target instanceof Element ? event.target.closest(".try-repl") : null;
      const code = target?.closest(".code-block")?.querySelector("pre code")?.textContent;
      if (code) {
        this.setOpen(true);
        void this.repl.trySnippet(code);
      }
    });
  }

  setOpen(open: boolean): void {
    this.panel.hidden = !open;
    this.toggle.setAttribute("aria-expanded", String(open));
    this.toggle.title = `${open ? "Close" : "Open"} the hd REPL (Ctrl+\`)`;
    document.body.classList.toggle("repl-open", open);
    if (!open) return;
    this.repl.load();
    this.repl.focus();
  }
}

const workerUrl = document.body.dataset.replWorker;
if (workerUrl) {
  const docked = new DockedRepl(workerUrl);
  // Tests and the console can drive the panel.
  (window as unknown as { hdRepl?: DockedRepl }).hdRepl = docked;
}
