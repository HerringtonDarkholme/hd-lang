import type { MethodDecl } from "../ast.ts";
import type { Diagnostic, SourceSpan } from "../diagnostics.ts";
import type { HirFunction, HirGenericBound, HirProgram, HirTrait, ValueType } from "../hir.ts";

export interface CheckResult {
  readonly program?: HirProgram;
  readonly diagnostics: readonly Diagnostic[];
}

export interface Signature {
  readonly name: string;
  readonly index: number;
  readonly suspending: boolean;
  readonly genericParameters: readonly string[];
  readonly genericBounds: readonly HirGenericBound[];
  readonly referenceParameters?: readonly string[];
  readonly valueParameters?: readonly string[];
  readonly rowParameters: readonly string[];
  /**
   * Every generic parameter in declared order, type and row alike, when a
   * row parameter is among them: the slots of an explicit type-argument list
   * (07-functions.md#r-fn.generic.explicit.row-dollar).
   */
  readonly typeArgumentOrder?: readonly string[];
  readonly parameters: readonly ValueType[];
  /** Parameter types with written `usize` spellings restored; display only. */
  readonly spelledParameters?: readonly ValueType[];
  readonly parameterNames: readonly string[];
  readonly defaultFunctionNames: readonly (string | undefined)[];
  readonly variadic: boolean;
  readonly tupleVararg?: boolean; // a final tuple or `Tuple`-bounded vararg (07 Varargs)
  readonly tupleParameters?: readonly string[]; // bounded by `std.function.Tuple`
  readonly intrinsic?: string; // a `lib/std` declaration's `@intrinsic("name")`
  readonly result: ValueType;
  /** `result` as written where it says `usize`: display only (checker/spelling.ts). */
  readonly spelledResult?: ValueType;
  readonly requirements: readonly string[];
  /** Declared in a `tests:` block (spec/lang/03-names-and-scopes.md#tests-blocks). */
  readonly testOnly?: boolean;
  /** A suffix function, marked `@num_suffix` (spec/lang/05-expressions.md#r-expr.literal-fn.marker). */
  readonly numSuffix?: boolean;
  /** A prefix function, marked `@str_prefix` (spec/lang/05-expressions.md#r-expr.literal-fn.marker). */
  readonly strPrefix?: boolean;
  /** Type-argument defaults, applied to what a use site leaves unsolved (04 Type-Argument Defaults). */
  readonly genericDefaults?: ReadonlyMap<string, ValueType>;
  readonly span: SourceSpan;
}

export interface InherentMethod {
  /** Authoritative AST method corresponding to this prepared callable. */
  readonly sourceMethod: MethodDecl;
  readonly targetType: ValueType;
  /** The implementation's generic parameters, which `targetType` may name. */
  readonly targetGenericParameters?: readonly string[];
  readonly name: string;
  /** `pub fn`: only a public inherent method of a part is promoted (spec 03). */
  readonly public: boolean;
  readonly associated: boolean;
  readonly receiverMutable: boolean;
  readonly parameters: readonly ValueType[];
  readonly parameterNames: readonly string[];
  readonly variadic: boolean;
  readonly suspending: boolean;
  readonly result: ValueType;
  readonly requirements: readonly string[];
  readonly functionName: string;
  readonly span: SourceSpan;
  /** Identity of a lexically scoped local implementation. */
  readonly localImplementation?: number;
}

export interface PlannedArgument {
  readonly parameterIndex: number;
  readonly argumentIndices: readonly number[];
  readonly kind: "single" | "vararg-elements";
}

export interface ResolvedTraitPath {
  readonly arguments: readonly ValueType[];
  readonly trait: HirTrait;
}

export interface FunctionCheckResult {
  readonly function?: HirFunction;
  readonly diagnostics: readonly Diagnostic[];
  /** Whether checking produced a panic node carrying a message. */
  readonly hasPanicDetail: boolean;
  /**
   * Requirements inferred before checking stopped (BF): a failed trial
   * still grew genuine requirements from its checked prefix, so row
   * inference harvests them instead of discarding the round.
   */
  readonly inferredRequirements?: readonly string[];
}
