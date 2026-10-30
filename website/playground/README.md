# hd Playground

A browser playground for hd-lang. It edits, checks, tests, and runs hd
programs entirely in the browser with the Rust compiler: the
[`hd_web`](../../crates/hd_web) crate built for `wasm32-unknown-unknown`,
the same front end, checker, and RC emitter as `hd`. It is part of
the website: the website build emits it as static files at `playground/`,
and the site's **Playground** page (`playground.html`) frames it.

```sh
pnpm run build:web-compiler   # writes assets/hd_web.wasm (not committed)
pnpm run website:build        # writes website/dist/, with the playground at playground/
pnpm run website:dev          # serves http://localhost:4173/playground/, rebuilt on change
pnpm run website:e2e          # after a build: drives the site, then the playground
```

The build needs `assets/hd_web.wasm`; `pnpm run build:web-compiler` builds
it with the `web` cargo profile. Every URL in the playground build is
relative, so it works at `/` and under a sub-path such as
`/hd-lang/playground/`. The e2e run serves the site under `/hd-lang/`, the
GitHub Pages base. It needs a local Chrome, Edge, or Chromium; `CHROME_PATH`
selects one. The playground steps are in [`e2e.ts`](e2e.ts). The browser needs WebAssembly support.

## Features

- CodeMirror 6 editor. Highlighting replays `classify(line)` from
  [`website/src/highlight.ts`](../src/highlight.ts), the highlighter that
  also colors the site's code blocks. There is one token classifier, and
  [`test/highlight.test.ts`](test/highlight.test.ts) checks that the
  editor's classes match it and that its keywords are the reserved words of
  the Rust lexer (`keyword` in
  [`crates/hd_syntax/src/lexer.rs`](../../crates/hd_syntax/src/lexer.rs)).
- **Run** (Ctrl+Enter or Cmd+Enter) compiles the project and runs it:
  - when the entry module declares `main`, Run calls `pub fn main`, as
    `hd run` does. A `main` that returns an `ExitCode` other than 0, or an
    `.Err`, is reported as a failure with its exit code;
  - without `main`, the entry file runs as a script, as `hd FILE` runs one:
    its top-level statements run in order and print only what they print.
    A file that holds only declarations runs nothing; **Test** runs its
    test cases.

  Console output streams into the output panel; `dbg` lines show marked as
  debug output. Runtime panics are reported with their panic code. A run
  longer than 15 seconds is stopped, and **Stop** ends a run early.

- **Check** (Ctrl+Shift+Enter) type-checks the whole project without running
  anything. Diagnostics show `path:line:column` and the diagnostic code.
  Clicking one jumps to it, and the editor underlines it.
- **Test** runs the test cases, as `hd test` does, whether or not the entry
  module declares `main`: the `tests:` blocks, `*_test.hd` test modules, and
  each program under `tests/`. Each test case, and each `it_each` row, runs
  in a fresh program instance, with the property runner and shrinking of
  `hd test` ([`host/cases.mjs`](../../host/cases.mjs), shared with the
  Node runner). `ignore`, `expect_panic`, and `timeout` apply. The output
  lists each case that did not pass as `hd test` prints it.
- **WAT**, beside **Output** in the output pane, shows the WebAssembly text
  of the module Run instantiates. The compiler wasm prints it with
  `wasmprinter` (the `wat` goal of `hd_web`), so the text is the binary the
  runtime executes, with the names of its name section. A file without
  `main` shows the script's module. When compilation fails, the view shows
  the diagnostics, which jump to their source positions, instead of an old
  module.

  The view is generated on demand. The page asks the worker for the text only
  while the view is open, and refreshes it after an edit, a run, or a test.
  Highlighting comes from a small tokenizer, [`src/wat.ts`](src/wat.ts),
  tested in [`test/wat.test.ts`](test/wat.test.ts). Lines are numbered. The
  view renders at most 10,000 lines; **Copy** and **Download .wat** give the
  full text.

- **REPL**, the third view of the output pane, is a REPL session over the
  project, as bare `hd` opens one in a package (see [The REPL](#the-repl)).

- Files: `+` adds a module, double-click renames one, and `×` deletes one.
  `▸` makes a module the entry module, which is marked `main`. A file path
  starts with `src/`, `tests/`, or `tasks/`, continues with identifier
  segments separated by `/`, and ends in `.hd`
  ([`src/project.ts`](src/project.ts)). The compiler reports anything else
  wrong with a path, such as a reserved word or `src/mod.hd`, as a
  diagnostic.
- **Share** copies a link that holds the whole project in the URL hash:
  - `#code=<base64url of the UTF-8 source>` for a single `src/main.hd`;
  - `#project=<base64url of UTF-8 JSON {"files": {path: source}, "main": path}>`
    otherwise.

  base64url is RFC 4648 section 5 without padding. The page reads the hash on
  load and on `hashchange`. When there is no hash, the last project is
  restored from `localStorage`.

- **Examples** bundles programs from [`../../examples`](../../examples),
  [`../../spec/conformance/runtime/valid`](../../spec/conformance/runtime/valid), and
  [`examples/`](examples) as text, so the menu shows the files the test suites
  run. [`examples/top-level.hd`](examples/top-level.hd) has no `main` and
  runs as a script. The playground's own examples show recent language
  features: closures, `i64` and `u8`, `tests:` blocks with `it`, `it_each`,
  and `snapshot`, `ExitCode`, literal suffixes, `@derive`, and mutable
  requirement traits. [`test/runner.test.ts`](test/runner.test.ts) runs
  every example.
- Light and dark themes follow `prefers-color-scheme`. The layout stacks the
  editor and output on narrow screens.

## How It Works

`src/main.ts` runs the page. `src/worker.ts` runs the compiler in a module
Web Worker, so a long compile or an endless loop never blocks the page.
Stopping a run terminates the worker. `src/compiler-client.ts` is the page
side of the worker, shared with the website's tour.

The worker fetches `hd_web.wasm`, which sits beside `worker.js`, and loads
it once. [`src/rust-runner.ts`](src/rust-runner.ts) holds the pipeline:

1. [`src/compiler-wire.ts`](src/compiler-wire.ts) encodes the project's
   files, the entry path, and a goal (run, check, test, or wat) into one
   request to the wasm's exports, and decodes the JSON answer: diagnostics,
   test cases, and the program's bytes or WebAssembly text. The standard
   library is inside the wasm.
2. Run instantiates the program with the browser host,
   [`host/web.mts`](../../host/web.mts), the browser layer over the same
   host core `hd run` uses on Node. Before it instantiates a program, it
   refuses one that imports a platform trait the browser cannot provide.
3. Test runs each case with the case loop of
   [`host/cases.mjs`](../../host/cases.mjs) over the same browser host.

The results are `RunResult` and `WatResult`
([`src/runner.ts`](src/runner.ts)), which the output panel, the WAT view,
and the tour show. The worker bundle holds only this glue and the REPL's
session and journal code, about 50 KB raw and 19 KB gzip; the compiler is
the separate `hd_web.wasm`, about 4.6 MB raw and 1.4 MB gzip.

## The REPL

The playground's **REPL** view and the website's docked panel (**›\_ REPL**
at the bottom of every docs page, or Ctrl+\`) are one panel,
[`src/repl-panel.ts`](src/repl-panel.ts), over a REPL session on the Rust
compiler (`cli.repl.*` in the spec, `f1c55020:future-work/compiler/repl-plan.md` job W5):

- Input: Enter runs an input; a line that ends in `:`, an open bracket, a
  `"""` string or a line of an indented block continues it, and an empty
  line ends a block, as at the terminal
  ([`src/repl-input.ts`](src/repl-input.ts)). Shift+Enter adds a line, Tab
  indents, Backspace in leading spaces removes one level, and Up and Down
  recall earlier inputs (history in memory, for the page's life).
- Output: each input is echoed with its number, `repl#N`, as its
  diagnostics and `dbg` lines name it. Standard output shows as written,
  value lines included (`[3, 6] : List[usize]`); `dbg` lines show muted;
  a panic, `cancelled` and diagnostics show as errors.
- The session's package is the project's files in the playground (its
  inputs see `src/lib.hd`, `cli.repl.package.lib`), and none on the
  website. Inputs entered while one runs wait their turn.
- Commands: `:help` lists them. `:reload` rebuilds the session from its
  journal over the editor's files as they are now (on the website, over the
  same empty package), and reports what it skipped and dropped as `hd`
  does. `:quit`, `:q`, `:exit` and Ctrl+D on an empty line end the
  session; the next input starts a new one, numbered from 1, and the
  website's panel closes.
- **Stop** first asks the worker to cancel the input, which ends an input
  waiting on the host at once: its `defer` suites run and it ends
  `cancelled` (`cli.repl.cancel`). An input that computes never reads that
  message, so after 250 ms the page terminates the worker, starts a fresh
  one and rebuilds the session from the journal (`cli.repl.rebuild`): each
  kept input is built again under its old number and replayed with every
  host call served from the journal. The stopped input is not in the
  journal, so its changes are lost; the panel says so, and reports the
  inputs the rebuild skipped (cancelled earlier) and dropped, as `hd`
  reports `:reload`.

How the pieces fit:

- The session runs in a playground compiler worker
  ([`src/worker.ts`](src/worker.ts)): the compiler's REPL session (the
  `repl*` goals of `hd_web`) and the JS session that runs each input's
  module ([`host/web.mts`](../../host/web.mts) `createWebSession`) sit side
  by side in [`src/repl-host.ts`](src/repl-host.ts). The playground's REPL
  view has its own worker, so Run and its Stop never end the session.
- The worker protocol: `repl-start` (files) answers `started`;
  `repl-input` (number, text) streams `repl-out` messages and answers
  `ran`, with the session's journal; `repl-stop` cancels; `repl-rebuild`
  (files, journal, each input's text) answers `rebuilt` with what it kept,
  skipped and dropped; `repl-quit` answers `ended`. The page side is
  [`src/repl-client.ts`](src/repl-client.ts). It numbers the inputs and
  keeps the files, texts and latest journal a fresh worker needs.
- [`test/repl-host.test.ts`](test/repl-host.test.ts) drives the worker
  protocol on Node, a terminated worker included;
  [`e2e-repl.ts`](e2e-repl.ts) drives the view and the docked panel in
  Chromium.
- A code block on the website that parses gets **Try in REPL**, which opens
  the panel and runs the block's top-level items as inputs, one by one, so
  each expression shows its value. A block that declares `main` gets
  **Open in playground** instead.

## Packages and Modules

A playground project is one package, built as `hd` builds a package
([10-modules.md](../../spec/lang/10-modules.md)): path-inferred modules
under `src/` (`src/a/b.hd` is module `a.b`, `src/a/mod.hd` is module `a`,
and `src/lib.hd` is the root module), uses between them, test modules
(`*_test.hd`), and integration test programs under `tests/`. The entry
module is the one marked `main`. Diagnostics name the file and line they
come from. There are no dependencies (`dep.<name>`) and no `hd.toml`
manifest.

## Other Limits

- Of the default runtime profile, only the traits a browser can provide are
  bound: `Console`, `Clock`, `Random`, `Args`, and `Http`, which reaches the
  page's own origin alone
  ([std/http.md](../../spec/std/http.md#playground)). A `main` whose row
  names another, such as `FsRead`, checks, but Run stops before running it
  with "main needs FsRead, which the playground does not provide".
- A runtime panic's report has no source frames yet: the frames need the
  compiler's source positions, which the playground does not ask for.
- A test case's `timeout` cuts a case that waits; a body that never yields
  is cut only by the 15-second run limit. Snapshot and regression files
  live in memory for one run.
- Triple-quoted multi-line strings compile, but the highlighter works line
  by line, so it does not color their later lines as string text.
