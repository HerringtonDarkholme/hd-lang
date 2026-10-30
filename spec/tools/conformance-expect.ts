/** One `run:` line of a conformance CLI case's `expect.txt`. */
export interface CliStep {
  readonly command: string;
  readonly exit: number;
  readonly files: readonly string[];
  readonly noFiles: readonly string[];
  readonly sameText: readonly (readonly [string, string])[];
  readonly stderr?: string;
  readonly stderrJson?: readonly unknown[];
  readonly stdout?: string;
  readonly stdoutJson?: readonly unknown[];
}

interface Draft {
  command: string;
  exit?: number;
  files: string[];
  noFiles: string[];
  sameText: [string, string][];
  stderrJson?: unknown[];
  stderrLines?: string[];
  stdoutJson?: unknown[];
  stdoutLines?: string[];
}

// Decodes the TEXT of one `# expect-stdout:` line (README, Standard Output).
export function decodeStdoutLine(text: string): string | undefined {
  if (/\s$/.test(text)) {
    return undefined;
  }
  let decoded = "";
  for (let index = 0; index < text.length; index += 1) {
    const char = text[index]!;
    if (char !== "\\") {
      decoded += char;
      continue;
    }
    const next = text[index + 1];
    if (next === "\\") {
      decoded += "\\";
    } else if (next === "t") {
      decoded += "\t";
    } else if (next === "u") {
      const escape = /^\{([0-9A-Fa-f]{1,6})\}/.exec(text.slice(index + 2));
      const scalar = escape ? Number.parseInt(escape[1]!, 16) : Number.NaN;
      if (!escape || scalar > 0x10ffff || (scalar >= 0xd800 && scalar <= 0xdfff)) {
        return undefined;
      }
      decoded += String.fromCodePoint(scalar);
      index += escape[0].length;
    } else {
      return undefined;
    }
    index += 1;
  }
  return decoded;
}

function jsonArray(value: string): unknown[] | undefined {
  try {
    const parsed: unknown = JSON.parse(value);
    return Array.isArray(parsed) ? parsed : undefined;
  } catch {
    return undefined;
  }
}

function applyExit(step: Draft, value: string, where: string): string | undefined {
  if (step.exit !== undefined) {
    return `${where}: a step has one 'exit:'`;
  }
  if (!/^\d{1,3}$/.test(value) || Number(value) > 255) {
    return `${where}: 'exit:' takes a status from 0 to 255`;
  }
  step.exit = Number(value);
  return undefined;
}

function applyText(
  step: Draft,
  kind: "stdout" | "stderr",
  value: string,
  where: string,
): string | undefined {
  const json = kind === "stdout" ? step.stdoutJson : step.stderrJson;
  if (json) {
    return `${where}: '${kind}:' and '${kind}-json:' exclude each other`;
  }
  const decoded = decodeStdoutLine(value);
  if (decoded === undefined) {
    return `${where}: invalid '${kind}:' text`;
  }
  if (kind === "stdout") {
    (step.stdoutLines ??= []).push(decoded);
  } else {
    (step.stderrLines ??= []).push(decoded);
  }
  return undefined;
}

function applyJson(
  step: Draft,
  kind: "stdout-json" | "stderr-json",
  value: string,
  where: string,
): string | undefined {
  const parsed = jsonArray(value);
  if (!parsed) {
    return `${where}: '${kind}:' takes a JSON array`;
  }
  if (kind === "stdout-json") {
    if (step.stdoutJson || step.stdoutLines) {
      return `${where}: '${kind}:' appears once and excludes 'stdout:'`;
    }
    step.stdoutJson = parsed;
  } else {
    if (step.stderrJson || step.stderrLines) {
      return `${where}: '${kind}:' appears once and excludes 'stderr:'`;
    }
    step.stderrJson = parsed;
  }
  return undefined;
}

const relativePath = (path: string): boolean =>
  path !== "" && !path.startsWith("/") && !path.split("/").includes("..");

function applyFile(
  step: Draft,
  kind: "file" | "no-file",
  value: string,
  where: string,
): string | undefined {
  if (!relativePath(value)) {
    return `${where}: '${kind}:' takes a relative path with no '..'`;
  }
  (kind === "file" ? step.files : step.noFiles).push(value);
  return undefined;
}

function applyLine(step: Draft, kind: string, value: string, where: string): string | undefined {
  switch (kind) {
    case "exit":
      return applyExit(step, value, where);
    case "stdout":
    case "stderr":
      return applyText(step, kind, value, where);
    case "stdout-json":
    case "stderr-json":
      return applyJson(step, kind, value, where);
    case "file":
    case "no-file":
      return applyFile(step, kind, value, where);
    case "same-text": {
      const paths = value.split(" ");
      if (paths.length !== 2 || !paths.every(relativePath)) {
        return `${where}: 'same-text:' takes two relative paths with no '..'`;
      }
      step.sameText.push([paths[0]!, paths[1]!]);
      return undefined;
    }
    default:
      return `${where}: unknown line kind '${kind}'`;
  }
}

/** Parses an `expect.txt`, returning a reason when it is invalid. */
export function parseCliExpect(text: string): CliStep[] | string {
  const drafts: Draft[] = [];
  for (const [index, raw] of text.split("\n").entries()) {
    const line = raw.replace(/\r$/, "");
    if (line.trim() === "" || line.startsWith("#")) {
      continue;
    }
    const where = `expect.txt line ${index + 1}`;
    const match = /^([a-z-]+):(?: (.*))?$/.exec(line);
    if (!match) {
      return `${where}: expected 'KIND: VALUE'`;
    }
    const [, kind, value = ""] = match;
    if (kind === "run") {
      if (!/^hd( \S+)*$/.test(value)) {
        return `${where}: 'run:' takes 'hd' and words with no quoting`;
      }
      drafts.push({ command: value, files: [], noFiles: [], sameText: [] });
      continue;
    }
    const step = drafts.at(-1);
    if (!step) {
      return `${where}: '${kind}:' comes before the first 'run:'`;
    }
    const problem = applyLine(step, kind!, value, where);
    if (problem) {
      return problem;
    }
  }
  if (drafts.length === 0) {
    return "expect.txt has no 'run:' line";
  }
  return drafts.map((draft) => ({
    command: draft.command,
    exit: draft.exit ?? 0,
    files: draft.files,
    noFiles: draft.noFiles,
    sameText: draft.sameText,
    stderr: draft.stderrLines?.map((line) => `${line}\n`).join(""),
    stderrJson: draft.stderrJson,
    stdout: draft.stdoutLines?.map((line) => `${line}\n`).join(""),
    stdoutJson: draft.stdoutJson,
  }));
}
