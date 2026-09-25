import type {
  Expression,
  DataDecl,
  EnumDecl,
  FunctionDecl,
  ImplDecl,
  MatchArm,
  MethodDecl,
  Parameter,
  Pattern,
  Program,
  ProviderContextEntry,
  Statement,
  TestDecl,
  TraitDecl,
  TypeRef,
  UseDecl,
} from "./ast.ts";
import type { Diagnostic, SourceSpan } from "./diagnostics.ts";
import { lex, type InterpolatedStringValue, type Token } from "./lexer.ts";

export interface ParseResult {
  readonly program?: Program;
  readonly diagnostics: readonly Diagnostic[];
}

class ParseFailure extends Error {}

const BINARY_PRECEDENCE: Readonly<Record<string, number>> = {
  or: 1,
  and: 2,
  "==": 3,
  "!=": 3,
  is: 3,
  "<": 4,
  "<=": 4,
  ">": 4,
  ">=": 4,
  "|": 5,
  "^": 6,
  "&": 7,
  "<<": 8,
  ">>": 8,
  "+": 9,
  "-": 9,
  "*": 10,
  "/": 10,
  "%": 10,
  "**": 11,
};

class Parser {
  private readonly tokens: readonly Token[];
  private index = 0;
  private readonly diagnostics: Diagnostic[] = [];
  private activeGenericParameters: ReadonlySet<string> = new Set();

  constructor(tokens: readonly Token[]) {
    this.tokens = tokens;
  }

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
        if (this.atText("struct")) this.fail("old-struct-declaration", "'struct' was replaced by 'data'", this.current().span);
        if (this.atText("import")) this.fail("old-import-declaration", "'import' was replaced by 'use'", this.current().span);
        if (this.atText("export")) this.fail("old-export-declaration", "'export' was replaced by 'pub use'", this.current().span);
        if (this.atText("use") || (this.atText("pub") && this.peek(1).text === "use")) {
          if (doc) this.fail("doc-comment-without-target", "documentation comments cannot attach to a use declaration", this.current().span);
          uses.push(this.parseUse());
          continue;
        }
        const public_ = this.matchText("pub");
        if (this.atText("fn")) functions.push(this.parseFunction(doc, public_));
        else if (this.atText("data")) data.push(this.parseData(doc, public_));
        else if (this.atText("enum")) enums.push(this.parseEnum(doc, public_));
        else if (this.atText("trait")) traits.push(this.parseTrait(doc, public_));
        else if (this.atText("impl")) implementations.push(this.parseImpl(doc));
        else if (this.atText("test") && this.peek(1).kind === "string") tests.push(this.parseTest(doc));
        else {
          if (doc) this.fail("doc-comment-without-target", "documentation comments must attach to a declaration or member", this.current().span);
          statements.push(this.parseStatement(true));
        }
      }
    } catch (error) {
      if (!(error instanceof ParseFailure)) throw error;
      return { diagnostics: this.diagnostics };
    }
    return {
      program: { uses, data, enums, traits, implementations, functions, tests, statements, span: { start, end: this.current().span.end } },
      diagnostics: this.diagnostics,
    };
  }

  parseExpressionFragment(): { readonly expression?: Expression; readonly diagnostics: readonly Diagnostic[] } {
    try {
      const expression = this.parseExpression();
      while (this.matchKind("newline")) {}
      if (!this.atKind("eof")) this.fail("expected-interpolation-end", `expected the end of an interpolation expression, found '${this.current().text}'`, this.current().span);
      return { expression, diagnostics: this.diagnostics };
    } catch (error) {
      if (!(error instanceof ParseFailure)) throw error;
      return { diagnostics: this.diagnostics };
    }
  }

  private parseFunction(doc?: string, public_ = false): FunctionDecl {
    const start = this.expectText("fn").span.start;
    const name = this.expectKind("identifier", "expected a function name");
    const suspending = this.matchText("!");
    const genericParameters: string[] = [];
    const genericBounds: Array<{ parameter: string; traits: string[]; span: SourceSpan }> = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
          const parameter = this.expectKind("identifier", "expected a generic parameter name");
          if (genericParameters.includes(parameter.text)) this.fail("duplicate-generic-parameter", `generic parameter '${parameter.text}' is declared more than once`, parameter.span);
          genericParameters.push(parameter.text);
          if (this.matchText(":")) {
            const traits: string[] = [];
            do {
              if (this.matchText("mut")) this.fail("unsupported-mutable-trait-bound", "mutable trait bounds are introduced after the initial dictionary slice", this.peek(-1).span);
              traits.push(this.expectKind("identifier", "expected a trait name after ':'").text);
            } while (this.matchText("+"));
            genericBounds.push({ parameter: parameter.text, traits, span: { start: parameter.span.start, end: this.peek(-1).span.end } });
          } else if (this.atText("...") || this.atText("=")) {
            this.fail("unsupported-generic-parameter", "bounds, packs, and defaults are introduced after the erased-generic MVP slice", this.current().span);
          }
        } while (this.matchText(",") && !this.atText("]"));
      }
      this.expectText("]");
    }
    this.activeGenericParameters = new Set(genericParameters);
    this.expectText("(");
    const parameters: Parameter[] = [];
    if (!this.atText(")")) {
      do {
        const parameterDoc = this.parseDocComments();
        if (["self", "Self", "super", "shape"].includes(this.current().text)) {
          this.fail("reserved-name", `'${this.current().text}' is reserved and cannot name a parameter`, this.current().span);
        }
        const parameterName = this.expectKind("identifier", "expected a parameter name");
        this.expectText(":");
        const type = this.parseType();
        const variadic = this.matchText("...");
        const defaultValue = this.matchText("=") ? this.parseExpression() : undefined;
        if (variadic && defaultValue) this.fail("vararg-default", "a variadic parameter cannot declare a default", defaultValue.span);
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
    this.expectText(")");
    this.expectText("->");
    const result = this.parseType();
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
      body,
      doc,
      span: { start, end: body.at(-1)?.span.end ?? result.span.end },
    };
  }

  private parseTest(doc?: string): TestDecl {
    if (doc) this.fail("doc-comment-without-target", "documentation comments cannot attach to a test block", this.current().span);
    const start = this.expectText("test").span.start;
    const name = this.expectKind("string", "expected a test name");
    const body = this.parseSuite();
    return { kind: "test", name: String(name.value ?? name.text), body, span: { start, end: body.at(-1)!.span.end } };
  }

  private parseUse(): UseDecl {
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
    const names: Array<{ name: string; alias?: string }> = [];
    let module: string;
    if (grouped) {
      module = parts.join(".");
      if (!this.atText("}")) {
        do {
          const name = this.expectKind("identifier", "expected an imported declaration name").text;
          const alias = this.matchText("as") ? this.expectKind("identifier", "expected an import alias").text : undefined;
          names.push({ name, ...(alias ? { alias } : {}) });
        } while (this.matchText(",") && !this.atText("}"));
      }
      this.expectText("}");
    } else {
      const name = parts.pop()!;
      module = parts.join(".");
      const alias = this.matchText("as") ? this.expectKind("identifier", "expected an import alias").text : undefined;
      names.push({ name, ...(alias ? { alias } : {}) });
    }
    const end = this.finishSimpleStatement(false);
    return { kind: "use", module, names, ...(public_ ? { public: true } : {}), span: { start, end } };
  }

  private parseTrait(doc?: string, public_ = false): TraitDecl {
    const start = this.expectText("trait").span.start;
    const name = this.expectKind("identifier", "expected a trait name");
    const genericParameters: string[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
          const parameter = this.expectKind("identifier", "expected a generic trait parameter");
          if (genericParameters.includes(parameter.text)) this.fail("duplicate-generic-parameter", `generic parameter '${parameter.text}' is declared more than once`, parameter.span);
          genericParameters.push(parameter.text);
        } while (this.matchText(",") && !this.atText("]"));
      }
      this.expectText("]");
    }
    if (this.matchText(":")) {
      if (!this.atKind("newline")) this.fail("unsupported-supertrait", "supertraits are introduced after the initial dictionary slice", this.current().span);
      this.expectKind("newline", "expected a line ending after a trait header");
      this.expectKind("indent", "expected an indented trait body");
      const methods: MethodDecl[] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const methodDoc = this.parseDocComments();
      if (!this.atText("fn") && !this.atText("pub")) this.fail("doc-comment-without-target", "documentation comments must attach to a declaration or member", this.current().span);
      if (this.atText("pub")) this.fail("trait-method-visibility", "trait methods inherit the trait's visibility and cannot be declared pub", this.current().span);
      methods.push(this.parseMethod(false, methodDoc));
      }
      const close = this.expectKind("dedent", "expected the end of the trait body");
      return { kind: "trait", ...(public_ ? { public: true } : {}), name: name.text, genericParameters, methods, doc, span: { start, end: close.span.end } };
    }
    const end = this.expectKind("newline", "expected a line ending after a marker trait").span.end;
    return { kind: "trait", ...(public_ ? { public: true } : {}), name: name.text, genericParameters, methods: [], doc, span: { start, end } };
  }

  private parseImpl(doc?: string): ImplDecl {
    const start = this.expectText("impl").span.start;
    if (this.atText("[")) this.fail("unsupported-generic-impl", "generic implementations are introduced after the initial dictionary slice", this.current().span);
    const first = this.expectKind("identifier", "expected an implementation target or trait name");
    const trait = this.matchText("for") ? first : undefined;
    const target = trait
      ? this.expectKind("identifier", "expected an implementation target type")
      : first;
    if (!this.matchText(":")) {
      if (!trait) this.fail("missing-impl-body", `inherent implementation for '${target.text}' requires a body`, target.span);
      const end = this.expectKind("newline", "expected a line ending after an implementation").span.end;
      return { kind: "impl", traitName: trait.text, targetName: target.text, methods: [], doc, span: { start, end } };
    }
    this.expectKind("newline", "expected a line ending after an implementation header");
    this.expectKind("indent", "expected an indented implementation body");
    const methods: MethodDecl[] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const methodDoc = this.parseDocComments();
      if (!this.atText("fn")) this.fail("doc-comment-without-target", "documentation comments must attach to a declaration or member", this.current().span);
      methods.push(this.parseMethod(true, methodDoc));
    }
    const close = this.expectKind("dedent", "expected the end of the implementation body");
    return { kind: "impl", ...(trait ? { traitName: trait.text } : {}), targetName: target.text, methods, doc, span: { start, end: close.span.end } };
  }

  private parseMethod(requireBody: boolean, doc?: string): MethodDecl {
    const start = this.expectText("fn").span.start;
    const name = this.expectKind("identifier", "expected a method name");
    const suspending = this.matchText("!");
    if (this.atText("[")) this.fail("unsupported-generic-method", "generic methods are introduced after the initial dictionary slice", this.current().span);
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
          this.fail("expected-token", "'mut' in a method parameter list must be followed by self", parameterName.span);
        }
        if (parameterName.text === "self") {
          const span = { start: mutableStart ?? parameterName.span.start, end: parameterName.span.end };
          parameters.push({ name: "self", type: { name: mutableReceiver ? "mut:Self" : "Self", span }, doc: parameterDoc, span });
        } else {
          this.expectText(":");
          const type = this.parseType();
          const variadic = this.matchText("...");
          parameters.push({ name: parameterName.text, type, variadic: variadic || undefined, doc: parameterDoc, span: { start: parameterName.span.start, end: this.peek(-1).span.end } });
        }
      } while (this.matchText(",") && !this.atText(")"));
    }
    this.expectText(")");
    this.expectText("->");
    const result = this.parseType();
    const requirements = this.matchText("$") ? this.parseRequirements() : [];
    if (!this.atText(":")) {
      if (requireBody) this.fail("missing-method-body", `implementation method '${name.text}' requires a body`, name.span);
      const end = this.expectKind("newline", "expected a line ending after a required method").span.end;
      return { name: name.text, suspending, parameters, result, requirements, doc, span: { start, end } };
    }
    const body = this.parseSuite();
    return { name: name.text, suspending, parameters, result, requirements, body, doc, span: { start, end: body.at(-1)?.span.end ?? result.span.end } };
  }

  private parseRequirements(): readonly string[] {
    if (this.atText("(") && this.peek(1).text === ")") {
      this.advance();
      this.advance();
      return [];
    }
    return [...this.parseRequirementExpression()].sort();
  }

  private parseRequirementExpression(): Set<string> {
    const requirements = this.parseRequirementUnion();
    while (this.matchText("-")) {
      const removed = this.parseRequirementKey();
      for (const requirement of [...requirements]) {
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

  private parseRequirementUnion(): Set<string> {
    const requirements = this.parseRequirementTerm();
    while (this.matchText("+")) {
      for (const requirement of this.parseRequirementTerm()) this.unionRequirement(requirements, requirement);
    }
    return requirements;
  }

  private unionRequirement(requirements: Set<string>, added: string): void {
    const [addedBase, ...addedExcluded] = added.split("\\");
    if (this.activeGenericParameters.has(addedBase!)) {
      const concrete = new Set([...requirements].filter((requirement) => !requirement.includes("\\") && !this.activeGenericParameters.has(requirement)));
      const effectiveExcluded = addedExcluded.filter((key) => !concrete.has(key));
      const existing = [...requirements].find((requirement) => requirement.split("\\")[0] === addedBase);
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
    for (const requirement of [...requirements]) {
      const [base, ...excluded] = requirement.split("\\");
      if (!this.activeGenericParameters.has(base!) || !excluded.includes(addedBase!)) continue;
      requirements.delete(requirement);
      requirements.add([base, ...excluded.filter((key) => key !== addedBase)].join("\\"));
    }
    requirements.add(added);
  }

  private parseRequirementTerm(): Set<string> {
    if (this.matchText("(")) {
      const requirements = this.parseRequirementExpression();
      this.expectText(")");
      return requirements;
    }
    return new Set([this.parseRequirementKey()]);
  }

  private parseRequirementKey(): string {
    const name = this.expectKind("identifier", "expected a concrete requirement name");
    if (!this.matchText("[")) return name.text;
    const arguments_: TypeRef[] = [];
    if (!this.atText("]")) {
      do arguments_.push(this.parseType()); while (this.matchText(",") && !this.atText("]"));
    }
    this.expectText("]");
    if (arguments_.length === 0) this.fail("generic-arity", `generic requirement '${name.text}' requires type arguments`, name.span);
    return `${name.text}[${arguments_.map((argument) => argument.name).join(",")}]`;
  }

  private parseData(doc?: string, public_ = false): DataDecl {
    const start = this.expectText("data").span.start;
    const name = this.expectKind("identifier", "expected a data type name");
    const genericParameters: string[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
          const parameter = this.expectKind("identifier", "expected a generic data parameter");
          if (genericParameters.includes(parameter.text)) this.fail("duplicate-generic-parameter", `generic parameter '${parameter.text}' is declared more than once`, parameter.span);
          genericParameters.push(parameter.text);
          if (this.atText(":")) this.fail("unsupported-generic-data-bound", "generic data bounds are introduced after the initial erased-data slice", this.current().span);
        } while (this.matchText(",") && !this.atText("]"));
      }
      this.expectText("]");
    }
    this.expectText(":");
    if (this.matchText("pass")) {
      const end = this.peek(-1).span.end;
      this.expectKind("newline", "expected a line ending after a fieldless data declaration");
      return { kind: "data", ...(public_ ? { public: true } : {}), name: name.text, genericParameters, fields: [], doc, span: { start, end } };
    }
    this.expectKind("newline", "expected a line ending after a data header");
    this.expectKind("indent", "expected an indented data body");
    const fields: DataDecl["fields"][number][] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const fieldDoc = this.parseDocComments();
      if (this.matchText("mut")) {
        const candidate = this.current();
        const code = this.peek(1).text === ":" ? "mutable-field-modifier" : "mutable-embedded-field";
        this.fail(code, "data fields express mutable access in their type rather than with a field modifier", candidate.span);
      }
      if (this.current().kind === "identifier" && this.peek(1).text !== ":") {
        const type = this.parseType();
        if (this.atText("=")) this.fail("embedded-field-default", "an embedded field cannot declare a default", this.current().span);
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
      fields.push({ name: fieldName.text, type, default: defaultValue, doc: fieldDoc, span: { start: fieldName.span.start, end } });
    }
    const close = this.expectKind("dedent", "expected the end of the data body");
    return { kind: "data", ...(public_ ? { public: true } : {}), name: name.text, genericParameters, fields, doc, span: { start, end: close.span.end } };
  }

  private parseEnum(doc?: string, public_ = false): EnumDecl {
    const start = this.expectText("enum").span.start;
    const name = this.expectKind("identifier", "expected an enum type name");
    const genericParameters: string[] = [];
    if (this.matchText("[")) {
      if (!this.atText("]")) {
        do {
          const parameter = this.expectKind("identifier", "expected a generic enum parameter");
          if (genericParameters.includes(parameter.text)) this.fail("duplicate-generic-parameter", `generic parameter '${parameter.text}' is declared more than once`, parameter.span);
          genericParameters.push(parameter.text);
          if (this.atText(":")) this.fail("unsupported-generic-enum-bound", "generic enum bounds are introduced after the initial erased-enum slice", this.current().span);
        } while (this.matchText(",") && !this.atText("]"));
      }
      this.expectText("]");
    }
    const sharedFields: EnumDecl["sharedFields"][number][] = [];
    if (this.matchText("(")) {
      if (!this.atText(")")) {
        do {
          const parameterStart = this.current().span.start;
          const explicitName = this.current().kind === "identifier" && this.peek(1).text === ":"
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
            fields.push({ name: fieldName.text, type, doc: fieldDoc, span: { start: fieldName.span.start, end: type.span.end } });
          } while (this.matchText(",") && !this.atText(")"));
        }
        this.expectText(")");
      }
      const result = this.matchText("->") ? this.parseExpression() : undefined;
      const end = result?.span.end ?? this.peek(-1).span.end;
      this.expectKind("newline", "expected a line ending after an enum variant");
      variants.push({ name: variantName.text, fields, result, doc: variantDoc, span: { start: variantName.span.start, end } });
    }
    const close = this.expectKind("dedent", "expected the end of the enum body");
    if (variants.length === 0) this.fail("empty-enum", "an enum must declare at least one variant", name.span);
    return { kind: "enum", ...(public_ ? { public: true } : {}), name: name.text, genericParameters, sharedFields, variants, doc, span: { start, end: close.span.end } };
  }

  private parseType(): TypeRef {
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
      if (inner.name.startsWith("mut:") || inner.name.startsWith("mut-suspend:")) {
        this.fail("duplicate-mutable-permission", "a type cannot apply 'mut' permission twice", inner.span);
      }
      const suspend = /^Suspend\[(.*)\]$/s.exec(inner.name);
      return {
        name: suspend ? `mut-suspend:${suspend[1]}` : `mut:${inner.name}`,
        span: { start, end: inner.span.end },
      };
    }
    if (this.matchText("$")) {
      const start = this.peek(-1).span.start;
      this.expectText(".");
      const context = this.expectKind("identifier", "expected Context after '$.'");
      if (context.text !== "Context") this.fail("expected-token", "expected Context after '$.'", context.span);
      this.expectText("[");
      const requirements = this.parseRequirements();
      const close = this.expectText("]");
      return { name: `context:${requirements.join("+")}`, span: { start, end: close.span.end } };
    }
    if (this.matchText("fn")) {
      const start = this.peek(-1).span.start;
      this.expectText("(");
      const parameters: Array<{ type: TypeRef; variadic: boolean }> = [];
      if (!this.atText(")")) {
        do {
          const type = this.parseType();
          const variadic = this.matchText("...");
          parameters.push({ type, variadic });
          if (variadic && this.atText(",") && this.peek(1).text !== ")") {
            this.fail("nonfinal-vararg", "a variadic function-type parameter must be final", this.peek(-1).span);
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
      return { name: `fn(${parameters.map((parameter) => `${parameter.type.name}${parameter.variadic ? "..." : ""}`).join(",")})->${result.name}${row}`, span: { start, end } };
    }
    const name = this.expectKind("identifier", "expected a type name");
    let rendered = name.text;
    let end = name.span.end;
    if (this.matchText("[")) {
      const arguments_: TypeRef[] = [];
      if (!this.atText("]")) {
        do arguments_.push(this.parseType()); while (this.matchText(",") && !this.atText("]"));
      }
      const close = this.expectText("]");
      if (arguments_.length === 0) this.fail("generic-arity", `generic type '${name.text}' requires type arguments`, name.span);
      rendered = `${name.text}[${arguments_.map((argument) => argument.name).join(",")}]`;
      end = close.span.end;
    }
    while (this.matchText("?")) {
      rendered += "?";
      end = this.peek(-1).span.end;
    }
    if (this.atText("(")) this.fail("unsupported-type-form", "function and tuple types are introduced with closure support", this.current().span);
    return { name: rendered, span: { start: name.span.start, end } };
  }

  private parseSuite(): readonly Statement[] {
    this.expectText(":");
    if (this.matchKind("newline")) {
      this.expectKind("indent", "expected an indented suite");
      const statements: Statement[] = [];
      while (!this.atKind("dedent") && !this.atKind("eof")) {
        if (this.matchKind("newline")) continue;
        statements.push(this.parseStatement(false));
      }
      this.expectKind("dedent", "expected the end of the indented suite");
      if (statements.length === 0) this.fail("empty-suite", "an indented suite must contain a statement", this.current().span);
      return statements;
    }
    return [this.parseStatement(true)];
  }

  private parseStatement(topOrInline: boolean): Statement {
    const start = this.current().span.start;
    if (this.atKind("doc-comment")) {
      this.fail("doc-comment-without-target", "documentation comments cannot attach to executable statements", this.current().span);
    }
    if (this.matchText("defer")) {
      const body = this.parseSuite();
      return { kind: "defer", body, span: { start, end: body.at(-1)!.span.end } };
    }
    if (this.atText("fn") && this.peek(1).kind === "identifier") return this.parseLocalFunction();
    if (this.matchText("let")) {
      const names = [this.expectKind("identifier", "expected a binding name")];
      while (this.matchText(",")) names.push(this.expectKind("identifier", "expected a binding name after ','"));
      const annotation = this.matchText(":") ? this.parseType() : undefined;
      this.expectText("=");
      const value = this.parseTrailingBlockCall(this.parseExpression());
      const end = this.finishExpressionStatement(value, topOrInline);
      return names.length === 1
        ? { kind: "binding", name: names[0]!.text, annotation, mutable: true, value, span: { start, end } }
        : { kind: "tuple-binding", bindings: names.map((name) => ({ name: name.text, span: name.span })), annotation, mutable: true, value, span: { start, end } };
    }
    if (this.matchText("return")) {
      const value = this.atKind("newline") || this.atKind("dedent") ? undefined : this.parseExpression();
      const end = value ? this.finishExpressionStatement(value, topOrInline) : this.finishSimpleStatement(topOrInline);
      return { kind: "return", value, span: { start, end } };
    }
    if (this.matchText("break")) {
      const value = this.atKind("newline") || this.atKind("dedent") || this.atKind("eof") ? undefined : this.parseExpression();
      const end = value ? this.finishExpressionStatement(value, topOrInline) : this.finishSimpleStatement(topOrInline);
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
      if (this.peek(2).kind !== "newline") this.fail("missing-let", "a typed mutable binding must begin with 'let'", this.current().span);
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
      while (this.matchText(",")) names.push(this.expectKind("identifier", "expected a binding name after ','"));
      this.expectText(":=");
      const value = this.parseTrailingBlockCall(this.parseExpression());
      const end = this.finishExpressionStatement(value, topOrInline);
      return { kind: "tuple-binding", bindings: names.map((name) => ({ name: name.text, span: name.span })), mutable: false, value, span: { start, end } };
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
      if (expression.kind === "member") return { kind: "field-assignment", target: expression, value, span: { start, end } };
      if (expression.kind === "index") return { kind: "index-assignment", target: expression, value, span: { start, end } };
      this.fail("invalid-assignment-target", "assignment requires a binding, data member, list element, or map entry", expression.span);
    }
    const end = this.finishExpressionStatement(expression, topOrInline);
    return { kind: "expression", expression, span: { start, end } };
  }

  private parseTrailingBlockCall(callee: Expression): Expression {
    if (!this.atText(":")) return callee;
    const body = this.parseSuite();
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

  private finishSimpleStatement(_topOrInline: boolean): SourceSpan["end"] {
    const previous = this.peek(-1);
    if (this.matchKind("newline")) return previous.span.end;
    if (this.atKind("dedent") || this.atKind("eof")) return previous.span.end;
    if (_topOrInline && (this.atText(")") || this.atText(",") || this.atText("]"))) return previous.span.end;
    this.fail("expected-newline", `expected a line ending, found '${this.current().text}'`, this.current().span);
  }

  private finishExpressionStatement(expression: Expression, topOrInline: boolean): SourceSpan["end"] {
    if (this.peek(-1).kind === "dedent") return expression.span.end;
    if (["if", "for", "while", "match", "closure", "provider-with"].includes(expression.kind)) return expression.span.end;
    return this.finishSimpleStatement(topOrInline);
  }

  private parseExpression(minimumPrecedence = 0): Expression {
    let left = this.parsePrefix();
    while (true) {
      if (this.atText("[") && left.kind === "name" && this.typeArgumentsFollowedByCall()) {
        const start = left.span.start;
        this.advance();
        const typeArguments: TypeRef[] = [];
        if (!this.atText("]")) {
          do typeArguments.push(this.parseType()); while (this.matchText(",") && !this.atText("]"));
        }
        const close = this.expectText("]");
        left = { ...left, typeArguments, span: { start, end: close.span.end } };
        continue;
      }
      if (this.atText("(")) {
        if (12 < minimumPrecedence) break;
        left = this.parseCall(left);
        continue;
      }
      if (this.atText("!") && this.peek(1).text === "(") {
        if (12 < minimumPrecedence) break;
        this.advance();
        left = this.parseCall(left, true);
        continue;
      }
      if (this.atText(".")) {
        if (12 < minimumPrecedence) break;
        this.advance();
        const member = this.current();
        if (member.kind !== "identifier" && !(member.kind === "integer" && /^[0-9]+$/.test(member.text))) {
          this.fail("expected-token", "expected a member name after '.'", member.span);
        }
        this.advance();
        left = { kind: "member", receiver: left, name: member.text, span: { start: left.span.start, end: member.span.end } };
        continue;
      }
      if (this.atText("[")) {
        if (12 < minimumPrecedence) break;
        this.advance();
        const index = this.parseExpression();
        const close = this.expectText("]");
        left = { kind: "index", receiver: left, index, span: { start: left.span.start, end: close.span.end } };
        continue;
      }
      if (this.atText("?")) {
        if (12 < minimumPrecedence) break;
        const suffix = this.advance();
        left = { kind: "propagate", operand: left, span: { start: left.span.start, end: suffix.span.end } };
        continue;
      }
      const precedence = BINARY_PRECEDENCE[this.current().text];
      if (precedence === undefined || precedence < minimumPrecedence) break;
      const comparisons = new Set(["==", "!=", "<", "<=", ">", ">=", "is"]);
      if (comparisons.has(this.current().text) && left.kind === "binary" && comparisons.has(left.operator)) {
        this.fail("comparison-chaining", "comparisons do not chain", this.current().span);
      }
      const operator = this.advance();
      const right = this.parseExpression(precedence + (operator.text === "**" ? 0 : 1));
      left = {
        kind: "binary",
        operator: operator.text,
        left,
        right,
        span: { start: left.span.start, end: right.span.end },
      };
    }
    return left;
  }

  private typeArgumentsFollowedByCall(): boolean {
    let depth = 0;
    for (let distance = 0; ; distance += 1) {
      const token = this.peek(distance);
      if (token.kind === "eof" || token.kind === "newline") return false;
      if (token.text === "[") depth += 1;
      else if (token.text === "]") {
        depth -= 1;
        if (depth === 0) {
          const next = this.peek(distance + 1);
          return next.text === "(" || (next.text === "!" && this.peek(distance + 2).text === "(");
        }
      }
    }
  }

  private parsePrefix(): Expression {
    const token = this.current();
    if (["+", "-", "~", "not"].includes(token.text)) {
      this.advance();
      const operand = this.parseExpression(11);
      return { kind: "unary", operator: token.text, operand, span: { start: token.span.start, end: operand.span.end } };
    }
    if (this.matchText("if")) return this.parseIf(token);
    if (this.matchText("for")) return this.parseFor(token);
    if (this.matchText("while")) return this.parseWhile(token);
    if (this.matchText("match")) return this.parseMatch(token);
    if (this.matchText("fn")) return this.parseClosure(token);
    if (this.matchText("$")) return this.parseProviderExpression(token);
    if (this.matchText(".")) {
      const variant = this.expectKind("identifier", "expected an enum variant name after '.'");
      return { kind: "contextual-variant", name: variant.text, span: { start: token.span.start, end: variant.span.end } };
    }
    if (token.kind === "integer") {
      this.advance();
      return { kind: "integer", value: token.value as bigint, span: token.span };
    }
    if (token.kind === "float") {
      this.advance();
      return { kind: "float", value: token.value as number, span: token.span };
    }
    if (token.kind === "string") {
      this.advance();
      if (typeof token.value === "string") return { kind: "string", value: token.value, span: token.span };
      const value = token.value as InterpolatedStringValue;
      const segments = value.segments.map((segment) => {
        if (segment.kind === "text") return segment;
        const lexed = lex(segment.source);
        const lexical = lexed.diagnostics[0];
        if (lexical) this.fail(lexical.code, lexical.message, segment.span);
        const parsed = new Parser(lexed.tokens).parseExpressionFragment();
        const diagnostic = parsed.diagnostics[0];
        if (diagnostic || !parsed.expression) {
          this.fail(diagnostic?.code ?? "invalid-string-interpolation", diagnostic?.message ?? "invalid interpolation expression", segment.span);
        }
        return { kind: "expression" as const, expression: parsed.expression, span: segment.span };
      });
      return { kind: "interpolated-string", segments, span: token.span };
    }
    if (token.kind === "character") {
      this.advance();
      return { kind: "character", value: token.value as string, span: token.span };
    }
    if (token.text === "true" || token.text === "false") {
      this.advance();
      return { kind: "boolean", value: token.text === "true", span: token.span };
    }
    if (token.text === "nil") {
      this.advance();
      return { kind: "nil", span: token.span };
    }
    if (this.matchText("[")) {
      const elements: Expression[] = [];
      if (!this.atText("]")) {
        do {
          const element = this.parseExpression();
          if (this.atText(":=")) this.fail("multi-binding-needs-parentheses", "a multi-name binding inside delimiters must be parenthesized", this.current().span);
          if (this.atText(":")) this.fail("trailing-block-position", "a trailing callback block is not valid inside brackets", this.current().span);
          elements.push(element);
        } while (this.matchText(",") && !this.atText("]"));
      }
      const close = this.expectText("]");
      return { kind: "list", elements, span: { start: token.span.start, end: close.span.end } };
    }
    if (this.matchText("{")) {
      const entries: Array<{ key: Expression; value: Expression; span: SourceSpan }> = [];
      if (!this.atText("}")) {
        do {
          const key = this.parseExpression();
          this.expectText(":");
          const value = this.parseExpression();
          entries.push({ key, value, span: { start: key.span.start, end: value.span.end } });
        } while (this.matchText(",") && !this.atText("}"));
      }
      const close = this.expectText("}");
      return { kind: "map", entries, span: { start: token.span.start, end: close.span.end } };
    }
    if (token.kind === "identifier" || token.text === "self") {
      this.advance();
      if (this.atText("{")) return this.parseDataExpression(token);
      return { kind: "name", name: token.text, span: token.span };
    }
    if (this.matchText("(")) {
      if (this.atText(")")) {
        const close = this.advance();
        return { kind: "tuple", elements: [], span: { start: token.span.start, end: close.span.end } };
      }
      const first = this.parseExpression();
      if (!this.matchText(",")) {
        this.expectText(")");
        return first;
      }
      const elements = [first];
      while (!this.atText(")")) {
        elements.push(this.parseExpression());
        if (!this.matchText(",")) break;
      }
      const close = this.expectText(")");
      return { kind: "tuple", elements, span: { start: token.span.start, end: close.span.end } };
    }
    this.fail("expected-expression", `expected an expression, found '${token.text}'`, token.span);
  }

  private parseDataExpression(name: Token): Expression {
    this.expectText("{");
    let spread: Expression | undefined;
    const fields: Array<{ name: string; value: Expression; span: SourceSpan }> = [];
    if (this.matchText("...")) {
      spread = this.parseExpression();
      if (!this.atText("}")) this.expectText(",");
    }
    if (!this.atText("}")) {
      do {
        if (this.atText("...")) {
          this.fail("data-spread-position", "a data spread must be the first and only spread in a data expression", this.current().span);
        }
        const field = this.expectKind("identifier", "expected a data field name");
        this.expectText(":");
        const value = this.parseExpression();
        fields.push({ name: field.text, value, span: { start: field.span.start, end: value.span.end } });
      } while (this.matchText(",") && !this.atText("}"));
    }
    const close = this.expectText("}");
    return { kind: "data", name: name.text, spread, fields, span: { start: name.span.start, end: close.span.end } };
  }

  private parseCall(callee: Expression, suspending = false): Expression {
    const typeArguments = callee.kind === "name" ? callee.typeArguments : undefined;
    if (typeArguments && callee.kind === "name") callee = { kind: "name", name: callee.name, span: callee.span };
    this.expectText("(");
    const args: Expression[] = [];
    const argumentNames: Array<string | undefined> = [];
    const argumentSpreads: boolean[] = [];
    let sawNamed = false;
    if (!this.atText(")")) {
      do {
        if (this.current().kind === "identifier" && this.peek(1).text === "=") {
          sawNamed = true;
          const name = this.advance();
          this.advance();
          argumentNames.push(name.text);
          args.push(this.parseExpression());
          argumentSpreads.push(false);
        } else {
          if (sawNamed) this.fail("argument-order", "positional arguments must precede named arguments", this.current().span);
          argumentNames.push(undefined);
          args.push(this.parseExpression());
          const spread = this.matchText("...");
          argumentSpreads.push(spread);
          const followedByNamedArgument = this.atText(",")
            && this.peek(1).kind === "identifier"
            && this.peek(2).text === "=";
          if (spread && this.atText(",") && this.peek(1).text !== ")" && !followedByNamedArgument) {
            this.fail("nonfinal-positional-spread", "a positional spread must be the final positional argument", this.peek(-1).span);
          }
        }
      } while (this.matchText(",") && !this.atText(")"));
    }
    const close = this.expectText(")");
    return {
      kind: suspending ? "suspend-call" : "call",
      callee,
      typeArguments,
      arguments: args,
      argumentNames: sawNamed ? argumentNames : undefined,
      argumentSpreads: argumentSpreads.some(Boolean) ? argumentSpreads : undefined,
      span: { start: callee.span.start, end: close.span.end },
    };
  }

  private parseIf(keyword: Token): Expression {
    const condition = this.parseExpression();
    const thenBody = this.parseSuite();
    let elseBody: readonly Statement[] = [];
    if (this.matchText("else")) {
      if (this.atText("if")) {
        const nestedKeyword = this.advance();
        const nested = this.parseIf(nestedKeyword);
        elseBody = [{ kind: "expression", expression: nested, span: nested.span }];
      } else {
        elseBody = this.parseSuite();
      }
    }
    return {
      kind: "if",
      condition,
      thenBody,
      elseBody,
      span: { start: keyword.span.start, end: elseBody.at(-1)?.span.end ?? thenBody.at(-1)!.span.end },
    };
  }

  private parseWhile(keyword: Token): Expression {
    const condition = this.parseExpression();
    const body = this.parseSuite();
    const elseBody = this.matchText("else") ? this.parseSuite() : [];
    return { kind: "while", condition, body, elseBody, span: { start: keyword.span.start, end: elseBody.at(-1)?.span.end ?? body.at(-1)!.span.end } };
  }

  private parseFor(keyword: Token): Expression {
    const names = [this.expectKind("identifier", "expected a loop binding name")];
    while (this.matchText(",")) names.push(this.expectKind("identifier", "expected a loop binding name after ','"));
    this.expectText("in");
    const iterable = this.parseExpression();
    const body = this.parseSuite();
    const elseBody = this.matchText("else") ? this.parseSuite() : [];
    return {
      kind: "for",
      bindings: names.map((name) => ({ name: name.text, span: name.span })),
      iterable,
      body,
      elseBody,
      span: { start: keyword.span.start, end: elseBody.at(-1)?.span.end ?? body.at(-1)!.span.end },
    };
  }

  private parseMatch(keyword: Token): Expression {
    const subject = this.parseExpression();
    this.expectText(":");
    this.expectKind("newline", "expected a line ending after a match header");
    this.expectKind("indent", "expected indented match arms");
    const arms: MatchArm[] = [];
    while (!this.atKind("dedent") && !this.atKind("eof")) {
      if (this.matchKind("newline")) continue;
      const pattern = this.parsePattern();
      const guard = this.matchText("if") ? this.parseExpression() : undefined;
      this.expectText("=>");
      let body: readonly Statement[];
      if (this.matchKind("newline")) {
        this.expectKind("indent", "expected an indented match arm body");
        const statements: Statement[] = [];
        while (!this.atKind("dedent") && !this.atKind("eof")) {
          if (this.matchKind("newline")) continue;
          statements.push(this.parseStatement(false));
        }
        this.expectKind("dedent", "expected the end of the match arm body");
        body = statements;
      } else {
        body = [this.parseStatement(true)];
      }
      arms.push({ pattern, guard, body, span: { start: pattern.span.start, end: body.at(-1)!.span.end } });
    }
    const close = this.expectKind("dedent", "expected the end of the match expression");
    if (arms.length === 0) this.fail("empty-match", "a match expression must contain an arm", keyword.span);
    return { kind: "match", subject, arms, span: { start: keyword.span.start, end: close.span.end } };
  }

  private parseClosure(keyword: Token): Expression {
    this.expectText("(");
    const parameters: Array<{ name: string; type?: TypeRef; span: SourceSpan }> = [];
    if (!this.atText(")")) {
      do {
        const name = this.expectKind("identifier", "expected a closure parameter name");
        const type = this.matchText(":") ? this.parseType() : undefined;
        parameters.push({ name: name.text, type, span: { start: name.span.start, end: type?.span.end ?? name.span.end } });
      } while (this.matchText(",") && !this.atText(")"));
    }
    this.expectText(")");
    const result = this.matchText("->") ? this.parseType() : undefined;
    const requirements = this.matchText("$") ? this.parseRequirements() : undefined;
    const body = this.parseSuite();
    return { kind: "closure", parameters, result, requirements, body, span: { start: keyword.span.start, end: body.at(-1)!.span.end } };
  }

  private parseLocalFunction(): Statement {
    const start = this.expectText("fn").span.start;
    const name = this.expectKind("identifier", "expected a local function name");
    if (this.matchText("!")) this.fail("unsupported-local-suspending-function", "suspending local functions are outside the current closure slice", this.peek(-1).span);
    if (this.atText("[")) this.fail("unsupported-local-generic-function", "generic local functions are outside the current erased-closure slice", this.current().span);
    this.expectText("(");
    const parameters: Array<{ name: string; type?: TypeRef; span: SourceSpan }> = [];
    if (!this.atText(")")) {
      do {
        const parameter = this.expectKind("identifier", "expected a local function parameter name");
        this.expectText(":");
        const type = this.parseType();
        if (this.matchText("...")) this.fail("unsupported-local-vararg", "variadic local functions are outside the current closure slice", this.peek(-1).span);
        if (this.atText("=")) this.fail("unsupported-local-default", "local function parameter defaults are outside the current closure slice", this.current().span);
        parameters.push({ name: parameter.text, type, span: { start: parameter.span.start, end: type.span.end } });
      } while (this.matchText(",") && !this.atText(")"));
    }
    this.expectText(")");
    this.expectText("->");
    const result = this.parseType();
    const requirements = this.matchText("$") ? this.parseRequirements() : [];
    const body = this.parseSuite();
    const end = body.at(-1)!.span.end;
    const closure: Expression = { kind: "closure", parameters, result, requirements, body, span: { start, end } };
    return {
      kind: "binding",
      name: name.text,
      mutable: false,
      annotation: {
        name: `fn(${parameters.map((parameter) => parameter.type!.name).join(",")})->${result.name}${requirements.length ? `$${requirements.join("+")}` : ""}`,
        span: { start: name.span.start, end: result.span.end },
      },
      value: closure,
      span: { start, end },
    };
  }

  private parseProviderExpression(namespace: Token): Expression {
    this.expectText(".");
    const operation = this.current();
    if (!new Set(["use", "with", "context"]).has(operation.text)) {
      this.fail("expected-token", "expected a provider-context operation after '$.'", operation.span);
    }
    this.advance();
    if (operation.text === "use") {
      this.expectText("(");
      const keys = [this.parseRequirementKey()];
      while (this.matchText(",") && !this.atText(")")) keys.push(this.parseRequirementKey());
      const close = this.expectText(")");
      const span = { start: namespace.span.start, end: close.span.end };
      return keys.length === 1
        ? { kind: "provider-use", key: keys[0]!, span }
        : { kind: "tuple", elements: keys.map((key) => ({ kind: "provider-use", key, span })), span };
    }
    if (operation.text === "with") {
      this.expectText("(");
      const entries = this.parseProviderEntries();
      this.expectText(")");
      const body = this.parseSuite();
      return { kind: "provider-with", entries, body, span: { start: namespace.span.start, end: body.at(-1)!.span.end } };
    }
    if (operation.text === "context") {
      this.expectText("(");
      const entries = this.parseProviderEntries();
      const close = this.expectText(")");
      return { kind: "provider-context", entries, span: { start: namespace.span.start, end: close.span.end } };
    }
    this.fail("unsupported-context-operation", `provider-context operation '$.${operation.text}' is not implemented yet`, operation.span);
  }

  private parseProviderEntries(): ProviderContextEntry[] {
    const entries: ProviderContextEntry[] = [];
    do {
      if (this.matchText("...")) {
        const start = this.peek(-1).span.start;
        const value = this.parseExpression();
        entries.push({ kind: "spread", value, span: { start, end: value.span.end } });
      } else {
        const keyStart = this.current().span.start;
        const key = this.parseRequirementKey();
        this.expectText("=");
        const value = this.parseExpression();
        entries.push({ kind: "binding", key, value, span: { start: keyStart, end: value.span.end } });
      }
    } while (this.matchText(",") && !this.atText(")"));
    return entries;
  }

  private parsePattern(): Pattern {
    const start = this.current().span.start;
    if (this.matchText("_")) return { kind: "wildcard", span: { start, end: this.peek(-1).span.end } };
    if (this.matchText("true")) return { kind: "boolean", value: true, span: { start, end: this.peek(-1).span.end } };
    if (this.matchText("false")) return { kind: "boolean", value: false, span: { start, end: this.peek(-1).span.end } };
    if (this.matchText("nil")) return { kind: "nil", span: { start, end: this.peek(-1).span.end } };
    const negative = this.matchText("-");
    const literal = this.current();
    if (literal.kind === "integer") {
      this.advance();
      const value = literal.value as bigint;
      return { kind: "integer", value: negative ? -value : value, span: { start, end: literal.span.end } };
    }
    if (literal.kind === "float") {
      this.advance();
      const value = literal.value as number;
      return { kind: "float", value: negative ? -value : value, span: { start, end: literal.span.end } };
    }
    if (negative) this.fail("expected-pattern", "'-' in a pattern must precede a numeric literal", literal.span);
    if (literal.kind === "string") {
      this.advance();
      if (typeof literal.value !== "string") this.fail("interpolated-pattern", "string patterns must be constant and cannot contain interpolation", literal.span);
      return { kind: "string", value: literal.value, span: literal.span };
    }
    if (literal.kind === "character") {
      this.advance();
      return { kind: "character", value: literal.value as string, span: literal.span };
    }
    if (this.matchText(".")) {
      const variant = this.expectKind("identifier", "expected a variant name after '.'");
      const { bindings, names, patterns } = this.parsePatternBindings();
      return { kind: "variant", variantName: variant.text, bindings, bindingNames: names, payloadPatterns: patterns, span: { start, end: this.peek(-1).span.end } };
    }
    const first = this.expectKind("identifier", "expected a supported match pattern");
    if (this.matchText("{")) {
      const fields: Array<{ name: string; pattern: Pattern; span: SourceSpan }> = [];
      if (!this.atText("}")) {
        do {
          const field = this.expectKind("identifier", "expected a data pattern field");
          const pattern = this.matchText("=")
            ? this.parsePattern()
            : { kind: "binding" as const, name: field.text, span: field.span };
          fields.push({ name: field.text, pattern, span: { start: field.span.start, end: pattern.span.end } });
        } while (this.matchText(",") && !this.atText("}"));
      }
      const close = this.expectText("}");
      return { kind: "data", typeName: first.text, fields, span: { start, end: close.span.end } };
    }
    if (this.matchText("?")) {
      return { kind: "optional-present", name: first.text, span: { start, end: this.peek(-1).span.end } };
    }
    if ((first.text === "Ok" || first.text === "Err") && this.atText("(")) {
      const { bindings, patterns } = this.parsePatternBindings();
      return { kind: "result-variant", variantName: first.text, bindings, payloadPatterns: patterns, span: { start, end: this.peek(-1).span.end } };
    }
    if (!this.matchText(".")) return { kind: "binding", name: first.text, span: first.span };
    const variant = this.expectKind("identifier", "expected a variant name after '.'");
    const { bindings, names, patterns } = this.parsePatternBindings();
    return { kind: "variant", enumName: first.text, variantName: variant.text, bindings, bindingNames: names, payloadPatterns: patterns, span: { start, end: this.peek(-1).span.end } };
  }

  private parsePatternBindings(): { readonly bindings: readonly (string | undefined)[]; readonly names: readonly (string | undefined)[]; readonly patterns: readonly Pattern[] } {
    const bindings: Array<string | undefined> = [];
    const names: Array<string | undefined> = [];
    const patterns: Pattern[] = [];
    let sawNamed = false;
    if (this.matchText("(")) {
      if (!this.atText(")")) {
        do {
          if (this.current().kind === "identifier" && this.peek(1).text === "=") {
            sawNamed = true;
            const name = this.advance();
            this.advance();
            const pattern = this.parsePattern();
            names.push(name.text);
            patterns.push(pattern);
            bindings.push(pattern.kind === "binding" ? pattern.name : undefined);
          } else {
            if (sawNamed) this.fail("pattern-order", "positional patterns must precede named patterns", this.current().span);
            names.push(undefined);
            const pattern = this.parsePattern();
            patterns.push(pattern);
            bindings.push(pattern.kind === "binding" ? pattern.name : undefined);
          }
        } while (this.matchText(",") && !this.atText(")"));
      }
      this.expectText(")");
    }
    return { bindings, names, patterns };
  }

  private parseDocComments(): string | undefined {
    if (!this.atKind("doc-comment")) return undefined;
    const lines: string[] = [];
    const first = this.current();
    let previous = first;
    while (this.atKind("doc-comment")) {
      const comment = this.advance();
      if (lines.length > 0 && (comment.span.start.line !== previous.span.end.line + 1 || comment.span.start.column !== first.span.start.column)) {
        this.fail("doc-comment-without-target", "documentation comment lines must be consecutive and share an indentation", comment.span);
      }
      lines.push(String(comment.value ?? comment.text));
      previous = comment;
      this.matchKind("newline");
    }
    const target = this.current();
    if (target.kind === "eof"
      || target.kind === "dedent"
      || target.span.start.line !== previous.span.end.line + 1
      || target.span.start.column !== first.span.start.column) {
      this.fail("doc-comment-without-target", "documentation comments must immediately precede a declaration or member at the same indentation", first.span);
    }
    return lines.join("\n");
  }

  private expectText(text: string): Token {
    if (!this.atText(text)) this.fail("expected-token", `expected '${text}', found '${this.current().text}'`, this.current().span);
    return this.advance();
  }

  private expectKind(kind: Token["kind"], message: string): Token {
    if (!this.atKind(kind)) this.fail("expected-token", message, this.current().span);
    return this.advance();
  }

  private matchText(text: string): boolean {
    if (!this.atText(text)) return false;
    this.advance();
    return true;
  }

  private matchKind(kind: Token["kind"]): boolean {
    if (!this.atKind(kind)) return false;
    this.advance();
    return true;
  }

  private atText(text: string): boolean {
    return this.current().text === text;
  }

  private atKind(kind: Token["kind"]): boolean {
    return this.current().kind === kind;
  }

  private current(): Token {
    return this.tokens[Math.min(this.index, this.tokens.length - 1)]!;
  }

  private peek(distance: number): Token {
    return this.tokens[Math.max(0, Math.min(this.index + distance, this.tokens.length - 1))]!;
  }

  private advance(): Token {
    const token = this.current();
    if (token.kind !== "eof") this.index += 1;
    return token;
  }

  private fail(code: string, message: string, span: SourceSpan): never {
    this.diagnostics.push({ code, message, span });
    throw new ParseFailure(message);
  }
}

export function parse(source: string): ParseResult {
  const lexed = lex(source);
  if (lexed.diagnostics.length > 0) return { diagnostics: lexed.diagnostics };
  return new Parser(lexed.tokens).parse();
}
