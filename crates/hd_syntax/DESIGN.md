# New Compiler Design: Front-Half Syntax Stages

Status: design record, checked against the code at 3209d690. "Implementation
differs" notes mark where the code falls short of the design. The code and
the spec win.

Part of the [compiler design](../../docs/compiler/ARCHITECTURE.md).

## 4. Front-Half Stages

### 4.1 Lexer

**Job.** `lex` turns a whole file into significant tokens, comments and
line facts in one pass (`src/lexer.rs`). Lexing the whole file up front
costs one file's tokens and lets the parser look ahead freely.

```rust
pub struct TokenBuf {
    pub kind: Vec<TokenKind>,       // u8; significant tokens only
    pub start: Vec<u32>,            // byte offsets
    pub end: Vec<u32>,
    pub line_first: Vec<u32>,       // bit set: the token is first on its physical line
    pub line_start: Vec<u32>,       // per physical line
    pub line_tok: Vec<TokenIdx>,    // first token of the line, or NONE
    pub line_indent: Vec<u16>,
    pub line_flags: Vec<LineFlags>, // NON_ASCII, HAS_TAB, CONTINUES_STRING
    pub com_start: Vec<u32>,        // comments, a side list
    pub com_end: Vec<u32>,
    pub com_kind: Vec<CommentKind>, // Plain, Doc, ModuleDoc
}
```

Indentation is a per-line column, and the layout cursor tracks bracket
depth itself, so a token is 9 bytes.

- **Lossless without trivia tokens.** Whitespace is the gap between
  tokens. Comments are a side list. Tokens, gaps and the original text
  reproduce the file byte for byte (`TokenBuf::reconstruct`), which the
  corpus and fuzz tests check.
- **Strings.** A string with interpolation lexes as `StrHead`, `StrMid`
  and `StrTail` tokens around the code tokens of each `${...}`. The lexer
  recurses into an interpolation, bounded by the nesting limit
  (`nesting-too-deep`). A line inside a multiline string is flagged
  `CONTINUES_STRING`, so it starts no layout line. Skim reuses `lex`, so
  the two cannot disagree on where a string ends.
- **Identifiers.** ASCII identifiers take the byte path. A non-ASCII
  identifier is checked for XID and NFC
  ([`lex.encoding.scalars`](../../spec/lang/01-lexical-structure.md#r-lex.encoding.scalars)),
  and for mixed confusable scripts. Keywords are one `match` on the text,
  which the Rust compiler lowers to a length switch and short compares.
- **Errors rarely stop it.** A bare CR, a tab, an unterminated
  single-line string (which ends at its line end) and other bad input
  each produce one diagnostic and, where a token stands, an `Error`
  token; lexing continues. The lexer also matches delimiters and reports
  an unclosed one. Invalid UTF-8 is the exception: the file gets one
  `invalid-token` diagnostic and no tokens, since a file that is not
  text has no later errors worth reporting.
- **No early decoding.** Identifier and literal tokens are spans. Escapes
  and numeric values are decoded later, when a value is needed.
- **Complexity.** O(n) time, 9 bytes per significant token plus 11 per
  line. `examples/throughput.rs` times lexing, parsing and skimming
  separately, so a regression points at its phase.

### 4.2 Layout

At bracket depth zero, Python's indentation stack decides layout. Inside
brackets, and for same-line suites, only the parser knows that a colon
opens a suite
([`lex.colon.cooperate`](../../spec/lang/01-lexical-structure.md#r-lex.colon.cooperate)).
So the parser drives a layout cursor (`src/layout.rs`).

```rust
pub enum Layout { Newline, Indent, Dedent, SuiteEnd }
pub enum TokenOrLayout { Token(TokenIdx), Layout { kind: Layout, at: TokenIdx }, Eof }
pub enum SuiteKind { SameLine, Indented, Missing }

impl LayoutCursor<'_> {
    pub fn peek(&mut self) -> TokenOrLayout;   // computes pending layout lazily
    pub fn bump(&mut self);
    /// Called right after the parser consumes a suite-introducing `:`.
    pub fn open_suite(&mut self, closure: bool) -> SuiteKind;
    /// After a match arm's `=>`: indented, or one statement on the line.
    pub fn open_arm(&mut self) -> SuiteKind;
    /// The parser finished a same-line suite body.
    pub fn close_inline(&mut self);
}
```

The cursor keeps a stack of blocks (body column, bracket depth, header
column, closure flag, opened inside brackets) and the bracket depths of
open same-line suites.

1. At a line-first token outside brackets, `peek` runs the algorithm of
   [Indentation Levels](../../spec/lang/01-lexical-structure.md#indentation-levels)
   from `line_first` and `line_indent`, including leading-dot and
   leading-`|>` continuation lines. Inside brackets a new line produces
   no layout.
2. `open_suite` returns `SameLine` when the next token is on the same
   line, `Indented` when the next line is deeper than the header, and
   `Missing` otherwise. It records the references that the nested rules
   need ([Suites Inside Delimiters](../../spec/lang/01-lexical-structure.md#suites-inside-delimiters)).
3. A same-line suite ends where the parser's inline statement ends. The
   parser calls `close_inline` and records the `SuiteEnd` itself, since
   it is the one that knows where the statement ended. An `else:` on the
   next line still attaches after a same-line body.
4. A closer at a nested suite's depth ends the suite (`lex.nested.closer`).
5. Layout tokens enter the green tree's `layout_at` and `layout_kind`
   columns, not its real-token stream. They stay zero-width positions
   between real tokens, so the formatter sees every decision, and
   `TokenIdx` keeps indexing real tokens only.
6. **Recovery.** `invalid-dedent`: report, go on at the enclosing level.
   An indentation no suite opened: the cursor emits an `Indent` and the
   parser reports `unexpected-indentation`, then reads the lines as part
   of the current suite. A closure end-rule violation: `syntax-error`,
   then the closure's suite closes at that line.

### 4.3 Skim Mode And The Header Pass

The header pass is the parser's item-level code run with a body policy
of `Skip`.

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

- `skip_body` runs the lexer without storing tokens. It stops at the
  first logical line, at bracket depth zero and outside any string, whose
  indentation is at most the header's and which is not blank or
  comment-only. A same-line body ends at its logical line end.
- Kept: template bodies (`impl ... by Structure`), the token ranges of
  parameter, field and shared-data defaults, and fact and decorator
  expressions.
- Skipped: function and method bodies, `tests:` blocks (their `use` lines
  are kept and marked test-only), top-level statements.
- With the direct-only `block_on` ban, nothing a dependent needs comes
  from a body.
- **Errors.** The header pass reports nothing. The file is fully parsed
  when its module is checked, and that parse reports each error once.
- A fully parsed file derives its skeleton from its tree, so no file is
  read twice.

Implementation differs: `skim` (`src/skim.rs:43`) lexes the whole file
into a `TokenBuf`, then a line scanner finds each line whose last colon
can open a suite. It returns a `HeaderSkeleton` (source hash, api text
hash, `token_hash` over code lines only, code-line starts, body ranges,
`use` line ranges, `broken`) and builds no tree. This is worse: it
stores every body token it then skips, and it is a second, heuristic
reading of the grammar. The test `skim_and_full_parse_agree_on_skeletons`
checks it against the full parse on std, dogfood and example files.

### 4.4 Parser And Green Tree

**Parser.** `parse` (`src/parser/`) is hand-written recursive descent.
It emits events (`Start { kind, forward_parent }`, `Token`, `Finish`,
`Tombstone`) into one vector and builds the flat tree from it in one
pass. `precede` wraps a finished node through `forward_parent`;
`abandon` turns a start into a tombstone.

- **Recovery sets.** A `Newline` or `Dedent` ends a statement. A token at
  column 0 always starts a new top-level item, so one broken item never
  swallows the next (`errors-per-run`). Inside brackets, the closing
  delimiter and `,` are the recovery set. Junk is wrapped in an `ERROR`
  node. The parser reports only the first error in a statement, and none
  where the lexer already reported.
  Inside brackets, the column-0 tokens that start an item are `fn name`,
  `data`, `enum`, `trait`, `impl`, `type`, `pub` and `tests`; no
  bracketed construct holds one, since a nested suite is indented deeper
  than its statement. The lexer's delimiter matching reports the openers
  still open there as `unclosed-delimiter` and records the token in
  `TokenBuf::resync`; the layout cursor drops the bracket depth to zero
  at it, and the parser suppresses errors only from each unclosed opener
  through the token that ended its scope (`grammar.recovery.unclosed`).
  Other column-0 lines (closers, operands, `@`, `use`) still continue
  the brackets.
- **Progress and depth.** Each repetition consumes at least one token,
  into an `ERROR` node if the grammar made no progress. Brackets, blocks,
  closures and interpolations share one depth counter. At 160 the parser
  reports `nesting-too-deep` once and skips the balanced remainder.
- **Operators.** Precedence climbing. A left-associative chain loops
  instead of recursing, so its length costs no stack depth; recursion is
  bounded by the number of precedence levels. Comparisons report
  `comparison-chaining`, and ranges do not chain.
- **Grammar checks.** A path-only alias such as `use a.b as c` parses.
  An associated type is only `type Name` in a trait or `type Name = Type`
  in an impl; `type Item < Eq = i32` is a `syntax-error`.

**Green tree** (`src/green.rs`).

```rust
pub struct GreenTree {
    kind: Vec<SyntaxKind>,        // u16
    first_token: Vec<TokenIdx>,
    last_token: Vec<TokenIdx>,    // inclusive
    subtree_len: Vec<u32>,        // preorder: node i's subtree is i .. i + subtree_len[i]
    pub layout_at: Vec<TokenIdx>, // zero-width layout positions, in order
    pub layout_kind: Vec<Layout>,
    pub err_node: Vec<NodeIdx>,
    pub err_code: Vec<Code>,
}
pub struct NodeRef<'t> { tree: &'t GreenTree, index: NodeIdx }
```

- Preorder with subtree sizes: a child walk is a loop, and skipping a
  subtree is one addition. `parent_links` builds parent links on demand.
- **Typed views** (`FnDecl`, `IfExpr`, ...) come from the `ast_nodes!`
  macro: each is a `NodeRef` of one kind. Accessors that find children by
  kind are written by hand on the views that need them.
- **No red tree and no incremental reparse.** A whole file reparses
  quickly. An editor tier can add them later.
- About 14 bytes per node.
- **Wire form.** `GreenTree::to_wire` writes tokens, tree columns,
  comments and line starts as little-endian sections behind an `HDST`
  header. Nothing reads it yet.
- **`dyn` types.** `dyn` followed by a trait path, its arguments and
  bindings is a `DYN_TYPE` node, as in `dyn Supplier[Item = i32]`. A bare
  trait name in type position parses as a named type: only resolution
  knows the name is a trait, so resolution reports it, with a fix-it
  that inserts `dyn`.
- **No GADT forms.** A variant has no result-type annotation.

Planned, not built: views generated from one grammar file for Rust and
JavaScript, and a JavaScript decoder of the wire form, so tools outside
the checker (formatter, `hd doc`, the playground) read the tree through
named views instead of raw columns.

### 4.5 Header Extraction And The API Text Hash

Each skeleton carries an **api text hash**, a cache key input: the hash
of the token kinds and texts, and the layout tokens, of everything that
can change its folder's interface:

- every `use` and `pub use` line, except test-only ones;
- every declaration header, private ones included, without bodies;
- every field and variant of every `data` and `enum`, private ones
  included, since derived bounds read member types
  ([`annot.bound.params`](../../spec/lang/14-annotations.md#r-annot.bound.params));
- decorators, `@derive`, `@error` and their message strings;
- template bodies, and the default and fact token ranges;
- a function body that declares a local `data`, `enum`, `trait`, `impl`
  or `type`, since those lift to module items and a local impl is in the
  folder interface.

Left out: other function bodies, `tests:` blocks, top-level statements,
comments, doc comments and whitespace.

**Layout is hashed.** Indentation is syntax. Moving a kept template
statement from inside an `if` to after it changes no token kind or text,
yet it changes control flow. So the hash reads the green tree's token
sequence for each kept range: the real tokens and the virtual layout
tokens (`Newline`, `Indent`, `Dedent`, `SuiteEnd`), in parse order. Since
`Indent` and `Dedent` are relative, shifting a whole declaration changes
nothing. The test `api_hash_includes_kept_layout` checks two template
bodies with equal tokens and different block structure.

Implementation differs: `api_hash` (`src/skim.rs`) walks the raw
`TokenBuf`, not the green tree.
- **What it cuts.** It cuts function bodies, `tests:` blocks and
  top-level statements. A top-level statement is a column-0 logical line
  that starts no declaration, `use`, decorator or `tests:`.
- **Local items.** A body or statement that declares a local item stays.
- **Layout markers.** It adds a line marker, plus indent or dedent
  markers from an indentation stack, at each logical line's first token.
  So a line break inside brackets changes nothing.
- **Body detection.** It also reads logical lines, so a header split
  inside its brackets still has its body cut.

Private items a template names are no exception. A private function that
a template body names has a written result type, and its omitted `$`
clause means the empty row
([crates/hd_resolve/DESIGN.md §4.10.1](../hd_resolve/DESIGN.md#4101-header-validation-stages)).
So its header text is its whole interface, and an edit to its body
changes no api text hash. Default and fact token ranges stay in the hash
because they are header text; the interface keeps only whether a default
exists and a fact's type, so the deep hash cuts off an edit inside them.

The folder interface key uses these hashes instead of source hashes
(`iface_key` in `crates/hd_cache/src/lib.rs`). So a body edit never
rebuilds its folder's interface. A private header edit rebuilds it, but
the rebuilt blob's deep hash changes only if the public part changed.
This two-level cutoff avoids Swift's over-invalidation, where adding a
private function recompiles every user
([swift #92617](https://github.com/swiftlang/swift/issues/92617)).

### 4.6 Item Index

The item tree is an **index** over the syntax tree (for parsed modules)
and the interface (for modules known only through their folder's blob).
It copies nothing that the node or the record already holds.

```rust
pub struct ItemIndex {               // per module; ItemIdx = source order
    pub at: Box<[ItemAt]>,           // u32: a header NodeIdx, or an interface record index (high bit set)
    pub parent: Box<[ItemIdx]>,      // NONE at top level; methods, variants, fields
    pub by_name: HashMap<Symbol, ItemIdx>,
}
```

Kind, name, visibility, header and body range are read through the
syntax tree's typed views or the interface reader. Derived impls
(`@derive`) and error impls (`@error`, `@from`) appear as synthetic
`Impl` items in the module of their declaration
([`trait.own.module.generated`](../../spec/lang/09-traits.md#r-trait.own.module.generated)).

Implementation differs: `parse` builds an `ItemIndex`
(`src/parser/mod.rs:42`, `:801`) of top-level tree items only, with
`name`, `kind` and an unused `flags` column, `parent` always `NONE`, and
no interface records or synthetic impls. No crate outside `hd_syntax`
reads it; resolution keeps its own item tables
(`crates/hd_resolve/src/lower.rs`). Not judged.
