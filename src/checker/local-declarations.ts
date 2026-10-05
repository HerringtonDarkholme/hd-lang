import type {
  DataDecl,
  EnumDecl,
  ImplDecl,
  Program,
  Statement,
  TraitDecl,
  TypeDecl,
} from "../ast.ts";
import { preserveSourceOrigin, type Diagnostic } from "../diagnostics.ts";
import { displayType } from "../types.ts";

// Local declarations (03-names-and-scopes.md#function-and-closure-scopes). A
// `data`, `enum`, `trait`, or `type` declared in a block suite is visible from
// its declaration point to the end of that suite. The prototype checks one
// module, so it hoists each one to module scope under a name source cannot
// write, `Name#n`, and renames the references in its scope; a reference
// outside the scope still names the source spelling and so does not find
// it. A local `impl` is likewise hoisted for coherence checking, but leaves
// an internal marker at its declaration point. Method lookup only includes
// the marked implementations in the current lexical suite. A local
// trait-less derivation block is rejected. A local data or enum type is not
// inspectable.

type Renames = ReadonlyMap<string, string>;

const SUITE_KEYS = new Set(["body", "thenBody", "elseBody"]);
const TYPE_KEYS = new Set(["type", "result", "annotation", "value", "alias", "base"]);
const TYPE_LIST_KEYS = new Set(["typeArguments", "ownerTypeArguments", "supertraits"]);
const NAME_LIST_KEYS = new Set(["traits", "requirements"]);

function isTypeRef(value: unknown): value is { readonly name: string } {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    typeof (value as { name?: unknown }).name === "string" &&
    keys.includes("span") &&
    keys.every((key) => key === "name" || key === "span" || key === "written")
  );
}

/** Renames every whole-word occurrence of a local type name in a type string. */
function renameWords(text: string, renames: Renames): string {
  if (renames.size === 0) return text;
  return text.replace(/[\p{ID_Start}_][\p{ID_Continue}]*/gu, (word, offset: number) =>
    text[offset - 1] === "#" ? word : (renames.get(word) ?? word),
  );
}

/** The type-constructor name an `impl` header starts with, as in `Box` for `Box[T]`. */
function headName(type: string): string {
  return type.replace(/^mut:/, "").split("[")[0]!;
}

export function hoistLocalDeclarations(program: Program): {
  readonly program: Program;
  readonly diagnostics: readonly Diagnostic[];
} {
  if (!program.localDeclarations) return { program, diagnostics: [] };
  const diagnostics: Diagnostic[] = [];
  const data: DataDecl[] = [];
  const enums: EnumDecl[] = [];
  const traits: TraitDecl[] = [];
  const types: TypeDecl[] = [];
  const implementations: ImplDecl[] = [];
  let counter = 0;
  let implementationCounter = 0;

  const rewrite = <T>(
    node: T,
    renames: Renames,
    localImplementations: readonly number[],
    key?: string,
  ): T => {
    if (Array.isArray(node)) {
      if (key !== undefined && SUITE_KEYS.has(key))
        return processSuite(
          node as unknown as readonly Statement[],
          renames,
          localImplementations,
        ) as unknown as T;
      if (key !== undefined && NAME_LIST_KEYS.has(key))
        return node.map((item) =>
          typeof item === "string"
            ? renameWords(item, renames)
            : rewrite(item, renames, localImplementations),
        ) as T;
      return node.map((item) => rewrite(item, renames, localImplementations, key)) as T;
    }
    if (!node || typeof node !== "object") return node;
    const result: Record<string, unknown> = {};
    const record = node as Record<string, unknown>;
    for (const [entry, value] of Object.entries(record)) {
      if (TYPE_KEYS.has(entry) && isTypeRef(value))
        result[entry] = { ...value, name: renameWords(value.name, renames) };
      else if (TYPE_LIST_KEYS.has(entry) && Array.isArray(value))
        result[entry] = value.map((item) =>
          isTypeRef(item)
            ? { ...item, name: renameWords(item.name, renames) }
            : rewrite(item, renames, localImplementations),
        );
      else result[entry] = rewrite(value, renames, localImplementations, entry);
    }
    // Names that spell a type outside a type position.
    const kind = record.kind;
    const rename = (field: string): void => {
      if (typeof result[field] === "string")
        result[field] = renames.get(result[field] as string) ?? result[field];
    };
    if (kind === "name" || (kind === "data" && !("genericParameters" in record))) rename("name");
    if (kind === "qualified-name") rename("owner");
    if (kind === "variant") rename("enumName");
    if (kind === "data" && "fields" in record && "typeName" in record) rename("typeName");
    if (kind === "impl") {
      result.targetName = renameWords(result.targetName as string, renames);
      if (typeof result.traitName === "string")
        result.traitName = renameWords(result.traitName, renames);
    }
    return preserveSourceOrigin(node, result) as T;
  };

  const processSuite = (
    statements: readonly Statement[],
    outer: Renames,
    outerImplementations: readonly number[],
  ): Statement[] => {
    let renames = outer;
    let visibleImplementations = [...outerImplementations];
    const declaredHere = new Set<string>();
    const result: Statement[] = [];
    for (const statement of statements) {
      if (statement.kind !== "local-declaration") {
        result.push(rewrite(statement, renames, visibleImplementations));
        continue;
      }
      const declaration = statement.declaration;
      // Local declarations carry no metadata (annot.traitless.local).
      if (declaration.kind === "impl" && declaration.traitName === undefined) {
        if (declaration.byStructure || declaration.delegate) {
          diagnostics.push(
            declaration.byStructure
              ? {
                  code: "misplaced-derivation",
                  message: "a trait-less derivation block must be declared at module scope",
                  span: statement.span,
                }
              : {
                  code: "invalid-delegation",
                  message: "a header without a trait never delegates",
                  span: declaration.delegate!.span,
                },
          );
          continue;
        }
      }
      if (declaration.kind === "impl") {
        const localTrait =
          declaration.traitName !== undefined && renames.has(headName(declaration.traitName));
        const localTarget = renames.has(headName(declaration.targetName));
        if (!localTrait && !localTarget) {
          diagnostics.push({
            code: "local-impl-nonlocal-pair",
            message:
              declaration.traitName === undefined
                ? `a local inherent implementation must target a local type, not '${declaration.targetName}'`
                : `a local implementation of '${displayType(declaration.traitName ?? "")}' for '${displayType(declaration.targetName)}' involves no local type or trait; declare it at module scope`,
            span: statement.span,
          });
          continue;
        }
        implementationCounter += 1;
        const localImplementation = implementationCounter;
        const inside = [...visibleImplementations, localImplementation];
        implementations.push({
          ...rewrite(declaration, renames, inside),
          localImplementation,
          localImplementations: inside,
        });
        result.push({
          kind: "local-implementation",
          implementation: localImplementation,
          span: statement.span,
        });
        visibleImplementations = inside;
        continue;
      }
      if (declaredHere.has(declaration.name))
        diagnostics.push({
          code: "duplicate-type",
          message: `type '${declaration.name}' is already declared in this suite`,
          span: declaration.span,
        });
      declaredHere.add(declaration.name);
      counter += 1;
      const hoisted = `${declaration.name}#${counter}`;
      renames = new Map(renames).set(declaration.name, hoisted);
      const renamed = {
        ...rewrite(declaration, renames, visibleImplementations),
        name: hoisted,
      };
      if (renamed.kind === "data")
        data.push({ ...renamed, local: true, localImplementations: visibleImplementations });
      else if (renamed.kind === "enum")
        enums.push({ ...renamed, local: true, localImplementations: visibleImplementations });
      else if (renamed.kind === "trait")
        traits.push({ ...renamed, localImplementations: visibleImplementations });
      else types.push(renamed);
    }
    return result;
  };

  const rewritten = rewrite(program, new Map(), []);
  return {
    program: {
      ...rewritten,
      localDeclarations: false,
      data: [...rewritten.data, ...data],
      enums: [...rewritten.enums, ...enums],
      traits: [...rewritten.traits, ...traits],
      implementations: [...rewritten.implementations, ...implementations],
      ...(types.length > 0 || rewritten.types
        ? { types: [...(rewritten.types ?? []), ...types] }
        : {}),
    },
    diagnostics,
  };
}
