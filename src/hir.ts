import type { SourceSpan } from "./diagnostics.ts";

export type ValueType = string;

/** One declared generic binder's concrete type at an erased storage boundary. */
export interface HirTypeSubstitution {
  readonly parameter: string;
  readonly type: ValueType;
}

export interface HirDataField {
  /** Marked `pub`; embedded fields are always public. */
  readonly public?: boolean;
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
  /** Declared `+T`/`-T` markers (04-type-system.md#variance); absent means all invariant. */
  readonly variances?: readonly ("+" | "-" | undefined)[];
  /** Parameters used as requirement rows in a field type, as `R` in `fn() -> void $ R`. */
  readonly rowParameters?: readonly string[];
  /** Type-argument defaults, which a data literal applies to what it leaves unsolved. */
  readonly genericDefaults?: ReadonlyMap<string, ValueType>;
  readonly fields: readonly HirDataField[];
  /** A newtype (`type Name(Base)`): its one field holds the base value. */
  readonly newtype?: true;
  /** Declared in a block suite, so not inspectable. */
  readonly local?: true;
  /** Declared by `lib/std`, whose private fields other code cannot read. */
  readonly standard?: true;
  /** The qualified name of a std data type, such as `std.ops.Range`. */
  readonly standardName?: string;
  readonly span: SourceSpan;
}

interface HirEnumVariant {
  readonly name: string;
  readonly tag: number;
  readonly fields: readonly HirDataField[];
  readonly factoryFunctionName?: string;
  readonly span: SourceSpan;
}

export interface HirEnum {
  readonly name: string;
  readonly index: number;
  /** Declared `+T`/`-T` markers (04-type-system.md#variance); absent means all invariant. */
  readonly variances?: readonly ("+" | "-" | undefined)[];
  /** Declared in a block suite, so not inspectable. */
  readonly local?: true;
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
  // Method-level bounds other than AnyVal, AnyRef, and Any. Each is passed as a
  // dictionary argument after the ordinary parameters, also through a
  // dynamic trait value (04-type-system.md#trait-values-and-any).
  readonly genericBounds?: readonly HirGenericBound[];
  // Method-level generic parameters bounded by AnyRef.
  readonly referenceParameters?: readonly string[];
  // Method-level generic parameters bounded by AnyVal. They keep the method
  // out of dynamic dispatch (04-type-system.md#trait-values-and-any).
  readonly valueParameters?: readonly string[];
  /** Type-argument defaults of the method-level parameters (09-traits.md#method-generic-parameters). */
  readonly genericDefaults?: ReadonlyMap<string, ValueType>;
  readonly suspending: boolean;
  readonly receiverMutable: boolean;
  readonly parameters: readonly ValueType[];
  readonly parameterNames: readonly string[];
  readonly variadic: boolean;
  readonly result: ValueType;
  readonly requirements: readonly string[];
  readonly span: SourceSpan;
}

interface HirAssociatedType {
  readonly name: string;
  readonly index: number;
  readonly span: SourceSpan;
}

export interface HirSupertrait {
  readonly traitIndex: number;
  readonly traitName: string;
  readonly traitArguments: readonly ValueType[];
  /** `Name = type` bindings in the supertrait list, where `type` may name `generic:Self`. */
  readonly associatedBindings?: readonly HirAssociatedBinding[];
}

export interface HirTrait {
  readonly name: string;
  /** The qualified name of a std trait, such as `std.ops.Add`. */
  readonly standardName?: string;
  readonly index: number;
  readonly genericParameters: readonly string[];
  readonly supertraits: readonly HirSupertrait[];
  readonly associatedTypes: readonly HirAssociatedType[];
  readonly methods: readonly HirTraitMethod[];
  readonly span: SourceSpan;
  /** Local implementations visible at this local trait's declaration point. */
  readonly localImplementations?: readonly number[];
}

export interface HirTraitMethodFunction {
  readonly methodIndex: number;
  readonly functionIndex: number;
  /**
   * The implementation strengthened the method's bound, so only a concrete
   * call reaches it; its dictionary entry traps (14 Walkers, Describers, And Sources).
   */
  readonly strengthened?: boolean;
}

export interface HirTraitImplementation {
  /** Originates in the standard library, not the program's exported surface. */
  readonly standard?: true;
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
  /**
   * A numeric-family implementation such as `impl[N < Num] Add for N`
   * (09-traits.md#r-trait.target.numeric-family): the types each parameter
   * may take. Its target is the bare parameter, and it matches a type only
   * through these lists.
   */
  readonly family?: NumericFamily;
  /**
   * Every method is an `@intrinsic` method (09-traits.md#intrinsic-methods):
   * the implementation is a declaration with no code. A direct call is an
   * `intrinsic-call`, and a dictionary is an `intrinsic` builtin.
   */
  readonly intrinsic?: true;
  readonly span: SourceSpan;
  /** Identity of a lexically scoped local implementation. */
  readonly localImplementation?: number;
}

/** The types each parameter of a numeric-family implementation may take. */
export type NumericFamily = Readonly<Record<string, readonly ValueType[]>>;

export interface HirTraitDictionaryPlan {
  readonly bounds: readonly HirExpression[];
  readonly implementationIndex: number;
  readonly supertraits: readonly HirTraitDictionaryPlan[];
  // Set for a compiler-supplied implementation that has no source `impl`
  // (`Any`, `Inspectable`, and forwarding dictionaries).
  // `implementationIndex` is then -1, and `bounds` holds the dictionaries an
  // `Inspectable` key reads, numbered from zero.
  readonly builtin?: HirBuiltinTraitImplementation;
}

export type HirBuiltinTraitImplementation =
  | {
      // A dynamic value of trait `sourceTraitIndex` used where its own trait or
      // a supertrait is bound: each method forwards through the value's own
      // table, reached by the supertrait `path` (09-traits.md#dynamic-trait-values).
      readonly kind: "forward";
      readonly traitIndex: number;
      readonly targetType: ValueType;
      readonly sourceTraitIndex: number;
      readonly path: readonly { readonly traitIndex: number; readonly fieldIndex: number }[];
    }
  | {
      // The compiler-supplied `Any`: a dictionary with no methods.
      readonly kind: "marker";
      readonly traitIndex: number;
      readonly targetType: ValueType;
    }
  | {
      // A dictionary of an intrinsic implementation for one concrete type,
      // such as `Ord` for `i64`: each method's entry is one small wrapper that
      // computes the operation inline (09-traits.md#intrinsic-methods). Each
      // method's parameter and result types are concrete.
      readonly kind: "intrinsic";
      readonly traitIndex: number;
      readonly targetType: ValueType;
      readonly methods: readonly {
        readonly name: string;
        readonly parameterTypes: readonly ValueType[];
        readonly resultType: ValueType;
      }[];
    }
  | {
      // The compiler-supplied `Inspectable` (spec/lang/09-traits.md#sealed-traits).
      // `runtime_type` builds a `TypeId` from the key: literal parts, and
      // `bound` parts naming a bound-pack dictionary whose key is spliced in.
      readonly kind: "inspectable";
      readonly traitIndex: number;
      readonly targetType: ValueType;
      readonly key: readonly (string | { readonly bound: number })[];
      // The dictionary is the bound evidence for a type parameter
      // instantiated with `mut targetType`: the recorded type drops that outer
      // `mut`, but a composite built from the parameter keeps it, so a nested
      // key read (a `bound` part) is prefixed with `mut ` (Inspectable
      // decision 16).
      readonly outerMut?: true;
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
  readonly kind: "data" | "enum" | "erased-variant" | "tuple";
  readonly typeIndex: number;
  readonly fieldIndex: number;
  readonly erasedFieldType?: ValueType;
  readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
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
  /** `Name = type` bindings: `parameter::Name` equals `type`. */
  readonly associatedBindings?: readonly HirAssociatedBinding[];
}

export interface HirAssociatedBinding {
  readonly name: string;
  readonly type: ValueType;
}

export interface HirFunction {
  /** Originates in the standard library, including its generated methods and closures. */
  readonly standard?: true;
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
  /**
   * A non-`pub` `main`: an ordinary function under the specification. The
   * prototype still exports it for implementation tests, which run it only
   * through an explicit `--entry main`.
   */
  readonly developmentEntry?: boolean;
  /**
   * A `lib/std` primitive: the emitter supplies the body, from its runtime
   * or through the generic host-function import (src/README.md, Compiler/library
   * boundary). The checked `body` is a placeholder.
   */
  readonly intrinsic?: string;
  /**
   * An `@intrinsic` trait method (09-traits.md#intrinsic-methods), which has
   * no code: no call reaches it, so it is not emitted.
   */
  readonly intrinsicMethod?: true;
  /** Runner options of a test body (spec/lang/10-modules.md#test-cases). */
  readonly testOptions?: {
    readonly name: string;
    /** An `it_each` table: the runner calls it once per row, as `name[i]`. */
    readonly table?: boolean;
    /** An `it_prop` or `it_prop_with` property: the runner calls it once per case. */
    readonly property?: boolean;
    readonly ignore?: string;
    readonly expectPanic?: string;
  };
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

interface HirProviderSpreadBinding {
  readonly key: string;
  readonly local: HirLocal;
  readonly fieldIndex: number;
}

interface HirMapEntry {
  readonly key: HirExpression;
  readonly value: HirExpression;
}

export interface HirDefaultArgument {
  readonly parameterIndex: number;
  readonly functionIndex: number;
}

export type HirEqualityDispatch =
  | {
      readonly kind: "function";
      readonly functionIndex: number;
      /** The dictionaries of a generic implementation's bounds, passed after the operands. */
      readonly bounds?: readonly HirExpression[];
    }
  | {
      readonly kind: "bound";
      readonly traitIndex: number;
      readonly methodIndex: number;
      readonly boundIndex: number;
      /**
       * Set when the bound's trait reaches `traitIndex` only as a supertrait,
       * as `T < Integer` reaches `PartialOrd`: the bound's own trait and the
       * supertrait fields from its dictionary to the compared one.
       */
      readonly via?: { readonly traitIndex: number; readonly path: readonly number[] };
    };

interface HirBuiltinEqualityStrategy {
  readonly kind: "builtin";
}

interface HirDispatchEqualityStrategy {
  readonly kind: "dispatch";
  readonly dispatch: HirEqualityDispatch;
}

export type HirEqualityStrategy = HirBuiltinEqualityStrategy | HirDispatchEqualityStrategy;

type HirOrderingOperator = "<" | "<=" | ">" | ">=";

interface HirBuiltinOrderingStrategy {
  readonly kind: "builtin";
}

interface HirDispatchOrderingStrategy {
  readonly kind: "dispatch";
  readonly dispatch: HirEqualityDispatch;
}

export type HirOrderingStrategy = HirBuiltinOrderingStrategy | HirDispatchOrderingStrategy;

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

interface HirComprehensionIfClause {
  readonly kind: "if";
  readonly condition: HirExpression;
  readonly span: SourceSpan;
}

export type HirComprehensionClause = HirComprehensionForClause | HirComprehensionIfClause;

export type HirExpression =
  | (HirExpressionBase & {
      readonly kind: "integer";
      readonly value: number;
      /** An `i64` literal's exact decimal value; `value` may round it. */
      readonly wide?: string;
    })
  | (HirExpressionBase & { readonly kind: "float"; readonly value: number })
  | (HirExpressionBase & { readonly kind: "string"; readonly bytes: readonly number[] })
  | (HirExpressionBase & {
      readonly kind: "string-build";
      readonly segments: readonly HirExpression[];
    })
  | (HirExpressionBase & {
      readonly kind: "value-equality";
      readonly left: HirExpression;
      readonly right: HirExpression;
      readonly valueType: ValueType;
      readonly strategy: HirEqualityStrategy;
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
      /** Kind 3 is a type-parameter key, compared through `keyDispatch`'s dictionary. */
      readonly keyKind: 0 | 1 | 2 | 3;
      readonly keyDispatch?: HirEqualityDispatch;
      /** For kind 3 with a concrete key type: the key type's `Eq` dictionary. */
      readonly keyDictionary?: HirExpression;
    })
  | (HirExpressionBase & {
      readonly kind: "map-comprehension";
      readonly clauses: readonly HirComprehensionClause[];
      readonly key: HirExpression;
      readonly value: HirExpression;
      readonly keyType: ValueType;
      readonly valueType: ValueType;
      readonly keyKind: 0 | 1 | 2 | 3;
      readonly keyDispatch?: HirEqualityDispatch;
      /** For kind 3 with a concrete key type: the key type's `Eq` dictionary. */
      readonly keyDictionary?: HirExpression;
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
  // A captured `let` local's shared storage (07-functions.md#captures); the
  // cell has type `cell:T`.
  | (HirExpressionBase & { readonly kind: "cell-new"; readonly value: HirExpression })
  | (HirExpressionBase & { readonly kind: "cell-get"; readonly cell: HirExpression })
  | (HirExpressionBase & {
      readonly kind: "cell-set";
      readonly cell: HirExpression;
      readonly value: HirExpression;
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
      // A call of the `@intrinsic` method `method`, such as `add` or `cmp`, on
      // a concrete primitive receiver, the first argument: the operation
      // inline (09-traits.md#r-trait.impl.intrinsic.inline).
      readonly kind: "intrinsic-call";
      readonly method: string;
      readonly arguments: readonly HirExpression[];
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
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
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
      readonly erasedResultType?: ValueType;
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
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
      readonly bounds?: readonly HirExpression[];
      readonly providers: readonly HirExpression[];
      readonly erasedParameterTypes?: readonly ValueType[];
      readonly erasedResultType?: ValueType;
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
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
      // `TypeId::of::[T]()`: `runtime_type` through the Inspectable dictionary
      // of T, which never reads its receiver.
      readonly kind: "inspect-type-id";
      readonly traitIndex: number;
      readonly dictionary: HirExpression;
    })
  | (HirExpressionBase & {
      // `downcast`, `downcast_mut`, and `downcast_val`: compares the erased
      // value's TypeId with the target dictionary's, then yields the stored
      // payload as `.Some`, or `.None`.
      readonly kind: "inspect-downcast";
      readonly traitIndex: number;
      readonly value: HirExpression;
      readonly dictionary: HirExpression;
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
      /** Reaches `traitIndex` through the supertraits of the bound's own trait. */
      readonly supertrait?: { readonly sourceTraitIndex: number; readonly path: readonly number[] };
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
      readonly bounds?: readonly HirExpression[];
      readonly providers: readonly HirExpression[];
      readonly erasedParameterTypes?: readonly ValueType[];
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
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
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
    })
  | (HirExpressionBase & {
      readonly kind: "enum";
      readonly enumIndex: number;
      readonly tag: number;
      readonly fields: readonly HirExpression[];
      readonly fieldIndices: readonly number[];
      readonly fieldTypes: readonly ValueType[];
      readonly erasedFieldTypes?: readonly ValueType[];
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
    })
  | (HirExpressionBase & {
      readonly kind: "member";
      readonly receiver: HirExpression;
      readonly dataIndex: number;
      readonly fieldIndex: number;
      readonly erasedFieldType?: ValueType;
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
    })
  | (HirExpressionBase & {
      /**
       * A copy of `value` for an embedded field (08 Data Embedding): a new object
       * of data type `dataIndex` whose ordinary fields hold `value`'s field values
       * and whose embedded fields hold copies of `value`'s parts.
       */
      readonly kind: "embedded-copy";
      readonly value: HirExpression;
      readonly dataIndex: number;
    })
  | (HirExpressionBase & {
      readonly kind: "field-set";
      readonly receiver: HirExpression;
      readonly value: HirExpression;
      readonly dataIndex: number;
      readonly fieldIndex: number;
      readonly erasedFieldType?: ValueType;
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
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
      readonly erasedTypeSubstitutions?: readonly HirTypeSubstitution[];
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
      /** The `u8` at a byte offset of any integer type; out of range panics. */
      readonly kind: "string-index";
      readonly receiver: HirExpression;
      readonly index: HirExpression;
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
      /** Reads `V`, panicking `index-out-of-bounds` on a missing key, instead of `V?`. */
      readonly required?: boolean;
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
  | (HirExpressionBase & {
      readonly kind: "panic";
      readonly message: HirExpression;
      /** A panic category other than `explicit-panic`, raised by generated code. */
      readonly category?: "structure-variant-mismatch";
      /**
       * A typed call whose run time the prototype lacks, such as an `all!`
       * call: checking accepts it, and emitting reports this diagnostic.
       */
      readonly unsupported?: { readonly code: string; readonly message: string };
    })
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
