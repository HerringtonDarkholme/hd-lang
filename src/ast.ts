import type { SourceSpan } from "./diagnostics.ts";
import type { HirFunction } from "./hir.ts";

/**
 * The type name a desugared prefixed string's template literal uses: the
 * hidden name of `std.ops.Template`, replaced by the program's local name
 * when it imports `Template` (spec/lang/05-expressions.md#prefixed-strings).
 */
export const TEMPLATE_PLACEHOLDER = "__std_ops_Template";

export interface TypeRef {
  readonly name: string;
  readonly span: SourceSpan;
  /**
   * Set on a `Self` or `mut Self` that an implementation's target replaced:
   * the target's constructor bounds were checked where the target is
   * written, so they are not reported again here.
   */
  readonly implementationTarget?: true;
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
  /** Parameter decorators: member metadata (14 Member Metadata). */
  readonly metadata?: readonly Expression[];
  readonly span: SourceSpan;
}

/** The decorator lines before a declaration (14 Prefix Decorators). */
export interface Decorators {
  /** Trait names listed by `@derive(...)` lines, in order. */
  readonly derives: readonly TypeRef[];
  /** Every other decorator: the attached value. */
  readonly facts: readonly Expression[];
  readonly span: SourceSpan;
}

/** A member line of a derivation block (14 Member Lines). */
export interface MemberLine {
  readonly name: string;
  readonly nameSpan: SourceSpan;
  readonly operator: "=" | "+=";
  /** Absent for `= pass`, or for a right side that is `pass` after `+=`. */
  readonly value?: Expression;
  readonly pass?: boolean;
  readonly span: SourceSpan;
}

export interface FunctionDecl {
  readonly kind: "function";
  readonly public?: boolean;
  readonly name: string;
  readonly suspending: boolean;
  readonly genericParameters: readonly string[];
  /** The parameters declared `$R`, which are row parameters (11-requirements-and-suspension.md#r-req.row.param.marked). */
  readonly rowParameters?: readonly string[];
  /** Type-argument defaults (04-type-system.md#type-argument-defaults). */
  readonly genericDefaults?: Readonly<Record<string, TypeRef>>;
  readonly genericBounds: readonly GenericBound[];
  readonly parameters: readonly Parameter[];
  readonly result: TypeRef;
  readonly requirements: readonly string[];
  /** The row as written, when it named a row alias (11-requirements-and-suspension.md#r-req.row.alias.diagnostics). */
  readonly writtenRequirements?: readonly string[];
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
  /** Declared in a `tests:` block, or a test body: test code (spec/lang/10-modules.md#test-modules). */
  readonly testOnly?: boolean;
  /** Runner options of a test body (spec/lang/10-modules.md#test-cases). */
  readonly testOptions?: HirFunction["testOptions"];
  /** Decorator lines before the declaration (14 Prefix Decorators). */
  readonly decorators?: Decorators;
  /**
   * A `lib/std` primitive whose body the compiler supplies: the standard-library
   * loader sets it from a std-only `@intrinsic("name")` line (src/README.md,
   * Compiler/library boundary). User code cannot set it.
   */
  readonly intrinsic?: string;
  /**
   * A bodiless `@intrinsic` implementation method outside `lib/std`, which
   * the loader gave no body: `@intrinsic` there is already `unknown-name`
   * (09-traits.md#r-trait.impl.intrinsic.std-only), so its empty body is
   * not checked for a missing result.
   */
  readonly bodiless?: boolean;
  /** Declared by the standard library (`lib/std/`), which declares prelude names such as `println`. */
  readonly standard?: boolean;
  /** The qualified name of a `lib/std` function, independent of its local binding. */
  readonly standardName?: string;
  /** Compiler-generated helper: not a host-callable root. */
  readonly compilerGenerated?: boolean;
  /**
   * A printer that the checker generated for `dbg` (checker/debug-print.ts):
   * it reads private members, as `dbg` shows them
   * (spec/lang/10-modules.md#r-module.dbg.value.data).
   */
  readonly privateAccess?: boolean;
  /** Carries a `std.ops.NumSuffix` value: a suffix function (05-expressions.md#r-expr.literal-fn.marker). */
  readonly numSuffix?: boolean;
  /** Carries a `std.ops.StrPrefix` value: a prefix function (05-expressions.md#r-expr.literal-fn.marker). */
  readonly strPrefix?: boolean;
  /** Local implementations lexically available in this synthetic function body. */
  readonly localImplementations?: readonly number[];
}

export interface MethodDecl {
  /** `pub fn` in an inherent implementation. */
  readonly public?: boolean;
  readonly name: string;
  readonly suspending: boolean;
  readonly genericParameters: readonly string[];
  /** The parameters declared `$R`, which are row parameters (11-requirements-and-suspension.md#r-req.row.param.marked). */
  readonly rowParameters?: readonly string[];
  /** Type-argument defaults (04-type-system.md#type-argument-defaults). */
  readonly genericDefaults?: Readonly<Record<string, TypeRef>>;
  readonly genericBounds: readonly GenericBound[];
  readonly parameters: readonly Parameter[];
  readonly result: TypeRef;
  readonly requirements: readonly string[];
  readonly resultOmitted?: boolean;
  readonly requirementsOmitted?: boolean;
  readonly body?: readonly Statement[];
  readonly doc?: string;
  /** Decorator lines before the method (14 Prefix Decorators). */
  readonly decorators?: Decorators;
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
  readonly genericBounds?: readonly GenericBound[];
  readonly genericDefaults?: Readonly<Record<string, TypeRef>>;
  readonly supertraits: readonly TypeRef[];
  /** Associated type bindings in the supertrait list, as in `trait C < Add[Self, Out = Self]`. */
  readonly supertraitBindings?: readonly AssociatedTypeBinding[];
  readonly associatedTypes: readonly AssociatedTypeDecl[];
  readonly methods: readonly MethodDecl[];
  readonly doc?: string;
  /**
   * Methods whose method-level bound an implementation may strengthen: `member`
   * of the standard `Walker`, `Describer`, and `Source` (14 Typed Derivation).
   */
  readonly strengthenableMembers?: readonly string[];
  /** The qualified name of a std trait, such as `std.ops.Add`, whatever its local name. */
  readonly standardName?: string;
  /** Declared by the standard library (`lib/std/`), which declares prelude names such as `Display`. */
  readonly standard?: boolean;
  /** Decorator lines before the trait (14 Prefix Decorators). */
  readonly decorators?: Decorators;
  readonly span: SourceSpan;
  /** Local implementations visible where this local declaration was written. */
  readonly localImplementations?: readonly number[];
}

export interface ImplDecl {
  readonly kind: "impl";
  readonly genericParameters: readonly string[];
  /** The parameters declared `$R`, which are row parameters (11-requirements-and-suspension.md#r-req.row.param.marked). */
  readonly rowParameters?: readonly string[];
  readonly genericBounds: readonly GenericBound[];
  readonly traitName?: string;
  readonly targetName: string;
  /** `impl Trait for C by E`: the embedded field `E` that the trait is delegated to. */
  readonly delegate?: { readonly name: string; readonly span: SourceSpan };
  /** `impl Trait for X by Structure`: a derivation template or block (14 Typed Derivation). */
  readonly byStructure?: SourceSpan;
  readonly memberLines?: readonly MemberLine[];
  readonly associatedTypes: readonly AssociatedTypeDecl[];
  readonly methods: readonly MethodDecl[];
  /**
   * Declared by the standard library (`lib/std/`), which may give a built-in
   * type inherent methods (09-traits.md#r-trait.own.inherent.std).
   */
  readonly standard?: boolean;
  readonly doc?: string;
  /** Decorator lines before the implementation (14 Prefix Decorators). */
  readonly decorators?: Decorators;
  readonly span: SourceSpan;
  /** Identity of a local implementation; absent on module-level implementations. */
  readonly localImplementation?: number;
  /** Local implementations visible inside this implementation's method bodies. */
  readonly localImplementations?: readonly number[];
}

export interface DataField {
  /** `pub name: T`; an embedded field takes no marker and is always public. */
  readonly public?: boolean;
  readonly name: string;
  readonly type: TypeRef;
  readonly embedded?: boolean;
  readonly default?: Expression;
  readonly doc?: string;
  /** Member decorators: the member's declaration facts (14 Member Metadata). */
  readonly metadata?: readonly Expression[];
  /** A payload parameter written without a name, such as `Raw(string)`. */
  readonly positional?: boolean;
  readonly span: SourceSpan;
}

/** A declared variance marker: `+T`, `-T`, or none (04-type-system.md#variance). */
export type VarianceMarker = "+" | "-" | undefined;

export interface DataDecl {
  readonly kind: "data";
  readonly public?: boolean;
  readonly name: string;
  readonly genericParameters: readonly string[];
  /** Bounds on the parameters, which the prototype checks only against a default. */
  readonly genericBounds?: readonly GenericBound[];
  readonly genericDefaults?: Readonly<Record<string, TypeRef>>;
  /** Present when a parameter is written `+T` or `-T`. */
  readonly variances?: readonly VarianceMarker[];
  readonly fields: readonly DataField[];
  readonly doc?: string;
  /** Lowered from `type Name(Base)`: its one field is the base value. */
  readonly newtype?: boolean;
  /** Declared in a block suite, so not inspectable (09-traits.md#inspectable-types). */
  readonly local?: boolean;
  readonly decorators?: Decorators;
  /**
   * The pattern of a typed fact type, the type argument of its
   * `@annotate::[Q](...)` (14-annotations.md#r-annot.typed-fact.pattern).
   * The typed-facts pass moves it here from the decorator.
   */
  readonly factPattern?: TypeRef;
  /** Declared by the standard library (`lib/std/`), which declares prelude names such as `Display`. */
  readonly standard?: boolean;
  /**
   * The qualified name of a `lib/std` declaration, such as
   * `std.annotation.Annotate`, which the compiler recognizes by name
   * (14 Target Kinds). The loader sets it; user code cannot.
   */
  readonly standardName?: string;
  readonly span: SourceSpan;
  /** Local implementations visible where this local declaration was written. */
  readonly localImplementations?: readonly number[];
}

/** `type Name = T` (a transparent alias) or `type Name(T)` (a newtype). */
export interface TypeDecl {
  readonly kind: "type";
  readonly public?: boolean;
  readonly name: string;
  readonly genericParameters: readonly string[];
  /** The parameters declared `$R`, which are row parameters (11-requirements-and-suspension.md#r-req.row.param.marked). */
  readonly rowParameters?: readonly string[];
  readonly genericBounds?: readonly GenericBound[];
  readonly genericDefaults?: Readonly<Record<string, TypeRef>>;
  readonly alias?: TypeRef;
  /**
   * The keys of a row alias, `type AppRow = $ Db + Cache` or `type NoRow = $()`
   * (11-requirements-and-suspension.md#row-aliases).
   */
  readonly row?: readonly string[];
  /**
   * The right side of a row alias written without `$`, as in
   * `type AppRow = Db + Cache` (11-requirements-and-suspension.md#r-req.row.alias.dollar.missing).
   */
  readonly bareRow?: SourceSpan;
  readonly base?: TypeRef;
  readonly doc?: string;
  readonly decorators?: Decorators;
  /** Declared by the standard library. */
  readonly standard?: boolean;
  /** The qualified name of a `lib/std` type, independent of its local binding. */
  readonly standardName?: string;
  readonly span: SourceSpan;
}

export interface EnumVariant {
  readonly name: string;
  /** Variant-local generic parameters, as `T` in `If[T](...) -> Expr[T]` (13-gadts.md). */
  readonly genericParameters?: readonly string[];
  readonly genericBounds?: readonly GenericBound[];
  readonly fields: readonly DataField[];
  /** The explicit result type after `->`, as `Expr[i64]` or `StatusCode` (13-gadts.md#variant-result-types). */
  readonly resultType?: TypeRef;
  /** The result's argument clause, as `StatusCode(404)`: a call that initializes shared enum data. */
  readonly result?: Extract<Expression, { kind: "call" }>;
  readonly doc?: string;
  readonly metadata?: readonly Expression[];
  readonly span: SourceSpan;
}

export interface EnumDecl {
  readonly kind: "enum";
  readonly public?: boolean;
  readonly name: string;
  readonly genericParameters: readonly string[];
  readonly genericBounds?: readonly GenericBound[];
  readonly genericDefaults?: Readonly<Record<string, TypeRef>>;
  /** Present when a parameter is written `+T` or `-T`. */
  readonly variances?: readonly VarianceMarker[];
  readonly sharedFields: readonly DataField[];
  readonly variants: readonly EnumVariant[];
  readonly doc?: string;
  /** Declared in a block suite, so not inspectable (09-traits.md#inspectable-types). */
  readonly local?: boolean;
  readonly decorators?: Decorators;
  /** The pattern of a typed fact type, as on a data declaration. */
  readonly factPattern?: TypeRef;
  /** Declared by the standard library (`lib/std/`), which declares prelude names such as `Display`. */
  readonly standard?: boolean;
  /** The qualified name of a `lib/std` enum, independent of its local binding. */
  readonly standardName?: string;
  readonly span: SourceSpan;
  /** Local implementations visible where this local declaration was written. */
  readonly localImplementations?: readonly number[];
}

// One `it("name", ...)` call of a `tests:` block (spec/lang/10-modules.md#test-cases).
export interface TestDecl {
  readonly kind: "test";
  readonly name: string;
  readonly body: readonly Statement[];
  /** The body was an explicit closure rather than a trailing block. */
  readonly explicit?: boolean;
  /** A trailing body that uses `?` (spec/lang/05-expressions.md#r-expr.try.test.with-try). */
  readonly propagates?: boolean;
  /** An `it_each` table, whose rows the runner runs as separate test cases. */
  readonly table?: boolean;
  /** An `it_prop` or `it_prop_with` property, which the runner runs once per case. */
  readonly property?: boolean;
  /** A `timeout` option, which the test function reports to the runner first. */
  readonly timed?: boolean;
  /** The written result of an explicit closure, or `Result[void, Error]` for `propagates`. */
  readonly result?: TypeRef;
  readonly ignore?: string;
  readonly expectPanic?: string;
  readonly span: SourceSpan;
}

export interface UseDecl {
  readonly kind: "use";
  readonly module: string;
  readonly names: readonly UseName[];
  readonly public?: boolean;
  /** Added by the std loader for a lib/std dependency, not written by the user. */
  readonly standard?: boolean;
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
  | {
      readonly kind: "wildcard";
      /** The unit pattern `()`, which matches only the unit value (06-control-flow.md#r-flow.match.unit). */
      readonly unit?: boolean;
      readonly span: SourceSpan;
    }
  | { readonly kind: "boolean"; readonly value: boolean; readonly span: SourceSpan }
  | { readonly kind: "integer"; readonly value: bigint; readonly span: SourceSpan }
  | { readonly kind: "float"; readonly value: number; readonly span: SourceSpan }
  | { readonly kind: "string"; readonly value: string; readonly span: SourceSpan }
  | { readonly kind: "character"; readonly value: string; readonly span: SourceSpan }
  | {
      /**
       * A range pattern `a..b`, `a..=b`, `a..`, or `..=b` with integer-literal
       * bounds (02-grammar.md#r-grammar.pattern.range).
       */
      readonly kind: "range";
      readonly start?: bigint;
      readonly end?: bigint;
      readonly inclusive: boolean;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "binding";
      readonly name: string;
      /** Written `mut name` in a `let` pattern (02-grammar.md#r-grammar.stmt.let-pattern.mut). */
      readonly mutableAccess?: boolean;
      /** From `mut` to the name, which the `redundant-let-mut` fix-it deletes. */
      readonly mutSpan?: SourceSpan;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "tuple";
      readonly elements: readonly Pattern[];
      /** The last element is a spread pattern `xs...` or `_...` (06-control-flow.md#spread-patterns). */
      readonly spread?: boolean;
      readonly span: SourceSpan;
    }
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
  /** The else block of a lowered let-else, which must diverge (06-control-flow.md#r-flow.let.else.falls-through). */
  readonly letElse?: boolean;
  readonly span: SourceSpan;
}

export interface BindingName {
  readonly name: string;
  /** Written `mut name` in a multi-name `let` (04-type-system.md#r-types.bind.let-pattern-mut). */
  readonly mutableAccess?: boolean;
  /** From `mut` to the name, which the `redundant-let-mut` fix-it deletes. */
  readonly mutSpan?: SourceSpan;
  /** Written `name...` last in a `let` list: a spread pattern (06-control-flow.md#spread-patterns). */
  readonly spread?: boolean;
  readonly span: SourceSpan;
}

export interface MapEntry {
  readonly key: Expression;
  readonly value: Expression;
  readonly span: SourceSpan;
}

interface ComprehensionForClause {
  readonly kind: "for";
  readonly bindings: readonly BindingName[];
  /** A pattern other than a name or a tuple of names; then `bindings` is empty (grammar.flow.for-pattern). */
  readonly pattern?: Pattern;
  readonly iterable: Expression;
  readonly span: SourceSpan;
}

interface ComprehensionIfClause {
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

/**
 * One initialization group's statement block in a joined program. The
 * package linker reports each group's block as data, so the checker can
 * tell a group of one module (its statements run in source order,
 * spec/lang/10-modules.md#r-module.init.source-order-single) from a larger
 * group (dependency order, spec/lang/10-modules.md#order-inside-a-group).
 */
export interface InitGroup {
  /** Index into `Program.statements` where the group's block starts. */
  readonly start: number;
  /** Whether the group holds several modules and may run in dependency order. */
  readonly multi: boolean;
}

/**
 * One initialization group's start in a joined program, as the package
 * linker reports it: a line number in the joined source. The parser maps
 * these to the statement indices of {@link InitGroup}, but only under
 * `joinedModules`.
 */
export interface InitGroupStart {
  /**
   * The joined-source line before the group's block starts (0 when the block
   * is the first line): the parser opens the group at the first statement
   * after this line. Never a line of the source itself, so no user comment
   * can collide with it.
   */
  readonly line: number;
  /** Whether the group holds several modules and may run in dependency order. */
  readonly multi: boolean;
}

/**
 * The name scope of one package module in a joined program (src/package.ts):
 * how the module's own spellings read in the joined program, which shares
 * one namespace. It travels beside the joined source, as the init-group
 * starts do, and the checker's module-path pass applies it
 * (checker/module-paths.ts).
 */
export interface ModuleScope {
  /** The module's first and last lines in the joined source. */
  readonly firstLine: number;
  readonly lastLine: number;
  /**
   * Local spellings that name a declaration under another joined spelling:
   * the module's own declaration that the linker gave a hidden spelling, or
   * a package use, renamed with `as` or of such a declaration.
   */
  readonly names: Readonly<Record<string, string>>;
  /** Module namespace uses, such as `use pkg.words`: local name to module identity. */
  readonly namespaces: Readonly<Record<string, string>>;
  /** The dependency package the module belongs to, by its id; absent in the root package. */
  readonly package?: string;
  /**
   * The fetched package the module belongs to, as messages name it: a
   * dependency fetched for a version requirement, whose `dbg` calls print
   * nothing (spec/lang/10-modules.md#r-module.dbg.dependency).
   */
  readonly fetched?: string;
  /** The joined spelling of each package declaration a use of the module imports. */
  readonly imports?: readonly string[];
  /**
   * The names that other linked modules declare at top level or bind
   * through a std use, and that this module neither declares nor imports,
   * which it must not name
   * (spec/lang/03-names-and-scopes.md#r-names.module.other-module,
   * spec/lang/03-names-and-scopes.md#r-names.module.use-own-module).
   */
  readonly foreign?: Readonly<Record<string, ForeignName>>;
}

/** Another module's top-level name, with what a diagnostic says about it. */
export interface ForeignName {
  /** Set when the name is a trait, whose unknown use in a type is `unknown-trait`. */
  readonly trait?: true;
  /**
   * The std declaration that another module's std use binds the name to, as
   * `{ module: "text", name: "join" }`; the checker asks std whether it is a
   * trait.
   */
  readonly standard?: { readonly module: string; readonly name: string };
  /** Where the name is declared and how to reach it, as `module 'cart' declares it; ...`. */
  readonly hint: string;
}

/** A package module that a namespace use names, with what a module path may select. */
export interface NamespaceModule {
  /** The module's name in a message: its identity, or `pkg` for the root module. */
  readonly shown: string;
  /** Each member's joined spelling, or null for a declaration without `pub`. */
  readonly members: Readonly<Record<string, string | null>>;
  /**
   * Each direct child module's name with the `use` path that imports it, as
   * `cart` with `pkg.shop.cart`: a path cannot reach it through its parent
   * (10-modules.md#r-module.path.no-child-import).
   */
  readonly children: Readonly<Record<string, string>>;
}

/** The module scopes of a joined package program. */
export interface PackageScopes {
  readonly scopes: readonly ModuleScope[];
  /** The modules that namespace uses name, by identity. */
  readonly modules: Readonly<Record<string, NamespaceModule>>;
  /**
   * The printed `TypeId` name of each package data type, enum, and trait, by
   * its joined spelling: its package name, module path, and name, as
   * `acme_shop.model.User` (spec/lang/09-traits.md#r-trait.typeid.name.package).
   */
  readonly typeIdNames?: Readonly<Record<string, string>>;
}

export interface Program {
  readonly uses: readonly UseDecl[];
  /**
   * The file's module documentation, its first `##` block when a blank line
   * follows it (01-lexical-structure.md#r-lex.doc.module); for tools.
   */
  readonly moduleDoc?: { readonly text: string; readonly span: SourceSpan };
  /** Present when the module declares a `type`. */
  readonly types?: readonly TypeDecl[];
  /** Set when a block suite declares a type or implementation. */
  readonly localDeclarations?: boolean;
  /**
   * Types written with `mut` on a primitive, such as `mut i32`, which the
   * checker rejects (04-type-system.md#r-types.prim.no-mut.error).
   */
  readonly mutPrimitives?: readonly TypeRef[];
  /**
   * Tuple types written with `mut`, such as `mut (User, i32)`, which the
   * checker rejects (04-type-system.md#r-types.tuple.no-mut).
   */
  readonly mutTuples?: readonly TypeRef[];
  readonly data: readonly DataDecl[];
  readonly enums: readonly EnumDecl[];
  readonly traits: readonly TraitDecl[];
  readonly implementations: readonly ImplDecl[];
  readonly functions: readonly FunctionDecl[];
  readonly tests: readonly TestDecl[];
  readonly statements: readonly Statement[];
  /**
   * Set when the source joins several package modules
   * (spec/lang/10-modules.md#initialization-order). A program without it is
   * one file: with top-level statements and no `main` it is a script
   * (spec/lang/10-modules.md#r-module.init.script).
   */
  readonly joinedModules?: true;
  /**
   * Under `joinedModules`, the linked entry module is a script, so the joined
   * top level infers its entry requirement row
   * (spec/lang/10-modules.md#r-module.init.script-row).
   */
  readonly scriptEntry?: true;
  /**
   * Initialization-group boundaries over `statements`, in linker order: the
   * parser maps the linker's group starts to statement indices, but only
   * under `joinedModules`. Absent in a single file (one single-module group)
   * and in joined sources without group data.
   */
  readonly initGroups?: readonly InitGroup[];
  /** The linker's module scopes, under `joinedModules` (checker/module-paths.ts). */
  readonly packageScopes?: PackageScopes;
  /** Names that the `tests:` block declares or uses (spec/lang/03-names-and-scopes.md#tests-blocks). */
  readonly testOnlyNames?: readonly string[];
  /**
   * The program holds test code: a `tests:` block or a test module's top
   * level. Only test code has the testing prelude names
   * (spec/lang/10-modules.md#r-module.prelude.test-only).
   */
  readonly testCode?: boolean;
  readonly span: SourceSpan;
}

export type Statement =
  | { readonly kind: "defer"; readonly body: readonly Statement[]; readonly span: SourceSpan }
  | {
      readonly kind: "binding";
      readonly name: string;
      readonly annotation?: TypeRef;
      readonly mutable: boolean;
      /** Written `let mut name` (04-type-system.md#r-types.bind.let-mut-infer). */
      readonly mutableAccess?: boolean;
      /** From `mut` to the name, which the `redundant-let-mut` fix-it deletes. */
      readonly mutSpan?: SourceSpan;
      readonly value: Expression;
      // A local `fn` declaration that omits its result type.
      readonly localFunction?: boolean;
      /** Compiler-generated initialization owned by std, retained only when the global is used. */
      readonly standard?: boolean;
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
      /**
       * A `let` whose pattern is neither a name nor a tuple of names, or any
       * `let` with an else block (02-grammar.md#let-statements,
       * 02-grammar.md#let-else-statements).
       */
      readonly kind: "pattern-binding";
      readonly pattern: Pattern;
      readonly annotation?: TypeRef;
      readonly value: Expression;
      readonly elseBody?: readonly Statement[];
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "assignment";
      readonly name: string;
      readonly value: Expression;
      /** Written with the copy assignment `...=` (VE-S). */
      readonly copy?: boolean;
      /** The operator of a compound assignment `name op= value`, such as `+`. */
      readonly compound?: string;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "field-assignment";
      readonly target: Extract<Expression, { kind: "member" }>;
      readonly value: Expression;
      readonly copy?: boolean;
      readonly compound?: string;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "index-assignment";
      readonly target: Extract<Expression, { kind: "index" }>;
      readonly value: Expression;
      readonly copy?: boolean;
      readonly compound?: string;
      readonly span: SourceSpan;
    }
  | {
      /** `v() = x`, a store through a callable value (05-expressions.md#callable-values). */
      readonly kind: "call-assignment";
      readonly target: Extract<Expression, { kind: "call" }>;
      readonly value: Expression;
      readonly copy?: boolean;
      readonly compound?: string;
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
    }
  | {
      /** Internal compile-time marker for a hoisted local implementation's lexical start. */
      readonly kind: "local-implementation";
      readonly implementation: number;
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
  | {
      readonly kind: "tuple";
      readonly elements: readonly Expression[];
      /** The last element is a spread `xs...` that supplies a rest element (05-expressions.md#tuple-rest-elements). */
      readonly spread?: boolean;
      readonly span: SourceSpan;
    }
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
      /** Type-binder candidate; value-owner lookup still uses the source spelling. */
      readonly genericTypeOwner?: string;
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
      // Set when a suffixed literal such as `250ms` desugared to this call
      // `ms(250)` of its suffix function (05-expressions.md#literal-suffixes).
      readonly literalSuffix?: string;
      // Set when a prefixed string such as `sql"a $x"` desugared to this call
      // `sql(Template { ... })` of its prefix function
      // (05-expressions.md#prefixed-strings).
      readonly stringPrefix?: string;
      // Set on the call that checks a typed fact against its target's type
      // (14-annotations.md#r-annot.typed-fact.check): the generic parameter
      // list, such as `[HdFactT < Integer]`, that its target's type needs.
      readonly typedFactScope?: string;
      // Set on the same call: the target's monomorphic type and the fact
      // type, whose pattern the checker matches against that type to give
      // the call its type arguments (r-annot.typed-fact.infer).
      readonly typedFact?: { readonly target: string; readonly factType: string };
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
      /**
       * The read of `m[k] op= v`: on a `Map` it has type `V` and panics when
       * the key is missing (05-expressions.md#r-expr.index.map.read-value).
       */
      readonly required?: boolean;
      readonly span: SourceSpan;
    }
  | { readonly kind: "propagate"; readonly operand: Expression; readonly span: SourceSpan }
  | {
      /**
       * A range expression: `a..b`, `a..`, `..b`, `a..=b`, `..=b`, or `..`
       * (05-expressions.md#range-expressions).
       */
      readonly kind: "range";
      readonly start?: Expression;
      readonly end?: Expression;
      readonly inclusive: boolean;
      readonly span: SourceSpan;
    }
  | {
      /**
       * `value |> step` (05-expressions.md#pipe-expressions). A bare step is a
       * name, path, or method reference called with the value; any other step
       * holds exactly one `_`, the value's slot.
       */
      readonly kind: "pipe";
      readonly value: Expression;
      readonly step: Expression;
      readonly bare: boolean;
      readonly span: SourceSpan;
    }
  | {
      readonly kind: "closure";
      readonly suspending?: boolean;
      /** A trailing callback block (spec/lang/07-functions.md#trailing-callback-blocks). */
      readonly trailing?: boolean;
      readonly parameters: readonly ClosureParameter[];
      readonly result?: TypeRef;
      readonly requirements?: readonly string[];
      /** A compiler-generated wrapper around a written test body. */
      readonly testBody?: true;
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
      /** A pattern other than a name or a tuple of names; then `bindings` is empty (grammar.flow.for-pattern). */
      readonly pattern?: Pattern;
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

/**
 * A final `List[T]` vararg, which is its function's rest element
 * (07-functions.md#r-fn.type.vararg-rest). A tuple-typed or `Tuple`-bounded
 * vararg is one ordinary input (07-functions.md#r-fn.type.tuple-vararg-input).
 */
export function listVararg(parameter: Parameter | undefined): boolean {
  return parameter?.variadic === true && /^List\[.*\]$/.test(parameter.type.name);
}

/** A final tuple-typed or `Tuple`-bounded vararg (07-functions.md#varargs). */
export function tupleVararg(parameter: Parameter | undefined): boolean {
  return parameter?.variadic === true && !listVararg(parameter);
}

/** A statement that stores into a place, plainly or as `place op= value`. */
export type AssignmentStatement = Extract<
  Statement,
  { kind: "assignment" | "field-assignment" | "index-assignment" | "call-assignment" }
>;
