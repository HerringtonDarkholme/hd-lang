// A tour page's editor (website/src/tour.ts renders the page). It replaces the
// static listing with the playground's CodeMirror setup and runs the code in
// the playground's compiler worker. Edits are kept per page in localStorage,
// and Reset restores the original snippet. A page has a Run button, a Test
// button, or both (website/src/tour.ts).
//
// Keys: Ctrl+Enter or Cmd+Enter presses the page's main button, Run or Test
// (with Shift it only checks), and Alt+Left and Alt+Right go to the previous
// and next page when the focus is not in the editor, where they move by word.
//
// website/build.ts bundles this file to assets/tour.js when the playground
// build exists.

import { setDiagnostics } from "@codemirror/lint";
import { EditorSelection, EditorState, RangeSetBuilder } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";

import { CompilerClient } from "../playground/src/compiler-client.ts";
import { editorExtensions, lintDiagnostics, offsetOf } from "../playground/src/editor.ts";
import { OutputPanel } from "../playground/src/output.ts";
import type { RunDiagnostic, RunMode } from "../playground/src/runner.ts";

const MAIN = "src/main.hd";
const STORAGE_PREFIX = "hd-tour:";

function loadEdit(key: string): string | undefined {
  try {
    return localStorage.getItem(STORAGE_PREFIX + key) ?? undefined;
  } catch {
    return undefined;
  }
}

function saveEdit(key: string, code: string | undefined): void {
  try {
    if (code === undefined) localStorage.removeItem(STORAGE_PREFIX + key);
    else localStorage.setItem(STORAGE_PREFIX + key, code);
  } catch {
    // Kept edits are a convenience; the page works without storage.
  }
}

/**
 * Wrapped lines hang one indentation step under their own line's
 * indentation, so a long line's continuation does not read as a new line at
 * the left margin. Each line gets its hang in `--hang`; the stylesheet turns it
 * into padding and a negative first-line indent (website/assets/style.css).
 */
const HANG_STEP = 4;

function hangs(view: EditorView): DecorationSet {
  const builder = new RangeSetBuilder<Decoration>();
  for (const { from, to } of view.visibleRanges) {
    for (let position = from; position <= to;) {
      const line = view.state.doc.lineAt(position);
      const indent = /^ */.exec(line.text)![0].length;
      const hang = Decoration.line({ attributes: { style: `--hang: ${indent + HANG_STEP}ch` } });
      builder.add(line.from, line.from, hang);
      position = line.to + 1;
    }
  }
  return builder.finish();
}

const hangingIndent = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = hangs(view);
    }
    update(update: ViewUpdate): void {
      if (update.docChanged || update.viewportChanged) this.decorations = hangs(update.view);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

function start(root: HTMLElement): void {
  const key = root.dataset.tourKey!;
  const original = JSON.parse(document.getElementById("tour-source")!.textContent!) as string;
  const status = document.getElementById("tour-status")!;
  const outputRoot = document.getElementById("tour-output")!;
  const code = document.getElementById("tour-code")!;
  const primary: RunMode = root.dataset.primary === "test" ? "test" : "run";
  // The page renders the hint for its buttons; Reset brings it back.
  const hint = [...outputRoot.childNodes].map((node) => node.cloneNode(true));
  const showHint = (): void =>
    outputRoot.replaceChildren(...hint.map((node) => node.cloneNode(true)));

  const client = new CompilerClient({
    workerUrl: root.dataset.worker!,
    onReady: () => (status.textContent = "Ready"),
  });
  status.textContent = "Loading the compiler…";

  const jumpTo = (diagnostic: RunDiagnostic): void => {
    const at = offsetOf(view.state.doc, diagnostic.line, diagnostic.column);
    view.dispatch({ selection: EditorSelection.cursor(at), scrollIntoView: true });
    view.focus();
  };
  const output = new OutputPanel(outputRoot, jumpTo);

  const showDiagnostics = (diagnostics: readonly RunDiagnostic[]): void =>
    view.dispatch(setDiagnostics(view.state, lintDiagnostics(view.state.doc, diagnostics)));

  const run = async (mode: RunMode): Promise<void> => {
    const project = { files: { [MAIN]: view.state.doc.toString() }, main: MAIN };
    output.begin(mode === "check" ? "Checking…" : mode === "test" ? "Testing…" : "Running…");
    const result = await client.run(mode, project, (line) => output.line(line));
    output.finish(result, mode);
    showDiagnostics(typeof result === "string" ? [] : result.diagnostics);
  };

  const extensions = editorExtensions({
    run: () => void run(primary),
    check: () => void run("check"),
    changed: () => {
      const text = view.state.doc.toString();
      saveEdit(key, text === original ? undefined : text);
    },
  });
  const view = new EditorView({
    state: EditorState.create({
      doc: loadEdit(key) ?? original,
      // The editor column is narrow, so long lines wrap instead of scrolling.
      extensions: [extensions, EditorView.lineWrapping, hangingIndent],
    }),
  });
  code.replaceChildren(view.dom);

  for (const mode of ["run", "test"] as const)
    document.getElementById(`tour-${mode}`)?.addEventListener("click", () => void run(mode));
  document.getElementById("tour-reset")!.addEventListener("click", () => {
    view.dispatch({ changes: { from: 0, to: view.state.doc.length, insert: original } });
    saveEdit(key, undefined);
    showDiagnostics([]);
    showHint();
  });

  document.addEventListener("keydown", (event) => {
    if (event.defaultPrevented) return;
    const inEditor = view.dom.contains(event.target as Node);
    if (event.key === "Enter" && (event.ctrlKey || event.metaKey) && !inEditor) {
      event.preventDefault();
      void run(event.shiftKey ? "check" : primary);
      return;
    }
    if (!event.altKey || event.ctrlKey || event.metaKey || event.shiftKey || inEditor) return;
    if (event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)
      return;
    const rel = event.key === "ArrowLeft" ? "prev" : event.key === "ArrowRight" ? "next" : "";
    const link = rel
      ? document.querySelector<HTMLAnchorElement>(`.tour-pager a[rel="${rel}"]`)
      : null;
    if (link) {
      event.preventDefault();
      window.location.href = link.href;
    }
  });

  // Tests and the console can drive the editor.
  (window as unknown as { hdTour?: unknown }).hdTour = { view, run, client };
}

const root = document.getElementById("tour-editor");
if (root) start(root);
