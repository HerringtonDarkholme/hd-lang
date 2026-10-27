# hd fmt

Proposal, revised 2026-10-27; domain: formatter internals and CLI layout; not accepted.
For [#906](https://github.com/HerringtonDarkholme/hd-lang/issues/906).
No formatter implementation or bulk reformat is part of this PR.

## The proposal

Make `hd fmt` opinionated, following the owner's Go direction: one style,
100 columns, four spaces, and no style settings. Parse once, attach comments,
build a compact document from the syntax tree, then print it once.
The same syntax, comment attachment and authored statement blank lines produce
the same layout; other input spacing and wrapping have no effect.
Keep literal spelling, comments and meaningful blank-line boundaries.

```text
fn total(xs: List[i32]) -> i32:
    combine(xs, +1)
```

Both `combine( xs,+1 )` and an unnecessarily vertical `combine` call become
that layout. Types are irrelevant: an unresolved `combine` still formats.
A style flag multiplies output variants and tests; a fixed style keeps editor,
CLI and future browser output identical. This adopts Go's tooling approach,
not its tabs or its particular line-breaking rules.

## Open questions

1. Approve the fixed style and parse/build/print architecture below? Recommendation: yes.
2. Keep formatting outside the web compiler until a Format feature is approved? Recommendation: yes; zero added download now.

The owner has settled suite layout: function and statement bodies stay on their
own lines. Match arms, closures and inline `if` expressions use one line when
they fit and break otherwise. Joining every fitting suite would move about
1,400 of std's 4,100 indented bodies onto headers, so it is excluded.

## Problems it solves

| Input today | Proposed result |
| --- | --- |
| `call( a,b )` and `call(a, b)` remain different | One canonical spacing rule for calls. |
| A short call previously broken across lines stays broken | Rejoin it when the canonical form fits. |
| Every additional overlong call triggers whole-file reparses | One parse and linear document work for the complete file. |
| Moving an opening-line comment can move a diagnostic annotation | Attach it explicitly and test its boundary, rather than protecting byte ranges. |

## How it works

Keep `format(&str) -> Result<String, Vec<Diagnostic>>` as the sole formatting
entry point. Parsing, comment attachment and layout stay private to `hd_fmt`;
there is no new public parsed-input or comment API without a second caller.
The existing lossless `round_trip` helper remains. Syntax errors return before layout.

A syntax rule turns `consume(a, b)` into this small document:

```plain
Group
├─ "consume("
├─ Indent(SoftLine, "a,", SoftLineOrSpace, "b", CommaIfBroken)
├─ SoftLine
└─ ")"
```

Try the group flat together with the rest of its physical line through the
first break; if the code fits 100 columns, print it flat, otherwise break it.
Flat soft lines disappear or become a space; broken ones become indented lines.
This call therefore prints `consume(a, b)` or one argument per line with its
closer on a separate line. There is no reparse or speculative source edit.

The private document uses a safe indexed `Vec` arena, borrowed source spans,
indentation, groups, soft/hard lines, verbatim text and suffix comments.
An iterative `fits` walk follows the same commands as printing, including
following punctuation. A flat parent makes its children flat without more fits.
The builder removes empty groups/indents and asserts that invariant. Width
cutoff alone does **not** bound other zero-width tags: Appendix E specifies
jump tables and operation-count gates. The bound is O(input + 100 × groups +
output), with O(input) auxiliary storage; emitted indentation counts as output.

The fixed style keeps function and statement-suite bodies on their own
lines, even when short. Match arms, closures and parenthesized conditionals may
group flat only where hd's inline grammar permits; a complete flat body stays
flat. An eligible parenthesized conditional joins **all** branches on one line when the whole expression
fits; otherwise every branch starts a separate line, with indented bodies for
statement suites. Other conditionals use one branch per line. Comments or
multiline bodies force the group to break.

Attach comments once, recording leading/trailing/dangling roles and boundary
slots. Before assigning a comment after a suite, use its source indentation:
shallower climbs to a parent, equal stays, deeper descends into the last child
suite where possible. Process intervening comments before popping completed
suites for the next token. A final indented comment therefore stays inside its
function, including at EOF. Preserve doc ownership/module-doc classification.

Preserve comment bytes and authored inline/standalone roles. Emit punctuation
before suffix comments; flush before later code. End-of-line comment text is
**excluded** from the 100-column code width, so a long trailing note alone never
expands `call(a, b)`. A comment between arguments can still force a break before
the next argument. Multiline literal/interpolation bytes are verbatim, including
interior comments; no duplicated attachment or injected indentation.

Normalize operator/assignment/keyword and comma/colon spacing. Nonempty flat
struct/map braces have one interior space; indentation is four spaces and width
counts Unicode scalars. Preserve unary/reference/bang/spread/range spelling and
required lexical separation. Optional trailing commas occur only in broken
groups; a singleton tuple retains its required comma. Old wrapping never forces
expansion. Keep at most one authored sibling blank line, except doc ownership.
Generated layout uses LF and one final newline; empty/blank files become empty.
Literal/comment bytes retain original whitespace and line endings.

Preserve authored parentheses except one explicit, precedence-safe removal class:
a continuation wrapper directly enclosing a binary/postfix/pipe chain in a
listed noncallee value/header slot. Recompute that wrapper from canonical width,
so shortening a chain also removes an obsolete wrapper. Call targets are never
eligible: `(handler.callback)(event)` must retain its group. Appendix E defines
the tree oracle and exact slots; never globally erase `ParenExpr`.

Long postfix chains normally use leading-dot lines at statement indent + four;
keep the first complete call as the head, then one remaining dot segment per
line. **No unparenthesized dot-line continuation in suite headers**: wrap the full condition/iterable/matchee
in continuation parentheses and break inside, then close before the suite colon.
**No dot lines after a same-depth pipe**. Break pipe chains with leading `|>`
lines; a bare pipe path stays atomic, never parenthesized. Pipe-step suites stay
inline, preserving the checker's single-line rule. Unbreakable syntax may exceed
100. Parenthesized callees remain opaque. Dedent a suite-final argument before
its comma, with the closer on another line.

Share the **existing tree and tokens** with lint. Comment attachment stays
private until a real second consumer needs ownership; then extract only the
common neutral data. Formatting never requests semantic/type facts or links
`hd_lint`. CLI integration formats independent files in parallel using existing
`--jobs` scheduling, with deterministic path ordering and one writer per file.

## Cost

| Component | Current | Proposal and acceptance evidence |
| --- | --- | --- |
| Source layout | 648 source lines; 527 in `width.rs` | Replace the width loop with syntax rules and a bounded document kernel; final LOC unmeasured. |
| Memory | Repeated complete strings and parse results | O(input) IDs/edges/comments plus one output; measure peak allocations. |
| CPU, recursive `lib/std` | 935.072 ms, 42 files / 879,422 B | Unmeasured; compare identical file hashes and profile after approval. |
| Syntax alone, same corpus | 14.492 ms | Baseline for separating parse from build/print cost; not a predicted formatter time. |
| Web compiler | 5,103,008 raw / 1,707,892 gzip B; formatter absent | Keep that dependency graph; no formatter code added now. |
| Formatter in web | No Format export today | If later shipped, measure complete raw/gzip delta; no size estimate substitutes for a build. |

Appendix D gives measured current timings. No new formatter exists yet:
new timing, allocation counts and added Wasm size are unmeasured, not claimed wins.
The frozen web control is from the same base, measured with Node gzip level 9;
it remains below the 2 MB compressed budget before any formatting feature.
After approval compare the same frozen inputs, compiler profile and host,
including every std file and doubled adversarial inputs. The serial dev-profile
budget is **at most 3× the same-run parser-only median** for all 42 std files:
about **43.5 ms** against today's 14.492 ms parse. Fix/print visit counts must
obey Appendix E's linear bounds; repeated-unit doubling must not add superlinear
work. Missing either gate requires redesign before the bulk reformat.
Publish corpus times, allocations, maximum memory and raw/gzip deltas.

## A. Stress test

The excerpts below are parser-checked inputs and hand-specified output, not an
implemented formatter. They establish syntax/structure only; implementation
idempotence, comment ownership and typing/verdict gates remain mandatory.

Short match arms, from `lib/std/encoding.hd:150`. The enclosing function
cannot flatten its direct `match`; each arm stays compact.

Before and proposed output are identical:

```text
impl Base64:
    fn padded(self) -> bool:
        match self:
            .Standard => true
            .StandardNoPad => false
            .Url => true
            .UrlNoPad => false
```

Parenthesized conditional, from `encoding.hd:176`. Statement branches retain
indented bodies; short parenthesized inner conditionals remain compact.

Before:

```text
impl Base64:
    fn value(self, code: u8) -> i32:
        if code >= 65 && code <= 90:
            i32(code) - 65
        else if code >= 97 && code <= 122:
            i32(code) - 71
        else if code >= 48 && code <= 57:
            i32(code) + 4
        else if code == (if self.is_url(): 45 else: 43):
            62
        else if code == (if self.is_url(): 95 else: 47):
            63
        else:
            -1
```

Proposed: identical to the input above.

Closure argument, from `http.hd:449`. A short closure stays flat; its containing
function body remains on its own line.

Before and proposed output are identical:

```text
impl Headers:
    pub fn remove(&mut self, name: HeaderName) -> void:
        self.pairs = self.pairs.filter(fn(pair): pair._0 != name)
```

Comment after an inner suite, verbatim `encoding.hd:23`. Its source column
keeps it in the containing function.

Before and proposed output are identical:

```text
pub fn hex_encode(bytes: List[u8]) -> string:
    let mut codes: List[u8] = zeroes(bytes.len() * 2)
    let index: usize = 0
    while index < bytes.len():
        byte := bytes[index]
        codes[index * 2] = hex_symbol(byte >> 4)
        codes[index * 2 + 1] = hex_symbol(byte & 15)
        index += 1
    # Every written byte is an ASCII hex digit.
    string_from_bytes(codes)
```

Suite-tail adversary **derived from** that std excerpt: move the same comment
after the final expression. This exact order is not claimed to occur in std.
The note must remain in the function at EOF.

Before and proposed output are identical:

```text
pub fn hex_encode(bytes: List[u8]) -> string:
    let mut codes: List[u8] = zeroes(bytes.len() * 2)
    let index: usize = 0
    while index < bytes.len():
        byte := bytes[index]
        codes[index * 2] = hex_symbol(byte >> 4)
        codes[index * 2 + 1] = hex_symbol(byte & 15)
        index += 1
    string_from_bytes(codes)
    # Every written byte is an ASCII hex digit.
```

Builder chain, from `error.hd:155`; the first complete call stays the head.

Before:

```text
impl Debug for ContextError:
    fn debug(self, out: *mut DebugWriter) -> void:
        out.debug_struct("ContextError").field("message", self.message).field("source", self.error.to_string()).finish()
```

Proposed:

```text
impl Debug for ContextError:
    fn debug(self, out: *mut DebugWriter) -> void:
        out.debug_struct("ContextError")
            .field("message", self.message)
            .field("source", self.error.to_string())
            .finish()
```

Function-valued field call, from `iter.hd:224`; never turn it into a method call
or join its function suite.

Before:

```text
impl[T] Iterator[T] for FromFn[T]:
    fn next(&mut self) -> T?:
        (self.step)()
```

Proposed:

```text
impl[T] Iterator[T] for FromFn[T]:
    fn next(&mut self) -> T?:
        (self.step)()
```

A Go [request-construction excerpt](https://github.com/golang/go/blob/6c49ac311e5642fa5399b46c736295a9dc4eeec9/src/net/http/request.go#L919)
translated into hd syntax still exercises ordinary long-record layout; this is
not a type-checked port. Before:

```text
fn request(method: string, url: Url, body: Body):
    Request { method: method, url: url, proto: "HTTP/1.1", proto_major: 1, proto_minor: 1, header: {}, body: body, host: url.host }
```

Proposed:

```text
fn request(method: string, url: Url, body: Body):
    Request {
        method: method,
        url: url,
        proto: "HTTP/1.1",
        proto_major: 1,
        proto_minor: 1,
        header: {},
        body: body,
        host: url.host,
    }
```

A previously vertical short call must join; its function body stays indented:

```text
fn compact():
    consume(
        value,
    )
```

```text
fn compact():
    consume(value)
```

Suite-header adversary, derived from std's iterator/header patterns. Every
header kind uses a complete continuation wrapper; dot lines never become suite
layout. Before:

```text
fn headers(source: Source):
    if source.configuration_for_the_current_authenticated_application().has_valid_authorization_for_the_selected_request():
        pass
    while source.configuration_for_the_current_authenticated_application().has_remaining_requests_for_the_selected_application():
        pass
    for item in source.configuration_for_the_current_authenticated_application().pending_requests_for_the_selected_application():
        pass
    match source.configuration_for_the_current_authenticated_application().response_status_for_the_selected_application():
        _ => pass
```

Proposed broken layout (when the header exceeds width):

```text
fn headers(source: Source):
    if (
        source.configuration_for_the_current_authenticated_application()
            .has_valid_authorization_for_the_selected_request()
    ):
        pass
    while (
        source.configuration_for_the_current_authenticated_application()
            .has_remaining_requests_for_the_selected_application()
    ):
        pass
    for item in (
        source.configuration_for_the_current_authenticated_application()
            .pending_requests_for_the_selected_application()
    ):
        pass
    match (
        source.configuration_for_the_current_authenticated_application()
            .response_status_for_the_selected_application()
    ):
        _ => pass
```

Pipe adversary uses leading pipes, never leading dots after a same-depth pipe.
The long bare names make the complete pipe exceed 100 columns.
Before:

```text
fn double_the_current_authenticated_request_count(value: i32) -> i32: value + value
fn increment_the_current_authenticated_request_count(value: i32) -> i32: value + +1
fn pipeline() -> i32:
    +1 |> double_the_current_authenticated_request_count |> increment_the_current_authenticated_request_count
```

Proposed broken layout, also retaining function bodies on their own lines:

```text
fn double_the_current_authenticated_request_count(value: i32) -> i32:
    value + value
fn increment_the_current_authenticated_request_count(value: i32) -> i32:
    value + +1
fn pipeline() -> i32:
    +1
        |> double_the_current_authenticated_request_count
        |> increment_the_current_authenticated_request_count
```

After a rename shortens a binary chain, canonical formatting removes only its
approved optional wrapper. Before:

```text
fn sum(a: i32, b: i32) -> i32:
    result := (a + b)
    result
```

Proposed:

```text
fn sum(a: i32, b: i32) -> i32:
    result := a + b
    result
```

| Adversary | Required handling |
| --- | --- |
| An inline function containing a direct if/match | No flat suite offered; obey existing inline/continued grammar. |
| Deeply nested/EOF tail comments, including the gap before else | Source-column container plus ordered boundary slot; never drift to the next declaration. |
| `(handler.callback)(event)` versus `handler.callback(event)` | Preserve authored callee parentheses in both printing and tree comparison. |
| Empty groups/indents and zero-width tags before a long token | Collapse empties in the builder; assert canonical IR and bounded fit work. |
| `consume(value # note` followed by `)` | Put a generated comma before the suffix flush. |
| Multiline/interpolated strings with comments | Copy opaque bytes once, including interior comments. |
| Singleton tuple, mixed operators, 100-column token plus punctuation | Preserve grammar/precedence; recognize unavoidable overflow. |

Parse log: all 20 `text` blocks parse cleanly with the frozen current parser. No type-check or runtime equivalence is
claimed by these parser probes; source-owner and verdict checks are separate gates.

## B. Prior art, with code

Studied **oxc `8e00068a`** and **Ruff `ba65abf8`** from their actual Rust
sources before choosing this design. Both separate language rules from a
Rome/Biome-derived document engine. That architecture does not itself prove
a linear worst case; their fit walkers can inspect alternate layouts.

### Oxfmt

The text API parses once; its AST API accepts a pre-existing program.
Node adapters dispatch to small formatting rules. This actual array-rule
fragment builds IR rather than changing source:

```rust
let elements = ArrayElementList::new(&array_elements, group_id);
write!(
    f,
    group(&soft_block_indent(&elements))
        .with_group_id(Some(group_id))
        .should_expand(should_expand)
);
```

Sources: [AST-in entry](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter/src/lib.rs#L242),
[array rule](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter/src/print/array_expression.rs#L24),
[generated adapters](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter/src/ast_nodes/generated/format.rs#L809).
Take parser reuse and node-specific rules internally. Defer a public AST-in API
until another consumer exists; no second AST or parser in the printer.

The core is now `oxc_formatter_core`, not only `oxc_formatter`.
It has arena-backed elements, borrowed text and cached text widths, groups,
indent tags, soft/hard lines, conditional content and suffix comments.
Take that vocabulary and borrowed text; use hd's safe indexed `Vec` arena,
rather than copying its packed pointer representation.
[IR](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter_core/src/format_element/mod.rs#L24).

Its printer reuses scratch queues and fast-paths nested groups under a flat
parent. `best_fitting` can try increasingly expanded alternatives. Fragment
memoization avoids rebuilding IR, not arbitrary fit scans. Take reusable storage
and the parent-flat rule; omit alternative search to keep hd's bound explicit.
[Printer](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter_core/src/printer/mod.rs#L237),
[memoization](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter_core/src/write/extensions.rs#L1).

Oxc **does not preattach comments to a node map**. It advances a cursor over
sorted parser comments and derives leading/trailing/dangling roles by source
positions. That saves a traversal/allocation but makes ownership queries depend
on printing state unless they explicitly inspect all comments. Prefer immutable
attachment for hd: one extra linear pass buys repeatable ownership checks.
[Comment design and cursor warning](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter/src/formatter/comments.rs#L1).

### Ruff

Python node adapters implement `FormatNodeRule`. A list rule builds separated
items and passes them to the generic engine; this actual fragment shows the boundary:

```rust
let items = format_with(|f| {
    f.join_comma_separated(item.end())
        .nodes(elts.iter())
        .finish()
});
parenthesized("[", &items, "]")
    .with_dangling_comments(dangling)
    .fmt(f)
```

Sources: [list rule](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/src/expression/expr_list.rs#L14),
[AST-in entry](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/src/lib.rs#L148).
Take the syntax/printing boundary; hd's rules handle suites and continuation, not Python's rules.

Ruff's IR also has groups, indentation, soft/hard lines, source slices and
`best_fitting`. Interned fragments use shared slices; forced-break propagation
memoizes them. Fit measurement still walks IR and has a parent-flat fast path.
Take cached summaries and borrowed text, but use a tree of output occurrences
and no unconstrained alternatives or reference-counted document graph.
[IR](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_formatter/src/format_element.rs#L20),
[printer](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_formatter/src/printer/mod.rs#L395),
[document preparation](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_formatter/src/format_element/document.rs#L20).

Unlike oxc, Ruff attaches comments in a source-order pass. Its compact map
stores leading/dangling/trailing ranges without three small vectors per node.
Node wrappers and a whole-file assertion catch missed comments. Take dense
attachment and exactly-once checks, adding hd's opener/doc/suite boundaries.
[Visitor](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/src/comments/visitor.rs#L29),
[map](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/src/comments/map.rs#L8).

### Decisions taken from the study

| Choice | Evidence and hd choice |
| --- | --- |
| Blank lines | Both query source gaps separately from normal wrapping. Adopt bounded gaps; reject Python's language-specific import/definition spacing. [Ruff suites](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/src/statement/suite.rs#L231), [oxc program](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter/src/print/program.rs#L169). |
| Trailing comma | Ruff can force expansion from an existing optional comma. Reject that hidden layout override: short groups must join. [Builder](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/src/builders.rs#L229). |
| Reliability | Ruff fixtures hard-fail parse, idempotence and normalized-AST changes. Oxc asserts a lossless fingerprint but can snapshot known second-pass differences. Take snapshots/corpora; hd idempotence stays a hard failure. [Ruff harness](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/tests/fixtures.rs#L378), [oxc harness](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter_tests/src/harness.rs#L230). |
| Fuzzing | Adopt arbitrary-input idempotence plus valid-grammar/tree/comment checks; no Python-specific normalizer exceptions. [Ruff target](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/fuzz/fuzz_targets/ruff_formatter_idempotency.rs#L11). |
| Measurements | Both formatter microbenchmarks parse outside timing. Report hd text-in total and private parse/build/print costs separately, never compare unlike scopes. [Oxc benchmark](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/tasks/benchmark/benches/formatter.rs#L8), [Ruff benchmark](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_benchmark/benches/formatter.rs#L42). |
| Shared front | Reuse parser/source/token views, not semantic analysis for formatting. Ruff's normal formatter builds syntax/trivia; oxc's semantic dependency is optional code-removal detection. hd keeps type facts in lint's separate request. [Ruff context](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/src/lib.rs#L157), [oxc dependencies](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/crates/oxc_formatter/Cargo.toml#L19). |

### Other prior art

These are architecture sketches/usage examples, not exact source quotations.

```go
// gofmt's approach: a fixed printer policy, no type resolution for formatting.
// Usage: gofmt -w main.go
mode := printer.UseSpaces | printer.TabIndent
cfg := printer.Config{Mode: mode, Tabwidth: 8}
file, err := parser.ParseFile(fset, name, src, parser.ParseComments)
if err == nil { _ = cfg.Fprint(output, fset, file) }
```

Take Go's shared style and syntax-only workflow; retain hd's four spaces and
width rule. [Go rationale](https://go.dev/blog/gofmt),
[pinned gofmt](https://github.com/golang/go/blob/6c49ac311e5642fa5399b46c736295a9dc4eeec9/src/cmd/gofmt/gofmt.go#L51).

```rust
// rustfmt's AST rewrite boundary (sketch).
// Usage: rustfmt file.rs
trait Rewrite {
    fn rewrite(&self, context: &RewriteContext<'_>, shape: Shape)
        -> Option<String>;
}
```

Take explicit width/indent context and extensive fixtures; hd instead builds
one shared document, rather than distributing candidate strings through nodes.
[Rewrite](https://github.com/rust-lang/rustfmt/blob/ad7516ba22245c40faeae4668cc75d89f2550b92/src/rewrite.rs#L16),
[shape](https://github.com/rust-lang/rustfmt/blob/ad7516ba22245c40faeae4668cc75d89f2550b92/src/shape.rs#L120).

```javascript
// Prettier-shaped document construction; usage: prettier --write file.js
const args = join([",", line], values);
const document = group(["(", indent([softline, args]), softline, ")"]);
const output = printDocToString(document, { printWidth: 100 });
```

Take the separation between document construction and width-aware printing;
reject a user-option surface. [Technical explanation](https://prettier.io/docs/technical-details).

```haskell
-- Wadler-style algebra (illustrative): text/concatenation/nesting/group.
args = text "(" <> nest 4 (line <> body) <> line <> text ")"
result = layout (pretty 100 (group args))
```

Take the small algebra; do not assume every implementation of it is linear.
[Wadler's paper](https://homepages.inf.ed.ac.uk/wadler/papers/prettier/prettier.pdf).

```rust
// dprint separates print items from printing (API-shaped sketch).
let mut items = PrintItems::new();
items.push_str("call(");
items.push_str("value)");
let result = Printer::new(items, options).print();
```

Take compact print items and width summaries; omit dynamic condition
reevaluation/savepoints from hd's initial bounded printer.
[Print items](https://github.com/dprint/dprint/blob/77a8efced3bb73c9267ff28e39fdd709fd8877d9/crates/core/src/formatting/print_items.rs#L11),
[printer](https://github.com/dprint/dprint/blob/77a8efced3bb73c9267ff28e39fdd709fd8877d9/crates/core/src/formatting/printer.rs#L13).


## C. Alternatives considered

Keep text splices: smallest patch, but cannot provide canonical output or remove
reparse-per-break costs. Port all Ruff/oxfmt infrastructure: proven machinery,
but alternatives, language-specific state and dependencies exceed hd's first need.
Use a universal `best_fitting`/alignment engine: more layouts and potentially
repeated exploration; one fixed flat/broken choice is easier to bound and test.
Start with a generic formatter plugin: introduces another parser/model boundary
without supplying hd's indentation or doc-comment rules.

## D. Today in hd

Measured on `3b0e4cb4b`, native Rust **dev, opt-level 1**, with the root's
exact locked dependency versions. Files were read before timing; one excluded
warmup preceded nine in-process passes. Calls use `black_box`; medians below
include parse/build work and exclude file IO/CLI startup. Corpus timing also
includes CSV row emission. Load went 5.34 → 4.82; unrelated work was present,
so these establish scale and shape, not a promised speedup. Scratch preserves
all raw rows, hashes, tool versions and process snapshots.

| Slowest file | Bytes | Current median ms |
| --- | ---: | ---: |
| `http.hd` | 76,489 | 332.015 |
| `json.hd` | 100,250 | 199.148 |
| `regex.hd` | 69,822 | 121.078 |

The full 42-file raw CSV and source hashes remain in scratch; the initial PR
revision also records the complete per-file table. The corpus total is 935.072 ms.

Five samples per generated file, each with independent long calls and no
comments; no filesystem work was timed:

| Calls | Bytes | Current median ms |
| ---: | ---: | ---: |
| 16 | 2,518 | 1.896 |
| 32 | 5,046 | 7.331 |
| 64 | 10,102 | 30.845 |
| 128 | 20,242 | 138.186 |
| 256 | 40,594 | 727.125 |

Current `format` parses in `lib.rs`; `width::layout` reparses and tries splices
until none succeeds. Three spacing/wrapping variants of `call(a, b)` remain
three distinct outputs, although each is already idempotent. Existing coverage
in `crates/hd_fmt/tests/width.rs` includes exact 100 columns, closures, literal
protection, precedence and std projection; retain these regressions.
`hd_web/Cargo.toml` depends on `hd_syntax` but not `hd_fmt` or `hd_lint`.

The initial scratch harness selected newer Unicode dependencies; that receipt
is retained as a preflight. It was corrected to the root lock before reported
measurements. A 35-file top-level-only inventory is also retained separately;
the table here uses **all 42 recursive std files**, including runtime modules.


## E. Delivery and checks

This is an architecture change over existing grammar, with no new syntax,
intrinsic, type-system exception or std API. Proposed normalization belongs in
the CLI spec after approval, with new IDs for changed meanings; no rule is edited here.

The nine current [width rules](../../spec/cli/command-line.md#formatting) map as follows:

| Current ID suffix (`cli.fmt.width.`) | Document rule / accounting at implementation |
| --- | --- |
| `wrap` | Replace with a new ID specifying canonical fit, scalar columns, excluded trailing-comment text and unavoidable overflow. |
| `elements` | Preserve: broken literal/call/parameter/list groups use element lines. |
| `comma-groups` | Preserve: share the same comma-group constructor across syntax families. |
| `indent` | Preserve: structural containing-line indent plus four. |
| `close` | Preserve: delimiter on its own line at the containing-line level. |
| `binary-grouping` | Preserve: syntax-aware continuation parentheses. |
| `binary-operands` | Preserve: operand lines with leading operators. |
| `protected` | Preserve: literal/comment text is verbatim. |
| `trailing-comma` | Preserve for broken groups; add a distinct new rule for removal of optional flat commas. |

New CLI IDs will also specify canonical spacing, joining short groups, suite
layout, preserved call-target parentheses, chain breaks and blank-line policy. Retire only
changed-meaning IDs in the implementing commit, with fixtures; this design
retires none. Preserve `cli.fmt.meaning`, `.idempotent`, `.syntax-error`, `.no-check`
and the existing command contract.

Proposed private IR sketch (Rust; not an implemented API):

```rust
struct Document<'s> {
    source: &'s str,
    tape: Vec<Command>,
    groups: Vec<GroupInfo>, // matching ends, exact flat widths, cannot_flatten
    next_flat: Vec<TapeIdx>,     // only remaining effective-event gaps
    next_expanded: Vec<TapeIdx>,
}
enum Command {
    Text(Span), Verbatim(Span), Atom(&'static str),
    StartGroup(GroupId), EndGroup, StartIndent, EndIndent,
    SoftLine(FlatSpace), HardLine, LineSuffix(CommentId),
    ModeText { flat: &'static str, broken: &'static str },
}
```

The builder omits/collapses groups and indents with empty effective bodies and
coalesces only redundant no-op/break commands, preserving authored blank-line and
comment boundaries. Assert this in tests; never emit
`Group(empty)` as printable IR. The printer and fitter consume this balanced tape with matching group-end
indices and one parent per nonleaf. `ModeText` has static punctuation only;
there are no arbitrary alternatives, cross-group conditions or shared nonleaf DAGs.
Cache each emitted text leaf's width once (capped at 101), plus literal first-line
width/newline information. Derive exact flat widths and non-flattenable flags
from those same leaves, not a separate syntax-level width prediction.

A candidate with an internal required newline cannot flatten. Otherwise fit it
flat, then measure its actual line continuation: enclosing broken groups retain
their mode, unselected later groups are provisionally expanded, and measurement
stops at the first effective break. Ignore suffix-comment bytes, but include
punctuation before their flush. A trailing suffix outside the group does not
force its body to break. Multiline verbatim is copied without reindentation.

Precompute per-mode next-effective-event jumps over zero-width tags/runs and
matching ends. Each fit jumps to a positive-width event, break, rejection or end;
it never clones a queue/stack or rescans literal bytes. Any mode transition
required by a jump is encoded there, not replayed through skipped tags. At width 100 it consumes
at most 101 positive-width events plus constant jump work. Fit each group at
most once; inherit flat mode under a verified flat parent. This yields
O(N + 100G + output) work and O(N) space, subject to the actual implementation
meeting these invariants. A cutoff **without** zero-run jumps is insufficient:
`Group(empty)` repeated before one overlong token otherwise causes quadratic
failed fits in an unnormalized IR. Feed that input through the builder and
assert collapse. Compare optimized fits against a simple reference measurer for
all modes, suffixes and continuations, not only empty-group adversaries.

The parser's 160-level syntax limit does not bound document depth: postfix
and same-precedence binary parsing build nested nodes in loops. Empty removal
alone therefore does not replace zero-event jumps or justify a new IR depth cap.

Instrument preprocessing/printing visits ≤ fixed constants × N, fit calls ≤ G,
and effective fit events ≤ 101 × fit calls plus fixed setup. Charge output
bytes separately. Exercise builder empty collapse, surviving zero tags, deep hard-line groups, cached
long tokens, 100/101 boundaries and long ignored comments; doubling alone is
not a substitute for these absolute bounds.

The wrapper whitelist is exactly a `ParenExpr` directly enclosing a binary,
postfix or pipe chain, in a binding RHS, return, expression statement, argument,
binary operand or suite-header condition/iterable/matchee. Removal also requires
precedence/associativity and comment ownership to remain unchanged. Callees,
standalone names/literals, tuples and every other parenthesis position are out.
Compute canonical width without this optional wrapper: keep/add one only when
continuation requires it. This handles both source-authored and previously-added
wrappers identically, without hidden provenance. Never wrap a bare pipe step/path;
parenthesize a complete pipe only in an approved outer slot. Existing authored
placeholder-step parentheses remain intact; no new per-step wrapper is introduced.

A paired-tree oracle allows **only** insertion/removal of those proven wrappers;
all other `ParenExpr` nodes and operator/statement nesting remain exact. Preserve
call-target groups regardless of width. Check verdict/codes as well as parsing:
parenthesizing `x |> f`'s bare `f` changes pipe semantics, and splitting an inline
pipe-step suite can introduce `multi-line-pipe-step`. Neither is allowed.

Standalone comment placement uses the innermost compatible suite/container and
source-ordered slot, including before `else`, after its last statement and EOF.
Compare columns before canonical indentation changes; do not make indentation a
setting or blindly send every comment to the next statement. The pinned Ruff
[branch-tail algorithm](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_python_formatter/src/comments/placement.rs#L701)
informs this, without copying Python-specific branches. Syntax-node rules must
check hd's inline/continued contexts before offering a flat suite. A flat suite
never permits a nested group's hard break; preserve closure separator dedents.

Implementation slices are about one hour each, each reviewed and tested:

1. Private source/comment ownership and node-coverage inventory; round-trip and doc/tail-comment tests.
2. Document/printer/fits kernel; reference equivalence, width boundaries, zero-tag adversaries and visit counters.
3. Expression/comma/chain rules; whitespace variants, preserved call targets and approved continuation wrappers.
4. Approved suite/conditional style and comments; all std and every parseable fixture in memory.
5. Parallel CLI integration using existing --jobs; spec/fixtures, fuzz fmt contract and hard cost budgets.
6. A separately approved #899 source-only reformat, preserving excluded raw/stage inputs and refreshing SDK provenance.

Reuse the parser's existing size/nesting limits; bound arena entries per syntax
node and use checked IDs. There are no new user budget knobs.
Each slice retains failed inputs. Gate idempotence and normalized-tree equality
as errors, not known differences. Tree comparison ignores positions/layout
and optional separator commas,
and permits only the listed continuation-wrapper insertions/removals. It preserves
all other parentheses, statement nesting, expression/operator structure and literal bytes.
Separately compare every comment's bytes, order, kind and semantic owner.
Test every production, empty/singleton/multiline group, Unicode, tabs/CRLF,
comments at every boundary, incomplete syntax, and nesting/size limits.
Format **every indexed conformance input that parses** in memory, preserving
its package/tree/test environment. Require the same check verdict and diagnostic
codes before/after, in addition to tree/comment equality and idempotence. Do not
rewrite fixture sources or checked baselines for this gate. Current parse-error
inputs stay untouched. Verdict checks supplement the tree oracle.

Extend the existing `spec/tools/fuzz` command adapter with a proposed `fmt FILE`
action: copy to an owned same-role input, run existing `hd fmt` on the copy and
return its bytes. This introduces no public stdout/style option. For accepted
parses, format → reparse → format again; compare exact second-pass text, tree,
comment ownership and original/formatted check verdict/codes. Keep type-invalid
inputs too; formatter syntax-only behavior must preserve their rejection codes.
This adapter/step is future implementation work, not an already-run gate.

Add deterministic operation-count bounds as well as timing; doubling input must
not hide quadratic work behind a small corpus. Never bless unrelated stage baselines.

`hd fmt FILE`, package selection, text/JSON diagnostics, error statuses and
`--check` remain as specified. Syntax errors leave that file untouched while
other files format. `--check` writes nothing; no settings or manifest keys are added.
There is no semantic cache invalidation or import sorting. The library's per-file
engine stays serial; CLI package formatting uses existing
bounded --jobs execution from its integration slice, including --jobs 1 for the
serial cost budget. Deduplicate selected paths; buffer reporting and sort by path
so scheduling changes neither bytes, statuses nor diagnostic ordering.
Keep the old formatter until the complete replacement's gates pass, then remove
it; do not ship two selectable styles. The bulk reformat waits for that boundary.
