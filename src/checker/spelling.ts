import type { HirExpression, HirGlobal, HirLocal, HirStatement, ValueType } from "../hir.ts";
import { DEFAULTED_SPANS, finalValueOf, isDefaultedLiteral } from "./literal-join.ts";

// Display spellings (04-type-system.md#r-types.alias.usize.display).
//
// `usize` is a transparent alias of `u32`, so every type the checker and the
// emitter compare is canonical: it says `u32` wherever `usize` was written.
// Diagnostics and the REPL still print `usize` for a type that was written
// `usize` or came from a size or a bare literal's default. That spelling is
// display metadata only: this module derives it on demand from the checked
// tree, and nothing that decides typing or emission reads it.

/** A numeric type word in a type's text, `usize` included. */
const NUMERIC_WORD = /(?<![\w$:])(?:[iu](?:8|16|32|64)|f(?:32|64)|usize)(?![\w])/g;
const U32_WORD = /(?<![\w$:])u32(?![\w])/;
const U32_WORDS = new RegExp(U32_WORD.source, "g");

/** An explicit spelling recorded where the checker knew it, such as a cast `usize(x)`. */
const SPELLED = new WeakMap<object, ValueType>();

/** What a binding's spelling comes from: its written annotation, or its initializer. */
const BINDING_SOURCES = new WeakMap<HirLocal | HirGlobal, ValueType | HirExpression>();

/** The deepest expression nesting a spelling walks; deeper trees print canonically. */
const MAX_DEPTH = 64;

/** Records `spelled` as how `node`'s type prints; `undefined` records nothing. */
export function spellAs<T extends object>(node: T, spelled: ValueType | undefined): T {
  if (spelled !== undefined && spelled.includes("usize")) SPELLED.set(node, spelled);
  return node;
}

/** Records what a binding's printed type comes from: a written type or its initializer. */
export function spellBinding(
  binding: HirLocal | HirGlobal,
  source: ValueType | HirExpression | undefined,
): void {
  if (source === undefined) return;
  if (typeof source === "string" ? source.includes("usize") : source.type.includes("u32"))
    BINDING_SOURCES.set(binding, source);
}

/**
 * `canonical` with each `u32` that `hint` spells `usize` printed as `usize`.
 * The hint must name the same numeric words in the same order once `usize`
 * reads as `u32`; otherwise `canonical` prints as it is, so a spelling never
 * shows a different type.
 */
export function respelled(canonical: ValueType, hint: ValueType | undefined): ValueType {
  if (hint === undefined || hint === canonical || !hint.includes("usize")) return canonical;
  const hinted = hint.match(NUMERIC_WORD) ?? [];
  const own = canonical.match(NUMERIC_WORD) ?? [];
  if (
    hinted.length !== own.length ||
    hinted.some((word, index) => (word === "usize" ? "u32" : word) !== own[index])
  )
    return canonical;
  let index = 0;
  return canonical.replace(NUMERIC_WORD, () => hinted[index++]!);
}

/** How a binding's type prints. */
export function spelledBindingType(binding: HirLocal | HirGlobal, depth = 0): ValueType {
  const source = BINDING_SOURCES.get(binding);
  if (source === undefined) return binding.type;
  return respelled(
    binding.type,
    typeof source === "string" ? source : spelledType(source, depth + 1),
  );
}

/** How an expression's type prints: its canonical type, with `usize` where it came from one. */
export function spelledType(expression: HirExpression, depth = 0): ValueType {
  const type = expression.type;
  if (!type.includes("u32")) return type;
  const explicit = SPELLED.get(expression);
  if (explicit !== undefined) return respelled(type, explicit);
  if (depth > MAX_DEPTH) return type;
  return respelled(type, spellingHint(expression, depth + 1));
}

/** The type text whose numeric words an expression's printed type takes. */
function spellingHint(expression: HirExpression, depth: number): ValueType | undefined {
  const spell = (child: HirExpression): ValueType => spelledType(child, depth);
  switch (expression.kind) {
    case "integer":
      // A bare literal that took its default, alone or in a literal group.
      return expression.type === "u32" &&
        (isDefaultedLiteral(expression) || DEFAULTED_SPANS.has(expression.span))
        ? "usize"
        : undefined;
    case "list-length":
    case "map-length":
      return "usize";
    case "local":
      return spelledBindingType(expression.local, depth);
    case "global":
      return spelledBindingType(expression.global, depth);
    case "permission-weaken":
      return spell(expression.operand);
    case "unary":
      return expression.operator === "cast" ? undefined : spell(expression.operand);
    case "binary": {
      if (expression.left.type !== expression.type) return spell(expression.right);
      return spell(preferred([expression.left, expression.right])!);
    }
    case "list": {
      const element = preferred(expression.elements);
      return element && `List[${spell(element)}]`;
    }
    case "tuple":
      return expression.elements.map(spell).join(",");
    case "map": {
      const keys = preferred(expression.entries.map((entry) => entry.key));
      const values = preferred(expression.entries.map((entry) => entry.value));
      return keys && values && `${spell(keys)},${spell(values)}`;
    }
    case "list-index":
      return spell(expression.receiver);
    case "if":
      return branchHint([expression.thenBody, expression.elseBody], spell);
    case "match":
      return branchHint(
        expression.arms.map((arm) => arm.body),
        spell,
      );
    case "call":
    case "trait-call":
      // A generic call prints a size where its arguments do; a call of a
      // declared result is spelled where it is checked.
      return expression.erasedResultType === undefined
        ? undefined
        : argumentsHint(
            expression.type,
            [
              ...(expression.kind === "trait-call" ? [expression.receiver] : []),
              ...expression.arguments,
            ],
            spell,
          );
    case "enum":
    case "data":
      return argumentsHint(expression.type, expression.fields, spell);
    case "variant-wrap":
      return expression.payload
        ? argumentsHint(expression.type, [expression.payload], spell)
        : undefined;
    default:
      return undefined;
  }
}

/** The member a join prints by: the first that is not built of literals only, or the first. */
function preferred(members: readonly HirExpression[]): HirExpression | undefined {
  return members.find((member) => !literalTree(member)) ?? members[0];
}

/** A literal, or unary and binary arithmetic over literals only, such as `2 * 3`. */
function literalTree(expression: HirExpression): boolean {
  switch (expression.kind) {
    case "integer":
    case "float":
      return true;
    case "unary":
      return expression.operator !== "cast" && literalTree(expression.operand);
    case "binary":
      return literalTree(expression.left) && literalTree(expression.right);
    default:
      return false;
  }
}

function branchHint(
  bodies: readonly (readonly HirStatement[])[],
  spell: (child: HirExpression) => ValueType,
): ValueType | undefined {
  const finals = bodies
    .map((body) => finalValueOf(body))
    .filter((value): value is HirExpression => value !== undefined && value.type !== "never");
  const member = preferred(finals);
  return member && spell(member);
}

/**
 * A generic application prints `usize` for its `u32`s when every argument
 * that has a `u32` in its type prints each one as `usize`, as
 * `Result.Ok(123)` is a `Result[usize, E]`.
 */
function argumentsHint(
  canonical: ValueType,
  members: readonly HirExpression[],
  spell: (child: HirExpression) => ValueType,
): ValueType | undefined {
  let sized = false;
  for (const member of members) {
    if (!member.type.includes("u32")) continue;
    const spelled = spell(member);
    if (U32_WORD.test(spelled)) return undefined;
    sized = true;
  }
  return sized ? canonical.replace(U32_WORDS, "usize") : undefined;
}

/** A call of `signature` printed with the result spelling its declaration wrote. */
export function spelledCall<T extends HirExpression>(
  signature: { readonly spelledResult?: ValueType },
  call: T,
): T {
  return signature.spelledResult === undefined
    ? call
    : spellAs(call, respelled(call.type, signature.spelledResult));
}

/** How a generic application of `members` prints, as `Result[usize, E]` for `Result.Ok(123)`. */
export function spelledApplication(
  canonical: ValueType,
  members: readonly HirExpression[],
): ValueType {
  return respelled(
    canonical,
    argumentsHint(canonical, members, (member) => spelledType(member)),
  );
}
