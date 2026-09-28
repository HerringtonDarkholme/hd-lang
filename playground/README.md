# hd Playground

A browser playground for hd-lang. It edits, checks, and runs hd programs
entirely in the browser with the prototype compiler in [`../src`](../src),
which emits Wasm GC through Binaryen. The build is static files, so it can be
hosted anywhere, including GitHub Pages.

```sh
npm run playground:build   # writes playground/dist/ (index.html + assets/)
npm run playground:dev     # rebuilds on change and serves http://localhost:8000/
npm run playground:e2e     # after a build: drives dist/ in headless Chromium
```

Every URL in `dist/` is relative, so the folder works at `/` and under a
sub-path such as `/hd-lang/playground/`. The e2e run serves it under that
sub-path. It needs a local Chrome, Edge, or Chromium; `CHROME_PATH` selects
one. The browser needs WebAssembly GC: Chrome 119, Firefox 120, Safari 18.2,
or later.

## Features

- CodeMirror 6 editor. Highlighting replays `classify(line)` from
  [`src/highlight.ts`](../src/highlight.ts), the highlighter the REPL uses.
  There is one token classifier, and
  [`test/highlight.test.ts`](test/highlight.test.ts) checks that the editor's
  classes match it.
- **Run** (Ctrl+Enter or Cmd+Enter) compiles the project and runs it:
  - when the entry module declares `main`, Run calls `pub fn main`, as
    `hd run` does;
  - without `main`, Run evaluates the entry module with REPL semantics
    ([`src/repl.ts`](../src/repl.ts), the session `hd repl` uses). Each
    top-level input runs in order: declarations join the session, statements
    run, and each expression prints its value and type as the REPL prints
    them, such as `42 : i32`. The first rejected input stops the run, and its
    diagnostic points at the file's line. As in the REPL, declarations cannot
    see top-level bindings. The other modules of a multi-file project become
    the session's first declarations;
  - a file without `main` whose top level holds only declarations runs its
    test cases, as before.

  Console output streams into the output panel. Runtime panics are reported
  with their panic code. A run longer than 15 seconds is stopped, and
  **Stop** ends a run early.
- **Check** (Ctrl+Shift+Enter) type-checks only, with the same semantics as
  Run: a file without `main` is checked input by input, as the REPL would
  check it, without running anything. Diagnostics show `path:line:column`
  and the diagnostic code. Clicking one jumps to it, and the editor
  underlines it.
- **Test** runs the test cases, as `hd test` does, whether or not the
  entry module declares `main`. It checks the file as a module, so top-level
  code must type-check as module initialization.
- **WAT**, beside **Output** in the output pane, shows the WebAssembly text
  of the module the project compiles to:
  - for a program, the module Run and Test instantiate. The prototype
    compiler emits this text itself (`emitWat`), and Binaryen assembles it
    into the Wasm binary without optimization passes, so the text is the
    module the runtime executes; there is no optimized variant to show.
    `hd build --wat FILE` prints the same text;
  - without `main`, Run compiles one module for each top-level input it
    evaluates, since REPL semantics runs each input as its own program. The
    view shows the last module the run compiled and says how many there
    were. Before the first Run, it asks for one;
  - when compilation fails, the view shows the diagnostics, which jump to
    their source positions, instead of an old module.

  The view is generated on demand. The page asks the worker for the text only
  while the view is open, and refreshes it after an edit, a run, or a test.
  The worker keeps the text of the module its last run or test compiled, so
  opening the view after a run compiles nothing. Highlighting comes from a
  small tokenizer, [`src/wat.ts`](src/wat.ts), tested in
  [`test/wat.test.ts`](test/wat.test.ts). Lines are numbered. The view
  renders at most 5,000 lines; **Copy** and **Download .wat** give the full
  text.
- Files: `+` adds a module, double-click renames one, and `×` deletes one.
  `▸` makes a module the entry module, which is marked `main`.
- **Share** copies a link that holds the whole project in the URL hash:
  - `#code=<base64url of the UTF-8 source>` for a single `src/main.hd`;
  - `#project=<base64url of UTF-8 JSON {"files": {path: source}, "main": path}>`
    otherwise.

  base64url is RFC 4648 section 5 without padding. The page reads the hash on
  load and on `hashchange`. When there is no hash, the last project is
  restored from `localStorage`.
- **Examples** bundles programs from [`../examples`](../examples),
  [`../spec/conformance/runtime/valid`](../spec/conformance/runtime/valid), and
  [`examples/`](examples) as text, so the menu shows the files the test suites
  run. [`examples/top-level.hd`](examples/top-level.hd) has no `main` and
  shows Run's REPL semantics.
- Light and dark themes follow `prefers-color-scheme`. The layout stacks the
  editor and output on narrow screens.

## How It Works

`src/main.ts` runs the page. `src/worker.ts` runs the compiler in a module
Web Worker, so a long compile or an endless loop never blocks the page.
Stopping a run terminates the worker. `src/runner.ts` holds the pipeline:
link, `analyze`, then `instantiate` and call the entry export with a
`Console` provider, the default runtime profile. It also reports the module
a run compiled, and `watProject` compiles a project's module without running
it, for the WAT view. Without `main`, it feeds the
entry module's top-level inputs (`splitInputs` in
[`src/repl-input.ts`](../src/repl-input.ts)) to a `ReplSession`, whose
`compiledModule()` gives the WAT of the last module it compiled.
`src/compiler-client.ts` is the page side of the worker, shared with the
website's REPL panel. The compiler's two Node
dependencies get browser versions in `build.ts`:

- the emitter's `.wat` runtime files, read with `node:fs`, are embedded as
  strings;
- `node:crypto`'s `createHash("sha256")`, which names functions for traces,
  is replaced by a small synchronous SHA-256 ([`src/shims/crypto.ts`](src/shims/crypto.ts)).

Binaryen's npm build already runs in browsers. It makes up most of the
14.6 MB worker bundle, which the page loads in the background.

## The Website REPL Panel

Every page of the website has a REPL panel docked at the bottom. **›\_ REPL**
or Ctrl+\` opens it over the page; Esc or Ctrl+\` closes it. It is the same
REPL as `hd repl`, with the same commands (`:type EXPR`, `:source`,
`:reset`, `:help`; `:quit` and Ctrl+D on an empty line close the panel):

- the panel script, [`../website/client/repl.ts`](../website/client/repl.ts),
  reads input with `needsMoreInput` from
  [`src/repl-input.ts`](../src/repl-input.ts): Enter evaluates, while a line
  ending in `:` or an open bracket continues and an empty line ends a block.
  Shift+Enter always adds a line, Tab indents, and Up and Down recall
  history, which is kept in `localStorage`. Input, echoed input, values, and
  `:type` results are highlighted with `classify` from
  [`src/highlight.ts`](../src/highlight.ts); errors show in red with their
  `line:column` in the input;
- the session runs in this playground's `worker.js`, through
  `CompilerClient`. The worker answers each input with `respond` from
  [`src/repl.ts`](../src/repl.ts), the function the terminal REPL calls. The
  panel script is about 13 KB. The 14.6 MB worker loads only when the panel
  first opens;
- **Stop** ends an input that runs too long, and so does the 15-second
  limit. The client replays the inputs the old worker had accepted into the
  new one, so the session survives;
- a code block that parses gets a **Try in REPL** button. It opens the panel
  and evaluates the block's inputs in order, so declarations join the
  session and statements and expressions run. A block that declares `main`
  is a whole program, which the REPL cannot take because it supplies its own
  `main`. Such a block gets a smaller **Open in playground** link instead.

The website build bundles the panel only when `playground/dist/` exists, so
build the playground first. `npm run website:e2e` drives the panel in a
headless Chromium after both builds.

## Packages and Modules

The prototype compiler checks one module at a time. Multi-file projects go
through the package linker, [`src/package.ts`](../src/package.ts). The linker
resolves uses between the package's files, then joins the modules reachable
from the entry into one program in initialization order. It maps each
diagnostic back to its file and line. The same linker serves single-file
projects.

What works, relative to [10-modules.md](../spec/10-modules.md):

- path-inferred modules under `src/`. `src/a/b.hd` is module `a.b`, and
  `src/a/mod.hd` is module `a`. Paths must be identifiers, and two paths may
  not name the same module after case folding;
- `use pkg.a.b.{X, Y}` and `use pkg.a.b.X`;
- relative uses from the containing directory module, as in
  `use self.types.{User}` and `use super.shared.{Email}`;
- `pub use` re-exports, typically in `mod.hd`;
- uses of missing modules, missing declarations, and private declarations
  are rejected, as are use cycles;
- modules are initialized after the modules they use, with ready modules in
  lexicographic order. Only modules reachable from the entry are linked;
- standard-library uses (`use std.testing.assert_equal`) may repeat across
  modules.

The linker's own diagnostic codes are `invalid-module-path`,
`duplicate-module-path`, `unknown-module`, `unknown-import`,
`private-import`, `use-cycle`, `unsupported-package-use`, and
`package-name-collision`.

Not supported yet:

- Module namespace uses (`use pkg.user.types` and then `types.User`) and
  renaming a package declaration with `as` are `unsupported-package-use`.
- Linked modules share one namespace. Two modules cannot declare the same
  top-level name, even privately (`package-name-collision`), and only the
  entry module may declare `main`.
- The linker checks that each `use` names a public declaration. It does not
  stop a module from naming another module's declaration without a `use`.
- There are no dependencies (`dep.<name>`) and no `hd.toml` manifest.

## Other Limits

The playground runs what the prototype compiler supports; see
[`../src/README.md`](../src/README.md). Beyond that:

- Only the default runtime profile is provided, which binds `Console`. The
  CLI's test profiles (`--profile`), scenarios, trace, record, and replay are
  not exposed.
- Triple-quoted multi-line strings are neither lexed by the prototype nor
  highlighted.
