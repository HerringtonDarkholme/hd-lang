import { copyFileSync, readdirSync, renameSync } from "node:fs";
import { join } from "node:path";

/** Archive the just-reported set; older sets live outside this raw-input folder. */
export function archiveProfiles(target: string, directory: string): [string, string][] {
  const restore: [string, string][] = [];
  // cost: this run's new raw profiles plus the fixed Cargo build directories.
  for (const name of readdirSync(target)) {
    const from = join(target, name);
    const to = join(directory, name);
    if (name.endsWith(".profraw")) {
      renameSync(from, to);
      restore.push([to, from]);
    } else if (name.endsWith(".profdata")) {
      copyFileSync(from, to);
    }
  }
  if (!restore.length) {
    throw new Error("the coverage set emitted no raw profiles");
  }
  return restore;
}

/** Restore all measured sets for the aggregate report, without scanning old sets. */
export function restoreProfiles(profiles: readonly [string, string][]): void {
  for (const [from, to] of profiles) {
    renameSync(from, to);
  }
}
