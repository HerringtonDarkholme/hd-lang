import { readFile, readdir, stat } from "node:fs/promises";
import { dirname, extname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const headingPattern = /^#{1,6}\s+(.+?)\s*#*\s*$/gm;
const linkPattern = /(?<!!)\[[^\]]*\]\(([^)]+)\)/g;

function githubSlug(text: string): string {
  return text
    .replaceAll(/<[^>]+>/g, "")
    .trim()
    .toLowerCase()
    .replaceAll(/[`*_~]/g, "")
    .replaceAll(/[^\p{L}\p{N}_\- ]/gu, "")
    .replaceAll(" ", "-");
}

async function anchors(path: string): Promise<Set<string>> {
  const counts = new Map<string, number>();
  const result = new Set<string>();
  const text = await readFile(path, "utf8");
  for (const match of text.matchAll(headingPattern)) {
    const base = githubSlug(match[1]!);
    const index = counts.get(base) ?? 0;
    counts.set(base, index + 1);
    result.add(index === 0 ? base : `${base}-${index}`);
  }
  return result;
}

function proseWithoutFences(text: string): string {
  return text.replaceAll(/^```.*?^```\s*$/gms, "");
}

function linkTarget(raw: string): string {
  const value = raw.trim();
  if (value.startsWith("<") && value.includes(">")) return value.slice(1, value.indexOf(">"));
  return value.split(/\s+/, 1)[0]!;
}

async function isFile(path: string): Promise<boolean> {
  try {
    return (await stat(path)).isFile();
  } catch {
    return false;
  }
}

async function markdownPaths(directory: string): Promise<string[]> {
  const paths: string[] = [];
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const path = resolve(directory, entry.name);
    if (entry.isDirectory()) paths.push(...(await markdownPaths(path)));
    else if (entry.isFile() && entry.name.endsWith(".md")) paths.push(path);
  }
  return paths.sort();
}

async function knownAnchors(path: string, cache: Map<string, Set<string>>): Promise<Set<string>> {
  const found = cache.get(path);
  if (found) return found;
  const loaded = await anchors(path);
  cache.set(path, loaded);
  return loaded;
}

async function checkReference(
  source: string,
  destination: string,
  cache: Map<string, Set<string>>,
): Promise<string | undefined> {
  if (!destination || /^(?:https?:\/\/|mailto:)/.test(destination)) return undefined;
  const [rawTarget = "", rawFragment] = destination.split("#", 2);
  const targetText = decodeURIComponent(rawTarget).split("?", 1)[0]!;
  if (targetText && ![".md", ".hd", ".tsv"].includes(extname(targetText))) return undefined;
  const target = targetText ? resolve(dirname(source), targetText) : source;
  if (!(await isFile(target))) return `${source}: missing link target ${destination}`;
  if (rawFragment) {
    const fragment = decodeURIComponent(rawFragment);
    if (!(await knownAnchors(target, cache)).has(fragment))
      return `${source}: missing link anchor ${destination}`;
  }
  return undefined;
}

async function main(args: readonly string[]): Promise<number> {
  if (args.length !== 2) {
    console.error("usage: check-spec-anchors.ts SPEC_DIR CASES_TSV");
    return 2;
  }
  const [specDirectory, casesPath] = args.map((path) => resolve(path));
  const repository = resolve(specDirectory!, "..");
  const failures: string[] = [];
  const cache = new Map<string, Set<string>>();
  const rows = (await readFile(casesPath!, "utf8")).split(/\r?\n/).slice(1).filter(Boolean);
  for (const row of rows) {
    const [fixture, , , reference = ""] = row.split("\t");
    const [fileText = "", fragment] = reference.split("#", 2);
    const target = resolve(specDirectory!, fileText);
    if (!(await isFile(target))) failures.push(`${fixture}: missing specification ${reference}`);
    else if (fragment && !(await knownAnchors(target, cache)).has(fragment))
      failures.push(`${fixture}: missing specification anchor ${reference}`);
  }
  const directories = [
    specDirectory!,
    resolve(repository, "guide"),
    resolve(repository, "future-work"),
  ];
  const sources = (await Promise.all(directories.map(markdownPaths))).flat();
  for (const source of sources) {
    const prose = proseWithoutFences(await readFile(source, "utf8"));
    for (const match of prose.matchAll(linkPattern)) {
      const failure = await checkReference(source, linkTarget(match[1]!), cache);
      if (failure) failures.push(failure);
    }
  }
  if (failures.length > 0) {
    console.error(
      `unresolved specification links:\n${failures.map((failure) => `- ${failure}`).join("\n")}`,
    );
    return 1;
  }
  console.log("specification links and anchors passed");
  return 0;
}

const invokedPath = process.argv[1] ? resolve(process.argv[1]) : "";
if (invokedPath === fileURLToPath(import.meta.url))
  process.exitCode = await main(process.argv.slice(2));
