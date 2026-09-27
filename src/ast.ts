import type { SourceSpan } from "./diagnostics.ts";

export interface TypeRef {
  readonly name: string;
  readonly span: SourceSpan;
}

export interface GenericBound {
  readonly parameter: string;
  readonly traits: readonly string[];
  /** Associated type bindings such as `Item = T` in `I < Supplier[Item = T]`. */
  readonly bindings?: readonly AssociatedTypeBinding[];
  readonly span: SourceSpan;
}

export interface AssociatedTypeBinding {
  /** The bound trait as written in `traits`, without `mut:`. */
  readonly trait: string;
  readonly name: string;
  readonly type: TypeRef;
  readonly span: SourceSpan;
}

export interface Parameter {
  readonly name: string;
  readonly type: TypeRef;
  readonly variadic?: boolean;
  readonly default?: Expression;
  readonly doc?: string;
  readonly span: SourceSpan;
}

export interface FunctionDecl {
  readonly kind: "function";
  readonly public?: boolean;
  readonly name: string;
  readonly suspending: boolean;
  readonly genericParameters: readonly string[];
  readonly genericBounds: readonly GenericBound[];
  readonly parameters: readonly Parameter[];
  readonly result: TypeRef;
  readonly requirements: readonly string[];
  // Set when the source omits `-> type` or the `$` clause. `result` is then a
  // `void` placeholder and `requirements` is empty until the checker infers
  // them (07-functions.md#declarations).
  readonly resultOmitted?: boolean;
  readonly requirementsOmitted?: boolean;
  readonly body: readonly Statement[];
  readonly doc?: string;
  readonly span: SourceSpan;
  // Present on the synthetic function the checker builds for a parameter,
  // data-field, or shared enum default. Such code must be requirement-free
  // (07-functions.md#default-values); `laterNames` are the parameters declared
  // after the defaulted one, which are not yet visible.
  readonly defaultContext?: { readonly laterNames: readonly string[] };
}

export interface MethodDecl {
  /** `pub fn` in an inherent implementation. */
  readonly public?: boolean;
  readonly name: string;
  readonly suspending: boolean;
  readonly genericParameters: readonly string[];
  readonly genericBounds: readonly GenericBound[];
  readonly parameters: readonly Parameter[];
  readonly result: TypeRef;
  readonly requirements: readonly string[];
  readonly resultOmitted?: boolean;
  readonly requirementsOmitted?: boolean;
  readonly body?: readonly Statement[];
  readonly doc?: string;
  readonly span: SourceSpan;
}

export interface AssociatedTypeDecl {
  readonly name: string;
  readonly value?: TypeRef;
  readonly doc?: string;
  readonly span: SourceSpan;
}

export interface TraitDecl {
  readonly kind: "trait";
  readonly public?: boolean;
  readonly name: string;
  readonly genericParameters: readonly string[];
  readonly supertraits: readonly TypeRef[];
  readonly associatedTypes: readonly AssociatedTypeDecl[];
  readonly methods: readonly MethodDecl[];
  readonly doc?: string;
  readonly span: SourceSpan;
}

export interface ImplDecl {
  readonly kind: "impl";
  readonly genericParameters: readonly string[];
  readonly genericBounds: readonly GenericBound[];
  readonly traitName?: string;
  readonly targetName: string;
  /** `impl Trait for C by E`: the embedded field `E` that the trait is delegated to. */
  readonly delegate?: { readonly name: string; readonly span: SourceSpan };
  readonly associatedTypes: readonly AssociatedTypeDecl[];
  readonly methods: readonly MethodDecl[];
  readonly doc?: string;
  readonly span: SourceSpan;
}

export interface DataField {
  /** `pub name: T`; an embedded field takes no marker and is always public. */
  readonly public?: boolean;
  readonly name: string;
  readonly type: TypeRef;
  readonly embedded?: boolean;
  readonly default?: Expression;
  readonly doc?: string;
  readonly span: SourceSpan;
}

export interface DataDecl {
  readonly kind: "data";
  readonly public?: boolean;
  readonly name: string;
  readonly genericParameters: readonly string[];
  readonly fields: readonly DataField[];
  readonly doc?: string;
  /** Lowered from `type Name(Base)`: its one field is the base value. */
  readonly newtype?: boolean;
  /** Declared in a block suite, so not inspectable (09-traits.md#inspectable-types). */
  readonly local?: boolean;
  readonly span: SourceSpan;
}

/** `type Name = T` (a transparent alias) or `type Name(T)` (a newtype). */
export interface TypeDecl {
  readonly kind: "type";
  readonly public?: boolean;
  readonly name: string;
  readonly genericParameters: readonly string[];
  readonly alias?: TypeRef;
  readonly base?: TypeRef;
  readonly doc?: string;
  readonly span: SourceSpan;
}

export interface EnumVariant {
  readonly name: string;
  readonly fields: readonly DataField[];
  readonly result?: Expression;
  readonly doc?: string;
  readonly span: SourceSpan;
}

export interface EnumDecl {
  readonly kind: "enum";
  readonly public?: boolean;
  readonly name: string;
  readonly genericParameters: readonly string[];
  readonly sharedFields: readonly DataField[];
  readonly variants: readonly EnumVariant[];
  readonly doc?: string;
  /** Declared in a block suite, so not inspectable (09-traits.md#inspectable-types). */
  readonly local?: boolean;
  readonly span: SourceSpan;
}

export interface TestDecl {
  readonly kind: "test";
  readonly name: string;
  readonly body: readonly Statement[];
  readonly span: SourceSpan;
}

export interface UseDecl {
  readonly kind: "use";
  readonly module: string;
  readonly names: readonly UseName[];
  readonly public?: boolean;
  readonly span: SourceSpan;
}

export interface UseName {
  readonly name: string;
  readonly alias?: string;
}

export interface DataPatternField {
  readonly name: string;
  readonly pattern: Pattern;
  readonly span: SourceSpan;
}

export type Pattern =
  | { readonly kind: "wildcard"; readonly span: SourceSpan }
  | { readonly kind: "boolean"; readonly value: boolean; readonly span: SourceSpan }
  | { readonly kind: "integer"; readonly value: bigint; readonly span: SourceSpan }
  | { readonly kind: "float"; readonly value: number; readonly span: SourceSpan }
  | { readonly kind: "string"; readonly value: string; readonly span: SourceSpan }
  | { readonly kind: "character"; readonly value: string; readonly span: SourceSpan }
  | { readonly kind: "binding"; readonly name: string; readonly span: SourceSpan }
  | { readonly kind: "tuple"; readonly elements: readonly Pattern[]; readonly span: SourceSpan }
  | {
      readonly kind: "data";
      readonly typeName: string;
      readonly fields: readonly DataPatternField[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "result-variant";
      readonly variantName: "Ok" | "Err";
      readonly bindings: readonly (string | undefined)[];
      readonly payloadPatterns?: readonly Pattern[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "variant";
      readonly enumName?: string;
      /** Written as `Name(...)` with neither a leading `.` nor an enum qualifier. */
      readonly bare?: boolean;
      readonly variantName: string;
      readonly bindings: readonly (string | undefined)[];
      readonly bindingNames?: readonly (string | undefined)[];
      readonly payloadPatterns?: readonly Pattern[];
      readonly span: SourceSpan;
    };

export interface MatchArm {
  readonly pattern: Pattern;
  readonly guard?: Expression;
  readonly body: readonly Statement[];
  readonly span: SourceSpan;
}

export interface BindingName {
  readonly name: string;
  readonly span: SourceSpan;
}

export interface MapEntry {
  readonly key: Expression;
  readonly value: Expression;
  readonly span: SourceSpan;
}

export interface ComprehensionForClause {
  readonly kind: "for";
  readonly bindings: readonly BindingName[];
  readonly iterable: Expression;
  readonly span: SourceSpan;
}

export interface ComprehensionIfClause {
  readonly kind: "if";
  readonly condition: Expression;
  readonly span: SourceSpan;
}

export type ComprehensionClause = ComprehensionForClause | ComprehensionIfClause;

export interface DataExpressionField {
  readonly name: string;
  readonly value: Expression;
  /** `Label: ...value`: the value is copied into an embedded field (VE-S). */
  readonly copy?: boolean;
  readonly span: SourceSpan;
}

export interface ClosureParameter {
  readonly name: string;
  readonly type?: TypeRef;
  readonly span: SourceSpan;
}

export type ProviderContextEntry =
  | {
      readonly kind: "binding";
      readonly key: string;
      readonly value: Expression;
      readonly span: SourceSpan;
    }
  | { readonly kind: "spread"; readonly value: Expression; readonly span: SourceSpan };

export interface Program {
  readonly uses: readonly UseDecl[];
  /** Present when the module declares a `type`. */
  readonly types?: readonly TypeDecl[];
  /** Set when a block suite declares a type or implementation. */
  readonly localDeclarations?: boolean;
  readonly data: readonly DataDecl[];
  readonly enums: readonly EnumDecl[];
  readonly traits: readonly TraitDecl[];
  readonly implementations: readonly ImplDecl[];
  readonly functions: readonly FunctionDecl[];
  readonly tests: readonly TestDecl[];
  readonly statements: readonly Statement[];
  readonly span: SourceSpan;
}

export type Statement =
  | { readonly kind: "defer"; readonly body: readonly Statement[]; readonly span: SourceSpan }
  | {
      readonly kind: "binding";
      readonly name: string;
      readonly annotation?: TypeRef;
      readonly mutable: boolean;
      readonly value: Expression;
      // A local `fn` declaration that omits its result type.
      readonly localFunction?: boolean;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "tuple-binding";
      readonly bindings: readonly BindingName[];
      readonly annotation?: TypeRef;
      readonly mutable: boolean;
      readonly value: Expression;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "assignment";
      readonly name: string;
      readonly value: Expression;
      /** Written with the copy assignment `...=` (VE-S). */
      readonly copy?: boolean;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "field-assignment";
      readonly target: Extract<Expression, { kind: "member" }>;
      readonly value: Expression;
      readonly copy?: boolean;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "index-assignment";
      readonly target: Extract<Expression, { kind: "index" }>;
      readonly value: Expression;
      readonly copy?: boolean;
      readonly span: SourceSpan;
    }
  | { readonly kind: "discard"; readonly value: Expression; readonly span: SourceSpan }
  | { readonly kind: "return"; readonly value?: Expression; readonly span: SourceSpan }
  | { readonly kind: "break"; readonly value?: Expression; readonly span: SourceSpan }
  | { readonly kind: "continue"; readonly span: SourceSpan }
  | { readonly kind: "expression"; readonly expression: Expression; readonly span: SourceSpan }
  | { readonly kind: "pass"; readonly span: SourceSpan }
  | {
      /** A `data`, `enum`, `trait`, `type`, or `impl` declared in a block suite. */
      readonly kind: "local-declaration";
      readonly declaration: DataDecl | EnumDecl | TraitDecl | TypeDecl | ImplDecl;
      readonly span: SourceSpan;
    };

export type Expression =
  | { readonly kind: "integer"; readonly value: bigint; readonly span: SourceSpan }
  | { readonly kind: "float"; readonly value: number; readonly span: SourceSpan }
  | { readonly kind: "string"; readonly value: string; readonly span: SourceSpan }
  | {
      readonly kind: "interpolated-string";
      readonly segments: readonly (
        | { readonly kind: "text"; readonly value: string; readonly span: SourceSpan }
        | {
            readonly kind: "expression";
            readonly expression: Expression;
            readonly span: SourceSpan;
          }
      )[];
      readonly span: SourceSpan;
    }
  | { readonly kind: "character"; readonly value: string; readonly span: SourceSpan }
  | { readonly kind: "boolean"; readonly value: boolean; readonly span: SourceSpan }
  | {
      readonly kind: "binding-expression";
      readonly bindings: readonly BindingName[];
      readonly value: Expression;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "list";
      readonly elements: readonly Expression[];
      // Per element: true for a suffix spread `xs...`.
      readonly spreads?: readonly boolean[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "list-comprehension";
      readonly clauses: readonly ComprehensionClause[];
      readonly value: Expression;
      readonly span: SourceSpan;
    }
  | { readonly kind: "tuple"; readonly elements: readonly Expression[]; readonly span: SourceSpan }
  | {
      readonly kind: "map";
      readonly entries: readonly MapEntry[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "map-comprehension";
      readonly clauses: readonly ComprehensionClause[];
      readonly key: Expression;
      readonly value: Expression;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "name";
      readonly name: string;
      readonly typeArguments?: readonly TypeRef[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "qualified-name";
      readonly owner: string;
      readonly ownerTypeArguments?: readonly TypeRef[];
      readonly name: string;
      readonly typeArguments?: readonly TypeRef[];
      readonly span: SourceSpan;
    }
  | { readonly kind: "contextual-variant"; readonly name: string; readonly span: SourceSpan }
  | {
      readonly kind: "unary";
      readonly operator: string;
      readonly operand: Expression;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "binary";
      readonly operator: string;
      readonly left: Expression;
      readonly right: Expression;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "call";
      readonly callee: Expression;
      readonly typeArguments?: readonly TypeRef[];
      readonly arguments: readonly Expression[];
      readonly argumentNames?: readonly (string | undefined)[];
      readonly argumentSpreads?: readonly boolean[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "suspend-call";
      readonly callee: Expression;
      readonly typeArguments?: readonly TypeRef[];
      readonly arguments: readonly Expression[];
      readonly argumentNames?: readonly (string | undefined)[];
      readonly argumentSpreads?: readonly boolean[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "data";
      readonly name: string;
      readonly typeArguments?: readonly TypeRef[];
      readonly spread?: Expression;
      readonly fields: readonly DataExpressionField[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "member";
      readonly receiver: Expression;
      readonly name: string;
      readonly typeArguments?: readonly TypeRef[];
      /** Written as `(x.name)`, so a following call calls the field's value (M2). */
      readonly parenthesized?: boolean;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "index";
      readonly receiver: Expression;
      readonly index: Expression;
      readonly span: SourceSpan;
    }
  | { readonly kind: "propagate"; readonly operand: Expression; readonly span: SourceSpan }
  | {
      readonly kind: "closure";
      readonly suspending?: boolean;
      readonly parameters: readonly ClosureParameter[];
      readonly result?: TypeRef;
      readonly requirements?: readonly string[];
      readonly body: readonly Statement[];
      readonly span: SourceSpan;
    }
  | { readonly kind: "provider-use"; readonly key: string; readonly span: SourceSpan }
  | {
      readonly kind: "provider-context";
      readonly entries: readonly ProviderContextEntry[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "provider-with";
      readonly entries: readonly ProviderContextEntry[];
      readonly body: readonly Statement[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "if";
      readonly condition: Expression;
      readonly thenBody: readonly Statement[];
      readonly elseBody: readonly Statement[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "for";
      readonly bindings: readonly BindingName[];
      readonly iterable: Expression;
      readonly body: readonly Statement[];
      readonly elseBody: readonly Statement[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "while";
      readonly condition: Expression;
      readonly body: readonly Statement[];
      readonly elseBody: readonly Statement[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "match";
      readonly subject: Expression;
      readonly arms: readonly MatchArm[];
      readonly span: SourceSpan;
    };
