# New Compiler Design: Front-Half Syntax Stages

Part of the [compiler design](README.md).

## 4. Front-Half Stages

### 4.1 Lexer

**Job.** Turn bytes into significant tokens, comments and line facts, in
one pass, pulled on demand by the parser.

```rust
pub struct TokenBuf {
    pub kind: Vec<TokenKind>,      // u8; significant tokens only
    pub start: Vec<u32>,           // byte offsets
    pub end: Vec<u32>,
    pub line_first: BitVec,        // the token is the first on its physical line
    // per physical line: start, first token, indent, flags; comments; decoded values
}
```

The full layout is [data-structures.md §3.11](data-structures.md#311-tokens-and-line-tables).
Indentation is a per-line column, and the layout cursor tracks bracket
depth itself, so a token is 9 bytes.

- **Lossless without trivia tokens.** Whitespace is the gap between
  tokens. Comments are a side list. Tokens, gaps and the original text
  reproduce the file byte for byte, which slice 1 tests on every file.
- **Modes.** The lexer keeps a frame stack: `Code { bracket_depth }` or
  `Str { delimiter, raw, prefix }`. Interpolation `${...}` pushes a code
  frame inside a string frame. Multiline `"""`, raw strings and prefixed
  strings are string frames. The same stack serves skim mode (§4.3), so
  the two cannot disagree (the TypeScript `preProcessFile` lesson).
- **Fast path.** ASCII bytes go through a byte-class table. Non-ASCII
  identifiers are checked for NFC with a quick-check table only when they
  occur ([`lex.encoding.scalars`](../../spec/lang/01-lexical-structure.md#r-lex.encoding.scalars)).
- **Errors never stop it.** Invalid UTF-8, a bare CR, a tab in
  indentation, a stray BOM, and an unterminated single-line string (which
  ends at its line end) each produce one diagnostic and an `Error` token,
  then lexing continues.
- **Precomputed classes (Yuku).** Token kinds are numbered so that
  "operator", "keyword" and "starts an expression" are range or bit
  tests, and a 256-entry table gives each kind's precedence and flags in
  one load. Keywords are found by a perfect hash on the first byte, the
  second byte, the last byte and the length: one probe, one length
  compare, one short compare.
- **No early decoding.** Identifier and literal tokens are spans. Escapes,
  numeric values and NFC checks run only when the parser or the checker
  asks for the value, into a small pool of decoded exceptions.
- **Complexity.** O(n) time, 9 bytes per significant token plus 11 per
  line.
  Lexing and parsing throughput are measured separately (the Carbon
  lesson), so a regression points at its phase.

### 4.2 Layout

**An input inconsistency, resolved.** The research says no layout decision
needs the parser ([Q4](research.md#q4-parser-and-cst)).
The spec says layout and parsing cooperate at a suite-introducing colon
([`lex.colon.cooperate`](../../spec/lang/01-lexical-structure.md#r-lex.colon.cooperate)),
and `SUITE_END` boundaries need parser-aware processing
([`lex.suite-end.no-spelling`](../../spec/lang/01-lexical-structure.md#r-lex.suite-end.no-spelling)).
Both are partly true. At bracket depth zero, Python's indentation stack
decides everything. Inside brackets, and for same-line suites, only the
parser knows that a colon opens a suite.

**Design: a layout cursor that the parser drives** (mine in this form).

```rust
pub struct LayoutCursor<'t> {
    toks: &'t TokenBuf,
    pos: TokenIdx,
    indents: SmallVec<[u16; 16]>,      // depth-zero indentation stack
    suites: SmallVec<[OpenSuite; 8]>,  // suites the parser opened
    pending: SmallVec<[Layout; 4]>,    // virtual tokens due before `pos`
}
pub struct OpenSuite {
    kind: SuiteKind,           // Indented | SameLine
    delim_depth: u16,          // bracket depth at the colon
    reference_indent: u16,     // lex.nested.reference
    statement_indent: u16,     // lex.nested.body-depth
    closure: bool,             // the lex.closure.* end rules apply
}
pub enum Layout { Newline, Indent, Dedent, SuiteEnd }

impl LayoutCursor<'_> {
    pub fn peek(&mut self) -> TokenOrLayout;   // computes pending layout lazily
    pub fn bump(&mut self);
    /// Called by the parser exactly when it consumes a suite-introducing colon.
    pub fn open_suite(&mut self, closure: bool);
}
```

1. At depth zero with no suite opened inside brackets, `peek` runs the
   algorithm of [Indentation Levels](../../spec/lang/01-lexical-structure.md#indentation-levels)
   from `line_first` and `indent`. No parser input is needed.
2. `open_suite` decides indented or same-line by whether the next token
   starts a physical line. It records the references that the nested rules
   need ([Suites Inside Delimiters](../../spec/lang/01-lexical-structure.md#suites-inside-delimiters)).
3. A same-line suite closes at a logical line end, at a comma or closing
   delimiter of its depth, or before `else`, innermost first
   ([Same-Line Suites](../../spec/lang/01-lexical-structure.md#same-line-suites)).
   The cursor emits the `SuiteEnd`s; the parser never counts them. An
   `else:` on the next line still attaches after a same-line body. **M2 gap
   2:** the implementation is right; this sentence makes the accepted form
   explicit.
4. Layout tokens enter the green tree's parallel `layout_at` and
   `layout_kind` columns, not its real-token stream. They remain zero-width
   positions between real tokens, so the formatter sees every decision.
   **M2 gap 1:** the implementation is right; making virtual tokens ordinary
   tree tokens would duplicate the compact side columns and blur `TokenIdx`.
5. **Recovery.** `invalid-dedent`: report once, dedent to the nearest
   lower active level, go on. `unexpected-indentation`: report, then treat
   the line as part of the current suite. A closure end-rule violation:
   `syntax-error`, then close the closure's suite at that line.

### 4.3 Skim Mode And The Header Pass

The header pass ([Q4b](research.md#design)) is the
parser's item-level code run with a body policy of `Skip`.

```rust
pub enum BodyPolicy { Parse, Skip }

// In the item parser, where the grammar expects suite_body after a header:
fn suite_body(p: &mut Parser) {
    match p.policy {
        BodyPolicy::Parse => parse_suite(p),
        BodyPolicy::Skip => {
            let range = p.lexer.skip_body(p.header_indent());  // skim mode
            p.skipped_body(range);                             // one SKIPPED_BODY node
        }
    }
}
```

- `skip_body` runs the lexer's frame machine without storing tokens. It
  stops at the first logical line, at bracket depth zero and outside any
  string, whose indentation is at most the header's and which is not blank
  or comment-only. A same-line body ends at its logical line end.
- Kept: template bodies (`impl ... by Structure`), the token ranges of
  parameter, field and shared-data defaults, and fact and decorator
  expressions, parsed as expressions but not resolved.
- Skipped: function and method bodies, `tests:` blocks (their `use` lines
  are kept and marked test-only), top-level statements.
- With answer 13 (the direct-only `block_on` ban), nothing a dependent
  needs comes from a body. There is no drive summary.

```rust
pub struct Skeleton {
    pub source_hash: Hash128,
    pub api_text_hash: Hash128,        // §4.5
    pub uses: Vec<UseDecl>,            // path, group, alias, pub, test_only, span
    pub tree: GreenTree,               // headers, with SKIPPED_BODY nodes
    pub items: Vec<SkelItem>,          // node, kind, name, visibility, body range, parent
    pub doc_comments: Vec<(u32, u32)>, // ranges only; the text stays in the source
    pub broken: bool,                  // some header failed to parse
}
```

- **Exactness.** `skeleton(skim(bytes)) == skeleton(full_tree(bytes))` on
  every fixture and std file is a slice 1 exit test. The research's script
  found the same body ends as the prototype on 2,888 files.
- **Errors.** The header pass reports nothing. A broken header marks the
  item broken (poison) and resynchronizes at the next column-0 line. The
  file is fully parsed when its module is checked, and that parse reports
  the error once.
- **When each runs** follows the table in
  [Q4b](research.md#design): skim for files whose
  module needs no check this run; full parse for changed files, missed
  modules, `hd fmt` and `hd fix`. A fully parsed file derives its skeleton
  from its tree, so no file is read twice.
- **M2 gap 4.** The design stands. M2's `skim` is still a line scanner over
  fully materialized tokens, not this item parser with `BodyPolicy::Skip`.
  The implementation must converge on this path before skim is complete.

### 4.4 Parser And Green Tree

**Parser.** Hand-written recursive descent emitting matklad-style events
(`Start(kind)`, `Token`, `Finish`, `Error`) into one vector, then built into
the flat tree in one pass.

- **Recovery sets.** A `Newline` or `Dedent` ends a statement. A token at
  column 0 always starts a new top-level item, so one broken item never
  swallows the next (`errors-per-run`). Inside brackets, the closing
  delimiter and `,` are the recovery set. Junk is wrapped in an `ERROR`
  node. The parser reports only the first error in a statement. After the
  lexer reports an unclosed delimiter, it suppresses parser diagnostics
  past that opener because later lines remain inside the broken construct.
  **M2 gap 6:** the implementation is right; these bounds prevent cascades
  while preserving recovery into later independent statements.
- **Progress.** Each repetition records its starting token and consumes one
  token into an `ERROR` node if the grammar made no progress. Recursive
  entry increments the shared depth counter. At 160 nested brackets,
  blocks, closures or interpolations, the subtree becomes an `ERROR` node
  with `nesting-too-deep`, and parsing skips its balanced remainder.
  **M2 gap 5:** the implementation is right; local progress guards cover
  parser bugs, while the input-controlled bound gets the specified limit
  diagnostic. A separate parser-fuel counter adds no protection.
- **Operators.** Precedence climbing with an explicit operand and operator
  stack, so a long chain of binary operators uses no native stack.

**Green tree.**

```rust
pub struct GreenTree {
    kinds: Vec<SyntaxKind>,     // u16
    first_token: Vec<u32>,
    last_token: Vec<u32>,       // inclusive
    subtree_len: Vec<u32>,      // preorder: node i's subtree is i .. i + subtree_len[i]
    layout_at: Vec<TokenIdx>,    // zero-width layout positions, in order
    layout_kind: Vec<Layout>,
}
#[derive(Copy, Clone)]
pub struct NodeRef<'t> { tree: &'t GreenTree, idx: NodeIdx }
```

- The full layout, the wire format and the generated JavaScript decoder
  are in [data-structures.md §3.13](data-structures.md#313-the-green-tree-its-wire-format-and-the-js-decoder).
- Preorder with subtree sizes: a child walk is a loop, and skipping a
  subtree is one addition. Parent links are built on demand for fix-its
  and the formatter.
- **Typed views** (`FnDecl`, `MatchExpr`, ...) are generated from one
  grammar file, `hd_syntax/hd.ungram`, by the codegen task. Each view is a
  `NodeRef` with accessors that find children by kind.
- **M2 gap 3.** The design stands. M2 has hand-written generic `NodeRef`
  accessors and no `hd.ungram`; the implementation must generate the named
  Rust and JavaScript views before tools depend on the tree boundary.
- **No red tree and no incremental reparse.** A whole file reparses in
  well under a millisecond at the target speed. An editor tier can add
  them later.
- About 14 bytes per node. With tokens, a 10k-line package's trees take a
  few MB, freed module by module (§3.5).
- **Named fields at the boundary (after Yuku).** The tree is the one IR
  that tools outside the checker read: the formatter, `hd doc`, fix-its,
  the playground's JavaScript and later the program database. They read
  it through the generated typed views, never through raw columns
  (§3.9.6). Storage stays compact; Yuku's 52-byte nodes with named fields
  buy the same ergonomics at four times the memory.
- **Wire format (Yuku).** Tokens, tree columns, comments and line starts
  are position-independent little-endian columns behind a small header.
  Sending a tree from the compiler worker to the page is one copy into an
  `ArrayBuffer`, and the generated JavaScript decoder reads it through
  typed-array views without building objects.
- **Recovery by truncation.** The parser's event vector is append-only.
  A speculative or failed parse truncates it to its checkpoint (§3.9.5).
- **`dyn` types (owner, 2026-10-07; spec change pending as S1d).** `dyn`
  followed by a trait path, its arguments and bindings is a type form, a
  `DYN_TYPE` node, as in `dyn Supplier[Item = i32]`. A bare trait name in
  type position still parses as a named type: only resolution knows that
  the name is a trait, so resolution reports it, with a fix-it that
  inserts `dyn`.
- **No GADT forms.** GADTs are removed (owner, 2026-10-07; spec removal
  pending as S1e), so a variant has no result-type annotation. Until
  S1e lands, the grammar file keeps no such production and chapter 13's
  fixtures are known failures.

### 4.5 Header Extraction And The API Text Hash

Each skeleton carries an **api text hash** (mine, as a cache key input):
the hash of the token kinds and texts, and the layout tokens, of
everything that can change its folder's interface:

- every `use` and `pub use` line, except test-only ones, since names in
  signatures resolve through them;
- every declaration header, private ones included, without bodies;
- every field and variant of every `data` and `enum`, private ones
  included, since derived bounds read member types
  ([`annot.bound.params`](../../spec/lang/14-annotations.md#r-annot.bound.params));
- decorators, `@derive`, `@error` and their message strings;
- template bodies, and the default and fact token ranges.

Left out: function bodies, `tests:` blocks, top-level statements, comments,
doc comments and whitespace.

**Layout is hashed (Codex re-review N-A3).** Indentation is syntax.
Moving a kept template statement from inside an `if` to after it changes
no token kind or text, yet it changes the template's control flow. So
the hash does not read the raw `TokenBuf`. It reads the green tree's
token sequence for each kept range: the real tokens, with their kinds
and texts, and the virtual layout tokens that the layout cursor produced
(`Newline`, `Indent`, `Dedent`, `SuiteEnd`, section 4.2), in parse order.
Whitespace and comments stay out. Since `Indent` and `Dedent` are
relative, shifting a whole declaration's indentation changes nothing. A
syntax test checks two template bodies with equal lexical tokens and
different block structure: their hashes differ.

Hidden template helpers are no exception. A private function that a
template body names has a written result type, and its omitted `$`
clause means the empty row
([resolution-and-interfaces.md §4.10.1](resolution-and-interfaces.md#4101-header-validation-stages)).
So its header text is its whole interface, and an edit to its body
changes no api text hash (Codex review, finding 6). Default and fact
token ranges stay in the hash because they are part of the header text;
the interface itself keeps only whether a default exists and a fact's
type, so the deep hash cuts off an edit inside them.

The folder interface key uses these hashes instead of source hashes
(§5.3). So a body edit never rebuilds its folder's interface. A private
header edit rebuilds it, but the rebuilt blob's deep hash changes only if
the public part changed. This two-level cutoff avoids Swift's
over-invalidation, where adding a private function recompiles every user
([swift #92617](https://github.com/swiftlang/swift/issues/92617)).

### 4.6 Item Index

The item tree is an **index** over the syntax tree (for parsed modules)
and the interface (for modules known only through their folder's blob).
It copies nothing that the node or the record already holds (§3.9.1).

```rust
pub struct ItemIndex {               // per module; ItemIdx = source order
    pub at: Box<[ItemAt]>,           // u32: a header NodeIdx, or an interface record index (high bit set)
    pub parent: Box<[ItemIdx]>,      // NONE at top level; methods, variants, fields
    pub by_name: HashMap<Symbol, ItemIdx>,   // an index into the columns, not storage (§3.9.4)
}
```

Kind, name, visibility, header and body range are read through the
syntax tree's typed views or the interface reader. Flags that neither
holds (result omitted, row omitted, broken) are one bit column in the
module scope.

Derived impls (`@derive`) and error impls (`@error`, `@from`) appear as
synthetic `Impl` items in the module of their declaration
([`trait.own.module.generated`](../../spec/lang/09-traits.md#r-trait.own.module.generated)).
