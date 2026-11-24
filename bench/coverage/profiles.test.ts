import assert from "node:assert/strict";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { archiveProfiles, restoreProfiles } from "./profiles.ts";

test("each set is isolated, its profdata retained, and the union restored exactly", () => {
  const root = mkdtempSync(join(tmpdir(), "hd-coverage-profiles-"));
  try {
    const target = join(root, "target");
    mkdirSync(join(target, "release"), { recursive: true });
    const restore: [string, string][] = [];
    for (const set of ["core", "checked"]) {
      const directory = join(root, set);
      mkdirSync(directory);
      writeFileSync(join(target, `${set}.profraw`), set);
      writeFileSync(join(target, "workspace.profdata"), set);
      const moved = archiveProfiles(target, directory);
      assert.equal(moved.length, 1);
      restore.push(...moved);
      assert.equal(existsSync(join(target, `${set}.profraw`)), false);
      assert.equal(readFileSync(join(directory, "workspace.profdata"), "utf8"), set);
      assert.equal(existsSync(join(target, "release")), true);
    }
    restoreProfiles(restore);
    for (const set of ["core", "checked"]) {
      assert.equal(readFileSync(join(target, `${set}.profraw`), "utf8"), set);
      assert.equal(readFileSync(join(root, set, "workspace.profdata"), "utf8"), set);
    }
    const empty = join(root, "empty");
    mkdirSync(empty);
    assert.throws(() => archiveProfiles(empty, root), /no raw profiles/);
  } finally {
    rmSync(root, { recursive: true, force: true });
  }
});
