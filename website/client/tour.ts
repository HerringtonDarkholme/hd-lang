// A tour page's editor (website/src/tour.ts renders the page). It replaces the
// static listing with the playground's CodeMirror setup and runs the code in
// the playground's compiler worker. Edits are kept per page in localStorage,
// and Reset restores the original snippet. A page has a Run button, a Test
// button, or both (website/src/tour.ts). A page with several files has one
// tab per file; Run sends them all as one package, and a diagnostic's jump
// opens the file it is in.
//
// Keys: Ctrl+Enter or Cmd+Enter presses the page's main button, Run or Test
// (with Shift it only checks), and Alt+Left and Alt+Right go to the previous
// and next page when the focus is not in the editor, where they move by word.
//
// website/build.ts bundles this file to assets/tour.js when the playground
// build exists.

import { EditorSelection, EditorState, RangeSetBuilder } from "@codemirror/state";
import {
  Decoration,
  EditorView,
  ViewPlugin,
  type DecorationSet,
  type ViewUpdate,
} from "@codemirror/view";

import { CompilerClient } from "../playground/src/compiler-client.ts";
import { editorExtensions, offsetOf, withDiagnostics } from "../playground/src/editor.ts";
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
  // Package path to source, src/main.hd first (website/src/tour.ts).
  const originals = JSON.parse(document.getElementById("tour-source")!.textContent!) as Record<
    string,
    string
  >;
  const paths = Object.keys(originals);
  // src/main.hd's edits keep the page's key; another file's add its path.
  const storageKey = (path: string): string => (path === MAIN ? key : `${key}:${path}`);
  const tabs = [...root.querySelectorAll<HTMLButtonElement>(".tour-tab")];
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

  // One editor state per file; the view shows the current file's.
  const states = new Map<string, EditorState>();
  let current = MAIN;

  /** Shows `path`'s file in the editor and marks its tab. */
  const open = (path: string): void => {
    if (path === current || !states.has(path)) return;
    states.set(current, view.state);
    current = path;
    view.setState(states.get(path)!);
    for (const tab of tabs) tab.setAttribute("aria-selected", String(tab.dataset.file === path));
  };

  const jumpTo = (diagnostic: RunDiagnostic): void => {
    open(diagnostic.path);
    const at = offsetOf(view.state.doc, diagnostic.line, diagnostic.column);
    view.dispatch({ selection: EditorSelection.cursor(at), scrollIntoView: true });
    view.focus();
  };
  const output = new OutputPanel(outputRoot, jumpTo);

  // Each file shows the diagnostics located in it.
  const showDiagnostics = (diagnostics: readonly RunDiagnostic[]): void => {
    states.set(current, view.state);
    for (const [path, state] of states) {
      const located = diagnostics.filter((diagnostic) => diagnostic.path === path);
      const next = withDiagnostics(state, located);
      states.set(path, next);
      if (path === current) view.setState(next);
    }
  };

  const project = (): { files: Record<string, string>; main: string } => {
    states.set(current, view.state);
    const files: Record<string, string> = {};
    for (const [path, state] of states) files[path] = state.doc.toString();
    return { files, main: MAIN };
  };

  const run = async (mode: RunMode): Promise<void> => {
    output.begin(mode === "check" ? "Checking…" : mode === "test" ? "Testing…" : "Running…");
    const result = await client.run(mode, project(), (line) => output.line(line));
    output.finish(result, mode);
    showDiagnostics(typeof result === "string" ? [] : result.diagnostics);
  };

  const extensions = editorExtensions({
    run: () => void run(primary),
    check: () => void run("check"),
    changed: () => {
      const text = view.state.doc.toString();
      saveEdit(storageKey(current), text === originals[current] ? undefined : text);
    },
  });
  // The editor column is narrow, so long lines wrap instead of scrolling.
  const newState = (text: string): EditorState =>
    EditorState.create({
      doc: text,
      extensions: [extensions, EditorView.lineWrapping, hangingIndent],
    });
  for (const path of paths)
    states.set(path, newState(loadEdit(storageKey(path)) ?? originals[path]!));
  const view = new EditorView({ state: states.get(MAIN)! });
  code.replaceChildren(view.dom);

  for (const tab of tabs) tab.addEventListener("click", () => open(tab.dataset.file!));
  for (const mode of ["run", "test"] as const)
    document.getElementById(`tour-${mode}`)?.addEventListener("click", () => void run(mode));
  document.getElementById("tour-reset")!.addEventListener("click", () => {
    open(MAIN);
    for (const path of paths) {
      states.set(path, newState(originals[path]!));
      saveEdit(storageKey(path), undefined);
    }
    view.setState(states.get(MAIN)!);
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
  (window as unknown as { hdTour?: unknown }).hdTour = { view, run, client, open };
}

const root = document.getElementById("tour-editor");
if (root) start(root);
