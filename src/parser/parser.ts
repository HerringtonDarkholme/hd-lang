import type {
  Expression,
  AssociatedTypeDecl,
  DataDecl,
  EnumDecl,
  FunctionDecl,
  GenericBound,
  ImplDecl,
  MethodDecl,
  Parameter,
  Program,
  Statement,
  TestDecl,
  TraitDecl,
  TypeRef,
  UseDecl,
  UseName,
} from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import { lex } from "../lexer.ts";
import { ExpressionParser } from "./expression.ts";
import { ParseFailure, type ExpressionParseResult } from "./base.ts";

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
}

class Parser extends ExpressionParser {
  parse(): ParseResult {
    const uses: UseDecl[] = [];
    const functions: FunctionDecl[] = [];
    const data: DataDecl[] = [];
    const enums: EnumDecl[] = [];
    const traits: TraitDecl[] = [];
    const implementations: ImplDecl[] = [];
    const tests: TestDecl[] = [];
    const statements: Statement[] = [];
    const start = this.current().span.start;
    try {
      while (!this.atKind("eof")) {
        if (this.matchKind("newline")) continue;
        const doc = this.parseDocComments();
        if (this.atText("struct"))
          this.fail(
            "old-struct-declaration",
            "'struct' was replaced by 'data'",
            this.current().span,
          );
        if (this.atText("import"))
          this.fail(
            "old-import-declaration",
            "'import' was replaced by 'use'",
            this.current().span,
          );
        if (this.atText("export"))
          this.fail(
            "old-export-declaration",
            "'export' was replaced by 'pub use'",
            this.current().span,
          );
        if (this.atText("use") || (this.atText("pub") && this.peek(1).text === "use")) {
          if (doc)
            this.fail(
              "doc-comment-without-target",
              "documentation comments cannot attach to a use declaration",
              this.current().span,
            );
          uses.push(this.parseUse());
          continue;
        }
        const public_ = this.matchText("pub");
        if (this.atText("fn")) functions.push(this.parseFunction(doc, public_));
        else if (this.atText("data")) data.push(this.parseData(doc, public_));
        else if (this.atText("enum")) enums.push(this.parseEnum(doc, public_));
        else if (this.atText("trait")) traits.push(this.parseTrait(doc, public_));
        else if (this.atText("impl")) implementations.push(this.parseImpl(doc));
        else if (this.atText("test") && this.peek(1).kind === "string")
          tests.push(this.parseTest(doc));
        else {
          if (doc)
            this.fail(
              "doc-comment-without-target",
              "documentation comments must attach to a declaration or member",
              this.current().span,
            );
          statements.push(this.parseStatement(true));
        }
      }
    } catch (error) {
      if (!(error instanceof ParseFailure)) throw error;
      return { diagnostics: this.diagnostics };
    }
    return {
      program: {
        uses,
        data,
        enums,
        traits,
        implementations,
        functions,
        tests,
        statements,
        span: { start, end: this.current().span.end },
      },
      diagnostics: this.diagnostics,
    };
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
        if (["self", "Self", "super", "shape"].includes(this.current().text)) {
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
            "vararg-default",
            "a variadic parameter cannot declare a default",
            defaultValue.span,
          );
        parameters.push({
          name: parameterName.text,
          type,
          variadic: variadic || undefined,
          default: defaultValue,
          doc: parameterDoc,
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
    if (this.matchText("->")) return { result: this.parseType(), resultOmitted: false };
    if (!this.atText("$") && !this.atText(":") && !this.atKind("newline")) this.expectText("->");
    return { result: { name: "void", span: close }, resultOmitted: true };
  }

  protected parseGenericParameters(): ParsedGenericParameters {
    const parameters: string[] = [];
    const bounds: GenericBound[] = [];
    if (!this.matchText("[")) return { parameters, bounds };
    if (!this.atText("]")) {
      do {
        const parameter = this.expectKind("identifier", "expected a generic parameter name");
        if (parameters.includes(parameter.text))
          this.fail(
            "duplicate-generic-parameter",
            `generic parameter '${parameter.text}' is declared more than once`,
            parameter.span,
          );
        parameters.push(parameter.text);
        if (this.matchText("<") || this.matchText(":")) {
          const traits = this.parseTraitBoundNames();
          bounds.push({
            parameter: parameter.text,
            traits,
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
    return { parameters, bounds };
  }

  protected parseTraitBoundNames(): string[] {
    const mutable = this.matchText("mut");
    const traits: string[] = [];
    do {
      const trait = this.parseType();
      traits.push(mutable ? `mut:${trait.name}` : trait.name);
    } while (this.matchText("+"));
    return traits;
  }

  protected parseTest(doc?: string): TestDecl {
    if (doc)
      this.fail(
        "doc-comment-without-target",
        "documentation comments cannot attach to a test block",
        this.current().span,
      );
    const start = this.expectText("test").span.start;
    const name = this.expectKind("string", "expected a test name");
    const body = this.parseSuite();
    return {
      kind: "test",
      name: String(name.value ?? name.text),
      body,
      span: { start, end: body.at(-1)!.span.end },
    };
  }

  protected parseUse(): UseDecl {
    const public_ = this.matchText("pub");
    const start = this.expectText("use").span.start;
    const parts = [this.expectKind("identifier", "expected a module path after use").text];
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
      if (!this.atText("}")) {
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
      }
      this.expectText("}");
    } else {
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

  protected parseTrait(doc?: string, public_ = false): TraitDecl {
    const start = this.expectText("trait").span.start;
    const name = this.expectKind("identifier", "expected a trait name");
    const genericParameters: string[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
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
      while (this.matchText("+"));
    }
    if (this.matchText(":")) {
      if (supertraits.length === 0 && !this.atKind("newline")) {
        do supertraits.push(this.parseType());
        while (this.matchText("+"));
        this.expectText(":");
      }
      this.expectKind("newline", "expected a line ending after a trait header");
      this.expectKind("indent", "expected an indented trait body");
      const methods: MethodDecl[] = [];
      const associatedTypes: AssociatedTypeDecl[] = [];
      while (!this.atKind("dedent") && !this.atKind("eof")) {
        if (this.matchKind("newline")) continue;
        const methodDoc = this.parseDocComments();
        if (this.matchText("type")) {
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
        methods.push(this.parseMethod(false, methodDoc));
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
    if (this.matchText("where")) {
      do {
        const parameter = this.parseType();
        if (!this.matchText("<")) this.expectText(":");
        const traits = this.parseTraitBoundNames();
        genericBounds.push({
          parameter: parameter.name,
          traits,
          span: { start: parameter.span.start, end: this.peek(-1).span.end },
        });
      } while (this.matchText(",") && !this.atText(":"));
    }
    if (!this.matchText(":")) {
      if (!trait)
        this.fail(
          "missing-impl-body",
          `inherent implementation for '${target.name}' requires a body`,
          target.span,
        );
      const end = this.expectKind("newline", "expected a line ending after an implementation").span
        .end;
      this.activeGenericParameters = enclosingGenericParameters;
      return {
        kind: "impl",
        genericParameters,
        genericBounds,
        traitName: trait.name,
        targetName: target.name,
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
      const methodDoc = this.parseDocComments();
      if (this.matchText("type")) {
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
      if (!this.atText("fn"))
        this.fail(
          "doc-comment-without-target",
          "documentation comments must attach to a declaration or member",
          this.current().span,
        );
      methods.push(this.parseMethod(true, methodDoc));
    }
    const close = this.expectKind("dedent", "expected the end of the implementation body");
    this.activeGenericParameters = enclosingGenericParameters;
    return {
      kind: "impl",
      genericParameters,
      genericBounds,
      ...(trait ? { traitName: trait.name } : {}),
      targetName: target.name,
      associatedTypes,
      methods,
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
    const enclosingGenericParameters = this.activeGenericParameters;
    this.activeGenericParameters = new Set([...enclosingGenericParameters, ...genericParameters]);
    this.expectText("(");
    const parameters: Parameter[] = [];
    if (!this.atText(")")) {
      do {
        const parameterDoc = this.parseDocComments();
        const mutableReceiver = this.matchText("mut");
        const mutableStart = mutableReceiver ? this.peek(-1).span.start : undefined;
        const parameterName = this.atText("self")
          ? this.advance()
          : this.expectKind("identifier", "expected a method parameter name");
        if (mutableReceiver && parameterName.text !== "self") {
          this.fail(
            "expected-token",
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

  protected parseRequirements(): readonly string[] {
    if (this.atText("(") && this.peek(1).text === ")") {
      this.advance();
      this.advance();
      return [];
    }
    return [...this.parseRequirementExpression()].sort();
  }

  protected parseRequirementExpression(): Set<string> {
    const requirements = this.parseRequirementUnion();
    while (this.matchText("-")) {
      const removed = this.parseRequirementKey();
      for (const requirement of Array.from(requirements)) {
        const [base, ...excluded] = requirement.split("\\");
        if (this.activeGenericParameters.has(base!)) {
          requirements.delete(requirement);
          requirements.add([base, ...new Set([...excluded, removed])].join("\\"));
        } else if (base === removed) {
          requirements.delete(requirement);
        }
      }
    }
    return requirements;
  }

  protected parseRequirementUnion(): Set<string> {
    const requirements = this.parseRequirementTerm();
    while (this.matchText("+")) {
      for (const requirement of this.parseRequirementTerm())
        this.unionRequirement(requirements, requirement);
    }
    return requirements;
  }

  protected unionRequirement(requirements: Set<string>, added: string): void {
    const [addedBase, ...addedExcluded] = added.split("\\");
    if (this.activeGenericParameters.has(addedBase!)) {
      const concrete = new Set(
        [...requirements].filter(
          (requirement) =>
            !requirement.includes("\\") && !this.activeGenericParameters.has(requirement),
        ),
      );
      const effectiveExcluded = addedExcluded.filter((key) => !concrete.has(key));
      const existing = [...requirements].find(
        (requirement) => requirement.split("\\")[0] === addedBase,
      );
      if (existing) {
        requirements.delete(existing);
        const existingExcluded = new Set(existing.split("\\").slice(1));
        const intersection = effectiveExcluded.filter((key) => existingExcluded.has(key));
        requirements.add([addedBase, ...intersection].join("\\"));
      } else {
        requirements.add([addedBase, ...effectiveExcluded].join("\\"));
      }
      return;
    }
    for (const requirement of Array.from(requirements)) {
      const [base, ...excluded] = requirement.split("\\");
      if (!this.activeGenericParameters.has(base!) || !excluded.includes(addedBase!)) continue;
      requirements.delete(requirement);
      requirements.add([base, ...excluded.filter((key) => key !== addedBase)].join("\\"));
    }
    requirements.add(added);
  }

  protected parseRequirementTerm(): Set<string> {
    if (this.matchText("(")) {
      const requirements = this.parseRequirementExpression();
      this.expectText(")");
      return requirements;
    }
    return new Set([this.parseRequirementKey()]);
  }

  protected parseRequirementKey(): string {
    const name = this.expectKind("identifier", "expected a concrete requirement name");
    if (!this.matchText("[")) return name.text;
    const arguments_: TypeRef[] = [];
    if (!this.atText("]")) {
      do arguments_.push(this.parseType());
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

  protected parseData(doc?: string, public_ = false): DataDecl {
    const start = this.expectText("data").span.start;
    const name = this.expectKind("identifier", "expected a data type name");
    const genericParameters: string[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
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
      const fieldDoc = this.parseDocComments();
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
        fields.push({ name, type, embedded: true, doc: fieldDoc, span: type.span });
        continue;
      }
      const fieldName = this.expectKind("identifier", "expected a data field name");
      this.expectText(":");
      const type = this.parseType();
      const defaultValue = this.matchText("=") ? this.parseExpression() : undefined;
      const end = defaultValue?.span.end ?? this.peek(-1).span.end;
      this.expectKind("newline", "expected a line ending after a data field");
      fields.push({
        name: fieldName.text,
        type,
        default: defaultValue,
        doc: fieldDoc,
        span: { start: fieldName.span.start, end },
      });
    }
    const close = this.expectKind("dedent", "expected the end of the data body");
    return {
      kind: "data",
      ...(public_ ? { public: true } : {}),
      name: name.text,
      genericParameters,
      fields,
      doc,
      span: { start, end: close.span.end },
    };
  }

  protected parseEnum(doc?: string, public_ = false): EnumDecl {
    const start = this.expectText("enum").span.start;
    const name = this.expectKind("identifier", "expected an enum type name");
    const genericParameters: string[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
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
      const variantDoc = this.parseDocComments();
      const variantName = this.expectKind("identifier", "expected an enum variant name");
      const fields: EnumDecl["variants"][number]["fields"][number][] = [];
      if (this.matchText("(")) {
        if (!this.atText(")")) {
          do {
            const fieldDoc = this.parseDocComments();
            const fieldName = this.expectKind("identifier", "expected a variant field name");
            this.expectText(":");
            const type = this.parseType();
            fields.push({
              name: fieldName.text,
              type,
              doc: fieldDoc,
              span: { start: fieldName.span.start, end: type.span.end },
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
      sharedFields,
      variants,
      doc,
      span: { start, end: close.span.end },
    };
  }

  protected parseType(): TypeRef {
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
        rendered += "?";
        end = this.peek(-1).span.end;
      }
      return { name: rendered, span: { start, end } };
    }
    if (this.matchText("mut")) {
      const start = this.peek(-1).span.start;
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
        this.fail("expected-token", "expected Context after '$.'", context.span);
      this.expectText("[");
      const requirements = this.parseRequirements();
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
      const result = this.parseType();
      const hasRequirements = this.matchText("$");
      const requirements = hasRequirements ? this.parseRequirements() : [];
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
        do arguments_.push(this.parseType());
        while (this.matchText(",") && !this.atText("]"));
      }
      const close = this.expectText("]");
      if (arguments_.length === 0)
        this.fail(
          "generic-arity",
          `generic type '${name.text}' requires type arguments`,
          name.span,
        );
      rendered = `${name.text}[${arguments_.map((argument) => argument.name).join(",")}]`;
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

  protected parseSuite(): readonly Statement[] {
    this.expectText(":");
    if (this.matchKind("newline")) {
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
    return [this.parseStatement(true)];
  }

  protected parseStatement(topOrInline: boolean): Statement {
    const start = this.current().span.start;
    if (this.atText("@"))
      this.fail(
        "decorator-not-top-level",
        "decorators are only valid on top-level declarations",
        this.current().span,
      );
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
      while (this.matchText(","))
        names.push(this.expectKind("identifier", "expected a binding name after ','"));
      const annotation = this.matchText(":") ? this.parseType() : undefined;
      this.expectText("=");
      const value = this.parseTrailingBlockCall(this.parseExpression());
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
    if (this.matchText("return")) {
      const value =
        this.atKind("newline") || this.atKind("dedent") ? undefined : this.parseExpression();
      const end = value
        ? this.finishExpressionStatement(value, topOrInline)
        : this.finishSimpleStatement(topOrInline);
      return { kind: "return", value, span: { start, end } };
    }
    if (this.matchText("break")) {
      const value =
        this.atKind("newline") || this.atKind("dedent") || this.atKind("eof")
          ? undefined
          : this.parseExpression();
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
      const value = this.parseExpression();
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
    if (this.atKind("identifier") && this.peek(1).text === "=") {
      const name = this.advance();
      this.advance();
      const value = this.parseExpression();
      const end = this.finishExpressionStatement(value, topOrInline);
      return { kind: "assignment", name: name.text, value, span: { start, end } };
    }
    const expression = this.parseTrailingBlockCall(this.parseExpression());
    if (this.matchText("=")) {
      const value = this.parseExpression();
      const end = this.finishExpressionStatement(value, topOrInline);
      if (expression.kind === "member")
        return { kind: "field-assignment", target: expression, value, span: { start, end } };
      if (expression.kind === "index")
        return { kind: "index-assignment", target: expression, value, span: { start, end } };
      this.fail(
        "invalid-assignment-target",
        "assignment requires a binding, data member, list element, or map entry",
        expression.span,
      );
    }
    const end = this.finishExpressionStatement(expression, topOrInline);
    return { kind: "expression", expression, span: { start, end } };
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
    if (_topOrInline && (this.atText(")") || this.atText(",") || this.atText("]")))
      return previous.span.end;
    this.fail(
      "expected-newline",
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

export function parse(source: string): ParseResult {
  const lexed = lex(source);
  if (lexed.diagnostics.length > 0) return { diagnostics: lexed.diagnostics };
  return new Parser(lexed.tokens).parse();
}
