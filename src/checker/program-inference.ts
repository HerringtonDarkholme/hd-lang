import type { FunctionDecl } from "../ast.ts";
import type { HirGlobal, HirTraitImplementation } from "../hir.ts";
import { FunctionChecker } from "./checker.ts";
import type { FunctionCheckResult, Signature } from "./context.ts";

import type { ProgramCheckContext } from "./program-context.ts";

// A signature map whose omitted result types are inferred on first lookup, so
// a caller always sees the callee's final result type.
class LazySignatures extends Map<string, Signature> {
  resolve?: (name: string) => void;

  override get(name: string): Signature | undefined {
    if (this.resolve && super.has(name)) this.resolve(name);
    return super.get(name);
  }

  peek(name: string): Signature | undefined {
    return super.get(name);
  }
}

function displayName(name: string): string {
  return name.replace(/^\$inherent\d+\./, "");
}

// Infers omitted result types and requirement rows of non-public functions and
// inherent methods (07-functions.md#declarations, 11-requirements-and-suspension.md).
//
// A result is inferred from the body the first time any checker looks the
// function up; a lookup that re-enters a function whose result is still being
// inferred is a cycle, reported once as recursive-function-needs-result-type.
// Rows start empty and are recomputed until no row grows, which yields the
// least rows for mutually recursive functions.
export class SignatureInference {
  readonly signatures = new LazySignatures();
  // Functions whose inference already reported an error; the final pass skips them.
  readonly failed = new Set<string>();
  private readonly declarationsByName = new Map<string, FunctionDecl>();
  private readonly pendingResults = new Set<string>();
  private readonly rowNames = new Set<string>();
  private readonly inferring: string[] = [];
  private globals = new Map<string, HirGlobal>();

  private readonly context: ProgramCheckContext;
  private readonly declarations: readonly FunctionDecl[];
  private readonly implementations: readonly HirTraitImplementation[];
  private readonly moduleDeclaration: FunctionDecl | undefined;

  constructor(
    context: ProgramCheckContext,
    declarations: readonly FunctionDecl[],
    signatures: ReadonlyMap<string, Signature>,
    implementations: readonly HirTraitImplementation[],
    moduleDeclaration: FunctionDecl | undefined,
  ) {
    this.context = context;
    this.declarations = declarations;
    this.implementations = implementations;
    this.moduleDeclaration = moduleDeclaration;
    for (const [name, signature] of signatures) this.signatures.set(name, signature);
    for (const declaration of declarations) {
      this.declarationsByName.set(declaration.name, declaration);
      if (declaration.resultOmitted) this.pendingResults.add(declaration.name);
      if (declaration.requirementsOmitted && !declaration.public)
        this.rowNames.add(declaration.name);
    }
    this.signatures.resolve = (name) => this.resolveResult(name);
  }

  get active(): boolean {
    return this.pendingResults.size > 0 || this.rowNames.size > 0;
  }

  useGlobals(globals: Map<string, HirGlobal>): void {
    this.globals = globals;
  }

  inferRows(): void {
    const rowDeclarations = this.declarations.filter((declaration) =>
      this.rowNames.has(declaration.name),
    );
    if (rowDeclarations.length === 0) return;
    for (;;) {
      this.globals = new Map();
      if (this.moduleDeclaration)
        this.run(
          this.moduleDeclaration,
          this.signatures.get(this.moduleDeclaration.name)!,
          false,
          false,
        );
      let changed = false;
      for (const declaration of rowDeclarations) {
        const signature = this.signatures.get(declaration.name);
        if (!signature || this.failed.has(declaration.name)) continue;
        const checked = this.run(declaration, signature, true, false);
        if (!checked.function) continue;
        const row = new Set(signature.requirements);
        const before = row.size;
        for (const requirement of checked.function.requirements) row.add(requirement);
        if (row.size === before) continue;
        this.signatures.set(declaration.name, { ...signature, requirements: [...row].sort() });
        changed = true;
      }
      if (!changed) return;
    }
  }

  run(
    declaration: FunctionDecl,
    signature: Signature,
    inferRequirements: boolean,
    inferResult: boolean,
  ): FunctionCheckResult {
    const { program, dataTypes, enumTypes, traitTypes, inherentMethods, imports } = this.context;
    return new FunctionChecker(
      declaration,
      signature,
      this.signatures,
      dataTypes,
      enumTypes,
      traitTypes,
      this.implementations,
      inherentMethods,
      !program.functions.includes(declaration),
      program.statements.length > 0 && declaration.body === program.statements,
      [],
      false,
      new Map(),
      -1,
      new Map(),
      inferRequirements,
      inferResult,
      undefined,
      imports,
      this.globals,
    ).check();
  }

  private resolveResult(name: string): void {
    if (!this.pendingResults.has(name)) return;
    const cycleStart = this.inferring.indexOf(name);
    if (cycleStart >= 0) {
      this.reportCycle(this.inferring.slice(cycleStart));
      return;
    }
    const declaration = this.declarationsByName.get(name)!;
    this.inferring.push(name);
    let checked: FunctionCheckResult;
    try {
      checked = this.run(declaration, this.signatures.peek(name)!, this.rowNames.has(name), true);
    } finally {
      this.inferring.pop();
    }
    this.pendingResults.delete(name);
    const signature = this.signatures.peek(name)!;
    if (this.failed.has(name)) return;
    if (!checked.function) {
      this.context.diagnostics.push(...checked.diagnostics);
      this.fail(name);
      return;
    }
    this.signatures.set(name, { ...signature, result: checked.function.result });
  }

  private reportCycle(members: readonly string[]): void {
    if (members.some((member) => this.failed.has(member))) return;
    const first = members
      .map((member) => this.declarationsByName.get(member)!)
      .reduce((left, right) => (right.span.start.offset < left.span.start.offset ? right : left));
    this.context.diagnostics.push({
      code: "recursive-function-needs-result-type",
      message: `function '${displayName(first.name)}' omits its result type but is recursive; declare the result of a function in the cycle`,
      span: first.span,
    });
    for (const member of members) this.fail(member);
  }

  // A failed function's calls type as `never`, so callers report nothing more.
  private fail(name: string): void {
    this.failed.add(name);
    const signature = this.signatures.peek(name);
    if (signature) this.signatures.set(name, { ...signature, result: "never" });
  }
}
