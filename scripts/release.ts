// Cut a local tagged release. Nothing here pushes or publishes.
// Cost: O(T log T + C + I + S): version tags T, new commit text C, affected
// issue titles I and spec text S. Release-only work, not an hd command path; one
// batched issue query per 40 unique issues, and one workspace lock update.
import { execFileSync } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { allRules, loadCorpusAt } from "../spec/tools/spec-corpus.ts";

export function versionParts(value: string): bigint[] {
  if (value.trim() !== value || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(value)) {
    throw new Error(`expected X.Y.Z without leading zeroes: ${value}`);
  }
  return value.split(".").map(BigInt);
}

export function compareVersions(left: string, right: string): number {
  const aa = versionParts(left);
  const bb = versionParts(right);
  for (let index = 0; index < 3; index += 1) {
    if (aa[index]! !== bb[index]!) {
      return aa[index]! > bb[index]! ? 1 : -1;
    }
  }
  return 0;
}

export function manifestVersion(manifest: string): string {
  const section = /^\[workspace\.package\]\r?\n([\s\S]*?)(?=^\[|$(?![\s\S]))/m.exec(manifest)?.[1];
  const version = section && /^version\s*=\s*"([^"]+)"/m.exec(section)?.[1];
  if (!version) {
    throw new Error("Cargo.toml has no workspace.package.version");
  }
  versionParts(version);
  return version;
}

export function bumpManifest(manifest: string, version: string): string {
  versionParts(version);
  manifestVersion(manifest);
  let packageChanged = false;
  let pins = 0;
  const result = manifest.replace(
    /(^\[workspace\.(package|dependencies)\]\r?\n)([\s\S]*?)(?=^\[|$(?![\s\S]))/gm,
    (_whole, heading: string, kind: string, body: string) => {
      if (kind === "package") {
        packageChanged = true;
        return heading + body.replace(/^(version\s*=\s*)"[^"]+"/m, `$1"${version}"`);
      }
      return (
        heading +
        body.replace(/(version\s*=\s*")=\d+\.\d+\.\d+"/g, (_pin, prefix: string) => {
          pins += 1;
          return `${prefix}=${version}"`;
        })
      );
    },
  );
  if (!packageChanged || pins === 0) {
    throw new Error("expected workspace package version and exact dependency pins");
  }
  return result;
}

export interface Change {
  sha: string;
  subject: string;
  issues: number[];
}

export function changes(log: string): Change[] {
  const fields = log.split("\0");
  const result: Change[] = [];
  for (let index = 0; index + 2 < fields.length; index += 3) {
    const sha = fields[index]!.trim();
    if (!sha) {
      continue;
    }
    const issues = fields[index + 2]!.split("\n")
      .filter((line) => /^Fixes\s+#/i.test(line.trim()))
      .flatMap((line) => [...line.matchAll(/#(\d+)/g)].map((match) => Number(match[1])));
    result.push({ sha, subject: fields[index + 1]!, issues: [...new Set(issues)] });
  }
  return result;
}

export function section(options: {
  version: string;
  date: string;
  base: string;
  commits: Change[];
  titles: ReadonlyMap<number, string>;
  retired: { id: string; chapter: string }[];
}): string {
  const rows = [
    `## v${options.version} — ${options.date}`,
    "",
    `Changes since ${options.base}.`,
    "",
    "### Breaking",
    "",
  ];
  rows.push(
    ...(options.retired.length
      ? options.retired.map((rule) => `- Retired \`${rule.id}\` (${rule.chapter}).`)
      : ["No specification rule IDs retired."]),
  );
  const grouped = new Map<number, Change[]>();
  const other: Change[] = [];
  for (const commit of options.commits) {
    if (commit.issues.length === 0) {
      other.push(commit);
    }
    for (const issue of commit.issues) {
      const group = grouped.get(issue) ?? [];
      group.push(commit);
      grouped.set(issue, group);
    }
  }
  for (const [issue, commits] of grouped) {
    const title = options.titles.get(issue);
    if (!title) {
      throw new Error(`missing title for #${issue}`);
    }
    rows.push(
      "",
      `### [#${issue}](https://github.com/HerringtonDarkholme/hd-lang/issues/${issue}) ${title.replaceAll(/[\r\n]/g, " ")}`,
      "",
    );
    rows.push(...commits.map((commit) => `- ${commit.subject} (\`${commit.sha.slice(0, 9)}\`).`));
  }
  if (other.length) {
    rows.push(
      "",
      "### Other changes",
      "",
      ...other.map((commit) => `- ${commit.subject} (\`${commit.sha.slice(0, 9)}\`).`),
    );
  }
  return rows.join("\n") + "\n";
}

function issueTitles(
  commits: Change[],
  queryCommand: (program: string, args: string[]) => string,
): Map<number, string> {
  const issues = [...new Set(commits.flatMap((commit) => commit.issues))];
  const titles = new Map<number, string>();
  for (let offset = 0; offset < issues.length; offset += 40) {
    const batch = issues.slice(offset, offset + 40);
    const fields = batch
      .map((number) => `i${number}: issue(number: ${number}) { title }`)
      .join("\n");
    const query = `query { repository(owner: "HerringtonDarkholme", name: "hd-lang") { ${fields} } }`;
    const response = JSON.parse(queryCommand("gh", ["api", "graphql", "-f", `query=${query}`])) as {
      errors?: unknown;
      data?: { repository: Record<string, { title: string } | null> };
    };
    if (response.errors || !response.data) {
      throw new Error(`issue-title query failed: ${JSON.stringify(response.errors)}`);
    }
    for (const number of batch) {
      const title = response.data.repository[`i${number}`]?.title;
      if (!title) {
        throw new Error(`cannot read issue #${number}`);
      }
      titles.set(number, title);
    }
  }
  return titles;
}

export function cutRelease(version: string, root = process.cwd()): void {
  versionParts(version);
  const command = (program: string, args: string[], timeout = 30_000): string => {
    // cost: bounded commands over this release's range, never repository-wide builds.
    return execFileSync(program, args, {
      cwd: root,
      encoding: "utf8",
      timeout,
      maxBuffer: 1 << 27,
    });
  };
  const git = (...args: string[]): string => command("git", args);
  if (git("status", "--porcelain").trim()) {
    throw new Error("release needs a clean worktree and index");
  }
  git("symbolic-ref", "--quiet", "HEAD");
  const tags = git("tag", "--list", "v*")
    .trim()
    .split("\n")
    .filter((tag) => /^v\d+\.\d+\.\d+$/.test(tag));
  tags.sort((aa, bb) => compareVersions(bb.slice(1), aa.slice(1)));
  const base = tags[0];
  if (!base || compareVersions(version, base.slice(1)) <= 0) {
    throw new Error(`v${version} must be greater than the last version tag (${base ?? "missing"})`);
  }
  git("merge-base", "--is-ancestor", base, "HEAD");
  const commits = changes(git("log", "--reverse", "--format=%H%x00%s%x00%b%x00", `${base}..HEAD`));
  const titles = issueTitles(commits, command);
  // Use the spec tool's inventory and legacy-layout reader, including rule moves.
  const before = allRules(loadCorpusAt(root, base));
  const after = new Set(allRules(loadCorpusAt(root, "HEAD")).map((entry) => entry.rule.id));
  const retired = before
    .filter((entry) => !after.has(entry.rule.id))
    .map((entry) => ({ id: entry.rule.id, chapter: entry.chapter.name }));
  const names = ["Cargo.toml", "Cargo.lock", "CHANGELOG.md"];
  const saved = new Map(
    names.map((name) => [
      name,
      existsSync(join(root, name)) ? readFileSync(join(root, name)) : null,
    ]),
  );
  const manifest = saved.get("Cargo.toml")!.toString();
  const old = saved.get("CHANGELOG.md")?.toString() ?? "# Changelog\n";
  const notes = section({
    version,
    date: new Date().toISOString().slice(0, 10),
    base,
    commits,
    titles,
    retired,
  });
  let committed = false;
  let staged = false;
  try {
    writeFileSync(join(root, "Cargo.toml"), bumpManifest(manifest, version));
    const previous = old.replace(/^# Changelog\s*/, "");
    writeFileSync(join(root, "CHANGELOG.md"), `# Changelog\n\n${notes}\n${previous}`);
    command("cargo", ["update", "--workspace", "--offline"], 120_000);
    git("add", "--", ...names);
    staged = true;
    command("git", ["commit", "-m", `Release v${version}`], 300_000);
    committed = true;
    git("tag", "-a", `v${version}`, "-m", `Release v${version}`);
  } catch (error) {
    if (!committed) {
      if (staged) {
        git("restore", "--staged", "--", ...names);
      }
      for (const [name, data] of saved) {
        if (data !== null) {
          writeFileSync(join(root, name), data);
        } else {
          rmSync(join(root, name), { force: true });
        }
      }
    }
    throw error;
  }
  console.log(
    `Created local v${version}. Review CHANGELOG.md, then push the commit, wait for main Test to pass, and push the tag yourself. Nothing was pushed.`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  if (process.argv.length !== 3) {
    throw new Error("usage: node scripts/release.ts X.Y.Z");
  }
  cutRelease(process.argv[2]!);
}
