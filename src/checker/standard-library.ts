import {
  TEMPLATE_PLACEHOLDER,
  type DataDecl,
  type EnumDecl,
  type FunctionDecl,
  type ImplDecl,
  type MethodDecl,
  type Program,
  type UseDecl,
} from "../ast.ts";
import { DiagnosticError, type SourceSpan } from "../diagnostics.ts";
import { parse } from "../parser/index.ts";
import {
  STANDARD_MODULES,
  standardDocument,
  standardSource,
  type StandardModule,
} from "./standard-sources.ts";
import { renameStandardBindings } from "./standard-bindings.ts";
import { withStandardSource } from "./standard-provenance.ts";
import { STANDARD_CORE_TYPE_ALIASES } from "./standard-core.ts";

// Joins the toy standard library (`lib/std/*.hd`) into the one module the
// prototype compiles. The use graph decides which modules join: the prelude
// (`lib/std/prelude.hd`), which every module implicitly uses
// (spec/lang/10-modules.md#r-module.prelude.fixed-uses), and every std module
// that a `use` reaches from the program, the prelude, or a joined module.
//
// - A joined module's top-level declarations and implementations are all
//   declared, apart from its templates, which a derivation instantiates.
// - A declaration is declared under the program's local name when the
//   program imports it, under its own name when it is a prelude name, and
//   otherwise under the hidden name `__std_<module>_<Name>`.
//
// Every added declaration keeps its physical `lib/std` location. Its logical
// position points at the `use` that brought the module in, or at the program
// when the prelude did, so source order remains a property of the joined unit.

/** `std.prelude`, the module of `use` lines that every module implicitly has. */
const PRELUDE: StandardModule = "prelude";
/**
 * `std.prelude.testing`, the `use` lines that only test code has
 * (spec/lang/10-modules.md#r-module.prelude.test-only).
 */
const TEST_PRELUDE: StandardModule = "prelude.testing";

/** The prelude modules a program implicitly uses: the test part only with test code. */
function preludeModules(program: Program): readonly StandardModule[] {
  return program.testCode === true ? [PRELUDE, TEST_PRELUDE] : [PRELUDE];
}

/** The `pub use` names of the program's prelude modules. */
function preludeExports(program: Program): ParsedModule["exports"] {
  return preludeModules(program).flatMap((module) => standardModule(module).exports);
}

interface ParsedModule {
  readonly name: StandardModule;
  readonly program: Program;
  /** Top-level declaration names, which the loader renames. */
  readonly names: readonly string[];
  /** The std modules that the module's `use` lines reach: its edges in the use graph. */
  readonly modules: readonly StandardModule[];
  /** `use std.<module>.<Name>` lines of the module that name a `lib/std` declaration. */
  readonly uses: readonly { readonly module: StandardModule; readonly name: string }[];
  /** The `pub use` ones among `uses`, as the prelude's names. */
  readonly exports: readonly { readonly module: StandardModule; readonly name: string }[];
  /**
   * `use` lines of a std name that the compiler provides rather than
   * `lib/std`, such as `use std.task.block_on`. The joined program imports
   * each under its hidden name, so the name keeps its ordinary meaning. A
   * `pub use` of one, as the prelude's `std.core` names, binds what the
   * compiler already provides, so it adds no import.
   */
  readonly compilerUses: readonly {
    readonly module: string;
    readonly name: string;
    readonly span: SourceSpan;
  }[];
}

const parsedModules = new Map<StandardModule, ParsedModule>();
/** Renamed parsed modules; the key records the declaration binding map. */
const renamedModules = new Map<string, Program>();
/** A std module's implementations, by binding-map key, for its templates. */
const templateModules = new Map<string, readonly ImplDecl[]>();

function isStandardModule(name: string): name is StandardModule {
  return (STANDARD_MODULES as readonly string[]).includes(name);
}

/**
 * The hidden name of a `std` declaration that the program did not import,
 * as `__std_testing_arbitrary_with` for `std.testing.arbitrary.with`.
 */
function hiddenStandardName(module: string, name: string): string {
  return `__std_${module.replaceAll(".", "_")}_${name}`;
}

/**
 * The declaration identity of a public function selected through an imported
 * std submodule, or undefined when `origin` is not a submodule or `member` is
 * not one of its public functions. `origin` is the qualified binding recorded
 * by program validation, as `std.testing.arbitrary`.
 *
 * The joined declaration's source spelling is deliberately not reconstructed
 * here: it may be a prelude name, a direct import, an alias, or a hidden name.
 * Declaration registries resolve this identity to whichever spelling the
 * loader actually chose.
 */
export function standardSubmoduleFunctionIdentity(
  origin: string | undefined,
  member: string,
): string | undefined {
  if (!origin?.startsWith("std.")) return undefined;
  const module = origin.slice("std.".length);
  if (!isStandardModule(module)) return undefined;
  const declaration = standardModule(module).program.functions.find(
    (candidate) => candidate.name === member && candidate.public === true,
  );
  return declaration ? `std.${module}.${member}` : undefined;
}

/** What a checked `assert_equal` or `snapshot` call runs (lib/std/testing.hd). */
export const CHECK_EQUAL = hiddenStandardName("testing", "check_equal");

/**
 * Compiler-provided names that the prototype declares only under their
 * standard names, since it supports no alias for them
 * (checker/standard-traits.ts): a std module's use of one keeps the name.
 */
const UNALIASED_COMPILER_NAMES = new Set(["inspect.Inspectable", "inspect.TypeId"]);

/** The name a std module's use of a compiler-provided name has in the joined program. */
function compilerUseName(module: string, name: string): string {
  return UNALIASED_COMPILER_NAMES.has(`${module}.${name}`)
    ? name
    : hiddenStandardName(module, name);
}

/** A `use` of a compiler-provided name, under the name `compilerUseName` gives it. */
function compilerUse(
  used: { readonly module: string; readonly name: string },
  span: SourceSpan,
): UseDecl {
  const alias = compilerUseName(used.module, used.name);
  return {
    kind: "use",
    module: `std.${used.module}`,
    names: [alias === used.name ? { name: used.name } : { name: used.name, alias }],
    standard: true,
    span,
  };
}

/** Whether the program already imports `name` from `module` under `local`. */
function imports(program: Program, module: string, name: string, local: string): boolean {
  return program.uses.some(
    (declaration) =>
      declaration.module === module &&
      declaration.names.some(
        (imported) => imported.name === name && (imported.alias ?? imported.name) === local,
      ),
  );
}

/**
 * The std modules that a `use` reaches: the module its path names, as
 * `std.cmp` for `use std.cmp.{max}`, or each module it names itself, as
 * `std.text` for `use std.text` and `std.testing.arbitrary` for
 * `use std.testing.arbitrary`.
 */
function usedModules(use: UseDecl): StandardModule[] {
  if (use.module !== "std" && !use.module.startsWith("std.")) return [];
  const path = use.module === "std" ? "" : use.module.slice("std.".length);
  const modules = new Set<StandardModule>();
  for (const imported of use.names) {
    const named = path === "" ? imported.name : `${path}.${imported.name}`;
    if (isStandardModule(named)) modules.add(named);
    else if (isStandardModule(path)) modules.add(path);
  }
  return [...modules];
}

/**
 * The name in a std function's `@intrinsic("name")` line. The line is
 * prototype-internal and std-only: the loader turns it into
 * `FunctionDecl.intrinsic`, so the checker never sees it, while the same line
 * in user code stays a rejected function decorator (src/README.md,
 * Compiler/library boundary).
 */
function intrinsicName(declaration: FunctionDecl): string | undefined {
  const decorators = declaration.decorators;
  const fact = decorators?.facts[0];
  if (!decorators || decorators.derives.length > 0 || decorators.facts.length !== 1) return;
  if (fact?.kind !== "call" || fact.callee.kind !== "name" || fact.callee.name !== "intrinsic")
    return;
  const [argument] = fact.arguments;
  return fact.arguments.length === 1 && argument?.kind === "string" ? argument.value : undefined;
}

/** Removes the std-only bare `@intrinsic` marker after the parser used it. */
function withoutMethodIntrinsic(declaration: MethodDecl): MethodDecl {
  const decorators = declaration.decorators;
  // Only a bodiless std method is an operation intrinsic. Keep the marker on
  // a written body so ordinary decorator checking rejects that invalid form.
  if (!decorators || declaration.body !== undefined) return declaration;
  const facts = decorators.facts.filter(
    (fact) => fact.kind !== "name" || fact.name !== "intrinsic",
  );
  if (facts.length === decorators.facts.length) return declaration;
  if (facts.length === 0 && decorators.derives.length === 0) {
    const { decorators: _decorators, ...method } = declaration;
    return method;
  }
  return { ...declaration, decorators: { ...decorators, facts } };
}

function parseModule(name: StandardModule, source: string): Program {
  const parsed = parse(source, { standardLibrary: true });
  if (!parsed.program || parsed.diagnostics.some((d) => d.severity !== "warning")) {
    const document = standardDocument(name);
    throw new DiagnosticError(
      parsed.diagnostics.map((diagnostic) =>
        withStandardSource(diagnostic, document, diagnostic.span),
      ),
    );
  }
  const functions = parsed.program.functions.map((declaration) => {
    const intrinsic = intrinsicName(declaration);
    if (!intrinsic) return { ...declaration, standard: true };
    const { decorators: _decorators, ...rest } = declaration;
    return { ...rest, intrinsic, standard: true };
  });
  // A std declaration may be a prelude name (spec/lang/10-modules.md#prelude).
  const standard = <T>(items: readonly T[]): T[] =>
    items.map((item) => ({ ...item, standard: true }));
  return {
    ...parsed.program,
    ...(parsed.program.types ? { types: standard(parsed.program.types) } : {}),
    data: standard(parsed.program.data),
    enums: standard(parsed.program.enums),
    traits: standard(
      parsed.program.traits.map((declaration) => ({
        ...declaration,
        methods: declaration.methods.map(withoutMethodIntrinsic),
      })),
    ),
    implementations: standard(
      parsed.program.implementations.map((declaration) => ({
        ...declaration,
        methods: declaration.methods.map(withoutMethodIntrinsic),
      })),
    ),
    functions,
  };
}

const declaredModules = new Map<
  StandardModule,
  {
    readonly program: Program;
    readonly names: readonly string[];
    readonly publicNames: readonly string[];
  }
>();

/** A module's parsed source, the names it declares, and the ones it declares `pub`. */
function declaredModule(name: StandardModule): {
  readonly program: Program;
  readonly names: readonly string[];
  readonly publicNames: readonly string[];
} {
  const cached = declaredModules.get(name);
  if (cached) return cached;
  const program = parseModule(name, standardSource(name));
  const declarations = [
    ...program.data,
    ...program.enums,
    ...program.traits,
    ...(program.types ?? []),
    ...program.functions,
  ];
  const names = declarations.map((declaration) => declaration.name);
  const publicNames = declarations
    .filter((declaration) => declaration.public === true)
    .map((declaration) => declaration.name);
  const declared = { program, names, publicNames };
  declaredModules.set(name, declared);
  return declared;
}

/** The names that `lib/std/<module>.hd` declares, or undefined when there is no such file. */
export function standardDeclarationNames(module: string): readonly string[] | undefined {
  return isStandardModule(module) ? declaredModule(module).names : undefined;
}

/** The names that `lib/std/<module>.hd` declares `pub`, or undefined when there is no such file. */
export function standardPublicNames(module: string): readonly string[] | undefined {
  return isStandardModule(module) ? declaredModule(module).publicNames : undefined;
}

function standardModule(name: StandardModule): ParsedModule {
  const cached = parsedModules.get(name);
  if (cached) return cached;
  const { program, names } = declaredModule(name);
  const modules = new Set<StandardModule>();
  const uses: { module: StandardModule; name: string }[] = [];
  const exports: { module: StandardModule; name: string }[] = [];
  const compilerUses: { module: string; name: string; span: SourceSpan }[] = [];
  for (const declaration of program.uses) {
    if (declaration.module !== "std" && !declaration.module.startsWith("std."))
      throw new Error(`std.${name} uses '${declaration.module}', which is not a std module`);
    for (const used of usedModules(declaration)) modules.add(used);
    const module = declaration.module.replace(/^std\.?/, "");
    for (const imported of declaration.names) {
      if (isStandardModule(module === "" ? imported.name : `${module}.${imported.name}`)) continue;
      // A compiler-provided name, such as `std.testing.assert_equal` or
      // `std.task.block_on`, is a compiler use: its module does not declare it.
      if (isStandardModule(module) && declaredModule(module).names.includes(imported.name)) {
        uses.push({ module, name: imported.name });
        if (declaration.public === true) exports.push({ module, name: imported.name });
      } else if (declaration.public !== true)
        compilerUses.push({ module, name: imported.name, span: declaration.span });
    }
  }
  const module = { name, program, names, modules: [...modules], uses, exports, compilerUses };
  parsedModules.set(name, module);
  return module;
}

function renameKey(module: StandardModule, renames: ReadonlyMap<string, string>): string {
  return `${module}\0${[...renames].map(([from, to]) => `${from}\0${to}`).join("\0")}`;
}

type ModuleDeclaration = {
  readonly name: string;
  readonly standard?: boolean;
  readonly standardName?: string;
};

/** Top-level declarations of a (renamed) module, in declaration order. */
function declarationsOf(program: Program): readonly ModuleDeclaration[] {
  return [
    ...(program.types ?? []),
    ...program.data,
    ...program.enums,
    ...program.traits,
    ...program.functions,
  ];
}

function baseName(type: string): string {
  return type.split("[")[0] ?? type;
}

/**
 * Records each data type's qualified name, such as `std.annotation.Annotate`,
 * which the compiler recognizes whatever local name it has
 * (spec/lang/14-annotations.md#r-annot.target.recognized). A rename keeps the
 * declaration order.
 */
function withStandardNames(renamed: Program, original: ParsedModule): Program {
  return {
    ...renamed,
    ...(renamed.types
      ? {
          types: renamed.types.map((declaration, index) => ({
            ...declaration,
            standardName: `std.${original.name}.${original.program.types![index]!.name}`,
          })),
        }
      : {}),
    data: renamed.data.map((declaration, index) => ({
      ...declaration,
      standardName: `std.${original.name}.${original.program.data[index]!.name}`,
    })),
    enums: renamed.enums.map((declaration, index) => ({
      ...declaration,
      standardName: `std.${original.name}.${original.program.enums[index]!.name}`,
    })),
    traits: renamed.traits.map((declaration, index) => ({
      ...declaration,
      standardName: `std.${original.name}.${original.program.traits[index]!.name}`,
    })),
    functions: renamed.functions.map((declaration, index) => ({
      ...declaration,
      standardName: `std.${original.name}.${original.program.functions[index]!.name}`,
    })),
  };
}

/**
 * The local names of the program's `std` imports that name a function with
 * no parameters, which a bare decorator calls
 * (spec/lang/14-annotations.md#r-annot.decorator.bare-call).
 */
export function importedMarkerFunctions(program: Program): Set<string> {
  const markers = new Set<string>();
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const functions = standardModule(module).program.functions;
    for (const imported of declaration.names)
      if (functions.some((item) => item.name === imported.name && item.parameters.length === 0))
        markers.add(imported.alias ?? imported.name);
  }
  return markers;
}

/** The `std` module and declared name of a name the program uses, local or hidden. */
function standardOrigin(
  program: Program,
  local: string,
): { readonly module: StandardModule; readonly name: string } | undefined {
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const imported = declaration.names.find((item) => (item.alias ?? item.name) === local);
    if (imported) return { module, name: imported.name };
  }
  // A submodule's prefix extends its parent's, so the longest is tried first.
  for (const module of [...STANDARD_MODULES].sort((a, b) => b.length - a.length))
    if (local.startsWith(hiddenStandardName(module, "")))
      return { module, name: local.slice(hiddenStandardName(module, "").length) };
  return undefined;
}

/** Renames one type spelling from a std module into the program's names. */
function programNames(program: Program, module: StandardModule): (text: string) => string {
  const nameOf = standardNameOf(program);
  const renames = moduleRenames(standardModule(module), nameOf);
  return (text) =>
    text.replace(/[\p{ID_Start}_][\p{ID_Continue}]*/gu, (word) => renames.get(word) ?? word);
}

/**
 * The result type's declaration of the `std` function that the program
 * calls by `callee`, its local or hidden name, with that declaration's name
 * and bound texts in the program's names. Typed facts read it
 * (spec/lang/14-annotations.md#member-typed-facts).
 */
export function standardResultDeclaration(
  program: Program,
  callee: string,
): { readonly declaration: DataDecl | EnumDecl; readonly name: string } | undefined {
  const found = standardOrigin(program, callee);
  if (!found) return undefined;
  const parsed = standardModule(found.module);
  const fn = parsed.program.functions.find((item) => item.name === found.name);
  if (!fn) return undefined;
  const base = baseName(fn.result.name);
  const declaration = [...parsed.program.data, ...parsed.program.enums].find(
    (item) => item.name === base,
  );
  if (!declaration) return undefined;
  const rename = programNames(program, found.module);
  const bounds = (declaration.genericBounds ?? []).map((bound) => ({
    ...bound,
    traits: bound.traits.map(rename),
  }));
  return { declaration: { ...declaration, genericBounds: bounds }, name: rename(base) };
}

/**
 * The supertraits without arguments of the `std` trait that the program
 * names `local`, in the program's names, as `Num` for `std.num.Integer`.
 */
export function standardSupertraits(program: Program, local: string): readonly string[] {
  const found = standardOrigin(program, local);
  if (!found) return [];
  const trait = standardModule(found.module).program.traits.find(
    (item) => item.name === found.name,
  );
  const rename = programNames(program, found.module);
  return (trait?.supertraits ?? [])
    .filter((supertrait) => !supertrait.name.includes("["))
    .map((supertrait) => rename(supertrait.name));
}

/**
 * How a module's source is renamed into the program: the prelude's names,
 * which every module implicitly uses, its own names, and the names it uses.
 */
function moduleRenames(
  parsed: ParsedModule,
  nameOf: (module: StandardModule, name: string) => string,
  structureNames: ReadonlyMap<string, string> = new Map(),
): Map<string, string> {
  const renames = new Map<string, string>();
  for (const used of standardModule(PRELUDE).exports)
    renames.set(used.name, nameOf(used.module, used.name));
  for (const name of parsed.names) renames.set(name, nameOf(parsed.name, name));
  for (const used of parsed.uses) renames.set(used.name, nameOf(used.module, used.name));
  for (const used of parsed.compilerUses)
    if (used.module !== STRUCTURE) renames.set(used.name, compilerUseName(used.module, used.name));
    else if (structureNames.has(used.name)) renames.set(used.name, structureNames.get(used.name)!);
  return renames;
}

/** The node with each `name` equal to `from` replaced by `to`. */
function renamed<T>(node: T, from: string, to: string): T {
  if (Array.isArray(node)) return node.map((item) => renamed(item, from, to)) as T;
  if (!node || typeof node !== "object") return node;
  const result: Record<string, unknown> = {};
  for (const [key, child] of Object.entries(node))
    result[key] = key === "name" && child === from ? to : renamed(child, from, to);
  return result as T;
}

/**
 * A desugared prefixed string names `std.ops.Template` by its hidden name
 * (`TEMPLATE_PLACEHOLDER`), which is declared only when the program does not
 * import `Template`; an import renames it to the local name
 * (spec/lang/05-expressions.md#r-expr.literal-fn.no-marker-import).
 */
function withTemplateName(program: Program): Program {
  for (const declaration of program.uses)
    for (const imported of declaration.names)
      if (declaration.module === "std.ops" && imported.name === "Template")
        return renamed(program, TEMPLATE_PLACEHOLDER, imported.alias ?? imported.name);
  return program;
}

/** `std.structure`, which the typed-derivation pass declares (checker/typed-derivation.ts). */
const STRUCTURE = "structure";

/** The `std.structure` names a module imports. */
function structureNamesOf(module: ParsedModule): Set<string> {
  return new Set(
    module.compilerUses.filter((used) => used.module === STRUCTURE).map((used) => used.name),
  );
}

/**
 * A module's templates, and its implementations of the `std.structure`
 * protocol traits, which only a derivation instantiates
 * (spec/lang/14-annotations.md#templates).
 */
function isTemplatePart(implementation: ImplDecl, structure: ReadonlySet<string>): boolean {
  return (
    implementation.byStructure !== undefined ||
    (implementation.traitName !== undefined && structure.has(baseName(implementation.traitName)))
  );
}

/**
 * A renamed module without its template parts: the typed-derivation pass
 * instantiates them through `standardTemplate`, and they name
 * `std.structure`, which the program declares only when it derives.
 */
function withoutTemplates(renamed: Program, original: ParsedModule): Program {
  const structure = structureNamesOf(original);
  return withStandardNames(
    {
      ...renamed,
      implementations: renamed.implementations.filter(
        (implementation) => !isTemplatePart(implementation, structure),
      ),
    },
    original,
  );
}

/**
 * The program's local names of `std` declarations, by `<module>.<Name>`:
 * the declarations it imports, and the prelude's, as if the program began
 * with the prelude's `pub use` lines
 * (spec/lang/10-modules.md#r-module.prelude.fixed-uses). A prelude name
 * that the program binds to another declaration of its own is a
 * `prelude-name-shadow` error, and the prelude's declaration keeps its
 * hidden name.
 */
function standardLocalNames(program: Program): Map<string, string> {
  const localNames = new Map<string, string>();
  const own = new Set<string>(
    declarationsOf(program)
      .filter((declaration) => !declaration.standard)
      .map((declaration) => declaration.name),
  );
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    const declared =
      declaration.module.startsWith("std.") && isStandardModule(module)
        ? standardModule(module).names
        : [];
    for (const imported of declaration.names) {
      const local = imported.alias ?? imported.name;
      if (declared.includes(imported.name)) localNames.set(`${module}.${imported.name}`, local);
      if (local !== imported.name) own.add(local);
    }
  }
  for (const used of preludeExports(program))
    if (!own.has(used.name)) localNames.set(`${used.module}.${used.name}`, used.name);
  return localNames;
}

/**
 * The program's names of `std.testing`'s runner capabilities, which `hd
 * test` binds for the case bodies it gives an `it_each`, `it_prop`, or
 * `it_prop_with` test case or a `timeout` option
 * (spec/std/testing.md#runner-capabilities).
 */
export function testRunnerNames(program: Program): TestRunnerNames {
  const nameOf = standardNameOf(program);
  return {
    test: nameOf("testing", "TestRunner"),
    property: nameOf("testing", "PropertyRunner"),
    process: nameOf("process", "Process"),
  };
}

/**
 * The host capability traits of the default profile, which `hd FILE`,
 * `hd run`, and a task bind (spec/cli/command-line.md#r-cli.host.default-profile).
 */
export const DEFAULT_PROFILE_TRAITS: readonly (readonly [StandardModule, string])[] = [
  ["console", "Console"],
  ["console", "ConsoleInput"],
  ["host", "Args"],
  ["host", "Env"],
  ["time", "Clock"],
  ["random", "Random"],
  ["fs", "FsRead"],
  ["fs", "FsWrite"],
];

/** The program's names of the default profile's traits. */
export function defaultProfileNames(program: Program): readonly string[] {
  const nameOf = standardNameOf(program);
  return DEFAULT_PROFILE_TRAITS.map(([module, name]) => nameOf(module, name));
}

export interface TestRunnerNames {
  readonly test: string;
  readonly property: string;
  /** `std.process.Process`, which `hd test` binds for an integration test (spec/cli/command-line.md#r-cli.test.process). */
  readonly process: string;
}

/** The program's name for each `std` declaration: its local name, or else its hidden name. */
function standardNameOf(program: Program): (module: StandardModule, name: string) => string {
  const localNames = standardLocalNames(program);
  return (module, name) => localNames.get(`${module}.${name}`) ?? hiddenStandardName(module, name);
}

/**
 * The spelling under which the program joins the public type `name` of the
 * std module `module`, which a module path such as `cmp.Ordering` names, or
 * undefined when `name` is no public data, enum, trait, or type there. A
 * function keeps its module path, which the checker resolves itself.
 */
export function standardTypeSpelling(
  program: Program,
): (module: string, name: string) => string | undefined {
  const nameOf = standardNameOf(program);
  return (module, name) => {
    if (!isStandardModule(module)) return undefined;
    const declared = declaredModule(module).program;
    const type = [
      ...declared.data,
      ...declared.enums,
      ...declared.traits,
      ...(declared.types ?? []),
    ].some((declaration) => declaration.name === name && declaration.public === true);
    return type ? nameOf(module, name) : undefined;
  };
}

/**
 * Declaration identities and additional local spellings of ordinary
 * `lib/std` declarations, each bound to the one spelling under which the
 * declaration is joined. Compiler-owned names have their own resolution path.
 */
export function standardImportAliases(program: Program): ReadonlyMap<string, string> {
  const nameOf = standardNameOf(program);
  const aliases = new Map<string, string>();
  const selectedModules = new Set(program.uses.flatMap(usedModules));
  for (const module of selectedModules)
    for (const name of standardModule(module).names)
      aliases.set(`std.${module}.${name}`, nameOf(module, name));
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (module === "core") {
      for (const imported of declaration.names) {
        if (!STANDARD_CORE_TYPE_ALIASES.has(imported.name)) continue;
        const local = imported.alias ?? imported.name;
        if (local !== imported.name) aliases.set(local, imported.name);
      }
      continue;
    }
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const declared = standardModule(module).names;
    for (const imported of declaration.names) {
      if (!declared.includes(imported.name)) continue;
      const local = imported.alias ?? imported.name;
      const target = nameOf(module, imported.name);
      if (local !== target) aliases.set(local, target);
    }
  }
  return aliases;
}

/** The joined declaration identity exported by the ordinary std prelude as `name`. */
export function standardPreludeBinding(program: Program, name: string): string | undefined {
  const exported = standardModule(PRELUDE).exports.find((item) => item.name === name);
  if (!exported) return undefined;
  const identity = `std.${exported.module}.${exported.name}`;
  return declarationsOf(program).find((declaration) => declaration.standardName === identity)?.name;
}

/**
 * The template of the `std` trait that the program imports as `trait`,
 * with the module's `std.structure` protocol implementations, written with
 * the program's names: a name it imports by its local name, any other `std`
 * name by its hidden name, and `std.structure` names as the typed-derivation
 * pass declares them (spec/lang/14-annotations.md#templates).
 */
export interface StandardTemplate {
  readonly template: ImplDecl;
  readonly support: readonly ImplDecl[];
  /** Every implementation of the trait's module, in the program's names. */
  readonly implementations: readonly ImplDecl[];
}

export function standardTemplate(
  program: Program,
  trait: string,
  /** The names of `std.structure` items the program renames (typed-derivation.ts). */
  structureNames: ReadonlyMap<string, string>,
  /** The trait's tuple template, `impl[T < Tuple] Trait for T by Structure`, rather than its template. */
  tuple = false,
): StandardTemplate | undefined {
  // The trait's module: one the program imports it from, or, for a prelude
  // trait such as `Eq` or `Hash`, the module that declares it. A joined std
  // declaration is resolved by its declaration identity, because its source
  // module's ordinary `use` lines no longer exist after the one-module join.
  const candidates: StandardModule[] = [];
  const identity = program.traits.find(
    (declaration) => declaration.name === trait && declaration.standardName?.startsWith("std."),
  )?.standardName;
  if (identity) {
    const module = [...STANDARD_MODULES]
      .sort((left, right) => right.length - left.length)
      .find((name) => identity.startsWith(`std.${name}.`));
    if (module) candidates.push(module);
  }
  for (const declaration of program.uses) {
    const module = declaration.module.replace(/^std\./, "");
    if (!declaration.module.startsWith("std.") || !isStandardModule(module)) continue;
    const imported = declaration.names.find((name) => (name.alias ?? name.name) === trait);
    if (
      imported &&
      standardModule(module).names.includes(imported.name) &&
      !candidates.includes(module)
    )
      candidates.push(module);
  }
  for (const used of preludeExports(program))
    if (used.name === trait && !candidates.includes(used.module)) candidates.push(used.module);
  const nameOf = standardNameOf(program);
  for (const module of candidates) {
    const parsed = standardModule(module);
    const renames = moduleRenames(parsed, nameOf, structureNames);
    const key = renameKey(module, renames);
    const structure = new Set(
      [...structureNamesOf(parsed)].map((name) => structureNames.get(name) ?? name),
    );
    let parsedImplementations = templateModules.get(key);
    if (!parsedImplementations) {
      parsedImplementations = renameStandardBindings(parsed.program, renames).implementations;
      templateModules.set(key, parsedImplementations);
    }
    const anchor = program.traits.find((declaration) => declaration.name === trait)?.span;
    const implementations = anchor
      ? withStandardSource(parsedImplementations, standardDocument(module), anchor)
      : parsedImplementations;
    // The template, or the tuple template, whose parameter is bounded by
    // `Tuple` (annot.template.tuple.separate).
    const template = implementations.find(
      (implementation) =>
        implementation.byStructure !== undefined &&
        implementation.genericBounds.length === (tuple ? 1 : 0) &&
        baseName(implementation.traitName ?? "") === trait,
    );
    if (!template) return undefined;
    const support = implementations.filter(
      (implementation) =>
        implementation.byStructure === undefined && isTemplatePart(implementation, structure),
    );
    return { template, support, implementations };
  }
  return undefined;
}

/**
 * The local names of the `std` traits with a tuple template that the
 * program sees: prelude traits, such as `Eq`, and imported ones, such as
 * `std.ops.Default` (spec/lang/14-annotations.md#tuple-templates).
 */
export function standardTupleTraits(program: Program): readonly string[] {
  const localNames = standardLocalNames(program);
  const traits: string[] = [];
  for (const module of STANDARD_MODULES)
    for (const implementation of declaredModule(module).program.implementations) {
      if (implementation.byStructure === undefined || implementation.genericBounds.length !== 1)
        continue;
      const local = localNames.get(`${module}.${baseName(implementation.traitName ?? "")}`);
      if (local !== undefined && !traits.includes(local)) traits.push(local);
    }
  return traits;
}

/**
 * The std modules of the program's use graph, each with the span of the
 * program's `use` that reaches it, or the program's span for one that only
 * the prelude reaches.
 */
function useGraph(program: Program): Map<StandardModule, SourceSpan> {
  const spans = new Map<StandardModule, SourceSpan>();
  const reach = (module: StandardModule, span: SourceSpan): void => {
    if (spans.has(module)) return;
    spans.set(module, span);
    for (const used of standardModule(module).modules) reach(used, span);
  };
  for (const use of program.uses) for (const module of usedModules(use)) reach(module, use.span);
  for (const module of preludeModules(program)) reach(module, program.span);
  return spans;
}

/** The std modules that the program's use graph joins, in `STANDARD_MODULES` order. */
export function standardModulesOf(program: Program): StandardModule[] {
  const graph = useGraph(program);
  return STANDARD_MODULES.filter((module) => graph.has(module));
}

/**
 * Joins the `std` modules of the program's use graph: `std.prelude`, every
 * std module a `use` of the program reaches, and every std module a `use`
 * of a joined module reaches.
 */
export function withStandardLibrary(source: Program): Program {
  const program = withTemplateName(source);
  const spans = useGraph(program);

  const nameOf = standardNameOf(program);
  const moduleProgram = (module: StandardModule): Program => {
    const parsed = standardModule(module);
    const renames = moduleRenames(parsed, nameOf);
    const key = renameKey(module, renames);
    const cached = renamedModules.get(key);
    if (cached) return cached;
    const renamed = withoutTemplates(renameStandardBindings(parsed.program, renames), parsed);
    renamedModules.set(key, renamed);
    return renamed;
  };

  // Module declarations, each source-mapped to the use that reached the module.
  const types = [...(program.types ?? [])];
  const data = [...program.data];
  const enums = [...program.enums];
  const traits = [...program.traits];
  const functions = [...program.functions];
  const implementations = [...program.implementations];
  // Compiler-provided names that a joined module uses, under hidden names.
  const uses = [...program.uses];
  const imported = new Set<string>();
  for (const module of STANDARD_MODULES) {
    const span = spans.get(module);
    if (!span) continue;
    const renamed = moduleProgram(module);
    const located = withStandardSource(renamed, standardDocument(module), span);
    types.push(...(located.types ?? []));
    data.push(...located.data);
    enums.push(...located.enums);
    traits.push(...located.traits);
    functions.push(...located.functions);
    implementations.push(
      // `std` owns its prelude traits, so its impls are never orphans.
      ...located.implementations.map((implementation) => ({
        ...implementation,
        standard: true,
      })),
    );
    for (const used of standardModule(module).compilerUses) {
      const alias = compilerUseName(used.module, used.name);
      if (imported.has(alias) || imports(program, `std.${used.module}`, used.name, alias)) continue;
      imported.add(alias);
      uses.push(compilerUse(used, withStandardSource(used.span, standardDocument(module), span)));
    }
  }
  return { ...program, uses, types, data, enums, traits, functions, implementations };
}
