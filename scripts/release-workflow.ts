// The tag-only release workflow's gate, native packaging and publication.
// Cost: a fixed five-asset set, one bounded Test-run query, streamed hashes;
// no build is done by this script. Native builds are in release.yml.
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import {
  appendFileSync,
  copyFileSync,
  createReadStream,
  existsSync,
  mkdirSync,
  readFileSync,
  writeFileSync,
} from "node:fs";
import { basename, join, resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { manifestVersion } from "./release.ts";

export const targets = [
  "x86_64-unknown-linux-gnu",
  "aarch64-unknown-linux-gnu",
  "x86_64-apple-darwin",
  "aarch64-apple-darwin",
  "x86_64-pc-windows-msvc",
] as const;

const command = (program: string, args: string[], timeout = 60_000): string => {
  // cost: one bounded child for a release operation, never on ordinary hd runs.
  return execFileSync(program, args, { encoding: "utf8", timeout, maxBuffer: 1 << 24 });
};
const git = (...args: string[]): string => command("git", args).trim();
const gh = (...args: string[]): string => command("gh", args);
export const archiveName = (target: string): string =>
  `hd-${target}.${target.includes("windows") ? "zip" : "tar.gz"}`;

export function releaseNotes(changelog: string, version: string): string {
  const escaped = version.replaceAll(".", "\\.");
  const found = new RegExp(
    `^## v${escaped}(?:[ \t].*)?\\n([\\s\\S]*?)(?=^## |$(?![\\s\\S]))`,
    "m",
  ).exec(changelog);
  if (!found) {
    throw new Error(`CHANGELOG.md has no v${version} section`);
  }
  return found[0].trim() + "\n";
}

export interface TestRun {
  id: number;
  head_sha: string;
  conclusion: string | null;
  status: string;
}

export function greenMainRun(runs: TestRun[], sha: string): TestRun {
  const run = runs.filter((entry) => entry.head_sha === sha).sort((aa, bb) => bb.id - aa.id)[0];
  if (!run || run.status !== "completed" || run.conclusion !== "success") {
    throw new Error(
      `main's latest Test run at ${sha} must be green; rerun Release after it passes`,
    );
  }
  return run;
}

export function gate(): void {
  const version = manifestVersion(readFileSync("Cargo.toml", "utf8"));
  const sha = git("rev-parse", "HEAD");
  const dry = process.env.GITHUB_EVENT_NAME === "workflow_dispatch";
  if (!dry && process.env.GITHUB_REF_NAME !== `v${version}`) {
    throw new Error(`tag must be v${version}, matching Cargo.toml`);
  }
  if (!dry && git("rev-parse", `v${version}^{commit}`) !== sha) {
    throw new Error("tag does not identify the checked-out commit");
  }
  const repo = process.env.GITHUB_REPOSITORY;
  if (!repo) {
    throw new Error("GITHUB_REPOSITORY is required");
  }
  const response = JSON.parse(
    gh(
      "api",
      `repos/${repo}/actions/workflows/test.yml/runs?branch=main&event=push&head_sha=${sha}&per_page=100`,
    ),
  ) as {
    workflow_runs: TestRun[];
  };
  const run = greenMainRun(response.workflow_runs, sha);
  const tag = dry ? `dry-run-v${version}-${process.env.GITHUB_RUN_ID}` : `v${version}`;
  if (!process.env.GITHUB_OUTPUT) {
    throw new Error("GITHUB_OUTPUT is required");
  }
  appendFileSync(
    process.env.GITHUB_OUTPUT,
    `version=${version}\nsha=${sha}\ntag=${tag}\ndry=${dry}\n`,
  );
  console.log(`Release ${tag} from ${sha}; Test run ${run.id} passed.`);
}

export async function checksum(file: string): Promise<string> {
  const hash = createHash("sha256");
  for await (const chunk of createReadStream(file)) {
    hash.update(chunk);
  }
  return hash.digest("hex");
}

export async function pack(target: string): Promise<void> {
  if (!targets.includes(target as (typeof targets)[number])) {
    throw new Error(`unknown release target: ${target}`);
  }
  const host = /^host: (.+)$/m.exec(command("rustc", ["-vV"]))?.[1];
  if (host !== target) {
    throw new Error(`native runner required: ${host} != ${target}`);
  }
  const version = manifestVersion(readFileSync("Cargo.toml", "utf8"));
  const work = resolve("target/release-assets");
  const bundle = join(work, target);
  mkdirSync(bundle, { recursive: true });
  const binary = target.includes("windows") ? "hd.exe" : "hd";
  copyFileSync(join("target", target, "dist", binary), join(bundle, binary));
  for (const name of [".node-version"]) {
    copyFileSync(name, join(bundle, name));
  }
  const archive = join(work, archiveName(target));
  const options = target.includes("windows") ? ["-a", "-cf"] : ["-czf"];
  command("tar", [...options, archive, "-C", bundle, binary, ".node-version"]);
  const unpacked = join(work, `smoke-${target}`);
  mkdirSync(unpacked, { recursive: true });
  command("tar", ["-xf", archive, "-C", unpacked]);
  const executable = join(unpacked, binary);
  const shown = command(executable, ["--version"]).trim();
  if (shown !== `hd ${version}`) {
    throw new Error(`wrong binary version: ${shown} (expected ${version})`);
  }
  const source = join(unpacked, "hello.hd");
  writeFileSync(source, 'pub fn main() -> void $ Console:\n    println("hello from hd")\n');
  const output = command(executable, ["--engine", "wasmtime", "--time-limit", "15s", source]);
  if (output.replaceAll("\r\n", "\n") !== "hello from hd\n") {
    throw new Error(`archive did not run hello-world: ${JSON.stringify(output)}`);
  }
  writeFileSync(`${archive}.sha256`, `${await checksum(archive)}  ${basename(archive)}\n`);
  console.log(`${target}: unpacked archive passed --version and hello-world on its native runner`);
}

function releaseByTag(repo: string, tag: string): { draft: boolean } | undefined {
  const [owner, name] = repo.split("/");
  if (!owner || !name) {
    throw new Error("expected owner/repository");
  }
  // Like gh's FetchRelease, use release(tagName:) for draft pending tags:
  // REST's /releases/tags endpoint returns only published releases.
  const query = `query($owner:String!,$name:String!,$tag:String!) {
    repository(owner:$owner,name:$name) { release(tagName:$tag) { isDraft } }
  }`;
  const response = JSON.parse(
    gh(
      "api",
      "graphql",
      "-f",
      `query=${query}`,
      "-f",
      `owner=${owner}`,
      "-f",
      `name=${name}`,
      "-f",
      `tag=${tag}`,
    ),
  ) as {
    errors?: unknown;
    data?: { repository: { release: { isDraft: boolean } | null } | null };
  };
  if (response.errors || !response.data?.repository) {
    throw new Error(`release lookup failed: ${JSON.stringify(response.errors)}`);
  }
  const release = response.data.repository.release;
  return release ? { draft: release.isDraft } : undefined;
}

async function verifiedAssets(directory: string): Promise<string[]> {
  const sums: string[] = [];
  const files: string[] = [];
  for (const target of targets) {
    const file = join(directory, archiveName(target));
    const actual = `${await checksum(file)}  ${basename(file)}\n`;
    if (readFileSync(`${file}.sha256`, "utf8") !== actual) {
      throw new Error(`artifact checksum mismatch: ${file}`);
    }
    sums.push(actual);
    files.push(file);
  }
  const sumFile = join(directory, "SHA256SUMS");
  writeFileSync(sumFile, sums.join(""));
  files.push(sumFile);
  return files;
}

// GitHub's release-body API limit; full text is attached, never truncated.
// https://github.com/cli/cli/issues/7705
const GITHUB_RELEASE_BODY_LIMIT = 125_000;

export async function writeReleaseNotes(options: {
  directory: string;
  version: string;
  repo: string;
  completeNotes: string;
}): Promise<string[]> {
  const notes = join(options.directory, "notes.md");
  if (options.completeNotes.length <= GITHUB_RELEASE_BODY_LIMIT) {
    writeFileSync(notes, options.completeNotes);
    return [];
  }
  const name = `CHANGELOG-v${options.version}.md`;
  const full = join(options.directory, name);
  writeFileSync(full, options.completeNotes);
  appendFileSync(join(options.directory, "SHA256SUMS"), `${await checksum(full)}  ${name}\n`);
  const url = `https://github.com/${options.repo}/releases/download/v${options.version}/${name}`;
  const commit = options.completeNotes.split("\n")[0];
  writeFileSync(
    notes,
    `${commit}\n\nThe complete changelog exceeds GitHub's release-body limit.\n\n[Read the complete release notes](${url}) (also verified in SHA256SUMS).\n`,
  );
  return [full];
}

export async function publish(): Promise<void> {
  const directory = resolve("target/release-download");
  const version = manifestVersion(readFileSync("Cargo.toml", "utf8"));
  const dry = process.env.GITHUB_EVENT_NAME === "workflow_dispatch";
  const tag = process.env.RELEASE_TAG;
  if (!tag || (!dry && tag !== `v${version}`)) {
    throw new Error("RELEASE_TAG does not match this release");
  }
  const files = await verifiedAssets(directory);
  const notes = join(directory, "notes.md");
  const heading = `Commit: ${git("rev-parse", "HEAD")}\n\n`;
  const changelog = existsSync("CHANGELOG.md") ? readFileSync("CHANGELOG.md", "utf8") : "";
  const completeNotes =
    heading +
    (dry
      ? "Draft validation of the tag release path; crates.io publishing is disabled.\n"
      : releaseNotes(changelog, version));
  const noteFiles = await writeReleaseNotes({
    directory,
    version,
    repo: process.env.GITHUB_REPOSITORY ?? "",
    completeNotes,
  });
  files.push(...noteFiles);
  const repo = process.env.GITHUB_REPOSITORY;
  if (!repo) {
    throw new Error("GITHUB_REPOSITORY is required");
  }
  const prior = releaseByTag(repo, tag);
  if (prior && dry && !prior.draft) {
    throw new Error("refusing to replace a published release with a dry run");
  }
  if (!prior) {
    gh(
      "release",
      "create",
      tag,
      "--draft",
      "--target",
      git("rev-parse", "HEAD"),
      "--title",
      tag,
      "--notes-file",
      notes,
    );
  }
  gh("release", "upload", tag, ...files, "--clobber");
  const edit = ["release", "edit", tag, "--title", tag, "--notes-file", notes];
  if (!dry) {
    edit.push("--latest");
  }
  gh(...edit, dry ? "--draft=true" : "--draft=false");
  console.log(
    `${dry ? "Draft" : "Release"} ${tag}: five native-tested archives and SHA256SUMS uploaded.`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(resolve(process.argv[1])).href) {
  switch (process.argv[2]) {
    case "gate":
      gate();
      break;
    case "pack":
      await pack(process.argv[3] ?? "");
      break;
    case "publish":
      await publish();
      break;
    default:
      throw new Error("usage: release-workflow.ts gate|pack TARGET|publish");
  }
}
