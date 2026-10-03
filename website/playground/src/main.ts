// The playground page: file tabs, the editor, run/check/share, and the
// output pane with its Output and WAT views.

import "./style.css";

import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

import { CompilerClient } from "./compiler-client.ts";
import { editorExtensions, offsetOf, withDiagnostics } from "./editor.ts";
import { EXAMPLES } from "./examples.ts";
import { OutputPanel } from "./output.ts";
import { asProject, DEFAULT_MAIN, orderedPaths, pathProblem, type Project } from "./project.ts";
import type { RunDiagnostic, RunMode } from "./runner.ts";
import { projectFromHash, projectHash } from "./share.ts";
import { WatPanel } from "./wat-view.ts";

const STORAGE_KEY = "hd-playground-project";

const byId = <T extends HTMLElement>(id: string): T => document.getElementById(id) as T;
const runButton = byId<HTMLButtonElement>("run");
const checkButton = byId<HTMLButtonElement>("check");
const testButton = byId<HTMLButtonElement>("test");
const stopButton = byId<HTMLButtonElement>("stop");
const shareButton = byId<HTMLButtonElement>("share");
const examplesSelect = byId<HTMLSelectElement>("examples");
const tabs = byId<HTMLElement>("tabs");
const status = byId<HTMLElement>("status");
const outputViewButton = byId<HTMLButtonElement>("view-output");
const watViewButton = byId<HTMLButtonElement>("view-wat");
const clearButton = byId<HTMLButtonElement>("clear");
const watCopyButton = byId<HTMLButtonElement>("wat-copy");
const watDownloadButton = byId<HTMLButtonElement>("wat-download");

const states = new Map<string, EditorState>();
let entry = DEFAULT_MAIN;
let current = DEFAULT_MAIN;
let hashShown: string | undefined;
let saveTimer: ReturnType<typeof setTimeout> | undefined;
let running = false;
// The WAT view asks the worker for the module only while the view is open.
let watOpen = false;
/** The project whose module the view shows, as JSON, and whether it may be out of date. */
let watShown: string | undefined;
let watStale = true;
let watInFlight = false;
let watTimer: ReturnType<typeof setTimeout> | undefined;

const extensions = editorExtensions({
  run: () => void execute("run"),
  check: () => void execute("check"),
  changed,
});
const view = new EditorView({ parent: byId("editor") });
const output = new OutputPanel(byId("output"), jumpTo);
const wat = new WatPanel(byId("wat"), jumpTo, downloadWat);
const compiler = new CompilerClient({ onReady: () => setStatus("Compiler ready") });

function setStatus(text: string): void {
  status.textContent = text;
}

function project(sync = true): Project {
  if (sync) states.set(current, view.state);
  const files: Record<string, string> = {};
  for (const [path, state] of states) files[path] = state.doc.toString();
  return { files, main: entry };
}

function newState(source: string): EditorState {
  return EditorState.create({ doc: source, extensions });
}

function load(next: Project): void {
  states.clear();
  for (const [path, source] of Object.entries(next.files)) states.set(path, newState(source));
  entry = next.main;
  current = next.main;
  view.setState(states.get(current)!);
  renderTabs();
  output.clear();
  watChanged();
}

function open(path: string): void {
  if (path === current) return;
  states.set(current, view.state);
  current = path;
  view.setState(states.get(path)!);
  renderTabs();
}

function changed(): void {
  // Edits leave a shared link behind: the URL no longer shows this project.
  if (location.hash && hashShown === location.hash) {
    history.replaceState(null, "", location.pathname + location.search);
    hashShown = undefined;
  }
  clearTimeout(saveTimer);
  saveTimer = setTimeout(save, 400);
  watChanged(500);
}

function save(): void {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(project()));
  } catch {
    // Storage is a convenience; the page works without it.
  }
}

function stored(): Project | undefined {
  try {
    const text = localStorage.getItem(STORAGE_KEY);
    return text ? asProject(JSON.parse(text)) : undefined;
  } catch {
    return undefined;
  }
}

function tabLabel(path: string): string {
  return path.replace(/^src\//, "");
}

function renderTabs(): void {
  const paths = orderedPaths(project());
  const children: HTMLElement[] = paths.map((path) => {
    const tab = document.createElement("div");
    tab.className = `tab${path === current ? " active" : ""}${path === entry ? " entry" : ""}`;
    tab.setAttribute("role", "tab");
    tab.setAttribute("aria-selected", String(path === current));
    const name = document.createElement("button");
    name.type = "button";
    name.className = "tab-name";
    name.textContent = tabLabel(path);
    name.title = `${path}${path === entry ? " (entry module)" : ""} · double-click to rename`;
    name.addEventListener("click", () => open(path));
    name.addEventListener("dblclick", () => startRename(tab, path));
    tab.append(name);
    if (paths.length > 1) {
      if (path !== entry) {
        const main = document.createElement("button");
        main.type = "button";
        main.className = "tab-action";
        main.textContent = "▸";
        main.title = `Make ${path} the entry module`;
        main.setAttribute("aria-label", main.title);
        main.addEventListener("click", () => {
          entry = path;
          renderTabs();
          save();
          watChanged();
        });
        tab.append(main);
      }
      const close = document.createElement("button");
      close.type = "button";
      close.className = "tab-action";
      close.textContent = "×";
      close.title = `Delete ${path}`;
      close.setAttribute("aria-label", close.title);
      close.addEventListener("click", () => remove(path));
      tab.append(close);
    }
    return tab;
  });
  const add = document.createElement("button");
  add.type = "button";
  add.className = "tab-add";
  add.textContent = "+";
  add.title = "Add a file";
  add.setAttribute("aria-label", "Add a file");
  add.addEventListener("click", addFile);
  tabs.replaceChildren(...children, add);
}

function freshPath(): string {
  for (let index = 1; ; index += 1) {
    const path = index === 1 ? "src/module.hd" : `src/module${index}.hd`;
    if (!states.has(path)) return path;
  }
}

function addFile(): void {
  const path = freshPath();
  states.set(current, view.state);
  states.set(path, newState(`pub fn hello() -> string: "hello from ${path}"\n`));
  open(path);
  const tab = [...tabs.querySelectorAll<HTMLElement>(".tab")].find(
    (candidate) => candidate.querySelector(".tab-name")?.textContent === tabLabel(path),
  );
  if (tab) startRename(tab, path);
  save();
}

function startRename(tab: HTMLElement, path: string): void {
  const input = document.createElement("input");
  input.className = "tab-rename";
  input.value = path;
  input.spellcheck = false;
  input.setAttribute("aria-label", "File path");
  tab.replaceChildren(input);
  input.focus();
  input.setSelectionRange(path.lastIndexOf("/") + 1, path.length - ".hd".length);
  let done = false;
  const finish = (commit: boolean): void => {
    if (done) return;
    const next = input.value.trim();
    const problem = commit && next !== path ? pathProblem(next, project(), path) : undefined;
    if (problem) {
      setStatus(problem);
      input.setCustomValidity(problem);
      input.reportValidity();
      return;
    }
    done = true;
    if (commit && next !== path) rename(path, next);
    else renderTabs();
  };
  input.addEventListener("keydown", (event) => {
    if (event.key === "Enter") finish(true);
    if (event.key === "Escape") finish(false);
  });
  input.addEventListener("input", () => input.setCustomValidity(""));
  input.addEventListener("blur", () => {
    finish(true);
    if (!done) finish(false);
  });
}

function rename(from: string, to: string): void {
  states.set(current, view.state);
  const entries = [...states].map(([path, state]) => [path === from ? to : path, state] as const);
  states.clear();
  for (const [path, state] of entries) states.set(path, state);
  if (entry === from) entry = to;
  if (current === from) current = to;
  renderTabs();
  save();
  watChanged();
  setStatus(`Renamed ${from} to ${to}`);
}

function remove(path: string): void {
  const source = (path === current ? view.state : states.get(path)!).doc.toString();
  if (source.trim() !== "" && !confirm(`Delete ${path}?`)) return;
  states.set(current, view.state);
  states.delete(path);
  const [first] = orderedPaths(project(false));
  if (entry === path) entry = states.has(DEFAULT_MAIN) ? DEFAULT_MAIN : first!;
  if (current === path) {
    current = entry;
    view.setState(states.get(current)!);
  }
  renderTabs();
  save();
  watChanged();
}

function jumpTo(diagnostic: RunDiagnostic): void {
  if (!states.has(diagnostic.path) && diagnostic.path !== current) return;
  open(diagnostic.path);
  const { doc } = view.state;
  const from = offsetOf(doc, diagnostic.line, diagnostic.column);
  const to = Math.max(from, offsetOf(doc, diagnostic.endLine, diagnostic.endColumn));
  view.dispatch({ selection: EditorSelection.range(from, to), scrollIntoView: true });
  view.focus();
}

function showDiagnostics(diagnostics: readonly RunDiagnostic[]): void {
  states.set(current, view.state);
  for (const [path, state] of states) {
    const next = withDiagnostics(
      state,
      diagnostics.filter((diagnostic) => diagnostic.path === path),
    );
    states.set(path, next);
    if (path === current) view.setState(next);
  }
}

const MODE_LABELS: Record<RunMode, readonly [busy: string, done: string]> = {
  run: ["Running", "Run finished"],
  check: ["Checking", "Check finished"],
  test: ["Testing", "Tests finished"],
};

async function execute(mode: RunMode): Promise<void> {
  const snapshot = project();
  const [busy, done] = MODE_LABELS[mode];
  running = true;
  runButton.disabled = true;
  checkButton.disabled = true;
  testButton.disabled = true;
  stopButton.hidden = false;
  output.begin(`${busy}…`);
  setStatus(busy);
  if (mode !== "check") void refreshWat();
  try {
    const outcome = await compiler.run(mode, snapshot, (line) => output.line(line));
    output.finish(outcome, mode);
    showDiagnostics(typeof outcome === "string" ? [] : outcome.diagnostics);
    setStatus(typeof outcome === "string" ? "Stopped" : done);
  } catch (error) {
    output.message(`The compiler could not start: ${String(error)}`, "error");
    setStatus("Compiler unavailable");
  } finally {
    runButton.disabled = false;
    checkButton.disabled = false;
    testButton.disabled = false;
    stopButton.hidden = true;
    running = false;
    // A run or test may have compiled a new module; Check compiles none.
    if (mode !== "check") watStale = true;
    void refreshWat();
  }
}

function showView(next: "output" | "wat"): void {
  watOpen = next === "wat";
  outputViewButton.setAttribute("aria-selected", String(!watOpen));
  watViewButton.setAttribute("aria-selected", String(watOpen));
  byId("output").hidden = watOpen;
  byId("wat").hidden = !watOpen;
  clearButton.hidden = watOpen;
  watCopyButton.hidden = !watOpen;
  watDownloadButton.hidden = !watOpen;
  void refreshWat();
}

/** The project changed: refresh an open WAT view after `delay` ms. */
function watChanged(delay = 0): void {
  watStale = true;
  clearTimeout(watTimer);
  if (watOpen) watTimer = setTimeout(() => void refreshWat(), delay);
}

function setWatText(text: string | undefined): void {
  watCopyButton.disabled = text === undefined;
  watDownloadButton.disabled = text === undefined;
}

async function refreshWat(): Promise<void> {
  if (!watOpen || watInFlight) return;
  if (running) {
    wat.message("Waiting for the run to finish…");
    setWatText(undefined);
    return;
  }
  const snapshot = project();
  const key = JSON.stringify(snapshot);
  if (!watStale && key === watShown) return;
  watInFlight = true;
  watStale = false;
  if (watShown !== key) wat.message("Compiling…");
  try {
    const result = await compiler.wat(snapshot);
    if (result === "busy") watStale = true;
    else if (result === "stopped" || result === "timeout")
      wat.message("Stopped: compiling took longer than 15 seconds.", "error");
    else wat.show(result);
    watShown = key;
  } catch (error) {
    wat.message(`The compiler could not start: ${String(error)}`, "error");
  } finally {
    watInFlight = false;
    setWatText(wat.text);
  }
  // The project changed while the worker compiled it.
  if (watStale || JSON.stringify(project()) !== key) {
    watStale = true;
    clearTimeout(watTimer);
    watTimer = setTimeout(() => void refreshWat(), 200);
  }
}

function watFileName(): string {
  return `${entry.replace(/^.*\//, "").replace(/\.hd$/, "")}.wat`;
}

function downloadWat(): void {
  if (wat.text === undefined) return;
  const link = document.createElement("a");
  link.href = URL.createObjectURL(new Blob([wat.text], { type: "text/plain" }));
  link.download = watFileName();
  link.click();
  setTimeout(() => URL.revokeObjectURL(link.href), 1000);
  setStatus(`Downloaded ${link.download}`);
}

async function copyWat(): Promise<void> {
  if (wat.text === undefined) return;
  try {
    await navigator.clipboard.writeText(wat.text);
    setStatus("WAT copied to the clipboard");
  } catch {
    setStatus("Could not copy; use Download .wat instead");
  }
}

async function share(): Promise<void> {
  const hash = projectHash(project());
  history.replaceState(null, "", hash);
  hashShown = location.hash;
  const url = location.href;
  try {
    await navigator.clipboard.writeText(url);
    setStatus("Link copied to the clipboard");
  } catch {
    prompt("Copy this link:", url);
  }
}

function loadFromHash(): boolean {
  if (location.hash === hashShown) return false;
  const shared = projectFromHash(location.hash);
  if (!shared) return false;
  load(shared);
  hashShown = location.hash;
  setStatus("Loaded the shared project");
  return true;
}

for (const example of EXAMPLES) {
  const option = document.createElement("option");
  option.value = example.id;
  option.textContent = example.title;
  examplesSelect.append(option);
}
examplesSelect.addEventListener("change", () => {
  const example = EXAMPLES.find(({ id }) => id === examplesSelect.value);
  examplesSelect.value = "";
  if (!example) return;
  load(example.project);
  if (location.hash) history.replaceState(null, "", location.pathname + location.search);
  hashShown = undefined;
  save();
  setStatus(`Loaded example: ${example.title}`);
});

runButton.addEventListener("click", () => void execute("run"));
checkButton.addEventListener("click", () => void execute("check"));
testButton.addEventListener("click", () => void execute("test"));
stopButton.addEventListener("click", () => compiler.stop());
shareButton.addEventListener("click", () => void share());
clearButton.addEventListener("click", () => output.clear());
outputViewButton.addEventListener("click", () => showView("output"));
watViewButton.addEventListener("click", () => showView("wat"));
watCopyButton.addEventListener("click", () => void copyWat());
watDownloadButton.addEventListener("click", downloadWat);
window.addEventListener("hashchange", () => void loadFromHash());
document.addEventListener("keydown", (event) => {
  if (event.key !== "Enter" || !(event.ctrlKey || event.metaKey) || view.hasFocus) return;
  event.preventDefault();
  void execute(event.shiftKey ? "check" : "run");
});

if (!loadFromHash()) load(stored() ?? EXAMPLES[0]!.project);
setStatus("Loading the compiler…");
