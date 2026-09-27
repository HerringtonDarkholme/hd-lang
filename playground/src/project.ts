import { moduleIdentity } from "../../src/package.ts";

/** A playground project: one package whose modules live under `src/`. */
export interface Project {
  /** Package path (`src/models/user.hd`) to source text. */
  readonly files: Readonly<Record<string, string>>;
  /** The entry module's path. */
  readonly main: string;
}

export const DEFAULT_MAIN = "src/main.hd";

/** Why `path` cannot name a package file, or undefined when it can. */
export function pathProblem(
  path: string,
  project?: Project,
  renaming?: string,
): string | undefined {
  if (moduleIdentity(path) === undefined)
    return "use src/<name>.hd with identifier names, like src/models/user.hd";
  if (project && path !== renaming && path in project.files) return `${path} already exists`;
  return undefined;
}

/** Checks untrusted input (a shared link, stored state) and returns a project. */
export function asProject(value: unknown): Project | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const { files, main } = value as { files?: unknown; main?: unknown };
  if (typeof files !== "object" || files === null) return undefined;
  const entries = Object.entries(files as Record<string, unknown>);
  if (entries.length === 0) return undefined;
  if (!entries.every(([path, source]) => typeof source === "string" && !pathProblem(path)))
    return undefined;
  const checked = Object.fromEntries(entries) as Record<string, string>;
  const entry = typeof main === "string" && main in checked ? main : entries[0]![0];
  return { files: checked, main: entry };
}

/** Sorted file paths with the entry module first. */
export function orderedPaths(project: Project): string[] {
  return Object.keys(project.files).sort((left, right) =>
    left === project.main ? -1 : right === project.main ? 1 : left.localeCompare(right),
  );
}
