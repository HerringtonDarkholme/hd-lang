# hd fmt

Proposal, 2026-10-26; domain: formatter internals and CLI layout; not accepted.
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

## Problems it solves

| Input today | Proposed result |
| --- | --- |
| `call( a,b )` and `call(a, b)` remain different | One canonical spacing rule for calls. |
| A short call previously broken across lines stays broken | Rejoin it when the canonical form fits. |
| Every additional overlong call triggers whole-file reparses | One parse and linear document work for the complete file. |
| Moving an opening-line comment can move a diagnostic annotation | Attach it explicitly and test its boundary, rather than protecting byte ranges. |

## How it works

Keep `hd_fmt::format` and lossless `round_trip`. Add a borrowed parsed entry
so editors and lint can reuse the existing syntax tree. Proposed Rust signatures:

```rust
pub fn format(source: &str) -> Result<String, Vec<Diagnostic>>;
pub fn format_parsed(input: FormatInput<'_>) -> Result<String, Vec<Diagnostic>>;
fn attach_comments(view: SourceView<'_>) -> CommentMap;
fn build<'s>(input: &FormatInput<'s>) -> Document<'s>;
fn print(document: &Document<'_>) -> String;
```

`format` parses once; syntax errors return before layout. `FormatInput` binds
one source/parse snapshot and its optional comment map. `Document` borrows that
source; no caller can print spans against another file. Cached-parse adapters
require matching source provenance, not equal lengths. Node IDs survive only
within that exact tree. Exhaustive per-kind rules cover every valid syntax kind.

The document uses a safe `Vec` arena and integer IDs, borrowing source text.
It has groups, concatenation, indentation, soft/hard lines, verbatim leaves,
suffix comments and conditional punctuation. Each group chooses flat or broken
once using cached, width-saturated summaries and mandatory suffix reservations.
Soft lines flatten or break; suites always use hard lines. Appendix E defines
the summaries; arbitrary `best_fitting`, backtracking and reparse loops are absent.
The bound is O(input + output), O(input) storage, with iterative traversal.
Output size matters when nested same-line suites expand into indentation text.

Attach leading/trailing/dangling comments in one source-ordered pass over existing
spans. Preserve authored inline/standalone roles, opener comments, doc ownership,
module-doc classification and exact comment bytes. Parent punctuation precedes
suffix-comment flushes. Multiline strings/interpolations are verbatim; their
interior comments are already emitted. Attachment checks are independent of printing.

The fixed style normalizes operator/assignment/keyword spacing, comma/colon
spacing, and delimiter interiors. Nonempty flat struct/map braces have one
interior space. Four-space suites are vertical; no neighbouring-column alignment.
Preserve unary signs, references, bang calls, spread/range tokens and required
lexical separation. Width counts Unicode scalars, matching today.

Join fitting groups; input wrapping or optional commas never force expansion.
Emit optional trailing commas only in broken groups; preserve singleton tuples.
Normalize redundant expression parentheses before layout using precedence and
associativity; retain delimiters needed for comments. Generated continuation
parentheses therefore do not accumulate on the second pass.

Preserve literal/numeric spelling and item order. Keep at most one authored
blank line between siblings, none at suite boundaries, except where doc ownership
requires it. Generated layout uses LF and one final newline; blank files become
empty. Literal/comment bytes retain their original line endings and whitespace.

Broken binary chains use continuation parentheses and leading operators.
An indented argument suite ends before its standalone separator comma, emitted
at the argument-header level; its closer starts another line. Indivisible tokens,
unbreakable syntax runs and unavoidable indentation may exceed 100 columns.

Share the tree, tokens and an **optional neutral source/comment view** in
`hd_syntax` with `hd_lint`; never share the printer or request types for formatting.
No attachment allocation or traversal occurs when neither tool requests it.
#908 separately designs lint's checked-body view; it does not enter this pipeline.

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
including every std file and doubled adversarial inputs. Publish per-file and
corpus times, allocations, maximum memory, and raw/gzip sizes before rollout.

## A. Stress test

Two library excerpts are translated into hd syntax for layout only; they are
not ports or type-checked packages. Both input and proposed output are parsed
with hd's existing parser. The new layout is hand-specified, not executed.

[Go's request constructor](https://github.com/golang/go/blob/6c49ac311e5642fa5399b46c736295a9dc4eeec9/src/net/http/request.go#L919)
exercises a long record with nested expressions. Before:

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

[CPython's encoder setup](https://github.com/python/cpython/blob/0e353134844fd50628f3730b9f704c3de1710e20/Lib/json/encoder.py#L258)
exercises a long call whose argument order must survive. Before:

```text
fn encoder():
    make_iterencode(markers, default, encoder, indent, floatstr, key_separator, item_separator, sort_keys, skipkeys, one_shot)
```

Proposed:

```text
fn encoder():
    make_iterencode(
        markers,
        default,
        encoder,
        indent,
        floatstr,
        key_separator,
        item_separator,
        sort_keys,
        skipkeys,
        one_shot,
    )
```

The shorter counterpart must join back, regardless of its input comma:

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

An indented closure exposed a grammar constraint: the separator must be
outside its body. This proposed output is accepted by today's parser:

```text
fn nested():
    consume( # opening marker
        fn(value):
            use_value(value)
        ,
    )
```

| Adversarial case | Finding and required handling |
| --- | --- |
| 16–256 independent overlong calls | Existing elapsed time grows much faster than input; instrument bounded document visits, not just a timeout. |
| `consume(value # note` followed by `)` | Adding a comma after flushing the comment swallows it; punctuation precedes suffix flush. |
| Nested calls followed by `)?`, `:` or a postfix | Child fit alone can overflow; reserve mandatory suffixes through the next legal break. |
| Same-line nested `if`/`else`, match arms and function bodies | Whitespace changes can change suite ownership; print structural suites and compare trees. |
| `(value)` versus `(value,)`, and mixed-precedence operators | Dropping punctuation indiscriminately changes meaning; tuple/precedence checks are mandatory. |
| Module docs, item docs and blank lines | Doc ownership can change without token changes; preserve attachment separately. |
| Multiline/interpolated strings containing comments | Treat the whole literal as verbatim; do not emit interior comments twice. |
| Long identifiers, Unicode and deeply nested suites | Define scalar columns and unavoidable overflow; traversal is iterative and cost includes output size. |

Parse log: all 8 `text` blocks accepted by `scripts/hd-conformance.sh parse` on the recorded base. Syntax acceptance alone does not establish
idempotence or tree preservation by the proposed formatter; those are rollout gates.


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
Take parser reuse and node-specific rules: no second AST, no parser in the printer.

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
| Measurements | Both formatter microbenchmarks parse outside timing. Report hd text-in and AST-in costs separately, never compare unlike scopes. [Oxc benchmark](https://github.com/oxc-project/oxc/blob/8e00068a158ec8cc17a9b4ad8b7189714d36a887/tasks/benchmark/benches/formatter.rs#L8), [Ruff benchmark](https://github.com/astral-sh/ruff/blob/ba65abf8ba7bfbcc059f7fb9712be5ba9cab6abc/crates/ruff_benchmark/benches/formatter.rs#L42). |
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

| File | Bytes | Current median ms |
| --- | ---: | ---: |
| `annotation.hd` | 3,954 | 0.077 |
| `cli.hd` | 33,335 | 67.319 |
| `cmp.hd` | 25,564 | 4.778 |
| `collections.hd` | 42,147 | 13.075 |
| `console.hd` | 9,664 | 0.269 |
| `convert.hd` | 488 | 0.012 |
| `digest.hd` | 9,143 | 4.024 |
| `encoding.hd` | 8,473 | 0.878 |
| `error.hd` | 7,613 | 0.433 |
| `format.hd` | 42,662 | 15.130 |
| `fs.hd` | 26,086 | 18.276 |
| `function.hd` | 888 | 0.012 |
| `hash.hd` | 22,641 | 3.237 |
| `host.hd` | 11,244 | 1.526 |
| `http.hd` | 76,489 | 332.015 |
| `http_wire.hd` | 994 | 0.060 |
| `inspect.hd` | 2,365 | 0.061 |
| `iter.hd` | 17,046 | 2.036 |
| `json.hd` | 100,250 | 199.148 |
| `net.hd` | 23,242 | 18.480 |
| `num.hd` | 37,764 | 6.543 |
| `ops.hd` | 28,846 | 30.327 |
| `option.hd` | 2,788 | 0.112 |
| `path.hd` | 5,411 | 0.207 |
| `prelude/testing.hd` | 456 | 0.009 |
| `prelude.hd` | 1,857 | 0.183 |
| `process.hd` | 10,879 | 0.314 |
| `random.hd` | 9,369 | 0.380 |
| `regex.hd` | 69,822 | 121.078 |
| `result.hd` | 2,782 | 0.116 |
| `rt/alloc.hd` | 18,557 | 0.856 |
| `rt/checked.hd` | 6,241 | 0.503 |
| `rt/checked_report.hd` | 1,201 | 0.149 |
| `rt/cycle.hd` | 14,554 | 1.269 |
| `rt/memory.hd` | 6,669 | 0.178 |
| `serde.hd` | 28,688 | 10.607 |
| `structure.hd` | 14,022 | 0.317 |
| `task.hd` | 21,538 | 9.678 |
| `testing/arbitrary.hd` | 1,346 | 0.031 |
| `testing.hd` | 34,916 | 36.132 |
| `text.hd` | 63,252 | 26.630 |
| `time.hd` | 34,176 | 9.732 |

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
| `wrap` | Replace with a new ID specifying canonical fit, column units and unavoidable overflow. |
| `elements` | Preserve: broken literal/call/parameter/list groups use element lines. |
| `comma-groups` | Preserve: share the same comma-group constructor across syntax families. |
| `indent` | Preserve: structural containing-line indent plus four. |
| `close` | Preserve: delimiter on its own line at the containing-line level. |
| `binary-grouping` | Preserve: syntax-aware continuation parentheses. |
| `binary-operands` | Preserve: operand lines with leading operators. |
| `protected` | Preserve: literal/comment text is verbatim. |
| `trailing-comma` | Preserve for broken groups; add a distinct new rule for removal of optional flat commas. |

New CLI IDs will also specify canonical spacing, joining short groups, suite
layout, redundant-parenthesis normalization and blank-line policy. Retire only
changed-meaning IDs in the implementing commit, with fixtures; this design
retires none. Preserve `cli.fmt.meaning`, `.idempotent`, `.syntax-error`, `.no-check`
and the existing command contract.

Proposed private IR sketch (Rust; not an implemented API):

```rust
struct Document<'s> {
    source: &'s str,
    nodes: Vec<Element>,
    edges: Vec<DocId>,
    summaries: Vec<Summary>,
    root: DocId,
}
enum Element {
    Text(Span), Verbatim(Span), Atom(&'static str),
    Concat(EdgeRange), Indent(DocId), Group(DocId),
    SoftLine(FlatSpace), HardLine, LineSuffix(CommentId),
    ModeText { flat: &'static str, broken: &'static str },
}
```

`ModeText` has only static punctuation, never two alternative body documents.
For each mode, `Summary` caches flat width capped at 101, first/last unbreakable
widths and break flags. Hard lines, multiline verbatim leaves and mandatory
suffix-comment flushes prevent flattening. Concatenation sums flat widths; a prefix stops where a
child can break, otherwise it extends into the next child's prefix. Suffixes
compose symmetrically. Parent and child builders declare legal boundaries,
including suffix-comment flushes; no generic scan tries to discover them.
A reverse sequence pass computes every suffix reservation once: reserve mandatory
punctuation/unbreakable prefixes only through the next independent legal break,
not the flat widths of all following groups. A flat parent forces descendants
flat; a broken parent lets nested groups choose at their actual columns.
Every node/edge has bounded visits and each output byte is written once.
No hidden recursive fit scan may substitute for these summaries.
Verbatim leaves update the column from their final physical line, without
injecting indentation. A suffix queue flushes after punctuation and before
further code; it marks the relevant group unflattenable. Embedded interpolation
comments are excluded from attachment, rather than emitted twice.
A nonleaf `DocId` has exactly one parent, enforced by the builder and an
invariant test; shared zero-output DAGs cannot cause exponential visits.
Standalone comments attach before the next source node within their container,
including argument/element nodes; with no following node they dangle there.




Implementation slices are about one hour each, each reviewed and tested:

1. Source/comment view and node-coverage inventory; round-trip and doc ownership tests.
2. Small document/printer kernel; exact snapshots, fixed-width boundary tests, node-visit counters.
3. Expression/comma-group rules; normalize whitespace variants to identical output and test precedence.
4. Suites, declarations, patterns and comments; complete syntax coverage, then all std files.
5. CLI integration and approved spec/fixtures; compare old/new cost and publish before any bulk reformat.
6. A separately approved #899 source-only reformat, preserving excluded raw/stage inputs and refreshing SDK provenance.

Reuse the parser's existing size/nesting limits; bound arena entries per syntax
node and use checked IDs. There are no new user budget knobs.
Each slice retains failed inputs. Gate idempotence and normalized-tree equality
as errors, not known differences. Tree comparison ignores positions, layout,
optional separator commas and demonstrably transparent continuation parentheses;
it preserves statement nesting, expression/operator structure and literal bytes.
Separately compare every comment's bytes, order, kind and semantic owner.
Test every production, empty/singleton/multiline group, Unicode, tabs/CRLF,
comments at every boundary, incomplete syntax, and nesting/size limits.
Grammar-generated fuzz cases and whitespace variants must keep these properties.
Add deterministic operation-count bounds as well as timing; doubling input must
not hide quadratic work behind a small corpus. Never bless unrelated stage baselines.

`hd fmt FILE`, package selection, text/JSON diagnostics, error statuses and
`--check` remain as specified. Syntax errors leave that file untouched while
other files format. `--check` writes nothing; no settings or manifest keys are added.
There is no semantic cache invalidation or import sorting. Independent-file
parallelism can follow measured need; the initial per-file engine stays serial.
Keep the old formatter until the complete replacement's gates pass, then remove
it; do not ship two selectable styles. The bulk reformat waits for that boundary.
