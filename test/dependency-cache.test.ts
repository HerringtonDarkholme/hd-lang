// Publishing a fetched version into the shared cache
// (spec/cli/command-line.md#cache): several `hd` processes may store one
// version at once (cli.cache.shared), and the hash record beside an entry
// must always be the hash of the tree that is in place (cli.cache.hash).
// The cache is a temporary directory; no git and no network.

import assert from "node:assert/strict";
import { mkdir, mkdtemp, readFile, rename, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { after, test } from "node:test";

import { cachedEntry, removeTree, storeEntry } from "../src/dependencies/cache.ts";
import { treeHash } from "../src/dependencies/sum.ts";

const HOST_PATH = "github.com/acme/text";
const VERSION = "1.0.0";

const base = await mkdtemp(join(tmpdir(), "hd-cache-race-"));
after(() => removeTree(base));

/** A package tree whose library returns `word`, so two trees hash differently. */
async function source(name: string, word: string): Promise<string> {
  const directory = join(base, name);
  for (const [path, text] of Object.entries({
    "hd.toml": '[package]\nname = "text"\n',
    "src/lib.hd": `pub fn word() -> string: "${word}"\n`,
  })) {
    await mkdir(dirname(join(directory, path)), { recursive: true });
    await writeFile(join(directory, path), text);
  }
  return directory;
}

const entryDirectory = (cache: string): string =>
  join(cache, "pkg", ...`${HOST_PATH}@${VERSION}`.split("/"));
const hashRecord = (cache: string): string =>
  join(cache, "hash", ...`${HOST_PATH}@${VERSION}`.split("/"));

test("two concurrent stores of one version publish one tree with that tree's hash", async () => {
  const left = await source("left", "left");
  const right = await source("right", "right");
  // Each round is a fresh cache, so the two stores race from the start.
  for (let round = 0; round < 12; round += 1) {
    const cache = join(base, `cache-${round}`);
    const results = await Promise.allSettled([
      storeEntry(cache, HOST_PATH, VERSION, left),
      storeEntry(cache, HOST_PATH, VERSION, right),
    ]);
    const placed = await treeHash(entryDirectory(cache));
    assert.equal((await readFile(hashRecord(cache), "utf8")).trim(), placed);
    for (const result of results) {
      if (result.status === "rejected") assert.fail(`round ${round}: ${String(result.reason)}`);
      assert.equal(result.value.directory, entryDirectory(cache));
      assert.equal(result.value.hash, placed, `round ${round}: a store reports another hash`);
    }
    assert.deepEqual(await cachedEntry(cache, HOST_PATH, VERSION), {
      directory: entryDirectory(cache),
      hash: placed,
    });
  }
});

test("a store that finds a placed tree without its record records that tree's hash", async () => {
  const cache = join(base, "cache-unrecorded");
  const placedSource = await source("placed", "placed");
  await storeEntry(cache, HOST_PATH, VERSION, placedSource);
  const placed = await treeHash(entryDirectory(cache));
  // As after a crash between placing the tree and writing its record.
  await rename(hashRecord(cache), join(base, "lost-record"));
  assert.equal(await cachedEntry(cache, HOST_PATH, VERSION), undefined);
  const entry = await storeEntry(cache, HOST_PATH, VERSION, await source("later", "later"));
  assert.equal(entry.hash, placed);
  assert.equal((await readFile(hashRecord(cache), "utf8")).trim(), placed);
});
