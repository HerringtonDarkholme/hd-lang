# Diagnostic Rendering Versus The Spec

Status: report only. Nothing in this report is accepted design or an
implementation change. It reviews [Diagnostics](../../spec/README.md#diagnostics),
[Machine Output](../../spec/cli/command-line.md#machine-output),
[Exit Status](../../spec/cli/command-line.md#exit-status), and
[Runtime Panics](../../spec/lang/06-control-flow.md#runtime-panics). It also
compares the renderer with
[Checking and TIR](checking-and-tir.md#414-diagnostics).

## Scope

This pass ran the current `hd_cli` through five isolated single-file programs.
Each directory contained only the named source file so package discovery could
not pull another probe into the result. The commands used a disposable
`HD_CACHE`. Text output below is stderr; none of the probes wrote stdout.

| Probe | Broken construct | Exit | Current output |
| --- | --- | ---: | --- |
| Parse | A local binding written without `let` | 1 | `error: main.hd:38..39: error missing-let: missing-let` |
| Name | The unresolved expression `missing` | 1 | ``error: main.hd:26..33: error unknown-name: unknown-name `missing` `` |
| Type | `true` as the result of a function returning `i32` | 1 | `error: main.hd:26..30: error type-mismatch: type-mismatch in result: expected i32, found bool` |
| Row | `println` in a function whose row is empty | 1 | ``error: main.hd:27..45: error missing-requirement: missing-requirement: this needs `$ Console`, which the enclosing function's row does not name`` |
| Runtime panic | `panic("requested failure")` | 3 | `panic: requested failure`, followed by `instantiate_ms=... run_ms=...` |

The parse probe receives the specific `missing-let` code rather than the
general `syntax-error` code. That is consistent with the Diagnostics table's
rule that the more specific code wins.

## Rule-by-rule comparison

The specification does not define a normative human-readable compile-error
template. Its normative requirements are the stable code, the source line,
machine-output shape, and exit behavior. The compiler design's compact template
is therefore recorded separately below and is not treated as a spec rule.

| Spec rule ID | Required form | New compiler output | Result |
| --- | --- | --- | --- |
| `spec/README.md`, Diagnostics | Diagnostic code is stable; use a more specific code when one applies. | The four compile failures use `missing-let`, `unknown-name`, `type-mismatch`, and `missing-requirement`, all stable codes in the table. | Match |
| `spec/README.md`, Diagnostics | Report the line where the smallest breaking construct begins. | The compact form exposes only zero-based byte ranges (`38..39`, `26..33`, `26..30`, `27..45`), so a user or tool cannot obtain the required line from the output. The underlying primary spans begin on the relevant constructs in these probes. | Differs |
| `cli.json.commands` | `hd FILE` accepts `--format json`. | `hd .q19/name/main.hd --format json` prints usage and exits 2. | Differs |
| `cli.json.run` | In JSON mode, `hd FILE` writes only its own JSON-line diagnostics and summary to stderr. | JSON mode is rejected; ordinary text is the only diagnostic form. | Differs |
| `cli.json.run.program` | Program stdout passes through untouched in JSON mode. | JSON mode is unavailable, so this guarantee cannot be exercised. The text-mode probes wrote no stdout. | Differs |
| `cli.json.kind` | Every JSON object has `kind` equal to `diagnostic`, `test`, or `summary`. | No JSON objects are emitted. | Differs |
| `cli.json.diagnostic` | One object per diagnostic, including stable code, severity, file, and position. | No JSON diagnostic object is emitted. The internal `DiagBuf::render_json` is an array and is not wired to the CLI. | Differs |
| `cli.json.diagnostic.fields` | A diagnostic object has `code`, `severity`, `message`, `file`, `line`, and `column`. | The internal JSON form has `code`, numeric file ID, `lo`, `hi`, and `message`; it lacks `kind`, `severity`, path, line, and column. | Differs |
| `cli.json.diagnostic.file` | Outside a package, `file` is the command-line path as written. | Text mode reduces `.q19/name/main.hd` to `main.hd`; JSON mode is unavailable. | Differs |
| `cli.json.summary.result` | The final JSON line is a summary, including on success. | No summary is emitted. | Differs |
| `cli.json.summary.result.fields` | Summary fields are `errors`, `warnings`, `passed`, `failed`, `ignored`, and `status`. | No summary is emitted. | Differs |
| `cli.json.diagnostic.fixes` | Every diagnostic object has a `fixes` list, empty when there is no fix. | No CLI JSON object or fixes field is emitted. | Differs |
| `cli.exit.hd-failure` | A compiler error, rejected command line, or internal failure exits 101. | Compile diagnostics exit 1; rejected `--format json` exits 2. | Differs |
| `flow.panic.report` | A runtime panic reports a stable failure category and an available source location. | The panic probe prints only `panic: requested failure`; it omits the category and source location. | Differs |
| `flow.panic.stable-categories` | Runtime panic categories come from the closed stable list. | No category is printed. | Differs |
| `flow.panic.explicit` | `panic(message)` produces category `explicit-panic`. | The message is preserved, but `explicit-panic` is absent from the report. | Differs |
| `module.entry.panic` | A panic exits with the runtime profile's distinct nonzero status. | The panic exits 3. | Match |
| `module.profile.panic-status` | A profile's panic status is never 101. | The panic exits 3. | Match |
| `cli.exit.program` | After a program is built, `hd FILE` exits with the program's status. | The runtime panic's status 3 reaches the CLI unchanged. | Match |

The remaining fix-it rules (`cli.json.fix.object` through
`cli.json.fix.disjoint`) cannot be observed until the CLI emits the required
diagnostic objects. Their byte-offset definition does not license byte offsets
in place of the diagnostic object's required `line` and `column` fields.

## Compiler changes needed

1. Give diagnostic rendering access to source line indexes and render a
   one-based line and column for the primary span. Keep byte offsets for JSON
   fix-it edits only.
2. Make one layer own the compact prefix. Today `DiagBuf::render_compact`
   includes severity and the CLI prepends `error:`, producing two `error`
   labels. The compiler design's compact form is
   `file:line:column: code: message`.
3. Give each diagnostic a message that does not repeat its code. The current
   placeholders produce `missing-let: missing-let`, `unknown-name:
   unknown-name ...`, and the same pattern for type and row errors.
4. Carry both the package-relative diagnostic path and the outside-package
   command-line spelling through source loading, then select the form required
   by `cli.json.diagnostic.file`.
5. Add `--format json` to the required commands. Emit JSON Lines, not an array;
   include `kind`, all required diagnostic fields, `fixes`, and a final summary,
   and route file/run records to stderr while leaving program stdout alone.
6. Map compiler, command-line, manifest, and internal failures to status 101.
   Preserve a started program's own status.
7. Carry panic category and source location through the Wasm/host boundary and
   print them. The explicit probe must identify `explicit-panic`. Do not print
   unconditional timing telemetry in ordinary program output.
8. Add text and JSON CLI tests for the five probes, asserting stdout, stderr,
   path spelling, line/column, field shape, summary, and exit status.

## Non-normative design comparison

`future-work/compiler/checking-and-tir.md` specifies compact output as
`file:line:col: code: message`. Current output instead uses
`file:lo..hi: severity code: message`; `hd_cli` then adds another severity
prefix. Its JSON design calls for the CLI JSON Lines form, while the current
unused `DiagBuf::render_json` produces one JSON array with internal file IDs and
byte spans. Both renderers therefore need a source-aware CLI adapter rather
than direct exposure.
