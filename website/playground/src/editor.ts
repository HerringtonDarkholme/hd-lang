// Editor states: one CodeMirror state per project file, so each file keeps
// its own undo history, selection, and diagnostics.

import { basicSetup } from "codemirror";
import { indentWithTab } from "@codemirror/commands";
import { setDiagnostics, type Diagnostic as LintDiagnostic } from "@codemirror/lint";
import { EditorState, Prec, type Extension, type Text } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";

import { hd } from "./hd-language.ts";
import type { RunDiagnostic } from "./runner.ts";

interface EditorCallbacks {
  readonly run: () => void;
  readonly check: () => void;
  readonly changed: () => void;
}

const theme = EditorView.theme({
  "&": { height: "100%", backgroundColor: "var(--editor-bg)", color: "var(--fg)" },
  ".cm-scroller": { fontFamily: "var(--mono)", lineHeight: "1.55" },
  ".cm-content": { caretColor: "var(--fg)" },
  ".cm-cursor, .cm-dropCursor": { borderLeftColor: "var(--fg)" },
  ".cm-gutters": {
    backgroundColor: "var(--editor-bg)",
    color: "var(--muted)",
    borderRight: "1px solid var(--border)",
  },
  ".cm-activeLine": { backgroundColor: "var(--active-line)" },
  ".cm-activeLineGutter": { backgroundColor: "var(--active-line)" },
  "&.cm-focused .cm-selectionBackground, .cm-selectionBackground, .cm-content ::selection": {
    backgroundColor: "var(--selection)",
  },
  ".cm-tooltip": {
    backgroundColor: "var(--panel-bg)",
    color: "var(--fg)",
    border: "1px solid var(--border)",
  },
  ".cm-panels": { backgroundColor: "var(--panel-bg)", color: "var(--fg)" },
});

export function editorExtensions(callbacks: EditorCallbacks): Extension {
  return [
    Prec.highest(
      keymap.of([
        // Ctrl+Enter and Cmd+Enter run on every platform; with Shift they check.
        { key: "Mod-Enter", run: () => (callbacks.run(), true) },
        { key: "Ctrl-Enter", run: () => (callbacks.run(), true) },
        { key: "Shift-Mod-Enter", run: () => (callbacks.check(), true) },
        { key: "Shift-Ctrl-Enter", run: () => (callbacks.check(), true) },
      ]),
    ),
    basicSetup,
    hd(),
    EditorState.tabSize.of(4),
    keymap.of([indentWithTab]),
    theme,
    EditorView.contentAttributes.of({ "aria-label": "hd source" }),
    EditorView.updateListener.of((update) => {
      if (update.docChanged) callbacks.changed();
    }),
  ];
}

/** The document offset of a 1-based line and column, clamped into `doc`. */
export function offsetOf(doc: Text, line: number, column: number): number {
  const target = doc.line(Math.min(Math.max(1, line), doc.lines));
  return target.from + Math.min(Math.max(0, column - 1), target.length);
}

function lintDiagnostics(doc: Text, diagnostics: readonly RunDiagnostic[]): LintDiagnostic[] {
  return diagnostics.map((diagnostic) => {
    const from = offsetOf(doc, diagnostic.line, diagnostic.column);
    let to = offsetOf(doc, diagnostic.endLine, diagnostic.endColumn);
    if (to <= from) to = Math.min(doc.lineAt(from).to, from + 1);
    return {
      from,
      to,
      severity: diagnostic.severity,
      source: diagnostic.code,
      message: diagnostic.message,
    };
  });
}

/** `state` with its diagnostics replaced. */
export function withDiagnostics(
  state: EditorState,
  diagnostics: readonly RunDiagnostic[],
): EditorState {
  return state.update(setDiagnostics(state, lintDiagnostics(state.doc, diagnostics))).state;
}
