# Wasmtime Review

## Review of M24

M44 review (2026-10-11) of Codex's M24 range (`53f476b4..8895174b`, six
commits: Process worker cleanup, compiler-heap gating, native provider
errors, bounded fixtures, reply/wake/termination centralization, and the
docs-only gate record). Read each code diff in full, checked every item
below against the current tree (paths reflect the later M36/M42/M43
splits), and cross-checked behavior claims against `spec/std` and
`spec/lang`. No code was changed. Severity is relative to a test-only
host: correctness of verdicts first, fixture robustness second.

### Findings

1. Fixture release runs after its assertions, against its own comment
   (`compiler/crates/hd_run_wasmtime/src/core.rs:967-977`).
   `worker_creation_failures_cancel_and_reap_before_scope_teardown`
   observes `waitpid` (proving `cancel_worker_start` reaped the child),
   then runs `child.kill()`/`child.wait()`, and only then asserts. The
   comment says "Always release the fixture before asserting", but if
   `assert_eq!(workers.attempts, fail_at)` panics, the `sleep 2` child
   is never reaped by the test and stays a zombie until the test binary
   exits. Fix: bind the observed values to locals, run kill/wait, then
   assert on the locals (the `waitpid`-before-cleanup proof is
   preserved, since the observation already happened).

2. Fixture budget assert races the read it guards
   (`compiler/crates/hd_run_wasmtime/src/http/test_support.rs:44-47`).
   `read_request` asserts the 2 s budget remains, then calls
   `stream.read(&mut buf).unwrap()`. If the budget expires between the
   two, the test panics with a bare `WouldBlock`/`TimedOut` instead of
   the "request completes within fixture budget" message. Bounded either
   way (no hang), but the failure misdirects. Fix: map the read error
   to a panic carrying the budget message.

3. `empty_wake_batch_does_not_require_an_exchange_buffer` does not prove
   invocation (`compiler/crates/hd_run_wasmtime/src/lib.rs:937-997`).
   `poll` returns the constant 7, so the test passes even if `wake(&[])`
   never called the guest export; only "no trap, no exchange buffer"
   (the module has no memory export) is actually proven — the
   "still invokes hd.wake with count zero" comment (line 960) is not.
   Fix: give the guest an observable invocation marker (a mutable
   global set inside `wake`, returned by a second `poll`, as the
   `Exit(85)` test next door does for its batch), or narrow the
   comment to what `Exit(7)`-without-trap proves.

4. `malformed_reply_shapes_fail_before_output_or_filesystem_effects`
   covers 3 of 12+ import kinds
   (`compiler/crates/hd_run_wasmtime/src/lib.rs:760-843`). The six table
   rows exercise `Console.write_line.start`, `rt.stderr`, and
   `FsWrite.write_text.start` only; `ProcessStart`, `HttpStart`, `Net`,
   `ReadLine`/`Sleep` starts, `TestRunner` methods and the float parse
   path go through the same new `validate_results`-before-`call` order
   untested. The ordering guarantee is generic, so this is coverage
   debt, not a suspected bug. Fix: extend the table (the harness makes
   each row cheap) or scope the name to the representative set.

5. Workspace members after the first compile untracked under
   `--max-memory` (`compiler/crates/hd_cli/src/memory_cap.rs:390-396`,
   `compiler/crates/hd_cli/src/main.rs:106`). `enter_guest()`
   permanently disarms enforcement while `arm()` runs once before
   dispatch, so in a workspace run every member after the first builds
   with no compiler accounting. The module docs disclose this
   ("owner-deferred lifecycle issue"), so this is a residual hole, not
   a hidden one. Fix: re-arm per member build (arm around each
   member's compile phase), or record the gap as accepted CLI behavior.

6. Commit messages cite test counts the tree does not contain. At
   `8895174b`, `src/net.rs` holds 16 `#[test]` (message: 12),
   `src/core.rs` holds 21 (message: 25), `src/http.rs` holds 30
   (message: 18). The integration files explain part of it
   (`tests/net_parity.rs` holds exactly 12), but as written the counts
   are unverifiable. Fix: state the scope with each count (file,
   filter, platform).

### Spec note (suggestion, not a violation)

`std-process` is silent on partial stdin consumption, and no rule pins
`Other` text beyond "as written" (`std-process.error.display.other`).
The BrokenPipe change (started child keeps its output and status)
therefore violates nothing, but the behavior is now load-bearing and
unpinned. If the owner wants it kept, it needs a rule; otherwise a
future alignment pass could flip it back without failing any gate.

### Checked and found sound

- BOM preservation in `read_text` follows `std-fs.read.text`
  ("returns that content"); the old strip was the deviation, and no
  conformance fixture reads BOM files, so there is no engine divergence
  in the suite.
- Removing the port-0 and send-family preflights follows
  `std-net.error.invalid-address` (host-only); removing the empty
  aggregate follows the owner direction against Node emulation.
- Keep-write-half-open after read EOF is compatible with `std-net.read
  ` (empty list at end; nothing said about write): the old error was
  the stricter behavior.
- `complete()` returning `u32` by reinterpretation is equivalent in
  practice (reactor handles start at 1; 2^31 pending operations is
  unreachable).
- The `Ret` centralization is shape-preserving at every call site
  (verified arm by arm against the diff); oversize payloads trap the
  same way, only without pre-clobbering the buffer.
- `PipeWorkers` failure injection and `cancel_worker_start` ordering
  are sound; `requested()` correctly ignores guest words after `--`;
  `SockRef` predates the `socket2/all` removal, so the removal is
  safe; all removed helpers (`process_error`, `inspect_string`,
  `node_errno`, `put`) have no remaining callers.
