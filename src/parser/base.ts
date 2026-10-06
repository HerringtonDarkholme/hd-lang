import type {
  Expression,
  InitGroupStart,
  PackageScopes,
  Statement,
  TypeRef,
  UseDecl,
  UseName,
  VarianceMarker,
} from "../ast.ts";
import type { Diagnostic, DiagnosticFix, SourcePosition, SourceSpan } from "../diagnostics.ts";
import type { Token, TokenKind } from "../lexer.ts";
import {
  contextKeys,
  functionParts,
  mutableInner,
  nominalGenericParts,
  optionalInner,
  restInner,
  rowArgumentKeys,
  rowArgumentType,
  tupleParts,
} from "../types.ts";

/** The generic parameters in scope at a declaration, by kind. */
interface GenericKinds {
  readonly types: ReadonlySet<string>;
  /** The parameters declared `$R` (11-requirements-and-suspension.md#r-req.row.param.marked). */
  readonly rows: ReadonlySet<string>;
  /** Each parameter's name token, for a fix-it that marks it `$`. */
  readonly spans: ReadonlyMap<string, SourceSpan>;
}

export class ParseFailure extends Error {}

export interface ExpressionParseResult {
  readonly expression?: Expression;
  readonly diagnostics: readonly Diagnostic[];
}

export interface ParseOptions {
  /**
   * The file is a test module, a `*_test.hd` file (spec/lang/10-modules.md#test-modules):
   * its top level is in test position, its items are test code, and it holds
   * no `tests:` block.
   */
  readonly testModule?: boolean;
  /**
   * The file is an integration test module under `tests/`
   * (spec/lang/10-modules.md#r-module.test.integration), so it may use the
   * `tests` root (spec/lang/10-modules.md#r-module.test.integration.tests-root).
   */
  readonly integrationTest?: boolean;
  /**
   * The source joins several modules of a package, each of which may hold a
   * `tests:` block (src/package.ts), so `duplicate-tests-block` does not apply.
   */
  readonly joinedModules?: boolean;
  /**
   * Initialization-group starts from the package linker, as joined-source
   * line numbers (src/package.ts). The parser maps them to statement indices
   * on `Program.initGroups`, but only under `joinedModules`; without them a
   * joined source has no group boundaries.
   */
  readonly initGroupStarts?: readonly InitGroupStart[];
  /**
   * The package linker's module scopes over the joined source
   * (src/package.ts). The parser puts them on `Program.packageScopes`, but
   * only under `joinedModules`.
   */
  readonly packageScopes?: PackageScopes;
  /**
   * The linked entry module is a script (src/package.ts), so the parser puts
   * `scriptEntry` on the program, under `joinedModules`.
   */
  readonly scriptEntry?: boolean;
  /** The source is a `lib/std` module, which may write `collect`'s type-argument default. */
  readonly standardLibrary?: boolean;
}

export abstract class ParserBase {
  // Mutable: a suite nested inside brackets gets its layout tokens spliced in
  // when the parser reaches its header (see `openNestedLayout`).
  protected readonly tokens: Token[];
  protected index = 0;
  protected readonly diagnostics: Diagnostic[] = [];
  protected activeGenericParameters: ReadonlySet<string> = new Set();
  /** The generic parameters of an enclosing implementation, which its methods see. */
  protected enclosingKinds: GenericKinds = { types: new Set(), rows: new Set(), spans: new Map() };

  protected options: ParseOptions = {};

  constructor(tokens: readonly Token[]) {
    this.tokens = [...tokens];
  }

  withOptions(options: ParseOptions): this {
    this.options = options;
    return this;
  }

  // Set while parsing a declaration or closure result (02-grammar.md#types):
  // function types along the result chain carry no row, so a trailing
  // requirement clause belongs to the declaration.
  protected rowlessResult = false;

  protected abstract parseType(postfix?: boolean): TypeRef;

  protected parseResultType(): TypeRef {
    this.rowlessResult = true;
    try {
      return this.parseType();
    } finally {
      this.rowlessResult = false;
    }
  }

  protected abstract parseSuite(closureBody?: boolean): readonly Statement[];
  protected abstract parseStatement(topOrInline: boolean): Statement;
  protected abstract finishSimpleStatement(topOrInline: boolean): SourceSpan["end"];
  protected abstract parseRequirementKey(): string;
  protected abstract parseExpressionSource(source: string): ExpressionParseResult;

  protected parseDocComments(): string | undefined {
    if (!this.atKind("doc-comment")) return undefined;
    const lines: string[] = [];
    const first = this.current();
    let previous = first;
    while (this.atKind("doc-comment")) {
      const comment = this.advance();
      if (
        lines.length > 0 &&
        (comment.span.start.line !== previous.span.end.line + 1 ||
          comment.span.start.column !== first.span.start.column)
      ) {
        this.fail(
          "doc-comment-without-target",
          "documentation comment lines must be consecutive and share an indentation",
          comment.span,
        );
      }
      lines.push(String(comment.value ?? comment.text));
      previous = comment;
      this.matchKind("newline");
    }
    const target = this.current();
    if (
      target.kind === "eof" ||
      target.kind === "dedent" ||
      target.span.start.line !== previous.span.end.line + 1 ||
      target.span.start.column !== first.span.start.column
    ) {
      this.fail(
        "doc-comment-without-target",
        "documentation comments must immediately precede a declaration or member at the same indentation",
        first.span,
      );
    }
    return lines.join("\n");
  }

  private depths?: number[];

  // Delimiter depths of the same-line suites being parsed, innermost last.
  protected readonly inlineSuiteDepths: number[] = [];

  /** The indentation of the physical line holding the token at `index`. */
  protected lineIndentAt(index: number): number {
    let first = index;
    const line = this.tokens[index]!.span.start.line;
    const layout = ["newline", "indent", "dedent"];
    while (
      first > 0 &&
      this.tokens[first - 1]!.span.start.line === line &&
      !layout.includes(this.tokens[first - 1]!.kind)
    )
      first -= 1;
    return this.tokens[first]!.span.start.column - 1;
  }

  /** The indentation of the first physical line of the logical line holding `index`. */
  protected logicalLineIndentAt(index: number): number {
    let first = index;
    while (first > 0 && !["newline", "indent", "dedent"].includes(this.tokens[first - 1]!.kind))
      first -= 1;
    return this.lineIndentAt(first);
  }

  /**
   * An indented suite nested inside brackets must start deeper than both its
   * header's line and the logical line containing the header
   * (01-lexical-structure.md#physical-and-logical-lines).
   */
  protected checkNestedSuiteIndent(colonIndex: number, first: Token): void {
    const reference = Math.max(this.lineIndentAt(colonIndex), this.logicalLineIndentAt(colonIndex));
    if (first.span.start.column - 1 <= reference)
      this.fail(
        "unexpected-indentation",
        "a nested suite's body must be deeper than the statement that contains its header",
        first.span,
      );
  }

  /**
   * A comma at the delimiter depth where a same-line suite opened closes that
   * suite (01-lexical-structure.md#physical-and-logical-lines), so a
   * multi-name binding or loop written directly in the suite is a syntax
   * error. Call this at such a comma.
   */
  protected rejectCommaClosingInlineSuite(): void {
    if (this.atText(",") && this.inlineSuiteDepths.at(-1) === this.delimiterDepth(this.index))
      this.fail(
        "syntax-error",
        "a comma ends a same-line suite, so several names need an indented body or parentheses",
        this.current().span,
      );
  }

  /**
   * Layout for an indented suite nested inside brackets
   * (01-lexical-structure.md#physical-and-logical-lines). The lexer emits no
   * layout inside brackets, so when the parser reaches a suite header whose
   * `:` (at `colonIndex`) ends its line, this splices in the `NEWLINE`,
   * `INDENT`, and `DEDENT` tokens that layout processing emits for the body:
   * its lines at the suite's delimiter depth, including the suites nested in
   * it at that depth, are laid out as at depth zero. The body ends at a
   * closing delimiter at that depth or at a line indented less than the body.
   * A closure body ends only at a line indented no farther than its header's
   * line that starts with `,` or a closing delimiter at that depth. Returns
   * false when the suite does not start on a later line inside brackets.
   */
  protected openNestedLayout(colonIndex: number, closureBody: boolean): boolean {
    const first = this.index;
    const firstToken = this.tokens[first]!;
    const colon = this.tokens[colonIndex]!;
    if (
      ["newline", "indent", "dedent", "eof"].includes(firstToken.kind) ||
      firstToken.span.start.line <= colon.span.end.line
    )
      return false;
    this.checkNestedSuiteIndent(colonIndex, firstToken);
    const depth = this.delimiterDepth(first);
    const headerIndent = this.lineIndentAt(colonIndex);
    const bodyIndent = firstToken.span.start.column - 1;
    const closers = new Set([")", "]", "}"]);
    const inserts: { readonly at: number; readonly kinds: readonly TokenKind[] }[] = [
      { at: first, kinds: ["newline", "indent"] },
    ];
    const indents = [bodyIndent];
    let previous = firstToken;
    let end = first + 1;
    for (; end < this.tokens.length; end += 1) {
      const token = this.tokens[end]!;
      if (token.kind === "eof") break;
      if (["newline", "indent", "dedent"].includes(token.kind)) continue;
      if (this.delimiterDepth(end) > depth) {
        previous = token;
        continue;
      }
      const closer = closers.has(token.text);
      const startsLine = token.span.start.line > previous.span.end.line;
      if (!startsLine) {
        if (!closer) {
          previous = token;
          continue;
        }
        if (closureBody)
          this.fail(
            "syntax-error",
            "a closing delimiter on a body line does not end an indented closure body; start the line with it",
            token.span,
          );
        break;
      }
      const indent = token.span.start.column - 1;
      if (indent > indents.at(-1)!) {
        const leadingDot =
          token.text === "." &&
          this.tokens[end + 1]?.kind === "identifier" &&
          previous.text !== ":" &&
          previous.text !== "=>";
        if (!leadingDot) {
          inserts.push({ at: end, kinds: ["newline", "indent"] });
          indents.push(indent);
        }
      } else if (indent >= bodyIndent) {
        const kinds: TokenKind[] = ["newline"];
        while (indent < indents.at(-1)!) {
          indents.pop();
          kinds.push("dedent");
        }
        if (indent !== indents.at(-1))
          this.fail(
            "invalid-dedent",
            `column ${indent + 1} is not an active indentation level`,
            token.span,
          );
        inserts.push({ at: end, kinds });
      } else {
        if (closureBody && indent > headerIndent)
          this.fail(
            "syntax-error",
            "a line after an indented closure body must be indented no farther than the closure header",
            token.span,
          );
        if (closureBody && !closer && token.text !== ",")
          this.fail(
            "syntax-error",
            "after an indented closure body inside brackets, the next line must start with ',' or a closing delimiter",
            token.span,
          );
        break;
      }
      previous = token;
    }
    inserts.push({ at: end, kinds: ["newline", ...indents.map((): TokenKind => "dedent")] });
    for (const insert of inserts.reverse()) {
      const position = this.tokens[insert.at]!.span.start;
      const span = { start: position, end: position };
      this.tokens.splice(
        insert.at,
        0,
        ...insert.kinds.map((kind) => ({ kind, text: kind === "newline" ? "\n" : "", span })),
      );
    }
    this.depths = undefined;
    return true;
  }

  // `+T` and `-T` declare variance (04-type-system.md#variance).
  protected parseVarianceMarker(): VarianceMarker {
    if (this.matchText("+")) return "+";
    if (this.matchText("-")) return "-";
    return undefined;
  }

  // After `return` or `break`: the statement ends without a value, at a line
  // ending or, in a same-line suite, where that suite ends.
  protected atValuelessEnd(topOrInline: boolean): boolean {
    if (this.atKind("newline") || this.atKind("dedent") || this.atKind("eof")) return true;
    return topOrInline && [")", ",", "]", "}", "else"].some((text) => this.atText(text));
  }

  /** The kinds in scope: the enclosing ones, with `own` parameters shadowing them. */
  protected genericKinds(
    own: {
      readonly parameters: readonly string[];
      readonly rows: readonly string[];
      readonly spans: ReadonlyMap<string, SourceSpan>;
    },
    enclosing: GenericKinds = this.enclosingKinds,
  ): GenericKinds {
    const rows = new Set([...enclosing.rows].filter((name) => !own.parameters.includes(name)));
    const types = new Set([...enclosing.types].filter((name) => !own.parameters.includes(name)));
    for (const name of own.parameters) (own.rows.includes(name) ? rows : types).add(name);
    return { types, rows, spans: new Map([...enclosing.spans, ...own.spans]) };
  }

  /**
   * A row parameter is declared `$R` and is used only in rows; any other
   * generic parameter is used only as a type. An unmarked parameter in a row
   * gets a fix-it that marks it (11-requirements-and-suspension.md#r-req.row.param.unmarked,
   * #r-req.row.param.as-type).
   */
  protected checkGenericKinds(
    kinds: GenericKinds,
    types: readonly TypeRef[],
    rows: readonly { readonly keys: readonly string[]; readonly span: SourceSpan }[] = [],
  ): void {
    if (kinds.types.size === 0 && kinds.rows.size === 0) return;
    const visitRow = (keys: readonly string[], span: SourceSpan): void => {
      for (const key of keys) {
        const nominal = nominalGenericParts(key);
        const name = nominal?.name ?? key;
        if (kinds.types.has(name)) {
          const declared = kinds.spans.get(name);
          this.fail(
            "generic-kind-mismatch",
            `generic parameter '${name}' is used in a requirement row, so declare it as the row parameter '$${name}'`,
            span,
            declared
              ? {
                  message: `declare '$${name}'`,
                  edits: [
                    { span: { start: declared.start, end: declared.start }, replacement: "$" },
                  ],
                }
              : undefined,
          );
        }
        nominal?.arguments.forEach((argument) => visitType(argument, span));
      }
    };
    const visitType = (type: string, span: SourceSpan): void => {
      if (kinds.rows.has(type))
        this.fail(
          "generic-kind-mismatch",
          `'${type}' is a row parameter, not a type; where a row goes, write '$ ${type}'`,
          span,
        );
      const inner = mutableInner(type) ?? optionalInner(type) ?? restInner(type);
      if (inner !== undefined) return visitType(inner, span);
      const row = rowArgumentKeys(type) ?? contextKeys(type);
      if (row) return visitRow(row, span);
      const tuple = tupleParts(type);
      if (tuple) return tuple.forEach((element) => visitType(element, span));
      const callable = functionParts(type);
      if (callable) {
        callable.parameters.forEach((parameter) => visitType(parameter, span));
        visitType(callable.result, span);
        return visitRow(callable.requirements, span);
      }
      nominalGenericParts(type)?.arguments.forEach((argument) => visitType(argument, span));
    };
    for (const type of types) visitType(type.name, type.span);
    for (const row of rows) visitRow(row.keys, row.span);
  }

  /**
   * A data type, enum, trait, or newtype declares no row parameter
   * (11-requirements-and-suspension.md#r-req.row.param.no-data.marked).
   */
  protected rejectRowParameters(
    rows: readonly string[],
    spans: ReadonlyMap<string, SourceSpan>,
    owner: string,
  ): void {
    const row = rows[0];
    if (row === undefined) return;
    this.fail(
      "generic-kind-mismatch",
      `${owner} declares no row parameter, so '$${row}' is not allowed; its generic parameters are types`,
      spans.get(row) ?? this.current().span,
    );
  }

  /** Checks a function's or method's signature, after its requirement clause. */
  protected checkSignatureKinds(
    generics: {
      readonly parameters: readonly string[];
      readonly rows: readonly string[];
      readonly spans: ReadonlyMap<string, SourceSpan>;
    },
    parameters: readonly { readonly type: TypeRef }[],
    result: TypeRef,
    requirements: readonly string[],
    clauseStart: SourcePosition,
  ): void {
    this.checkGenericKinds(
      this.genericKinds(generics),
      [...parameters.map((parameter) => parameter.type), result],
      requirements.length > 0
        ? [{ keys: requirements, span: { start: clauseStart, end: this.peek(-1).span.end } }]
        : [],
    );
  }

  // A type argument may be a row, `$()` or `$ A + B`, for a row-kinded
  // parameter (02-grammar.md#types); a single key reads as a type.
  protected parseTypeArgument(): TypeRef {
    const start = this.current().span.start;
    if (this.matchText("$")) {
      const keys = this.parseRequirements(false);
      return { name: rowArgumentType(keys), span: { start, end: this.peek(-1).span.end } };
    }
    return this.parseType();
  }

  // A requirement row joins keys with `+` in every position, and `$()` is the
  // empty row (02-grammar.md#types). Inside a type (`header` false), a comma
  // after a key ends the row and belongs to the enclosing list.
  protected parseRequirements(header = true): readonly string[] {
    const open = this.current();
    if (this.matchText("(")) {
      if (!this.atText(")")) {
        this.parseRequirementKey();
        if (this.atText(",")) this.rejectOldRowSeparator();
        this.fail(
          "syntax-error",
          "parentheses never surround a nonempty requirement row: write '$ A + B'",
          open.span,
        );
      }
      this.expectText(")");
      return [];
    }
    const keys = [this.parseRowKey()];
    while (this.matchText("+")) keys.push(this.parseRowKey());
    if (header && this.atText(",")) this.rejectOldRowSeparator();
    return [...new Set(keys)].sort();
  }

  /**
   * The keys after `=` of a row alias, `type AppRow = $ Db + Cache` or
   * `type NoRow = $()`, or undefined when the right side is an ordinary
   * type, including one key without `$` (02-grammar.md#type-declarations).
   * A right side that joins keys with `+` but has no `$` parses too, marked
   * `bare` for the checker to reject (02-grammar.md#r-grammar.type-decl.row-alias.bare).
   */
  protected parseRowAliasTarget():
    | { readonly keys: readonly string[]; readonly bare: boolean }
    | undefined {
    if (this.matchText("$")) return { keys: this.parseRequirements(false), bare: false };
    let depth = 0;
    for (let distance = 0; this.index + distance < this.tokens.length; distance += 1) {
      const token = this.peek(distance);
      if (token.kind === "eof" || (depth === 0 && token.kind === "newline")) return undefined;
      if (token.text === "[" || token.text === "(") depth += 1;
      else if (token.text === "]" || token.text === ")") depth -= 1;
      else if (token.text === "+" && depth === 0)
        return { keys: this.parseRequirements(false), bare: true };
    }
    return undefined;
  }

  // `$ A, B` and `$(A, B)` are the former comma-list rows (02-grammar.md#types).
  private rejectOldRowSeparator(): never {
    this.fail(
      "old-row-separator",
      "requirement keys are joined with '+': write '$ A + B'",
      this.current().span,
    );
  }

  protected parseRowKey(): string {
    const key = this.parseRequirementKey();
    if (this.atText("-"))
      this.fail(
        "syntax-error",
        "requirement rows have no subtraction: take the callback as 'fn(...) -> T $ R + K' and declare '$ R' to remove K",
        this.current().span,
      );
    return key;
  }

  /** True when the current token starts on the line where the previous token ends. */
  protected onPreviousLine(): boolean {
    return this.current().span.start.line === this.peek(-1).span.end.line;
  }

  /** The number of open `(`, `[`, and `{` before the token at `index`. */
  protected delimiterDepth(index: number): number {
    if (!this.depths) {
      const depths: number[] = [];
      let depth = 0;
      for (const token of this.tokens) {
        depths.push(depth);
        if (token.text === "(" || token.text === "[" || token.text === "{") depth += 1;
        else if (token.text === ")" || token.text === "]" || token.text === "}")
          depth = Math.max(0, depth - 1);
      }
      this.depths = depths;
    }
    return this.depths[index] ?? 0;
  }

  /**
   * Outside brackets, a control-flow header cannot end in a suite
   * (02-grammar.md#statements): layout would carry that suite over the `:`.
   */
  protected rejectHeaderEndingInSuite(keyword: Token, header: Expression): void {
    if (this.peek(-1).kind === "dedent" && this.delimiterDepth(this.tokens.indexOf(keyword)) === 0)
      this.fail(
        "syntax-error",
        "a control-flow header outside brackets cannot end in an indented suite",
        header.span,
      );
  }

  protected expectText(text: string): Token {
    if (!this.atText(text))
      this.fail(
        "syntax-error",
        `expected '${text}', found '${this.current().text}'`,
        this.current().span,
      );
    return this.advance();
  }

  protected expectKind(kind: Token["kind"], message: string): Token {
    if (!this.atKind(kind)) this.fail("syntax-error", message, this.current().span);
    return this.advance();
  }

  /**
   * A `mut` before a parameter name goes on its type, as
   * `todos: mut List[Todo]`: fail with the move as a fix-it.
   */
  protected checkMutParameterName(): void {
    if (!this.atText("mut")) return;
    if (this.peek(1).kind !== "identifier" || this.peek(2).text !== ":") return;
    const mutToken = this.current();
    const nameToken = this.peek(1);
    this.advance();
    this.advance();
    const colon = this.expectText(":");
    // A space already follows the colon, or one is added with `mut`.
    const spacer = this.current().span.start.offset > colon.span.end.offset ? " mut" : "mut ";
    this.fail("syntax-error", "`mut` goes on the parameter's type, not its name", mutToken.span, {
      message: "move `mut` after the `:` onto the type",
      edits: [
        {
          span: { start: mutToken.span.start, end: nameToken.span.start },
          replacement: "",
        },
        { span: { start: colon.span.end, end: colon.span.end }, replacement: spacer },
      ],
    });
  }

  protected matchText(text: string): boolean {
    if (!this.atText(text)) return false;
    this.advance();
    return true;
  }

  protected matchKind(kind: Token["kind"]): boolean {
    if (!this.atKind(kind)) return false;
    this.advance();
    return true;
  }

  // A raw identifier never matches a keyword or contextual word.
  protected atText(text: string): boolean {
    const token = this.current();
    return token.text === text && !token.raw;
  }

  protected atKind(kind: Token["kind"]): boolean {
    return this.current().kind === kind;
  }

  protected current(): Token {
    return this.tokens[Math.min(this.index, this.tokens.length - 1)]!;
  }

  protected peek(distance: number): Token {
    return this.tokens[Math.max(0, Math.min(this.index + distance, this.tokens.length - 1))]!;
  }

  protected advance(): Token {
    const token = this.current();
    if (token.kind !== "eof") this.index += 1;
    return token;
  }

  // The prototype parses no decorators, but the grammar lets derive lines
  // precede only data, enum, and newtype declarations
  // (02-grammar.md#r-grammar.annot.alias-no-decorator). Looking past
  // `@derive(...)` lines, this returns the span of a transparent alias's
  // `type` keyword, which is then a syntax error.
  protected aliasAfterDeriveLines(): SourceSpan | undefined {
    let offset = 0;
    while (this.peek(offset).text === "@" && this.peek(offset + 1).text === "derive") {
      if (this.peek(offset + 2).text !== "(") return undefined;
      offset += 3;
      let depth = 1;
      while (depth > 0) {
        const token = this.peek(offset);
        if (token.kind === "eof" || token.kind === "newline") return undefined;
        if (token.text === "(") depth += 1;
        else if (token.text === ")") depth -= 1;
        offset += 1;
      }
      if (this.peek(offset).kind !== "newline") return undefined;
      offset += 1;
    }
    if (offset === 0) return undefined;
    if (this.peek(offset).text === "pub") offset += 1;
    const keyword = this.peek(offset);
    if (keyword.text !== "type" || this.peek(offset + 1).kind !== "identifier") return undefined;
    offset += 2;
    if (this.peek(offset).text === "[") {
      let depth = 0;
      do {
        const token = this.peek(offset);
        if (token.kind === "eof" || token.kind === "newline") return undefined;
        if (token.text === "[") depth += 1;
        else if (token.text === "]") depth -= 1;
        offset += 1;
      } while (depth > 0);
    }
    return this.peek(offset).text === "=" ? keyword.span : undefined;
  }

  /**
   * At the start of a statement, `(`, names separated by commas, `)`, and
   * `:=` form the multi-name binding list, never a tuple expression. That
   * form is since retired: a pattern before `:=` is `missing-let`
   * (02-grammar.md#r-grammar.stmt.short-binding.let-only). Returns the number
   * of names, or undefined when the tokens do not have that shape.
   */
  protected bindingListLength(): number | undefined {
    if (!this.atText("(")) return undefined;
    let offset = 1;
    let count = 0;
    for (;;) {
      if (this.peek(offset).kind !== "identifier") return undefined;
      count += 1;
      offset += 1;
      if (this.peek(offset).text !== ",") break;
      offset += 1;
    }
    return this.peek(offset).text === ")" && this.peek(offset + 1).text === ":="
      ? count
      : undefined;
  }

  /**
   * A multi-name `let` or `:=` without parentheses is an error whose fix-it
   * adds them (02-grammar.md#r-grammar.stmt.let-list.bare,
   * 02-grammar.md#r-grammar.stmt.short-binding.bare-list).
   */
  protected failBareNameList(
    form: "let" | ":=" | "for" | "comprehension",
    first: { readonly span: SourceSpan },
    last: { readonly span: SourceSpan },
  ): never {
    const span = { start: first.span.start, end: last.span.end };
    const [example, where] = {
      let: ["let (a, b) = ...", "a 'let' binding"],
      ":=": ["(a, b) := ...", "a ':=' binding"],
      // A `for` list too (02-grammar.md#r-grammar.flow.for-list.bare).
      for: ["for (a, b) in ...", "a for loop"],
      comprehension: ["[for (a, b) in ... => ...]", "a comprehension for clause"],
    }[form];
    this.fail(
      "syntax-error",
      `several names in ${where} go in parentheses, as in '${example}'`,
      span,
      {
        message: "put the names in parentheses",
        edits: [
          { span: { start: span.start, end: span.start }, replacement: "(" },
          { span: { start: span.end, end: span.end }, replacement: ")" },
        ],
      },
    );
  }

  protected fail(code: string, message: string, span: SourceSpan, fix?: DiagnosticFix): never {
    this.diagnostics.push({ code, message, span, ...(fix ? { fix } : {}) });
    throw new ParseFailure(message);
  }

  // `use` begins a use declaration only before a use root
  // (01-lexical-structure.md#keywords-and-reserved-words).
  protected atUseDeclaration(): boolean {
    const offset = this.atText("pub") ? 1 : 0;
    const word = this.peek(offset);
    const root = this.peek(offset + 1);
    return (
      word.text === "use" &&
      !word.raw &&
      !root.raw &&
      ["pkg", "std", "dep", "self", "super", "tests"].includes(root.text)
    );
  }

  protected parseUse(): UseDecl {
    const public_ = this.matchText("pub");
    const start = this.expectText("use").span.start;
    // The `tests` root names integration test modules, so only an
    // integration test module may use it; anywhere else it is test-only-use
    // (10-modules.md#r-module.test.tests-root-elsewhere).
    const testsRoot = this.atText("tests");
    if (testsRoot && !this.options.integrationTest)
      this.fail(
        "test-only-use",
        "the tests use root is available only in an integration test module under tests/",
        this.current().span,
      );
    // A use root: `pkg`, `std`, `dep`, `super`, or the reserved word `self`
    // or `tests`.
    const parts = [
      this.atText("self") || testsRoot
        ? this.advance().text
        : this.expectKind("identifier", "expected a module path after use").text,
    ];
    let grouped = false;
    while (this.matchText(".")) {
      if (this.matchText("{")) {
        grouped = true;
        break;
      }
      parts.push(this.expectKind("identifier", "expected a module path component").text);
    }
    const names: UseName[] = [];
    let module: string;
    if (grouped) {
      module = parts.join(".");
      do {
        const name = this.expectKind("identifier", "expected an imported declaration name").text;
        if (this.atText("."))
          this.fail(
            "direct-variant-use",
            "enum variants cannot be imported directly",
            this.current().span,
          );
        const alias = this.matchText("as")
          ? this.expectKind("identifier", "expected an import alias").text
          : undefined;
        names.push({ name, ...(alias ? { alias } : {}) });
      } while (this.matchText(",") && !this.atText("}"));
      this.expectText("}");
    } else {
      // Only the grouped form accepts a `pub` prefix (10 Use Forms).
      if (public_)
        this.fail(
          "syntax-error",
          "only the grouped use form accepts 'pub'; write 'pub use module.{Name}'",
          { start, end: this.peek(-1).span.end },
        );
      const name = parts.pop()!;
      module = parts.join(".");
      const alias = this.matchText("as")
        ? this.expectKind("identifier", "expected an import alias").text
        : undefined;
      names.push({ name, ...(alias ? { alias } : {}) });
    }
    const end = this.finishSimpleStatement(false);
    return {
      kind: "use",
      module,
      names,
      ...(public_ ? { public: true } : {}),
      span: { start, end },
    };
  }
}
