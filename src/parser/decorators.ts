import type {
  AssociatedTypeBinding,
  Decorators,
  Expression,
  GenericBound,
  MemberLine,
  TypeDecl,
  TypeRef,
  VarianceMarker,
} from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { ExpressionParser } from "./expression.ts";

interface ParsedGenericParameters {
  readonly parameters: readonly string[];
  readonly bounds: readonly GenericBound[];
  /** Parameters written `reified`; the prototype erases them all. */
  readonly reified?: readonly string[];
  /** Type-argument defaults (04-type-system.md#type-argument-defaults). */
  readonly defaults?: Readonly<Record<string, TypeRef>>;
  /** `+T` and `-T` markers of a declaration's type parameters, one per parameter. */
  readonly variances?: readonly VarianceMarker[];
}

/** Which generic parameter list is parsed (02-grammar.md#generic-parameters-and-bounds). */
interface GenericParameterForm {
  /** A data type's or enum's `type_params` take variance markers; a trait's reject them. */
  readonly variance?: "allow" | "reject";
  /** `generic_params` of an implementation take no default (grammar.generic.default.positions). */
  readonly defaults?: boolean;
  /** Who owns the parameters, as in "a trait's generic parameters", for the variance error. */
  readonly owner?: string;
}

// Decorators, member lines, generic parameters, and trait bounds
// (spec/02-grammar.md#annotations and #r-grammar.impl.derivation-line),
// shared by the declaration parser.
export abstract class DecoratorParser extends ExpressionParser {
  protected parseGenericParameters(
    form: GenericParameterForm = { defaults: true },
  ): ParsedGenericParameters {
    const parameters: string[] = [];
    const bounds: GenericBound[] = [];
    const reified: string[] = [];
    const defaults: Record<string, TypeRef> = {};
    const variances: VarianceMarker[] = [];
    if (!this.matchText("[")) return { parameters, bounds };
    if (!this.atText("]")) {
      do {
        if (form.variance) {
          const marker = this.current();
          const variance = this.parseVarianceMarker();
          if (variance && form.variance === "reject")
            this.fail(
              "invalid-variance",
              `${form.owner ?? "these generic parameters"} take no variance marker`,
              marker.span,
            );
          variances.push(variance);
        }
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
        }
        // `= type` after the bound is a type-argument default, which an
        // implementation's parameters do not take (grammar.generic.default.positions).
        if (this.atText("=")) {
          if (!form.defaults)
            this.fail(
              "syntax-error",
              "an implementation's generic parameters take no default",
              this.current().span,
            );
          this.advance();
          defaults[parameter.text] = this.parseTypeArgument();
        }
      } while (this.matchText(",") && !this.atText("]"));
    }
    this.expectText("]");
    return {
      parameters,
      bounds,
      ...(reified.length > 0 ? { reified } : {}),
      ...(Object.keys(defaults).length > 0 ? { defaults } : {}),
      ...(variances.some(Boolean) ? { variances } : {}),
    };
  }

  // Whether the decorator lines here precede a data, enum, function, trait,
  // newtype, alias, or implementation declaration
  // (spec/02-grammar.md#r-grammar.annot.item-targets).
  protected decoratedDeclarationFollows(): boolean {
    let offset = 0;
    while (this.peek(offset).text === "@") {
      let depth = 0;
      offset += 1;
      for (;;) {
        const token = this.peek(offset);
        if (token.kind === "eof") return false;
        if (token.text === "(" || token.text === "[" || token.text === "{") depth += 1;
        else if (token.text === ")" || token.text === "]" || token.text === "}") depth -= 1;
        else if (token.kind === "newline" && depth <= 0) break;
        offset += 1;
      }
      offset += 1;
      while (this.peek(offset).kind === "doc-comment" || this.peek(offset).kind === "newline")
        offset += 1;
    }
    if (this.peek(offset).text === "pub") offset += 1;
    const keyword = this.peek(offset).text;
    return (
      ["data", "enum", "fn", "trait", "impl"].includes(keyword) ||
      (keyword === "type" && this.peek(offset + 1).kind === "identifier")
    );
  }

  // `@derive(A, b.B)` lines and fact decorators `@expression`.
  protected parseDecoratorLines(): Decorators {
    const start = this.current().span.start;
    const derives: TypeRef[] = [];
    const facts: Expression[] = [];
    let end = this.current().span.end;
    while (this.atText("@")) {
      this.advance();
      if (this.atText("derive") && this.peek(1).text === "(") {
        this.advance();
        this.expectText("(");
        do {
          if (this.atText(")")) break;
          const first = this.expectKind("identifier", "expected a trait name in @derive");
          let name = first.text;
          let nameEnd = first.span.end;
          while (this.atText(".")) {
            this.advance();
            const part = this.expectKind("identifier", "expected a trait name in @derive");
            name += `.${part.text}`;
            nameEnd = part.span.end;
          }
          derives.push({ name, span: { start: first.span.start, end: nameEnd } });
        } while (this.matchText(","));
        end = this.expectText(")").span.end;
      } else {
        const fact = this.parseExpression();
        facts.push(fact);
        end = fact.span.end;
      }
      this.expectKind("newline", "expected a line ending after a decorator");
      while (this.matchKind("newline")) {}
    }
    return { derives, facts, span: { start, end } };
  }

  // Member decorators: `@expression` lines before a field or variant, or
  // inline before a payload parameter (spec/02-grammar.md#data-declarations).
  protected parseMemberDecorators(inline: boolean): Expression[] {
    const metadata: Expression[] = [];
    while (this.atText("@")) {
      this.advance();
      metadata.push(this.parseExpression());
      if (!inline) {
        this.expectKind("newline", "expected a line ending after a member decorator");
        while (this.matchKind("newline")) {}
      }
    }
    return metadata;
  }

  // Doc comments and member decorator lines before a field or variant.
  protected parseMemberPrefix(): {
    readonly doc?: string;
    readonly metadata: { readonly metadata?: readonly Expression[] };
  } {
    const before = this.parseDocComments();
    const metadata = this.parseMemberDecorators(false);
    const doc = this.parseDocComments() ?? before;
    return { doc, metadata: metadata.length > 0 ? { metadata } : {} };
  }

  // A transparent alias takes no decorator
  // (spec/02-grammar.md#r-grammar.annot.alias-no-decorator).
  protected checkTypeDecorators(declaration: TypeDecl, typeStart: SourceSpan): void {
    if (!declaration.decorators || !declaration.alias) return;
    this.fail(
      "syntax-error",
      "a transparent alias takes no decorator; decorate a data, enum, or newtype declaration",
      typeStart,
    );
  }

  // Doc comments and decorator lines before a member of a trait or
  // implementation body. Decorators precede only a method, `fn` or `pub fn`
  // (spec/02-grammar.md#r-grammar.annot.member-targets).
  protected parseMethodPrefix(): { doc?: string; decorators?: Decorators } {
    const before = this.parseDocComments();
    if (!this.atText("@")) return { doc: before };
    const decorators = this.parseDecoratorLines();
    const doc = this.parseDocComments() ?? before;
    if (!this.atText("fn") && !this.atText("pub"))
      this.fail(
        "syntax-error",
        "a decorator in a trait or implementation body must precede a method",
        this.current().span,
      );
    return { doc, decorators };
  }

  // Decorators on a method's value parameter; a receiver takes none
  // (spec/02-grammar.md#r-grammar.fn.decorator-param-targets).
  protected parseMethodParameterDecorators(): Expression[] {
    const metadata = this.parseMemberDecorators(true);
    const receiver = this.atText("self") || (this.atText("mut") && this.peek(1).text === "self");
    if (metadata.length > 0 && receiver)
      this.fail("syntax-error", "a receiver parameter takes no decorator", this.current().span);
    return metadata;
  }

  // A member line `f = [...]`, `f += [...]`, `f = pass`, or a `Self` line
  // (spec/02-grammar.md#r-grammar.impl.derivation-line).
  protected parseMemberLine(): MemberLine | undefined {
    if (
      !(this.atKind("identifier") || this.atText("Self")) ||
      (this.peek(1).text !== "=" && this.peek(1).text !== "+=")
    )
      return undefined;
    const name = this.advance();
    const operator = this.advance().text as "=" | "+=";
    if (this.atText("pass") && this.peek(1).kind === "newline") {
      const pass = this.advance();
      this.expectKind("newline", "expected a line ending after a member line");
      return {
        name: name.text,
        nameSpan: name.span,
        operator,
        pass: true,
        span: { start: name.span.start, end: pass.span.end },
      };
    }
    const value = this.parseExpression();
    this.expectKind("newline", "expected a line ending after a member line");
    return {
      name: name.text,
      nameSpan: name.span,
      operator,
      value,
      span: { start: name.span.start, end: value.span.end },
    };
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
  protected boundHasBindings(): boolean {
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

  protected parseBoundTraitWithBindings(bindings: AssociatedTypeBinding[]): TypeRef {
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
}
