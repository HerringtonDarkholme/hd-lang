import type { Program } from "../ast.ts";
import type { CheckResult } from "./context.ts";
import { createProgramDeclarations } from "./program-declarations.ts";
import { checkEmbeddedMemberConflicts, checkEmbeddingLimits } from "./program-embedding.ts";
import { prepareImplementations } from "./program-implementations.ts";
import { lowerCheckedProgram } from "./program-lower.ts";
import { createProgramSignatures } from "./program-signatures.ts";
import {
  declareProgramTypes,
  defineProgramData,
  defineProgramEnums,
  defineProgramTraits,
} from "./program-types.ts";
import { validateProgram } from "./program-validation.ts";
import type { ProgramCheckContext } from "./program-context.ts";
import { validateHostCapabilities } from "./host-capabilities.ts";
import { withStandardTraits } from "./standard-traits.ts";

export interface CheckOptions {
  readonly hostCapabilities?: readonly string[];
}

export function check(source: Program, options: CheckOptions = {}): CheckResult {
  const program = withStandardTraits(source);
  const context: ProgramCheckContext = {
    program,
    diagnostics: [],
    imports: new Map(),
    dataTypes: new Map(),
    enumTypes: new Map(),
    traitTypes: new Map(),
    implementationPreparations: [],
    inherentMethods: [],
    inherentDeclarations: [],
    hostCapabilities: new Set(["Console", ...(options.hostCapabilities ?? [])]),
  };
  validateProgram(context);
  // A missing required result type leaves no signature to check against.
  if (context.diagnostics.some((diagnostic) => diagnostic.code === "missing-result-type"))
    return { diagnostics: context.diagnostics };
  declareProgramTypes(context);
  defineProgramData(context);
  defineProgramEnums(context);
  defineProgramTraits(context);
  validateHostCapabilities(context);
  prepareImplementations(context);
  checkEmbeddingLimits(context);
  checkEmbeddedMemberConflicts(context);
  const declarations = createProgramDeclarations(context);
  if (!declarations) return { diagnostics: context.diagnostics };
  const signatures = createProgramSignatures(context, declarations);
  if (context.diagnostics.length > 0) return { diagnostics: context.diagnostics };
  return lowerCheckedProgram(context, declarations, signatures);
}
