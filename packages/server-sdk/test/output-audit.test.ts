import assert from "node:assert/strict";
import { test } from "node:test";
import fs from "node:fs/promises";
import { syncBuiltinESMExports } from "node:module";
import { fileURLToPath } from "node:url";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { spawnSync } from "node:child_process";
import {
  publishCloudflare,
  publishVercelNode,
  auditCloudflareOutput,
  auditVercelNodeOutput,
} from "../packaging/output.ts";

const profiles = [
  {
    platform: "cloudflare",
    publish: publishCloudflare,
    audit: auditCloudflareOutput,
    store: "generations",
    selector: "wrangler.json",
    entry: "entry.js",
    configuration: { name: "example", compatibility_date: "2026-10-23" },
  },
  {
    platform: "vercel-node",
    publish: publishVercelNode,
    audit: auditVercelNodeOutput,
    store: "functions",
    selector: "config.json",
    entry: "index.js",
    configuration: { name: "example" },
  },
];
async function fixture(run) {
  const root = await fs.mkdtemp(join(tmpdir(), "hd-sdk-audit-"));
  try {
    await run(join(root, "output"));
  } finally {
    await fs.rm(root, { recursive: true, force: true });
  }
}
const input = (profile, output, version) => ({
  outputDirectory: output,
  configuration: profile.configuration,
  files: new Map([
    [profile.entry, Buffer.from("export default " + JSON.stringify(version))],
    ["sdk/value.js", Buffer.from(version)],
  ]),
});
async function activeDirectory(profile, output) {
  const selected = JSON.parse(await fs.readFile(join(output, profile.selector), "utf8"));
  const name =
    profile.platform === "cloudflare"
      ? selected.main.split("/")[1]
      : selected.routes[0].dest.slice(1) + ".func";
  return join(output, profile.store, name);
}

// Observe real filesystem work, including hashed bytes, without a publisher hook.
async function countWork(operation, store) {
  const originals = new Map();
  const count = { operations: 0, entries: 0, bytes: 0 };
  for (const name of ["lstat", "readFile", "readdir"]) {
    const original = fs[name];
    originals.set(name, original);
    fs[name] = async (...args) => {
      count.operations++;
      if (name === "readdir") {
        assert.notEqual(String(args[0]), store, "publish never enumerates retained generations");
      }
      const result = await original(...args);
      if (name === "readdir") {
        count.entries += result.length;
      } else if (name === "readFile") {
        count.bytes += Buffer.byteLength(result);
      }
      return result;
    };
  }
  syncBuiltinESMExports();
  try {
    await operation();
    return count;
  } finally {
    for (const [name, original] of originals) {
      fs[name] = original;
    }
    syncBuiltinESMExports();
  }
}

for (const profile of profiles) {
  test(`${profile.platform}: publish work stays flat at 10 and 100 retained generations`, async (context) => {
    const counts = [];
    for (const retained of [10, 100]) {
      await fixture(async (output) => {
        for (let index = 0; index < retained; index++) {
          await profile.publish(input(profile, output, String(index).padStart(4, "0")));
        }
        const store = join(output, profile.store);
        const older = await countWork(() => profile.publish(input(profile, output, "0000")), store);
        const fresh = await countWork(() => profile.publish(input(profile, output, "next")), store);
        counts.push({ older, fresh });
        assert.equal(
          (await fs.readdir(store)).length,
          retained + 1,
          "all old generations retained",
        );
        assert.equal((await profile.audit(output)).generations, retained + 1);
      });
    }
    assert.deepEqual(counts[0], counts[1]);
    assert.ok(
      counts[0].older.operations > 0 && counts[0].older.entries > 0 && counts[0].older.bytes > 0,
    );
    context.diagnostic(JSON.stringify({ retained: [10, 100], counts }));
  });

  test(`${profile.platform}: inactive damage waits for audit, and a damaged destination cannot activate`, () =>
    fixture(async (output) => {
      await profile.publish(input(profile, output, "old"));
      const older = await activeDirectory(profile, output);
      await profile.publish(input(profile, output, "current"));
      await fs.writeFile(join(older, profile.entry), "changed retained bytes");
      await profile.publish(input(profile, output, "next"));
      const before = await fs.readFile(join(output, profile.selector), "utf8");
      await assert.rejects(profile.audit(output), /added or changed/);
      await assert.rejects(profile.publish(input(profile, output, "old")), /added or changed/);
      assert.equal(await fs.readFile(join(output, profile.selector), "utf8"), before);
      assert.equal(await fs.readFile(join(older, profile.entry), "utf8"), "changed retained bytes");
    }));

  test(`${profile.platform}: unknown inactive directories are preserved by publish and rejected by audit`, () =>
    fixture(async (output) => {
      await profile.publish(input(profile, output, "old"));
      const unrelated = join(output, profile.store, "unrelated");
      await fs.mkdir(unrelated);
      await profile.publish(input(profile, output, "next"));
      await assert.rejects(profile.audit(output), /unrecognized generation/);
      assert.deepEqual(await fs.readdir(unrelated), []);
      await fs.rm(unrelated, { recursive: true });
      assert.equal((await profile.audit(output)).generations, 2);
    }));

  test(`${profile.platform}: selector paths and canonical configuration are validated before activation`, () =>
    fixture(async (output) => {
      await profile.publish(input(profile, output, "old"));
      const selector = join(output, profile.selector);
      const before = await fs.readFile(selector, "utf8");
      for (const kind of ["escape", "extra", "missing"]) {
        const value = JSON.parse(before);
        if (kind === "extra") {
          value.unexpected = true;
        } else if (profile.platform === "cloudflare") {
          value.main = kind === "escape" ? "generations/../../outside/entry.js" : null;
        } else {
          value.routes[0].dest = kind === "escape" ? "/../../outside" : null;
        }
        const changed = JSON.stringify(value, null, 2) + "\n";
        await fs.writeFile(selector, changed);
        await assert.rejects(profile.audit(output), /configuration was changed/);
        await assert.rejects(
          profile.publish(input(profile, output, "next")),
          /configuration was changed/,
        );
        assert.equal(await fs.readFile(selector, "utf8"), changed);
      }
    }));

  test(`${profile.platform}: an inactive symlink is left alone until audit or destination validation`, () =>
    fixture(async (output) => {
      await profile.publish(input(profile, output, "old"));
      const older = await activeDirectory(profile, output);
      await profile.publish(input(profile, output, "current"));
      const outside = output + "-outside";
      await fs.mkdir(outside);
      await fs.writeFile(join(outside, "sentinel"), "keep");
      await fs.rm(older, { recursive: true });
      await fs.symlink(outside, older, "dir");
      await profile.publish(input(profile, output, "next"));
      await assert.rejects(profile.audit(output), /invalid generation directory/);
      await assert.rejects(
        profile.publish(input(profile, output, "old")),
        /invalid generation directory/,
      );
      assert.equal(await fs.readFile(join(outside, "sentinel"), "utf8"), "keep");
    }));

  test(`${profile.platform}: audit command checks retained output without changing it`, () =>
    fixture(async (output) => {
      await profile.publish(input(profile, output, "old"));
      const before = await fs.readFile(join(output, profile.selector), "utf8");
      const command = new URL("../packaging/audit-output.ts", import.meta.url);
      // cost: two explicit audit processes per platform fixture, independent of retained count.
      const run = () =>
        spawnSync(
          process.execPath,
          ["--experimental-strip-types", fileURLToPath(command), profile.platform, output],
          { encoding: "utf8" },
        );
      const good = run();
      assert.equal(good.status, 0, good.stderr);
      assert.match(good.stdout, /audited 1 generations/);
      await fs.mkdir(join(output, profile.store, "unknown"));
      const bad = run();
      assert.equal(bad.status, 1);
      assert.match(bad.stderr, /unrecognized generation/);
      assert.equal(await fs.readFile(join(output, profile.selector), "utf8"), before);
    }));
}
