import type { SourceSpan } from "./diagnostics.ts";

export type ValueType = string;

export interface HirDataField {
  readonly name: string;
  readonly type: ValueType;
  readonly index: number;
  readonly embedded?: boolean;
  readonly defaultFunctionName?: string;
  readonly span: SourceSpan;
}

export interface HirData {
  readonly name: string;
  readonly index: number;
  readonly genericParameters: readonly string[];
  readonly fields: readonly HirDataField[];
  readonly span: SourceSpan;
}

export interface HirEnumVariant {
  readonly name: string;
  readonly tag: number;
  readonly fields: readonly HirDataField[];
  readonly factoryFunctionName?: string;
  readonly span: SourceSpan;
}

export interface HirEnum {
  readonly name: string;
  readonly index: number;
  readonly genericParameters: readonly string[];
  readonly sharedFields: readonly HirDataField[];
  readonly variants: readonly HirEnumVariant[];
  readonly fields: readonly HirDataField[];
  readonly span: SourceSpan;
}

export interface HirTraitMethod {
  readonly name: string;
  readonly index: number;
  readonly associated: boolean;
  readonly genericParameters: readonly string[];
  readonly suspending: boolean;
  readonly receiverMutable: boolean;
  readonly parameters: readonly ValueType[];
  readonly parameterNames: readonly string[];
  readonly variadic: boolean;
  readonly result: ValueType;
  readonly requirements: readonly string[];
  readonly span: SourceSpan;
}

export interface HirAssociatedType {
  readonly name: string;
  readonly index: number;
  readonly span: SourceSpan;
}

export interface HirSupertrait {
  readonly traitIndex: number;
  readonly traitName: string;
  readonly traitArguments: readonly ValueType[];
}

export interface HirTrait {
  readonly name: string;
  readonly index: number;
  readonly genericParameters: readonly string[];
  readonly supertraits: readonly HirSupertrait[];
  readonly associatedTypes: readonly HirAssociatedType[];
  readonly methods: readonly HirTraitMethod[];
  readonly span: SourceSpan;
}

export interface HirTraitMethodFunction {
  readonly methodIndex: number;
  readonly functionIndex: number;
}

export interface HirTraitImplementation {
  readonly index: number;
  readonly traitIndex: number;
  readonly traitName: string;
  readonly traitArguments: readonly ValueType[];
  readonly targetType: ValueType;
  readonly associatedTypes: readonly ValueType[];
  readonly genericBounds: readonly HirGenericBound[];
  readonly genericParameters: readonly string[];
  readonly supertraitImplementations: readonly number[];
  readonly methodFunctions: readonly HirTraitMethodFunction[];
  readonly span: SourceSpan;
}

export interface HirTraitDictionaryPlan {
  readonly bounds: readonly HirExpression[];
  readonly implementationIndex: number;
  readonly supertraits: readonly HirTraitDictionaryPlan[];
  // Set for a standard-library implementation that has no source `impl`
  // (Display, PartialEq, PartialOrd on primitives and built-in composites).
  // `implementationIndex` is then -1, and `bounds` holds the dictionaries the
  // strategy's bound dispatches read, renumbered from zero.
  readonly builtin?: HirBuiltinTraitImplementation;
}

export type HirBuiltinTraitImplementation =
  | {
      readonly kind: "display";
      readonly traitIndex: number;
      readonly targetType: ValueType;
    }
  | {
      readonly kind: "equality";
      readonly traitIndex: number;
      readonly targetType: ValueType;
      readonly strategy: HirEqualityStrategy;
    }
  | {
      readonly kind: "ordering";
      readonly traitIndex: number;
      readonly targetType: ValueType;
      readonly strategy: HirOrderingStrategy;
    };

export interface HirPatternPathStep {
  readonly dataIndex: number;
  readonly fieldIndex: number;
}

export interface HirMatchTest {
  readonly path?: readonly HirPatternPathStep[];
  readonly enumFieldIndex?: number;
  readonly erasedFieldType?: ValueType;
  readonly valueType?: ValueType;
  readonly accessPath?: readonly HirPatternAccessStep[];
  readonly tag?: number;
  readonly tagEnumIndex?: number;
  readonly literal?: HirExpression;
}

export interface HirMatchBinding {
  readonly local: HirLocal;
  readonly fieldIndex: number;
  readonly type: ValueType;
  readonly erasedFieldType?: ValueType;
  readonly path?: readonly HirPatternPathStep[];
  readonly enumFieldIndex?: number;
  readonly enumFieldType?: ValueType;
  readonly enumErasedFieldType?: ValueType;
  readonly accessPath?: readonly HirPatternAccessStep[];
}

export interface HirMatchArm {
  readonly tag?: number;
  readonly literal?: HirExpression;
  readonly guard?: HirExpression;
  readonly tests?: readonly HirMatchTest[];
  readonly bindings: readonly HirMatchBinding[];
  readonly body: readonly HirStatement[];
  readonly span: SourceSpan;
}

export interface HirPatternAccessStep {
  readonly kind: "data" | "enum" | "erased-variant";
  readonly typeIndex: number;
  readonly fieldIndex: number;
  readonly erasedFieldType?: ValueType;
  readonly valueType: ValueType;
}

export interface HirLocal {
  readonly name: string;
  readonly type: ValueType;
  readonly index: number;
  readonly mutable: boolean;
  readonly drivable?: boolean;
  readonly parameter: boolean;
  readonly span: SourceSpan;
}

export interface HirGlobal {
  readonly name: string;
  readonly type: ValueType;
  readonly index: number;
  readonly mutable: boolean;
  readonly drivable?: boolean;
  readonly span: SourceSpan;
}

export interface HirCapture {
  readonly source: HirLocal;
  readonly fieldIndex: number;
}

export interface HirGenericBound {
  readonly parameter: string;
  readonly traitName: string;
  readonly traitIndex: number;
  readonly traitArguments: readonly ValueType[];
  readonly mutable: boolean;
}

export interface HirFunction {
  readonly name: string;
  readonly index: number;
  readonly suspending: boolean;
  readonly suspensionIndex?: number;
  readonly variadic: boolean;
  readonly genericParameters: readonly string[];
  readonly genericBounds: readonly HirGenericBound[];
  readonly rowParameters: readonly string[];
  readonly parameters: readonly HirLocal[];
  readonly result: ValueType;
  readonly requirements: readonly string[];
  readonly locals: readonly HirLocal[];
  readonly body: readonly HirStatement[];
  readonly span: SourceSpan;
  readonly synthetic: boolean;
  readonly closure: boolean;
  readonly captures: readonly HirCapture[];
  // The executable entry point: a public top-level `main` or `main!`
  // (10-modules.md#executable-entry-point), or the empty `main` synthesized
  // for a script. A non-public `main` is an ordinary function.
  readonly entry?: boolean;
}

export interface HirProgram {
  readonly data: readonly HirData[];
  readonly enums: readonly HirEnum[];
  readonly traits: readonly HirTrait[];
  readonly implementations: readonly HirTraitImplementation[];
  readonly globals: readonly HirGlobal[];
  readonly functions: readonly HirFunction[];
  readonly closures: readonly HirFunction[];
  readonly hostCapabilities: readonly string[];
  readonly initializer?: number;
}

export type HirProviderContextEntry =
  | {
      readonly kind: "binding";
      readonly key: string;
      readonly local: HirLocal;
      readonly value: HirExpression;
    }
  | {
      readonly kind: "spread";
      readonly contextLocal: HirLocal;
      readonly value: HirExpression;
      readonly providers: readonly HirProviderSpreadBinding[];
    };

export interface HirProviderSpreadBinding {
  readonly key: string;
  readonly local: HirLocal;
  readonly fieldIndex: number;
}

export interface HirMapEntry {
  readonly key: HirExpression;
  readonly value: HirExpression;
}

export interface HirDefaultArgument {
  readonly parameterIndex: number;
  readonly functionIndex: number;
}

export type HirEqualityDispatch =
  | { readonly kind: "function"; readonly functionIndex: number }
  | {
      readonly kind: "bound";
      readonly traitIndex: number;
      readonly methodIndex: number;
      readonly boundIndex: number;
    };

export interface HirBuiltinEqualityStrategy {
  readonly kind: "builtin";
}

export interface HirDispatchEqualityStrategy {
  readonly kind: "dispatch";
  readonly dispatch: HirEqualityDispatch;
}

export interface HirTupleEqualityStrategy {
  readonly kind: "tuple";
  readonly elements: readonly HirEqualityStrategy[];
}

export interface HirOptionalEqualityStrategy {
  readonly kind: "optional";
  readonly value: HirEqualityStrategy;
}

export interface HirResultEqualityStrategy {
  readonly kind: "result";
  readonly ok: HirEqualityStrategy;
  readonly error: HirEqualityStrategy;
}

export interface HirListEqualityStrategy {
  readonly kind: "list";
  readonly element: HirEqualityStrategy;
}

export interface HirMapEqualityStrategy {
  readonly kind: "map";
  readonly value: HirEqualityStrategy;
}

export type HirEqualityStrategy =
  | HirBuiltinEqualityStrategy
  | HirDispatchEqualityStrategy
  | HirTupleEqualityStrategy
  | HirOptionalEqualityStrategy
  | HirResultEqualityStrategy
  | HirListEqualityStrategy
  | HirMapEqualityStrategy;

export type HirOrderingOperator = "<" | "<=" | ">" | ">=";

export interface HirBuiltinOrderingStrategy {
  readonly kind: "builtin";
}

export interface HirDispatchOrderingStrategy {
  readonly kind: "dispatch";
  readonly dispatch: HirEqualityDispatch;
}

export interface HirTupleOrderingStrategy {
  readonly kind: "tuple";
  readonly elements: readonly HirOrderingStrategy[];
}

export interface HirOptionalOrderingStrategy {
  readonly kind: "optional";
  readonly value: HirOrderingStrategy;
}

export interface HirListOrderingStrategy {
  readonly kind: "list";
  readonly element: HirOrderingStrategy;
}

export type HirOrderingStrategy =
  | HirBuiltinOrderingStrategy
  | HirDispatchOrderingStrategy
  | HirTupleOrderingStrategy
  | HirOptionalOrderingStrategy
  | HirListOrderingStrategy;

export type HirStatement =
  | { readonly kind: "defer"; readonly body: readonly HirStatement[]; readonly span: SourceSpan }
  | {
      readonly kind: "binding";
      readonly local: HirLocal;
      readonly value: HirExpression;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "assignment";
      readonly local: HirLocal;
      readonly value: HirExpression;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "global-binding";
      readonly global: HirGlobal;
      readonly value: HirExpression;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "global-assignment";
      readonly global: HirGlobal;
      readonly value: HirExpression;
      readonly span: SourceSpan;
    }
  | { readonly kind: "discard"; readonly value: HirExpression; readonly span: SourceSpan }
  | { readonly kind: "return"; readonly value?: HirExpression; readonly span: SourceSpan }
  | { readonly kind: "break"; readonly value?: HirExpression; readonly span: SourceSpan }
  | { readonly kind: "continue"; readonly span: SourceSpan }
  | { readonly kind: "expression"; readonly expression: HirExpression; readonly span: SourceSpan }
  | { readonly kind: "pass"; readonly span: SourceSpan };

interface HirExpressionBase {
  readonly type: ValueType;
  readonly span: SourceSpan;
}

export interface HirComprehensionForClause {
  readonly kind: "for";
  readonly iterable: HirExpression;
  readonly iteratorKind: "iterator" | "list" | "map" | "trait";
  readonly iteratorFunctionIndex?: number;
  readonly yieldType: ValueType;
  readonly bindings: readonly HirLocal[];
  readonly span: SourceSpan;
}

export interface HirComprehensionIfClause {
  readonly kind: "if";
  readonly condition: HirExpression;
  readonly span: SourceSpan;
}

export type HirComprehensionClause = HirComprehensionForClause | HirComprehensionIfClause;

export type HirExpression =
  | (HirExpressionBase & { readonly kind: "integer"; readonly value: number })
  | (HirExpressionBase & { readonly kind: "float"; readonly value: number })
  | (HirExpressionBase & { readonly kind: "string"; readonly bytes: readonly number[] })
  | (HirExpressionBase & {
      readonly kind: "string-build";
      readonly segments: readonly HirExpression[];
    })
  | (HirExpressionBase & { readonly kind: "display"; readonly operand: HirExpression })
  | (HirExpressionBase & {
      readonly kind: "string-transform";
      readonly operation: "lower" | "trim";
      readonly receiver: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "string-split";
      readonly receiver: HirExpression;
      readonly separator: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "console-print";
      readonly provider: HirExpression;
      readonly value: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "assert-equal";
      readonly arguments: readonly HirExpression[];
      readonly argumentParameterIndices?: readonly number[];
      readonly valueType: ValueType;
      readonly strategy: HirEqualityStrategy;
    })
  | (HirExpressionBase & {
      readonly kind: "value-equality";
      readonly left: HirExpression;
      readonly right: HirExpression;
      readonly valueType: ValueType;
      readonly strategy: HirEqualityStrategy;
    })
  | (HirExpressionBase & {
      readonly kind: "assert";
      readonly arguments: readonly HirExpression[];
      readonly argumentParameterIndices?: readonly number[];
    })
  | (HirExpressionBase & {
      readonly kind: "value-ordering";
      readonly left: HirExpression;
      readonly right: HirExpression;
      readonly valueType: ValueType;
      readonly strategy: HirOrderingStrategy;
      readonly operator: HirOrderingOperator;
    })
  | (HirExpressionBase & { readonly kind: "permission-weaken"; readonly operand: HirExpression })
  | (HirExpressionBase & { readonly kind: "character"; readonly value: number })
  | (HirExpressionBase & { readonly kind: "boolean"; readonly value: boolean })
  | (HirExpressionBase & {
      readonly kind: "binding-expression";
      readonly bindings: readonly HirLocal[];
      readonly value: HirExpression;
      readonly elementTypes?: readonly ValueType[];
    })
  | (HirExpressionBase & {
      readonly kind: "list";
      readonly elements: readonly HirExpression[];
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "list-comprehension";
      readonly clauses: readonly HirComprehensionClause[];
      readonly value: HirExpression;
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "tuple";
      readonly elements: readonly HirExpression[];
      readonly elementTypes: readonly ValueType[];
    })
  | (HirExpressionBase & {
      readonly kind: "map";
      readonly entries: readonly HirMapEntry[];
      readonly keyType: ValueType;
      readonly valueType: ValueType;
      readonly keyKind: 0 | 1;
    })
  | (HirExpressionBase & {
      readonly kind: "map-comprehension";
      readonly clauses: readonly HirComprehensionClause[];
      readonly key: HirExpression;
      readonly value: HirExpression;
      readonly keyType: ValueType;
      readonly valueType: ValueType;
      readonly keyKind: 0 | 1;
    })
  | (HirExpressionBase & {
      readonly kind: "variant-wrap";
      readonly variant: "optional-present" | "optional-absent" | "result-ok" | "result-error";
      readonly payload?: HirExpression;
      readonly payloadType?: ValueType;
    })
  | (HirExpressionBase & { readonly kind: "variant-tag"; readonly receiver: HirExpression })
  | (HirExpressionBase & {
      readonly kind: "variant-payload";
      readonly receiver: HirExpression;
      readonly payloadType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "propagate";
      readonly operand: HirExpression;
      readonly payloadType: ValueType;
      readonly successTag: number;
      readonly returnType: ValueType;
    })
  | (HirExpressionBase & { readonly kind: "local"; readonly local: HirLocal })
  | (HirExpressionBase & { readonly kind: "global"; readonly global: HirGlobal })
  | (HirExpressionBase & {
      readonly kind: "capture";
      readonly closureIndex: number;
      readonly fieldIndex: number;
    })
  | (HirExpressionBase & {
      readonly kind: "unary";
      readonly operator: string;
      readonly operand: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "binary";
      readonly operator: string;
      readonly left: HirExpression;
      readonly right: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "call";
      readonly functionIndex: number;
      readonly functionName: string;
      readonly arguments: readonly HirExpression[];
      readonly argumentParameterIndices?: readonly number[];
      readonly defaultArguments?: readonly HirDefaultArgument[];
      readonly parameterTypes?: readonly ValueType[];
      readonly bounds?: readonly HirExpression[];
      readonly providers: readonly HirExpression[];
      readonly erasedParameterTypes?: readonly ValueType[];
      readonly erasedResultType?: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "suspend-construct";
      readonly functionIndex: number;
      readonly functionName: string;
      readonly arguments: readonly HirExpression[];
      readonly argumentParameterIndices?: readonly number[];
      readonly defaultArguments?: readonly HirDefaultArgument[];
      readonly parameterTypes?: readonly ValueType[];
      readonly bounds?: readonly HirExpression[];
      readonly providers: readonly HirExpression[];
      readonly erasedParameterTypes?: readonly ValueType[];
    })
  | (HirExpressionBase & {
      readonly kind: "suspension-wrap";
      readonly suspension: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "suspend-drive";
      readonly functionIndex: number;
      readonly suspension: HirExpression;
      readonly erasedResultType?: ValueType;
      readonly blockOn?: boolean;
    })
  | (HirExpressionBase & {
      readonly kind: "suspension-drive";
      readonly suspension: HirExpression;
      readonly blockOn?: boolean;
    })
  | (HirExpressionBase & {
      readonly kind: "suspend-cancel";
      readonly functionIndex: number;
      readonly suspension: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "trait-suspend-construct";
      readonly receiver: HirExpression;
      readonly traitIndex: number;
      readonly methodIndex: number;
      readonly supertraitPath?: readonly number[];
      readonly arguments: readonly HirExpression[];
      readonly argumentParameterIndices?: readonly number[];
      readonly providers: readonly HirExpression[];
      readonly erasedParameterTypes?: readonly ValueType[];
    })
  | (HirExpressionBase & {
      readonly kind: "trait-suspend-drive";
      readonly traitIndex: number;
      readonly methodIndex: number;
      readonly suspension: HirExpression;
      readonly blockOn?: boolean;
    })
  | (HirExpressionBase & {
      readonly kind: "trait-suspend-cancel";
      readonly traitIndex: number;
      readonly methodIndex: number;
      readonly suspension: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "suspension-cancel";
      readonly suspension: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "function-value";
      readonly functionIndex: number;
      readonly functionName: string;
    })
  | (HirExpressionBase & { readonly kind: "closure-self"; readonly closureIndex: number })
  | (HirExpressionBase & {
      readonly kind: "closure";
      readonly closureIndex: number;
      readonly captures: readonly HirExpression[];
    })
  | (HirExpressionBase & {
      readonly kind: "closure-call";
      readonly callee: HirExpression;
      readonly arguments: readonly HirExpression[];
      readonly providers: readonly HirExpression[];
    })
  | (HirExpressionBase & {
      readonly kind: "trait-wrap";
      readonly value: HirExpression;
      readonly traitIndex: number;
      readonly dictionary: HirTraitDictionaryPlan;
    })
  | (HirExpressionBase & {
      readonly kind: "trait-upcast";
      readonly value: HirExpression;
      readonly sourceTraitIndex: number;
      readonly targetTraitIndex: number;
      readonly supertraitPath: readonly number[];
    })
  | (HirExpressionBase & {
      readonly kind: "trait-dictionary";
      readonly traitIndex: number;
      readonly dictionary: HirTraitDictionaryPlan;
    })
  | (HirExpressionBase & {
      readonly kind: "trait-bound-dictionary";
      readonly traitIndex: number;
      readonly boundIndex: number;
    })
  | (HirExpressionBase & {
      readonly kind: "trait-bound";
      readonly value: HirExpression;
      readonly traitIndex: number;
      readonly boundIndex: number;
    })
  | (HirExpressionBase & {
      readonly kind: "trait-call";
      readonly receiver: HirExpression;
      readonly traitIndex: number;
      readonly methodIndex: number;
      readonly supertraitPath?: readonly number[];
      readonly arguments: readonly HirExpression[];
      readonly argumentParameterIndices?: readonly number[];
      readonly providers: readonly HirExpression[];
      readonly erasedParameterTypes?: readonly ValueType[];
      readonly erasedResultType?: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "provider-use";
      readonly providerIndex: number;
      readonly key: string;
    })
  | (HirExpressionBase & {
      readonly kind: "provider-pack";
      readonly keys: readonly string[];
      readonly providers: readonly HirExpression[];
      readonly bases: readonly HirExpression[];
    })
  | (HirExpressionBase & {
      readonly kind: "provider-context";
      readonly keys: readonly string[];
      readonly entries: readonly HirProviderContextEntry[];
    })
  | (HirExpressionBase & {
      readonly kind: "provider-with";
      readonly entries: readonly HirProviderContextEntry[];
      readonly body: readonly HirStatement[];
    })
  | (HirExpressionBase & {
      readonly kind: "data";
      readonly dataIndex: number;
      readonly spread?: HirExpression;
      readonly fields: readonly HirExpression[];
      readonly fieldIndices: readonly number[];
      readonly erasedFieldTypes?: readonly ValueType[];
    })
  | (HirExpressionBase & {
      readonly kind: "enum";
      readonly enumIndex: number;
      readonly tag: number;
      readonly fields: readonly HirExpression[];
      readonly fieldIndices: readonly number[];
      readonly fieldTypes: readonly ValueType[];
      readonly erasedFieldTypes?: readonly ValueType[];
    })
  | (HirExpressionBase & {
      readonly kind: "member";
      readonly receiver: HirExpression;
      readonly dataIndex: number;
      readonly fieldIndex: number;
      readonly erasedFieldType?: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "field-set";
      readonly receiver: HirExpression;
      readonly value: HirExpression;
      readonly dataIndex: number;
      readonly fieldIndex: number;
      readonly erasedFieldType?: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "list-set";
      readonly receiver: HirExpression;
      readonly index: HirExpression;
      readonly value: HirExpression;
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "list-append";
      readonly receiver: HirExpression;
      readonly value: HirExpression;
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "map-set";
      readonly receiver: HirExpression;
      readonly key: HirExpression;
      readonly value: HirExpression;
      readonly keyType: ValueType;
      readonly valueType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "enum-member";
      readonly receiver: HirExpression;
      readonly enumIndex: number;
      readonly fieldIndex: number;
      readonly erasedFieldType?: ValueType;
    })
  | (HirExpressionBase & { readonly kind: "string-length"; readonly receiver: HirExpression })
  | (HirExpressionBase & {
      readonly kind: "string-starts-with";
      readonly receiver: HirExpression;
      readonly prefix: HirExpression;
    })
  | (HirExpressionBase & { readonly kind: "list-length"; readonly receiver: HirExpression })
  | (HirExpressionBase & {
      readonly kind: "list-iterator";
      readonly receiver: HirExpression;
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "iterator-next";
      readonly receiver: HirExpression;
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "map-iterator";
      readonly receiver: HirExpression;
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "list-index";
      readonly receiver: HirExpression;
      readonly index: HirExpression;
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "tuple-index";
      readonly receiver: HirExpression;
      readonly index: number;
      readonly elementType: ValueType;
    })
  | (HirExpressionBase & { readonly kind: "map-length"; readonly receiver: HirExpression })
  | (HirExpressionBase & {
      readonly kind: "map-index";
      readonly receiver: HirExpression;
      readonly key: HirExpression;
      readonly keyType: ValueType;
      readonly valueType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "map-remove";
      readonly receiver: HirExpression;
      readonly key: HirExpression;
      readonly keyType: ValueType;
      readonly valueType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "map-entry-key";
      readonly receiver: HirExpression;
      readonly index: HirExpression;
      readonly keyType: ValueType;
    })
  | (HirExpressionBase & {
      readonly kind: "map-entry-value";
      readonly receiver: HirExpression;
      readonly index: HirExpression;
      readonly valueType: ValueType;
    })
  | (HirExpressionBase & { readonly kind: "panic"; readonly message: HirExpression })
  | (HirExpressionBase & {
      readonly kind: "if";
      readonly condition: HirExpression;
      readonly thenBody: readonly HirStatement[];
      readonly elseBody: readonly HirStatement[];
    })
  | (HirExpressionBase & {
      readonly kind: "for";
      readonly iterable: HirExpression;
      readonly iteratorKind: "iterator" | "list" | "map" | "trait";
      readonly iteratorFunctionIndex?: number;
      readonly yieldType: ValueType;
      readonly bindings: readonly HirLocal[];
      readonly body: readonly HirStatement[];
      readonly elseBody: readonly HirStatement[];
    })
  | (HirExpressionBase & {
      readonly kind: "while";
      readonly condition: HirExpression;
      readonly body: readonly HirStatement[];
      readonly elseBody: readonly HirStatement[];
    })
  | (HirExpressionBase & {
      readonly kind: "match";
      readonly subject: HirExpression;
      readonly representation: "enum" | "erased-variant" | "scalar" | "data";
      readonly enumIndex?: number;
      readonly arms: readonly HirMatchArm[];
    });
