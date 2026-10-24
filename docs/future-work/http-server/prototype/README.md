# HTTP Handler Controls

These are research controls for [the design record](../../HTTP_SERVER.md), not a shipped server or handler ABI.
They exercise today's typed code and measure hosts; the decided design uses an ordinary `main!` and an SDK capability, not the handler entry the `gap` package declares.
The hd program calls an ordinary typed handler from `main!`.
The JS bridge supplies a JSON envelope through Args and reads one Console line.
Application logging would corrupt that bridge; production uses the SDK capability instead.

Run from the repository root, with a dev `hd` binary supporting both engines.
Keep `HD_CACHE`, `TMPDIR`, and results inside the isolated worktree.
These commands use existing tools and install nothing.

```sh
mkdir -p .cache-muse-http/tmp .cache-muse-http/results
export HD_CACHE="$PWD/.cache-muse-http/hd"
export TMPDIR="$PWD/.cache-muse-http/tmp"
hd check docs/future-work/http-server/prototype/app/src/main.hd
(cd docs/future-work/http-server/prototype/app && hd build --engine node)
node docs/future-work/http-server/prototype/test-control.mjs "$PWD/.cache-muse-http/results"
node docs/future-work/http-server/prototype/measure.mjs "$PWD/.cache-muse-http/results"
```

`test-control` checks typed JSON, invalid JSON/UTF-8, query preservation, timeout, outgoing HTTP, eight overlapping calls, and cancellation.
It also expects a pending synchronous `block_on` to be refused.
The provider grants only the loopback fixture and refuses redirects; it is not a conforming replacement for the complete Http provider.
The ready-only `rt.block` probe mirrors readiness completion but never sleeps on the JavaScript thread.

For an installed `@edge-runtime/vm@5.0.0` with `@edge-runtime/primitives@6.0.0`, pass its parent directory containing `node_modules`:

```sh
node --experimental-vm-modules docs/future-work/http-server/prototype/test-edge.mjs "$PWD/.cache-muse-http/results" /absolute/tool/directory
```

This loads the actual host modules inside the VM context.
The Wasm module is compiled outside that context, modeling a static module import.
It does not test a hosted Vercel deployment or its bundler.

For an existing `workerd` binary, prepare a scratch bundle:

```sh
python3 docs/future-work/http-server/prototype/prepare-workerd.py .cache-muse-http/workerd
workerd serve .cache-muse-http/workerd/server.capnp
# In another terminal, from the same worktree:
node docs/future-work/http-server/prototype/test-workerd.mjs "$PWD/.cache-muse-http/results"
```

The control binds loopback ports 18767 and 18768; both must be available.
Stop the server after the test.
For the JSPI feature check, serve `features.capnp` instead and request `http://127.0.0.1:18766`.
A successful suspended call returns `result: 7`, not merely function names.

The `gap` package deliberately declares the rejected `kind = "handler"` key.
Today, checking or building it warns `unknown-manifest-key`, then builds an empty script rather than a handler.
The ordinary `handle!` declaration parses and checks; it is not an entry export.

Measurements expose the existing private heap through a measurement-only Wasm export.
They report initial and small-request memory, not a memory bound for arbitrary applications.
The concatenated gzip figure is a reproducible size proxy, not a platform bundler's admission result.
