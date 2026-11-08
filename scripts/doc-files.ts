// The prose docs that check-rule-citations.ts and check-removed-forms.ts read
// besides compiler source: docs/, the top-level README and AGENTS, the crate
// READMEs and DESIGN files, and the website guide. A file under docs/audit/ is
// a dated record: it keeps what it said then, so only its links are checked.

import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { join } from "node:path";

export interface DocFile {
  file: string;
  text: string;
  dated: boolean;
}

export function docFiles(root: string): DocFile[] {
  return execFileSync(
    "git",
    [
      "ls-files",
      "-z",
      "--",
      "docs/**/*.md",
      "docs/*.md",
      "README.md",
      "AGENTS.md",
      "crates/README.md",
      "crates/*/DESIGN.md",
      "website/guide/*.md",
    ],
    { cwd: root, encoding: "utf8" },
  )
    .split("\0")
    .filter(Boolean)
    .map((file) => ({
      file,
      text: readFileSync(join(root, file), "utf8"),
      dated: file.startsWith("docs/audit/"),
    }));
}

// A line that points into git history ("git history at 5d6992009",
// "git show 28072b8a1:docs/...") names a path that is gone on purpose.
export const HISTORY = /git (?:history|show)|\b[0-9a-f]{9}\b/;
