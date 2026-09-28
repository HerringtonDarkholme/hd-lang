import type { Decorators, Expression, MemberLine, TypeDecl, TypeRef } from "../ast.ts";
import type { SourceSpan } from "../diagnostics.ts";
import { ExpressionParser } from "./expression.ts";

// Decorators and member lines (spec/02-grammar.md#annotations and
// #r-grammar.impl.derivation-line), shared by the declaration parser.
export abstract class DecoratorParser extends ExpressionParser {
  // Whether the decorator lines here precede a data, enum, newtype, alias,
  // or function declaration (spec/02-grammar.md#annotations).
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
      ["data", "enum", "fn"].includes(keyword) ||
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

  // A newtype takes only `@derive` lines, and an alias none
  // (spec/02-grammar.md#r-grammar.annot.newtype-derive.error).
  protected checkTypeDecorators(declaration: TypeDecl, typeStart: SourceSpan): void {
    const decorators = declaration.decorators;
    if (!decorators || (!declaration.alias && decorators.facts.length === 0)) return;
    this.fail(
      "syntax-error",
      declaration.alias
        ? "a transparent alias takes no derive decorator; derive on a data, enum, or newtype declaration"
        : "a newtype takes only @derive decorators",
      declaration.alias ? typeStart : decorators.span,
    );
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
}
