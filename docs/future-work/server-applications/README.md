# Server application experiments

These accompany [the #806 proposal](../SERVER_APPLICATIONS.md), which is not accepted behavior.
The sources target `4d2e52d27d4bd3456303f912fb22a57109aa1e72` and use ordinary packages and main functions.
They implement neither a registered application ABI nor a production SDK.
The controller accepts trusted loopback peers only; it lacks authentication, lease generations and production cancellation recovery.

Build a native-capable dev CLI with `cargo build -j2 -p hd_cli --features wasmtime`.
Use an absolute path for `HD` because the controllers start their guests inside individual packages.
Run commands from the repository root, with scratch directories inside your isolated worktree:

```sh
export HD="$PWD/target/debug/hd"
export HD_CACHE="$PWD/.cache-server-design/cache"
export TMPDIR="$PWD/.cache-server-design/tmp"
export HD_JOBS=2
mkdir -p "$HD_CACHE" "$TMPDIR"
node docs/future-work/server-applications/control.mjs "$HD" wasmtime typed http "$TMPDIR/http.json"
```

The native live matrix uses shapes `library`, `entry`, `typed` and modes `http`, `worker`, `queue`, `cron`, `drain`, `signal`.
The first two differ in application orchestration; `entry` calls ordinary hooks through a main shim.
`typed` exercises generic deserialize/handle/error-map/serialize dispatch.
It does not erase the handler's error or add Clock to a callback's row.

For `signal`, wait for the controller's JSON `{"event":"active","count":2}`, then send SIGTERM to that controller process.
It stops admission without forwarding the first signal to the guest, drains its two active jobs, and records `signalCount: 1`.
Only signal a controller you launched for this experiment.
The controller has a 20-second guard and kills its owned guest process group on POSIX systems.

The Node live matrix currently fails because concurrent Net reads block the guest's polling thread.
Do not interpret the finite controls as live Node parity.
[#810](https://github.com/HerringtonDarkholme/hd-lang/issues/810) tracks the prerequisite.
Use the bounded reproducer to observe it directly:

```sh
node docs/future-work/server-applications/read-gap.mjs "$HD" node "$TMPDIR/read-node.json"
node docs/future-work/server-applications/read-gap.mjs "$HD" wasmtime "$TMPDIR/read-native.json"
```

Both controller commands succeed when they observe their expected outcome.
Node is expected to exit 3 with `panic: time-limit: the program exceeded its time limit` and one ready client.
Wasmtime is expected to exit 0, report two ready clients and print `1 1`.
A future #810 fix must update this historical expected-failure probe deliberately.

Run each finite package with `hd run --engine ENGINE -- ARGS`, from that package directory.
The `model` and `transport` packages are libraries; check them with `hd check --all`.
Parse each `src/*.hd` with `bash scripts/hd-conformance.sh parse FILE` from the repository root.
Check and format each source using `hd check --all` in its package and `hd fmt --check FILE`.

| Package | Arguments and expected observation |
| --- | --- |
| `workers` | No arguments; admitted 2, completed 2, queued 1, workers 2. |
| `race-cancel` | No arguments; winner 0, completed 0, cleaned 1. |
| `shared-state` | No arguments; stale 1, current 2. |
| `values` | No arguments; original 2, copy 9, captured 3. |
| `instance-state` | No arguments; independent counts 1 and 1, then a local duplicate. |
| `pure-http` | No arguments; greeting JSON; main has Console alone. |
| `config` | No arguments; synthetic PORT/TOPIC decoding, invalid port rejection, redacted secret and explicit revealed length 14. |
| `library`, `entry-control` | `http`, `worker`, `queue` or `cron`; finite typed replies and final stopped state. |
| `isolated-job` | `http '{"name":"Ada"}'`, `http 'bad json'`, `worker 3`, `queue 7`, or `cron 7`; one JSON reply and fresh state each process. |
| `poison` | `normal` completes with a cleanup marker; `panic` intentionally exits 3 without running its pending defer. |
| `entry-gap` | `hd build --engine ENGINE`; ignored `kind = application` warning and a 407-byte ordinary program module. |
| `read-concurrency` | Requires the `read-gap.mjs` peer; a port argument alone supplies no test server. |
| `live-library`, `live-entry`, `live-typed` | Require `control.mjs`; main reads its private peer's port argument. |

`control.mjs` owns actual HTTP ingress in `http` mode; other ingress types are simulations.
The HTTP control expects greeting/200 and invalid-body/400 responses.
Queue/cron send IDs 7, 7, 8 and observe two local commits, plus an acknowledged duplicate.
Drain/signal admit 3 and 4, leave 99 queued and finish with state count 7.

The model's list-based duplicate set is tiny test data, not a proposed production deduplication algorithm.
Production durable idempotency belongs at the external effect/acknowledgement boundary.
Neither queue simulation nor independent state objects exercise a real broker crash or transaction.

## Measurement method

Build with `hd build --engine node` and `hd build --engine wasmtime` in each package.
Inspect `build/debug/<executable>.wasm`; the proposal reports bytes, defined functions and complete code-section payload bytes.
All results use dev builds; single build wall times include CLI and cache effects and are not benchmark statistics.
The registered-entry gap's tiny output is unused hooks being omitted, not a measured application ABI.

Reference physical/code lines, after formatting, are `control.mjs` 298/293 and `read-gap.mjs` 95/92.
Code counts omit blank lines and lines starting with `//`; they retain braces and continuation lines.
HD uses the same count with `#` comments; transport 77/68, model 99/85, typed caller 40/35.
These counts describe the checked reference, not complete production implementation cost.

The source-matched web compiler baseline is 5,709,379 raw bytes and 1,913,091 gzip bytes.
Its SHA-256 is `7b99aa5838cce7bb523f7fde347ad6633eab920c88c215cb887917d8868df59c`.
Compression uses Node `gzipSync(bytes, {level: 9})`; compare with the same implementation and settings.
This documentation job changes no compiler source and claims no measured future host/compiler size delta.

Raw commands, results and frozen binaries are preserved locally in `/private/tmp/muse-806-server-app-1792801107010977000`.
The committed sources and commands reproduce the controls without relying on that directory.
Platform evidence is macOS; the Windows kill fallback does not establish descendant cleanup.
No full test suite is required by these documentation experiments.
