import type {
  Expression,
  AssociatedTypeBinding,
  AssociatedTypeDecl,
  Decorators,
  MemberLine,
  DataDecl,
  EnumDecl,
  FunctionDecl,
  GenericBound,
  ImplDecl,
  MethodDecl,
  Parameter,
  Program,
  Statement,
  TraitDecl,
  TypeDecl,
  TypeRef,
  VarianceMarker,
} from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { lex, type Token } from "../lexer.ts";
import { optionalType } from "../types.ts";
import { DecoratorParser } from "./decorators.ts";
import { ParseFailure, type ExpressionParseResult, type ParseOptions } from "./base.ts";
import {
  emptyModuleItems,
  finishTestCases,
  isCallOf,
  testCase,
  type ModuleItems,
} from "./test-cases.ts";

export interface ParseResult {
  readonly program?: Program;
  readonly diagnostics: readonly Diagnostic[];
}

interface FunctionTypeParameter {
  readonly type: TypeRef;
  readonly variadic: boolean;
}

interface ParsedGenericParameters {
  readonly parameters: readonly string[];
  readonly bounds: readonly GenericBound[];
  /** Parameters written `reified`; the prototype erases them all. */
  readonly reified?: readonly string[];
}

class Parser extends DecoratorParser {
  parse(): ParseResult {
    const items = emptyModuleItems();
    this.localDeclarations = false;
    const start = this.current().span.start;
    let testsBlock = false;
    try {
      while (!this.atKind("eof")) {
        if (this.matchKind("newline")) continue;
        const doc = this.parseDocComments();
        // A file holds at most one top-level `tests:` block
        // (spec/02-grammar.md#test-blocks).
        if (this.atText("tests") && this.peek(1).text === ":") {
          if (doc)
            this.fail(
              "doc-comment-without-target",
              "documentation comments cannot attach to a tests block",
              this.current().span,
            );
          const { testModule, joinedModules } = this.options;
          if (testModule || (testsBlock && !joinedModules))
            this.fail(
              testModule ? "misplaced-tests-block" : "duplicate-tests-block",
              testModule
                ? "a test module holds its test cases at top level, not in a tests: block"
                : "a file may have at most one tests: block",
              this.current().span,
            );
          testsBlock = true;
          this.parseTestsBlock(items);
          continue;
        }
        this.parseModuleItem(doc, items, this.options.testModule === true);
      }
      finishTestCases(items, (code, message, span) => this.fail(code, message, span));
    } catch (error) {
      if (!(error instanceof ParseFailure)) throw error;
      return { diagnostics: this.diagnostics };
    }
    const { uses, types, data, enums, traits, implementations, functions, tests, statements } =
      items;
    return {
      program: {
        uses,
        ...(types.length > 0 ? { types } : {}),
        ...(this.localDeclarations ? { localDeclarations: true } : {}),
        data,
        enums,
        traits,
        implementations,
        functions,
        tests,
        statements,
        ...(items.testOnlyNames.size > 0 ? { testOnlyNames: [...items.testOnlyNames] } : {}),
        ...(this.bareContextKeys.length > 0 ? { bareContextKeys: [...this.bareContextKeys] } : {}),
        span: { start, end: this.current().span.end },
      },
      diagnostics: this.diagnostics,
    };
  }

  // One top-level item, or one item of the `tests:` block, whose statements
  // must all be `it(...)` calls (spec/10-modules.md#test-cases).
  private parseModuleItem(doc: string | undefined, items: ModuleItems, inTests: boolean): void {
    if (this.atText("struct"))
      this.fail("old-struct-declaration", "'struct' was replaced by 'data'", this.current().span);
    if (this.atText("import"))
      this.fail("old-import-declaration", "'import' was replaced by 'use'", this.current().span);
    if (this.atText("export"))
      this.fail(
        "old-export-declaration",
        "'export' was replaced by 'pub use'",
        this.current().span,
      );
    // Nothing outside a `tests:` block sees its items, so none is `pub`
    // (spec/03-names-and-scopes.md#r-names.tests.no-pub); a test module's may be.
    if (inTests && !this.options.testModule && this.atText("pub"))
      this.fail(
        "public-test-item",
        "an item inside a tests block cannot be pub; share test helpers from a _test.hd module",
        this.current().span,
      );
    if (this.atUseDeclaration()) {
      if (doc)
        this.fail(
          "doc-comment-without-target",
          "documentation comments cannot attach to a use declaration",
          this.current().span,
        );
      const use = this.parseUse();
      items.uses.push(use);
      if (inTests) for (const name of use.names) items.testOnlyNames.add(name.alias ?? name.name);
      return;
    }
    // Decorator lines before a data, enum, function, trait, newtype, or
    // implementation declaration (spec/02-grammar.md#annotations). Anything
    // else falls through to the statement parser, which rejects the decorator.
    let decorators: Decorators | undefined;
    if (this.atText("@") && this.decoratedDeclarationFollows()) {
      decorators = this.parseDecoratorLines();
      doc = this.parseDocComments() ?? doc;
    }
    const public_ = this.matchText("pub");
    const declared = (name: string): void => {
      if (inTests) items.testOnlyNames.add(name);
    };
    const decorated = decorators ? { decorators } : {};
    if (this.atText("fn")) {
      const declaration = { ...this.parseFunction(doc, public_), ...decorated };
      declared(declaration.name);
      items.functions.push(inTests ? { ...declaration, testOnly: true } : declaration);
    } else if (this.atText("data")) {
      const declaration = { ...this.parseData(doc, public_), ...decorated };
      declared(declaration.name);
      items.data.push(declaration);
    } else if (this.atText("enum")) {
      const declaration = { ...this.parseEnum(doc, public_), ...decorated };
      declared(declaration.name);
      items.enums.push(declaration);
    } else if (this.atText("trait")) {
      const declaration = { ...this.parseTrait(doc, public_), ...decorated };
      declared(declaration.name);
      items.traits.push(declaration);
    } else if (this.atText("type") && this.peek(1).kind === "identifier") {
      const typeStart = this.current().span;
      const declaration = { ...this.parseTypeDecl(doc, public_), ...decorated };
      this.checkTypeDecorators(declaration, typeStart);
      declared(declaration.name);
      items.types.push(declaration);
    } else if (this.atText("impl"))
      items.implementations.push({ ...this.parseImpl(doc), ...decorated });
    else {
      // Top-level bindings cannot be public (10 Name Resolution Across Packages).
      if (public_)
        this.fail(
          "syntax-error",
          "'pub' must precede a declaration; top-level bindings cannot be public",
          this.peek(-1).span,
        );
      if (doc)
        this.fail(
          "doc-comment-without-target",
          "documentation comments must attach to a declaration or member",
          this.current().span,
        );
      const statement = this.parseStatement(!inTests);
      if (!inTests) items.statements.push(statement);
      else if (isCallOf(statement, "it"))
        items.tests.push(
          testCase(statement, (code, message, span) => this.fail(code, message, span)),
        );
      else items.pendingEach.push(statement);
    }
  }

  private parseTestsBlock(items: ModuleItems): void {
    this.expectText("tests");
    this.expectText(":");
    this.expectKind("newline", "expected a line ending after 'tests:'");
    this.expectKind("indent", "expected an indented tests block");
    let count = 0;
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const doc = this.parseDocComments();
      this.parseModuleItem(doc, items, true);
      count += 1;
    }
    this.expectKind("dedent", "expected the end of the tests block");
    if (count === 0)
      this.fail("empty-suite", "a tests block must contain an item", this.current().span);
  }

  parseExpressionFragment(): ExpressionParseResult {
    try {
      const expression = this.parseExpression();
      while (this.matchKind("newline")) {}
      if (!this.atKind("eof"))
        this.fail(
          "expected-interpolation-end",
          `expected the end of an interpolation expression, found '${this.current().text}'`,
          this.current().span,
        );
      return { expression, diagnostics: this.diagnostics };
    } catch (error) {
      if (!(error instanceof ParseFailure)) throw error;
      return { diagnostics: this.diagnostics };
    }
  }

  protected parseExpressionSource(source: string): ExpressionParseResult {
    const lexed = lex(source);
    if (lexed.diagnostics.length > 0) return { diagnostics: lexed.diagnostics };
    return new Parser(lexed.tokens).parseExpressionFragment();
  }

  protected parseFunction(doc?: string, public_ = false): FunctionDecl {
    const start = this.expectText("fn").span.start;
    const name = this.expectKind("identifier", "expected a function name");
    const suspending = this.matchText("!");
    const parsedGenerics = this.parseGenericParameters();
    const genericParameters = [...parsedGenerics.parameters];
    const genericBounds = [...parsedGenerics.bounds];
    const enclosingGenericParameters = this.activeGenericParameters;
    this.activeGenericParameters = new Set([...enclosingGenericParameters, ...genericParameters]);
    this.expectText("(");
    const parameters: Parameter[] = [];
    if (!this.atText(")")) {
      do {
        const parameterDoc = this.parseDocComments();
        const parameterMetadata = this.parseMemberDecorators(true);
        if (["self", "Self"].includes(this.current().text) && !this.current().raw) {
          this.fail(
            "reserved-name",
            `'${this.current().text}' is reserved and cannot name a parameter`,
            this.current().span,
          );
        }
        const parameterName = this.expectKind("identifier", "expected a parameter name");
        this.expectText(":");
        const type = this.parseType();
        const variadic = this.matchText("...");
        const defaultValue = this.matchText("=") ? this.parseExpression() : undefined;
        if (variadic && defaultValue)
          this.fail(
            "syntax-error",
            "a variadic parameter cannot declare a default",
            defaultValue.span,
          );
        parameters.push({
          name: parameterName.text,
          type,
          variadic: variadic || undefined,
          default: defaultValue,
          doc: parameterDoc,
          ...(parameterMetadata.length > 0 ? { metadata: parameterMetadata } : {}),
          span: { start: parameterName.span.start, end: defaultValue?.span.end ?? type.span.end },
        });
      } while (this.matchText(",") && !this.atText(")"));
    }
    const close = this.expectText(")");
    const { result, resultOmitted } = this.parseOptionalResult(close.span);
    const requirementsOmitted = !this.atText("$");
    const requirements = this.matchText("$") ? this.parseRequirements() : [];
    const body = this.parseSuite();
    this.activeGenericParameters = new Set();
    return {
      kind: "function",
      ...(public_ ? { public: true } : {}),
      name: name.text,
      suspending,
      genericParameters,
      genericBounds,
      parameters,
      result,
      requirements,
      ...(resultOmitted ? { resultOmitted } : {}),
      ...(requirementsOmitted ? { requirementsOmitted } : {}),
      body,
      doc,
      span: { start, end: body.at(-1)?.span.end ?? result.span.end },
    };
  }

  // A declaration may omit `-> type` (07-functions.md#declarations); the
  // checker decides whether that is allowed and infers the result.
  protected parseOptionalResult(close: SourceSpan): {
    readonly result: TypeRef;
    readonly resultOmitted: boolean;
  } {
    if (this.matchText("->")) return { result: this.parseResultType(), resultOmitted: false };
    if (!this.atText("$") && !this.atText(":") && !this.atKind("newline")) this.expectText("->");
    return { result: { name: "void", span: close }, resultOmitted: true };
  }

  protected parseGenericParameters(): ParsedGenericParameters {
    const parameters: string[] = [];
    const bounds: GenericBound[] = [];
    const reified: string[] = [];
    if (!this.matchText("[")) return { parameters, bounds };
    if (!this.atText("]")) {
      do {
        // `reified` modifies a parameter only directly before its name; the
        // prototype erases every generic parameter.
        // An unbackticked `reified` is always the modifier, so a lone one is
        // an error; a parameter named reified is written `` `reified` ``.
        if (this.atText("reified") && !this.current().raw) {
          if (this.peek(1).kind !== "identifier")
            this.fail(
              "syntax-error",
              "`reified` must be followed by a generic parameter name; write `reified` in backticks to name a parameter reified",
              this.current().span,
            );
          this.advance();
          reified.push(this.current().text);
        }
        const parameter = this.expectKind("identifier", "expected a generic parameter name");
        if (parameters.includes(parameter.text))
          this.fail(
            "duplicate-generic-parameter",
            `generic parameter '${parameter.text}' is declared more than once`,
            parameter.span,
          );
        parameters.push(parameter.text);
        if (this.atText(":"))
          this.fail(
            "syntax-error",
            `a generic bound is written with '<', not ':': write '${parameter.text} < Trait'`,
            this.current().span,
          );
        if (this.matchText("<")) {
          const bindings: AssociatedTypeBinding[] = [];
          const traits = this.parseTraitBoundNames(bindings);
          if (traits.length > 0 || bindings.length > 0)
            bounds.push({
              parameter: parameter.text,
              traits,
              ...(bindings.length > 0 ? { bindings } : {}),
              span: { start: parameter.span.start, end: this.peek(-1).span.end },
            });
        } else if (this.atText("...") || this.atText("=")) {
          this.fail(
            "unsupported-generic-parameter",
            "packs and defaults are outside the current erased-generic slice",
            this.current().span,
          );
        }
      } while (this.matchText(",") && !this.atText("]"));
    }
    this.expectText("]");
    return { parameters, bounds, ...(reified.length > 0 ? { reified } : {}) };
  }

  protected parseTraitBoundNames(bindings: AssociatedTypeBinding[] = []): string[] {
    const mutable = this.matchText("mut");
    const traits: string[] = [];
    do {
      const trait = this.boundHasBindings()
        ? this.parseBoundTraitWithBindings(bindings)
        : this.parseType();
      traits.push(mutable ? `mut:${trait.name}` : trait.name);
    } while (this.matchBoundJoiner());
    return traits;
  }

  // Several bounds are joined with `&`; `+` is the former spelling
  // (02-grammar.md#r-grammar.generic.bound.old-plus).
  protected matchBoundJoiner(): boolean {
    if (this.atText("+"))
      this.fail(
        "old-bound-operator",
        "several bounds are joined with '&': write 'A & B'",
        this.current().span,
      );
    return this.matchText("&");
  }

  /** True at `Trait[..., Name = type]`: a bound trait with associated type bindings. */
  private boundHasBindings(): boolean {
    if (this.current().kind !== "identifier" || this.peek(1).text !== "[") return false;
    let depth = 0;
    for (let distance = 1; ; distance += 1) {
      const token = this.peek(distance);
      if (token.kind === "eof" || token.kind === "newline") return false;
      if (token.text === "[" || token.text === "(") depth += 1;
      else if (token.text === "]" || token.text === ")") {
        depth -= 1;
        if (depth === 0) return false;
      } else if (depth === 1 && token.kind === "identifier" && this.peek(distance + 1).text === "=")
        return true;
    }
  }

  private parseBoundTraitWithBindings(bindings: AssociatedTypeBinding[]): TypeRef {
    const name = this.expectKind("identifier", "expected a trait name");
    this.expectText("[");
    const positional: TypeRef[] = [];
    const own: Omit<AssociatedTypeBinding, "trait">[] = [];
    while (!this.atText("]")) {
      if (this.current().kind === "identifier" && this.peek(1).text === "=") {
        const binding = this.advance();
        this.advance();
        const type = this.parseType();
        own.push({
          name: binding.text,
          type,
          span: { start: binding.span.start, end: type.span.end },
        });
      } else {
        if (own.length > 0)
          this.fail(
            "syntax-error",
            "positional trait arguments must precede associated type bindings",
            this.current().span,
          );
        positional.push(this.parseType());
      }
      if (!this.matchText(",")) break;
    }
    const close = this.expectText("]");
    const rendered = positional.length
      ? `${name.text}[${positional.map((argument) => argument.name).join(",")}]`
      : name.text;
    for (const binding of own) bindings.push({ ...binding, trait: rendered });
    return { name: rendered, span: { start: name.span.start, end: close.span.end } };
  }

  protected parseTrait(doc?: string, public_ = false): TraitDecl {
    const start = this.expectText("trait").span.start;
    const name = this.expectKind("identifier", "expected a trait name");
    const genericParameters: string[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
          // Trait parameters are invariant (09-traits.md#generic-traits).
          const marker = this.current();
          if (this.parseVarianceMarker())
            this.fail(
              "invalid-variance",
              "a trait's generic parameters take no variance marker",
              marker.span,
            );
          const parameter = this.expectKind("identifier", "expected a generic trait parameter");
          if (genericParameters.includes(parameter.text))
            this.fail(
              "duplicate-generic-parameter",
              `generic parameter '${parameter.text}' is declared more than once`,
              parameter.span,
            );
          genericParameters.push(parameter.text);
        } while (this.matchText(",") && !this.atText("]"));
      }
      this.expectText("]");
    }
    const supertraits: TypeRef[] = [];
    if (this.matchText("<")) {
      do supertraits.push(this.parseType());
      while (this.matchBoundJoiner());
    }
    if (this.matchText(":")) {
      if (supertraits.length === 0 && !this.atKind("newline")) {
        do supertraits.push(this.parseType());
        while (this.matchBoundJoiner());
        this.expectText(":");
      }
      this.expectKind("newline", "expected a line ending after a trait header");
      this.expectKind("indent", "expected an indented trait body");
      const methods: MethodDecl[] = [];
      const associatedTypes: AssociatedTypeDecl[] = [];
      while (!this.atKind("dedent") && !this.atKind("eof")) {
        if (this.matchKind("newline")) continue;
        const { doc: methodDoc, decorators: methodDecorators } = this.parseMethodPrefix();
        if (!methodDecorators && this.matchText("type")) {
          const associatedName = this.expectKind("identifier", "expected an associated type name");
          const end = this.expectKind("newline", "expected a line ending after an associated type")
            .span.end;
          associatedTypes.push({
            name: associatedName.text,
            doc: methodDoc,
            span: { start: associatedName.span.start, end },
          });
          continue;
        }
        if (!this.atText("fn") && !this.atText("pub"))
          this.fail(
            "doc-comment-without-target",
            "documentation comments must attach to a declaration or member",
            this.current().span,
          );
        if (this.atText("pub"))
          this.fail(
            "trait-method-visibility",
            "trait methods inherit the trait's visibility and cannot be declared pub",
            this.current().span,
          );
        const method = this.parseMethod(false, methodDoc);
        methods.push(methodDecorators ? { ...method, decorators: methodDecorators } : method);
      }
      const close = this.expectKind("dedent", "expected the end of the trait body");
      return {
        kind: "trait",
        ...(public_ ? { public: true } : {}),
        name: name.text,
        genericParameters,
        supertraits,
        associatedTypes,
        methods,
        doc,
        span: { start, end: close.span.end },
      };
    }
    const end = this.expectKind("newline", "expected a line ending after a marker trait").span.end;
    return {
      kind: "trait",
      ...(public_ ? { public: true } : {}),
      name: name.text,
      genericParameters,
      supertraits,
      associatedTypes: [],
      methods: [],
      doc,
      span: { start, end },
    };
  }

  protected parseImpl(doc?: string): ImplDecl {
    const start = this.expectText("impl").span.start;
    const parsedGenerics = this.parseGenericParameters();
    const genericParameters = [...parsedGenerics.parameters];
    const genericBounds = [...parsedGenerics.bounds];
    const enclosingGenericParameters = this.activeGenericParameters;
    this.activeGenericParameters = new Set([...enclosingGenericParameters, ...genericParameters]);
    const first = this.parseType();
    const trait = this.matchText("for") ? first : undefined;
    const target = trait ? this.parseType() : first;
    // `by` is contextual: it delegates to an embedded field, or, without a
    // trait, declares a trait-less derivation block (02 grammar.impl.traitless-by).
    let delegateName: Token | undefined;
    if (this.atText("by") && this.peek(1).kind === "identifier") {
      this.advance();
      delegateName = this.expectKind("identifier", "expected an embedded field name");
    }
    // `by Structure` is never a delegation (spec/09-traits.md#r-trait.by.structure).
    const delegate = !delegateName
      ? {}
      : delegateName.text === "Structure"
        ? { byStructure: delegateName.span }
        : { delegate: { name: delegateName.text, span: delegateName.span } };
    const memberLines: MemberLine[] = [];
    // `impl_decl` may end at its header line, for an inherent implementation
    // too (02-grammar.md#traits-and-implementations).
    if (!this.matchText(":")) {
      const end = this.expectKind("newline", "expected a line ending after an implementation").span
        .end;
      this.activeGenericParameters = enclosingGenericParameters;
      return {
        kind: "impl",
        genericParameters,
        genericBounds,
        ...(trait ? { traitName: trait.name } : {}),
        targetName: target.name,
        ...delegate,
        associatedTypes: [],
        methods: [],
        doc,
        span: { start, end },
      };
    }
    this.expectKind("newline", "expected a line ending after an implementation header");
    this.expectKind("indent", "expected an indented implementation body");
    const methods: MethodDecl[] = [];
    const associatedTypes: AssociatedTypeDecl[] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const { doc: methodDoc, decorators: methodDecorators } = this.parseMethodPrefix();
      const memberLine = methodDecorators ? undefined : this.parseMemberLine();
      if (memberLine) {
        memberLines.push(memberLine);
        continue;
      }
      if (!methodDecorators && this.matchText("type")) {
        const associatedName = this.expectKind("identifier", "expected an associated type name");
        this.expectText("=");
        const value = this.parseType();
        const end = this.expectKind(
          "newline",
          "expected a line ending after an associated type binding",
        ).span.end;
        associatedTypes.push({
          name: associatedName.text,
          value,
          doc: methodDoc,
          span: { start: associatedName.span.start, end },
        });
        continue;
      }
      if (!this.atText("fn") && !this.atText("pub"))
        this.fail(
          "doc-comment-without-target",
          "documentation comments must attach to a declaration or member",
          this.current().span,
        );
      // `pub` is permitted only in an inherent implementation (02 Traits And Implementations).
      if (trait && this.atText("pub"))
        this.fail(
          "trait-method-visibility",
          "trait methods inherit the trait's visibility and cannot be declared pub",
          this.current().span,
        );
      const publicMethod = this.matchText("pub");
      const parsed = this.parseMethod(true, methodDoc);
      const method = methodDecorators ? { ...parsed, decorators: methodDecorators } : parsed;
      methods.push(publicMethod ? { public: true, ...method } : method);
    }
    const close = this.expectKind("dedent", "expected the end of the implementation body");
    this.activeGenericParameters = enclosingGenericParameters;
    return {
      kind: "impl",
      genericParameters,
      genericBounds,
      ...(trait ? { traitName: trait.name } : {}),
      targetName: target.name,
      ...delegate,
      associatedTypes,
      methods,
      ...(memberLines.length > 0 ? { memberLines } : {}),
      doc,
      span: { start, end: close.span.end },
    };
  }

  protected parseMethod(requireBody: boolean, doc?: string): MethodDecl {
    const start = this.expectText("fn").span.start;
    const name = this.expectKind("identifier", "expected a method name");
    const suspending = this.matchText("!");
    const parsedGenerics = this.parseGenericParameters();
    const genericParameters = [...parsedGenerics.parameters];
    const genericBounds = [...parsedGenerics.bounds];
    const reified = parsedGenerics.reified ? { reifiedParameters: parsedGenerics.reified } : {};
    const enclosingGenericParameters = this.activeGenericParameters;
    this.activeGenericParameters = new Set([...enclosingGenericParameters, ...genericParameters]);
    this.expectText("(");
    const parameters: Parameter[] = [];
    if (!this.atText(")")) {
      do {
        const parameterDoc = this.parseDocComments();
        const parameterMetadata = this.parseMethodParameterDecorators();
        const mutableReceiver = this.matchText("mut");
        const mutableStart = mutableReceiver ? this.peek(-1).span.start : undefined;
        const parameterName = this.atText("self")
          ? this.advance()
          : this.expectKind("identifier", "expected a method parameter name");
        if (mutableReceiver && parameterName.text !== "self") {
          this.fail(
            "syntax-error",
            "'mut' in a method parameter list must be followed by self",
            parameterName.span,
          );
        }
        if (parameterName.text === "self") {
          const span = {
            start: mutableStart ?? parameterName.span.start,
            end: parameterName.span.end,
          };
          parameters.push({
            name: "self",
            type: { name: mutableReceiver ? "mut:Self" : "Self", span },
            doc: parameterDoc,
            span,
          });
        } else {
          this.expectText(":");
          const type = this.parseType();
          const variadic = this.matchText("...");
          parameters.push({
            name: parameterName.text,
            type,
            variadic: variadic || undefined,
            doc: parameterDoc,
            ...(parameterMetadata.length > 0 ? { metadata: parameterMetadata } : {}),
            span: { start: parameterName.span.start, end: this.peek(-1).span.end },
          });
        }
      } while (this.matchText(",") && !this.atText(")"));
    }
    const close = this.expectText(")");
    const { result, resultOmitted } = this.parseOptionalResult(close.span);
    const requirementsOmitted = !this.atText("$");
    const requirements = this.matchText("$") ? this.parseRequirements() : [];
    if (!this.atText(":")) {
      if (requireBody)
        this.fail(
          "missing-method-body",
          `implementation method '${name.text}' requires a body`,
          name.span,
        );
      const end = this.expectKind("newline", "expected a line ending after a required method").span
        .end;
      this.activeGenericParameters = enclosingGenericParameters;
      return {
        name: name.text,
        suspending,
        genericParameters,
        genericBounds,
        ...reified,
        parameters,
        result,
        requirements,
        ...(resultOmitted ? { resultOmitted } : {}),
        doc,
        span: { start, end },
      };
    }
    const body = this.parseSuite();
    this.activeGenericParameters = enclosingGenericParameters;
    return {
      name: name.text,
      suspending,
      genericParameters,
      genericBounds,
      ...reified,
      parameters,
      result,
      requirements,
      ...(resultOmitted ? { resultOmitted } : {}),
      ...(requirementsOmitted ? { requirementsOmitted } : {}),
      body,
      doc,
      span: { start, end: body.at(-1)?.span.end ?? result.span.end },
    };
  }

  protected parseRequirementKey(): string {
    // A requirement key has no `mut` prefix: the trait's `mut self` methods
    // decide the access (spec/11-requirements-and-suspension.md#r-req.mut.no-spelling).
    if (this.atText("mut"))
      this.fail(
        "syntax-error",
        "a requirement key has no `mut` prefix; drop `mut`, since the trait's `mut self` methods decide the access",
        this.current().span,
      );
    const name = this.expectKind("identifier", "expected a concrete requirement name");
    if (!this.matchText("[")) return name.text;
    const arguments_: TypeRef[] = [];
    if (!this.atText("]")) {
      // A generic row alias takes a row argument, as in `WithLog[$ Db + Clock]`
      // (11-requirements-and-suspension.md#r-req.row.alias.generic).
      do arguments_.push(this.parseTypeArgument());
      while (this.matchText(",") && !this.atText("]"));
    }
    this.expectText("]");
    if (arguments_.length === 0)
      this.fail(
        "generic-arity",
        `generic requirement '${name.text}' requires type arguments`,
        name.span,
      );
    return `${name.text}[${arguments_.map((argument) => argument.name).join(",")}]`;
  }

  // `type Name[T] = type` or `type Name[T](type)` (02-grammar.md#type-declarations).
  protected parseTypeDecl(doc?: string, public_ = false): TypeDecl {
    const start = this.expectText("type").span.start;
    const name = this.expectKind("identifier", "expected a type name");
    const { parameters: genericParameters } = this.parseGenericParameters();
    const enclosingGenericParameters = this.activeGenericParameters;
    this.activeGenericParameters = new Set([...enclosingGenericParameters, ...genericParameters]);
    let alias: TypeRef | undefined;
    let row: readonly string[] | undefined;
    let base: TypeRef | undefined;
    if (this.matchText("=")) {
      row = this.parseRowAliasTarget();
      if (!row) alias = this.parseType();
    } else {
      this.expectText("(");
      base = this.parseType();
      this.expectText(")");
    }
    this.activeGenericParameters = enclosingGenericParameters;
    const end = this.finishSimpleStatement(false);
    return {
      kind: "type",
      ...(public_ ? { public: true } : {}),
      name: name.text,
      genericParameters,
      ...(alias ? { alias } : {}),
      ...(row ? { row } : {}),
      ...(base ? { base } : {}),
      doc,
      span: { start, end },
    };
  }

  protected parseData(doc?: string, public_ = false): DataDecl {
    const start = this.expectText("data").span.start;
    const name = this.expectKind("identifier", "expected a data type name");
    const genericParameters: string[] = [];
    const variances: VarianceMarker[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
          variances.push(this.parseVarianceMarker());
          const parameter = this.expectKind("identifier", "expected a generic data parameter");
          if (genericParameters.includes(parameter.text))
            this.fail(
              "duplicate-generic-parameter",
              `generic parameter '${parameter.text}' is declared more than once`,
              parameter.span,
            );
          genericParameters.push(parameter.text);
          if (this.atText(":"))
            this.fail(
              "unsupported-generic-data-bound",
              "generic data bounds are introduced after the initial erased-data slice",
              this.current().span,
            );
        } while (this.matchText(",") && !this.atText("]"));
      }
      this.expectText("]");
    }
    this.expectText(":");
    if (this.matchText("pass")) {
      const end = this.peek(-1).span.end;
      this.expectKind("newline", "expected a line ending after a fieldless data declaration");
      return {
        kind: "data",
        ...(public_ ? { public: true } : {}),
        name: name.text,
        genericParameters,
        ...(variances.some(Boolean) ? { variances } : {}),
        fields: [],
        doc,
        span: { start, end },
      };
    }
    this.expectKind("newline", "expected a line ending after a data header");
    this.expectKind("indent", "expected an indented data body");
    const fields: DataDecl["fields"][number][] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const { doc: fieldDoc, metadata } = this.parseMemberPrefix();
      if (this.matchText("mut")) {
        const candidate = this.current();
        const code =
          this.peek(1).text === ":" ? "mutable-field-modifier" : "mutable-embedded-field";
        this.fail(
          code,
          "data fields express mutable access in their type rather than with a field modifier",
          candidate.span,
        );
      }
      if (this.atText("pub") && this.peek(1).kind === "identifier" && this.peek(2).text !== ":")
        this.fail(
          "syntax-error",
          "an embedded field takes no 'pub' marker: embedded fields are always public",
          this.current().span,
        );
      if (this.current().kind === "identifier" && this.peek(1).text !== ":") {
        const type = this.parseType();
        if (this.atText("="))
          this.fail(
            "embedded-field-default",
            "an embedded field cannot declare a default",
            this.current().span,
          );
        this.expectKind("newline", "expected a line ending after an embedded field");
        const genericStart = type.name.indexOf("[");
        const name = genericStart < 0 ? type.name : type.name.slice(0, genericStart);
        fields.push({ name, type, embedded: true, doc: fieldDoc, ...metadata, span: type.span });
        continue;
      }
      const publicField = this.matchText("pub");
      const fieldName = this.expectKind("identifier", "expected a data field name");
      this.expectText(":");
      const type = this.parseType();
      const defaultValue = this.matchText("=") ? this.parseExpression() : undefined;
      const end = defaultValue?.span.end ?? this.peek(-1).span.end;
      this.expectKind("newline", "expected a line ending after a data field");
      fields.push({
        ...(publicField ? { public: true } : {}),
        name: fieldName.text,
        type,
        default: defaultValue,
        doc: fieldDoc,
        ...metadata,
        span: { start: fieldName.span.start, end },
      });
    }
    const close = this.expectKind("dedent", "expected the end of the data body");
    return {
      kind: "data",
      ...(public_ ? { public: true } : {}),
      name: name.text,
      genericParameters,
      ...(variances.some(Boolean) ? { variances } : {}),
      fields,
      doc,
      span: { start, end: close.span.end },
    };
  }

  protected parseEnum(doc?: string, public_ = false): EnumDecl {
    const start = this.expectText("enum").span.start;
    const name = this.expectKind("identifier", "expected an enum type name");
    const genericParameters: string[] = [];
    const variances: VarianceMarker[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
          variances.push(this.parseVarianceMarker());
          const parameter = this.expectKind("identifier", "expected a generic enum parameter");
          if (genericParameters.includes(parameter.text))
            this.fail(
              "duplicate-generic-parameter",
              `generic parameter '${parameter.text}' is declared more than once`,
              parameter.span,
            );
          genericParameters.push(parameter.text);
          if (this.atText(":"))
            this.fail(
              "unsupported-generic-enum-bound",
              "generic enum bounds are introduced after the initial erased-enum slice",
              this.current().span,
            );
        } while (this.matchText(",") && !this.atText("]"));
      }
      this.expectText("]");
    }
    const sharedFields: EnumDecl["sharedFields"][number][] = [];
    if (this.matchText("(")) {
      if (!this.atText(")")) {
        do {
          const parameterStart = this.current().span.start;
          const explicitName =
            this.current().kind === "identifier" && this.peek(1).text === ":"
              ? this.advance()
              : undefined;
          if (explicitName) this.expectText(":");
          const type = this.parseType();
          const defaultValue = this.matchText("=") ? this.parseExpression() : undefined;
          sharedFields.push({
            name: explicitName?.text ?? String(sharedFields.length),
            type,
            default: defaultValue,
            span: { start: parameterStart, end: defaultValue?.span.end ?? type.span.end },
          });
        } while (this.matchText(",") && !this.atText(")"));
      }
      this.expectText(")");
    }
    this.expectText(":");
    this.expectKind("newline", "expected a line ending after an enum header");
    this.expectKind("indent", "expected an indented enum body");
    const variants: EnumDecl["variants"][number][] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const { doc: variantDoc, metadata: variantMetadata } = this.parseMemberPrefix();
      const variantName = this.expectKind("identifier", "expected an enum variant name");
      const fields: EnumDecl["variants"][number]["fields"][number][] = [];
      if (this.matchText("(")) {
        if (!this.atText(")")) {
          do {
            const fieldDoc = this.parseDocComments();
            const fieldStart = this.current().span.start;
            const payloadMetadata = this.parseMemberDecorators(true);
            // An unnamed positional payload field is named by its position
            // (08-data-and-enums.md#variant-payloads).
            const fieldName =
              this.current().kind === "identifier" && this.peek(1).text === ":"
                ? this.advance().text
                : undefined;
            if (fieldName !== undefined) this.expectText(":");
            const type = this.parseType();
            fields.push({
              name: fieldName ?? String(fields.length),
              type,
              doc: fieldDoc,
              ...(payloadMetadata.length > 0 ? { metadata: payloadMetadata } : {}),
              ...(fieldName === undefined ? { positional: true } : {}),
              span: { start: fieldStart, end: type.span.end },
            });
          } while (this.matchText(",") && !this.atText(")"));
        }
        this.expectText(")");
      }
      const result = this.matchText("->") ? this.parseExpression() : undefined;
      const end = result?.span.end ?? this.peek(-1).span.end;
      this.expectKind("newline", "expected a line ending after an enum variant");
      variants.push({
        name: variantName.text,
        fields,
        result,
        doc: variantDoc,
        ...variantMetadata,
        span: { start: variantName.span.start, end },
      });
    }
    const close = this.expectKind("dedent", "expected the end of the enum body");
    if (variants.length === 0)
      this.fail("empty-enum", "an enum must declare at least one variant", name.span);
    return {
      kind: "enum",
      ...(public_ ? { public: true } : {}),
      name: name.text,
      genericParameters,
      ...(variances.some(Boolean) ? { variances } : {}),
      sharedFields,
      variants,
      doc,
      span: { start, end: close.span.end },
    };
  }

  protected parseType(): TypeRef {
    const rowless = this.rowlessResult;
    this.rowlessResult = false;
    if (this.matchText("_")) return { name: "_", span: this.peek(-1).span };
    if (this.matchText("(")) {
      const start = this.peek(-1).span.start;
      const elements: TypeRef[] = [];
      let tuple = false;
      if (!this.atText(")")) {
        elements.push(this.parseType());
        if (this.matchText(",")) {
          tuple = true;
          while (!this.atText(")")) {
            elements.push(this.parseType());
            if (!this.matchText(",")) break;
          }
        }
      } else {
        tuple = true;
      }
      const close = this.expectText(")");
      let rendered = tuple
        ? `(${elements.map((element) => element.name).join(",")}${elements.length === 1 ? "," : ""})`
        : elements[0]!.name;
      let end = close.span.end;
      while (this.matchText("?")) {
        rendered = optionalType(rendered);
        end = this.peek(-1).span.end;
      }
      return { name: rendered, span: { start, end } };
    }
    if (this.matchText("mut")) {
      const start = this.peek(-1).span.start;
      // A function type carries no access permission
      // (02-grammar.md#r-grammar.type.mut.no-function).
      if (this.atText("fn"))
        this.fail(
          "syntax-error",
          "'mut' cannot precede a function type; function values carry no permission",
          this.peek(-1).span,
        );
      this.rowlessResult = rowless;
      const inner = this.parseType();
      if (inner.name.startsWith("mut:")) {
        this.fail(
          "duplicate-mutable-permission",
          "a type cannot apply 'mut' permission twice",
          inner.span,
        );
      }
      return {
        name: `mut:${inner.name}`,
        span: { start, end: inner.span.end },
      };
    }
    if (this.matchText("$")) {
      const start = this.peek(-1).span.start;
      this.expectText(".");
      const context = this.expectKind("identifier", "expected Context after '$.'");
      if (context.text !== "Context")
        this.fail("syntax-error", "expected Context after '$.'", context.span);
      this.expectText("[");
      // `$.Context[Key]` or `$.Context[$ A + B]`; `$()` is the empty context.
      // A bare key is one key, so a row alias there is a kind mismatch
      // (11-requirements-and-suspension.md#r-req.row.alias.kind).
      const keyStart = this.current().span.start;
      const row = this.matchText("$");
      const requirements = row ? this.parseRequirements(false) : [this.parseRowKey()];
      if (!row)
        this.bareContextKeys.push({
          key: requirements[0]!,
          span: { start: keyStart, end: this.peek(-1).span.end },
        });
      const close = this.expectText("]");
      return { name: `context:${requirements.join("+")}`, span: { start, end: close.span.end } };
    }
    if (this.matchText("fn")) {
      const start = this.peek(-1).span.start;
      const suspending = this.matchText("!");
      this.expectText("(");
      const parameters: FunctionTypeParameter[] = [];
      if (!this.atText(")")) {
        do {
          const type = this.parseType();
          const variadic = this.matchText("...");
          parameters.push({ type, variadic });
          if (variadic && this.atText(",") && this.peek(1).text !== ")") {
            this.fail(
              "nonfinal-vararg",
              "a variadic function-type parameter must be final",
              this.peek(-1).span,
            );
          }
        } while (this.matchText(",") && !this.atText(")"));
      }
      this.expectText(")");
      this.expectText("->");
      this.rowlessResult = rowless;
      const result = this.parseType();
      const hasRequirements = !rowless && this.matchText("$");
      const requirements = hasRequirements ? this.parseRequirements(false) : [];
      const end = hasRequirements ? this.peek(-1).span.end : result.span.end;
      const row = requirements.length ? `$${requirements.join("+")}` : "";
      return {
        name: `fn${suspending ? "!" : ""}(${parameters.map((parameter) => `${parameter.type.name}${parameter.variadic ? "..." : ""}`).join(",")})->${result.name}${row}`,
        span: { start, end },
      };
    }
    const name = this.atText("Self")
      ? this.advance()
      : this.expectKind("identifier", "expected a type name");
    let rendered = name.text;
    let end = name.span.end;
    if (this.matchText("[")) {
      const arguments_: TypeRef[] = [];
      if (!this.atText("]")) {
        do arguments_.push(this.parseTypeArgument());
        while (this.matchText(",") && !this.atText("]"));
      }
      const close = this.expectText("]");
      if (arguments_.length === 0)
        this.fail(
          "generic-arity",
          `generic type '${name.text}' requires type arguments`,
          name.span,
        );
      // `Option[T]` is exactly `T?`; both spellings render to one type.
      rendered =
        name.text === "Option" && arguments_.length === 1
          ? optionalType(arguments_[0]!.name)
          : `${name.text}[${arguments_.map((argument) => argument.name).join(",")}]`;
      end = close.span.end;
    }
    if (this.matchText("::")) {
      const member = this.expectKind("identifier", "expected an associated type name");
      rendered = `${rendered}::${member.text}`;
      end = member.span.end;
    }
    while (this.matchText("?")) {
      rendered += "?";
      end = this.peek(-1).span.end;
    }
    if (this.atText("("))
      this.fail(
        "unsupported-type-form",
        "function and tuple types are introduced with closure support",
        this.current().span,
      );
    return { name: rendered, span: { start: name.span.start, end } };
  }

  protected parseSuite(closureBody = false): readonly Statement[] {
    const colonIndex = this.index;
    this.expectText(":");
    // An indented body nested inside brackets gets its layout now.
    this.openNestedLayout(colonIndex, closureBody);
    if (!this.atKind("newline")) {
      this.inlineSuiteDepths.push(this.delimiterDepth(colonIndex));
      try {
        return [this.parseStatement(true)];
      } finally {
        this.inlineSuiteDepths.pop();
      }
    }
    this.expectKind("newline", "expected a line ending after ':'");
    this.expectKind("indent", "expected an indented suite");
    const statements: Statement[] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      statements.push(this.parseStatement(false));
    }
    this.expectKind("dedent", "expected the end of the indented suite");
    if (statements.length === 0)
      this.fail("empty-suite", "an indented suite must contain a statement", this.current().span);
    return statements;
  }

  private localDeclarations = false;

  // `data`, `enum`, `trait`, `type`, and `impl` may be declared in a block
  // suite (03-names-and-scopes.md#module-scope).
  private parseLocalDeclaration(): Statement | undefined {
    const declaration = this.atText("data")
      ? this.parseData()
      : this.atText("enum")
        ? this.parseEnum()
        : this.atText("trait")
          ? this.parseTrait()
          : this.atText("impl")
            ? this.parseImpl()
            : this.atText("type") && this.peek(1).kind === "identifier"
              ? this.parseTypeDecl()
              : undefined;
    if (!declaration) return undefined;
    this.localDeclarations = true;
    return { kind: "local-declaration", declaration, span: declaration.span };
  }

  protected parseStatement(topOrInline: boolean): Statement {
    const start = this.current().span.start;
    const local = this.parseLocalDeclaration();
    if (local) return local;
    if (this.atText("@")) {
      const alias = this.aliasAfterDeriveLines();
      if (alias)
        this.fail(
          "syntax-error",
          "a transparent alias takes no derive decorator; derive on a data, enum, or newtype declaration",
          alias,
        );
      this.fail(
        "decorator-not-top-level",
        "decorators are only valid on top-level declarations",
        this.current().span,
      );
    }
    if (this.atKind("doc-comment")) {
      this.fail(
        "doc-comment-without-target",
        "documentation comments cannot attach to executable statements",
        this.current().span,
      );
    }
    if (this.matchText("defer")) {
      const body = this.parseSuite();
      return { kind: "defer", body, span: { start, end: body.at(-1)!.span.end } };
    }
    if (this.atText("fn") && this.peek(1).kind === "identifier") return this.parseLocalFunction();
    if (this.matchText("let")) {
      const names = [this.expectKind("identifier", "expected a binding name")];
      this.rejectCommaClosingInlineSuite();
      while (this.matchText(","))
        names.push(this.expectKind("identifier", "expected a binding name after ','"));
      const annotation = this.matchText(":") ? this.parseType() : undefined;
      this.expectText("=");
      const value = this.parseRightSide();
      const end = this.finishExpressionStatement(value, topOrInline);
      return names.length === 1
        ? {
            kind: "binding",
            name: names[0]!.text,
            annotation,
            mutable: true,
            value,
            span: { start, end },
          }
        : {
            kind: "tuple-binding",
            bindings: names.map((name) => ({ name: name.text, span: name.span })),
            annotation,
            mutable: true,
            value,
            span: { start, end },
          };
    }
    // A trailing block may complete each right-hand side that accepts a suite
    // expression (02-grammar.md#statements).
    if (this.matchText("return")) {
      const value = this.atValuelessEnd(topOrInline) ? undefined : this.parseRightSide();
      const end = value
        ? this.finishExpressionStatement(value, topOrInline)
        : this.finishSimpleStatement(topOrInline);
      return { kind: "return", value, span: { start, end } };
    }
    if (this.matchText("break")) {
      const value = this.atValuelessEnd(topOrInline) ? undefined : this.parseRightSide();
      const end = value
        ? this.finishExpressionStatement(value, topOrInline)
        : this.finishSimpleStatement(topOrInline);
      return { kind: "break", value, span: { start, end } };
    }
    if (this.matchText("continue")) {
      const end = this.finishSimpleStatement(topOrInline);
      return { kind: "continue", span: { start, end } };
    }
    if (this.matchText("pass")) {
      const end = this.finishSimpleStatement(topOrInline);
      return { kind: "pass", span: { start, end } };
    }
    if (this.matchText("_")) {
      this.expectText(":=");
      const value = this.parseRightSide();
      const end = this.finishExpressionStatement(value, topOrInline);
      return { kind: "discard", value, span: { start, end } };
    }
    if (this.atKind("identifier") && this.peek(1).text === ":") {
      if (this.peek(2).kind !== "newline")
        this.fail(
          "missing-let",
          "a typed mutable binding must begin with 'let'",
          this.current().span,
        );
    }
    if (this.atKind("identifier") && this.peek(1).text === ":=") {
      const name = this.advance();
      this.advance();
      const value = this.parseTrailingBlockCall(this.parseExpression());
      const end = this.finishExpressionStatement(value, topOrInline);
      return { kind: "binding", name: name.text, mutable: false, value, span: { start, end } };
    }
    if (this.atKind("identifier") && this.peek(1).text === ",") {
      const names = [this.advance()];
      this.rejectCommaClosingInlineSuite();
      while (this.matchText(","))
        names.push(this.expectKind("identifier", "expected a binding name after ','"));
      this.expectText(":=");
      const value = this.parseTrailingBlockCall(this.parseExpression());
      const end = this.finishExpressionStatement(value, topOrInline);
      return {
        kind: "tuple-binding",
        bindings: names.map((name) => ({ name: name.text, span: name.span })),
        mutable: false,
        value,
        span: { start, end },
      };
    }
    if (this.atKind("identifier") && (this.peek(1).text === "=" || this.peek(1).text === "...=")) {
      const name = this.advance();
      const copy = this.advance().text === "...=";
      const value = this.parseRightSide();
      const end = this.finishExpressionStatement(value, topOrInline);
      return {
        kind: "assignment",
        name: name.text,
        value,
        ...(copy ? { copy } : {}),
        span: { start, end },
      };
    }
    const expression = this.parseTrailingBlockCall(this.parseExpression());
    if (this.atText("=") || this.atText("...=")) {
      // `place ...= value` is the copy assignment into an embedded field; the
      // checker rejects it on any other place (02-grammar.md#statements).
      const copy = this.advance().text === "...=";
      const value = this.parseRightSide();
      const end = this.finishExpressionStatement(value, topOrInline);
      const marker = copy ? { copy } : {};
      if (expression.kind === "member")
        return {
          kind: "field-assignment",
          target: expression,
          value,
          ...marker,
          span: { start, end },
        };
      if (expression.kind === "index")
        return {
          kind: "index-assignment",
          target: expression,
          value,
          ...marker,
          span: { start, end },
        };
      this.fail(
        "invalid-assignment-target",
        "assignment requires a binding, data member, list element, or map entry",
        expression.span,
      );
    }
    const end = this.finishExpressionStatement(expression, topOrInline);
    return { kind: "expression", expression, span: { start, end } };
  }

  // The right side of `let ... =`, `=`, `_ :=`, `return`, and `break`: an
  // expression or trailing block call. A nested binding there cannot end in a
  // suite unless parenthesized (02-grammar.md#statements).
  private parseRightSide(): Expression {
    const value = this.parseTrailingBlockCall(this.parseExpression());
    if (value.kind !== "binding-expression") return value;
    let inner: Expression = value;
    while (inner.kind === "binding-expression") inner = inner.value;
    const last = this.peek(-1);
    const parenthesized = last.text === ")" && last.span.end.offset > value.span.end.offset;
    const suiteKinds = ["if", "for", "while", "match", "closure", "provider-with"];
    if (!parenthesized && suiteKinds.includes(inner.kind))
      this.fail(
        "syntax-error",
        "a nested binding that ends in a suite must be parenthesized",
        value.span,
      );
    return value;
  }

  protected parseTrailingBlockCall(callee: Expression): Expression {
    if (!this.atText(":")) return callee;
    const body = this.parseSuite();
    if (this.atText(":"))
      this.fail(
        "trailing-block-position",
        "a call accepts only one trailing callback block",
        this.current().span,
      );
    const callback: Expression = {
      kind: "closure",
      trailing: true,
      parameters: [],
      body,
      span: { start: callee.span.end, end: body.at(-1)!.span.end },
    };
    if (callee.kind === "call") {
      return {
        ...callee,
        arguments: [...callee.arguments, callback],
        argumentNames: callee.argumentNames ? [...callee.argumentNames, undefined] : undefined,
        argumentSpreads: callee.argumentSpreads ? [...callee.argumentSpreads, false] : undefined,
        span: { start: callee.span.start, end: callback.span.end },
      };
    }
    if (callee.kind === "suspend-call") {
      return {
        ...callee,
        arguments: [...callee.arguments, callback],
        argumentNames: callee.argumentNames ? [...callee.argumentNames, undefined] : undefined,
        argumentSpreads: callee.argumentSpreads ? [...callee.argumentSpreads, false] : undefined,
        span: { start: callee.span.start, end: callback.span.end },
      };
    }
    return {
      kind: "call",
      callee,
      arguments: [callback],
      span: { start: callee.span.start, end: callback.span.end },
    };
  }

  protected finishSimpleStatement(_topOrInline: boolean): SourceSpan["end"] {
    const previous = this.peek(-1);
    if (this.matchKind("newline")) return previous.span.end;
    if (this.atKind("dedent") || this.atKind("eof")) return previous.span.end;
    // A same-line suite also ends before the `else` of its conditional or loop.
    if (_topOrInline && [")", ",", "]", "}", "else"].some((text) => this.atText(text)))
      return previous.span.end;
    this.fail(
      "syntax-error",
      `expected a line ending, found '${this.current().text}'`,
      this.current().span,
    );
  }

  protected finishExpressionStatement(
    expression: Expression,
    topOrInline: boolean,
  ): SourceSpan["end"] {
    if (this.peek(-1).kind === "dedent") return expression.span.end;
    if (["if", "for", "while", "match", "closure", "provider-with"].includes(expression.kind))
      return expression.span.end;
    return this.finishSimpleStatement(topOrInline);
  }
}

export function parse(source: string, options: ParseOptions = {}): ParseResult {
  const lexed = lex(source);
  if (lexed.diagnostics.length > 0) return { diagnostics: lexed.diagnostics };
  return new Parser(lexed.tokens).withOptions(options).parse();
}
