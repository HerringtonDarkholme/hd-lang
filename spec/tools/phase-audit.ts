// Audits the phase column of spec/conformance/cases.tsv against the stable
// diagnostic ownership used by diagnostic-codes.ts.
import { diagnosticCodePhases, type DiagnosticPhase } from "./diagnostic-codes.ts";
import { knownCodes } from "./spec-prose.ts";

export interface PhaseMismatch {
  readonly path: string;
  readonly code: string;
  readonly rowPhase: string;
  readonly expectedPhase: DiagnosticPhase | null;
}

export interface PhaseAudit {
  readonly rows: number;
  readonly checked: number;
  readonly mismatches: readonly PhaseMismatch[];
}

/** Checks every reject, warning, and panic expectation in a fixture index. */
export function phaseAudit(readme: string, controlFlow: string, cases: string): PhaseAudit {
  const lines = cases.trimEnd().split(/\r?\n/);
  const header = lines.shift()?.split("\t") ?? [];
  const pathAt = header.indexOf("path");
  const phaseAt = header.indexOf("phase");
  const expectationAt = header.indexOf("expectation");
  if (pathAt < 0 || phaseAt < 0 || expectationAt < 0)
    throw new Error("cases.tsv lacks path, phase, or expectation");

  const diagnostics = diagnosticCodePhases(readme, cases);
  const panics = knownCodes("", controlFlow);
  const mismatches: PhaseMismatch[] = [];
  let checked = 0;
  for (const line of lines) {
    if (line === "") continue;
    const fields = line.split("\t");
    const match = /^(reject|warn|panic):([a-z0-9-]+)$/.exec(fields[expectationAt] ?? "");
    if (!match) continue;
    checked += 1;
    const [, kind, code] = match;
    const expectedPhase =
      kind === "panic" ? (panics.has(code!) ? "runtime" : null) : (diagnostics.get(code!) ?? null);
    const rowPhase = fields[phaseAt] ?? "";
    if (expectedPhase !== rowPhase)
      mismatches.push({
        path: fields[pathAt] ?? "",
        code: code!,
        rowPhase,
        expectedPhase,
      });
  }
  return { rows: lines.filter((line) => line !== "").length, checked, mismatches };
}

export function phaseAuditReport(result: PhaseAudit): string {
  if (result.mismatches.length === 0)
    return `phase audit passed (${result.checked} diagnostic rows)\n`;
  const noun = result.mismatches.length === 1 ? "mismatch" : "mismatches";
  const lines = result.mismatches.map((mismatch) =>
    mismatch.expectedPhase === null
      ? `${mismatch.path}: ${mismatch.code} is absent from the diagnostics tables (row says ${mismatch.rowPhase})`
      : `${mismatch.path}: ${mismatch.code} belongs to ${mismatch.expectedPhase}, row says ${mismatch.rowPhase}`,
  );
  return `phase audit: ${result.mismatches.length} ${noun}\n${lines.join("\n")}\n`;
}
