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
- **Run** (Ctrl+Enter or Cmd+Enter) compiles the project and runs its
  `pub fn main` or script body. With no entry point, it runs the `test`
  blocks, as `hd test` does. Console output streams into the output panel.
  Runtime panics are reported with their panic code. A run longer than 15
  seconds is stopped, and **Stop** ends a run early.
- **Check** (Ctrl+Shift+Enter) type-checks only. Diagnostics show
  `path:line:column` and the diagnostic code. Clicking one jumps to it, and
  the editor underlines it.
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
  run.
- Light and dark themes follow `prefers-color-scheme`. The layout stacks the
  editor and output on narrow screens.

## How It Works

`src/main.ts` runs the page. `src/worker.ts` runs the compiler in a module
Web Worker, so a long compile or an endless loop never blocks the page.
Stopping a run terminates the worker. `src/runner.ts` holds the pipeline:
link, `analyze`, then `instantiate` and call the entry export with a
`Console` provider, the default runtime profile. The compiler's two Node
dependencies get browser versions in `build.ts`:

- the emitter's `.wat` runtime files, read with `node:fs`, are embedded as
  strings;
- `node:crypto`'s `createHash("sha256")`, which names functions for traces,
  is replaced by a small synchronous SHA-256 ([`src/shims/crypto.ts`](src/shims/crypto.ts)).

Binaryen's npm build already runs in browsers. It makes up most of the
14.6 MB worker bundle, which the page loads in the background.

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
