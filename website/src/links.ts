import { existsSync, statSync } from "node:fs";
import { readFile } from "node:fs/promises";
import { join } from "node:path";

const ATTRIBUTE = /\s(?:href|src)="([^"]*)"/g;
const ID = /\sid="([^"]*)"/g;
const EXTERNAL = /^(?:[a-z][a-z0-9+.-]*:|\/\/)/i;

function decodeEntities(value: string): string {
  return value
    .replaceAll("&quot;", '"')
    .replaceAll("&lt;", "<")
    .replaceAll("&gt;", ">")
    .replaceAll("&amp;", "&");
}

/** Maps a site path below the base to the file that serves it. */
function servedFile(outDir: string, sitePath: string): string {
  const path = decodeURIComponent(sitePath);
  const direct = join(outDir, path);
  if (path === "" || path.endsWith("/")) return join(direct, "index.html");
  if (existsSync(direct) && statSync(direct).isDirectory()) return join(direct, "index.html");
  return direct;
}

/**
 * Checks every `href` and `src` in `pages` (paths relative to `outDir`).
 * Internal URLs must start with `base`, name a file in `outDir`, and, when
 * they carry a fragment, name an element id in that file. A fragment that
 * starts with `code=` is playground state, not an anchor.
 */
export async function checkLinks(
  outDir: string,
  base: string,
  pages: readonly string[],
): Promise<string[]> {
  const idCache = new Map<string, Set<string>>();
  const ids = async (file: string): Promise<Set<string>> => {
    let found = idCache.get(file);
    if (!found) {
      const html = await readFile(file, "utf8");
      found = new Set([...html.matchAll(ID)].map((match) => decodeEntities(match[1]!)));
      idCache.set(file, found);
    }
    return found;
  };
  const failures: string[] = [];
  for (const page of pages) {
    const pageFile = join(outDir, page);
    const html = await readFile(pageFile, "utf8");
    for (const match of html.matchAll(ATTRIBUTE)) {
      const url = decodeEntities(match[1]!);
      if (EXTERNAL.test(url)) continue;
      const hash = url.indexOf("#");
      const path = hash < 0 ? url : url.slice(0, hash);
      const fragment = hash < 0 ? "" : decodeURIComponent(url.slice(hash + 1));
      let target = pageFile;
      if (path !== "") {
        if (!path.startsWith(base)) {
          failures.push(`${page}: ${url} is not under the site base ${base}`);
          continue;
        }
        target = servedFile(outDir, path.slice(base.length));
        if (!existsSync(target)) {
          failures.push(`${page}: ${url} does not resolve to a generated file`);
          continue;
        }
      }
      if (fragment === "" || fragment.startsWith("code=") || !target.endsWith(".html")) continue;
      if (!(await ids(target)).has(fragment))
        failures.push(`${page}: ${url} names a missing anchor #${fragment}`);
    }
  }
  return failures;
}
