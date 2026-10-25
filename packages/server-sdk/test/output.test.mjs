import assert from "node:assert/strict";
import { test } from "node:test";
import { mkdtemp, rm, readFile, writeFile, mkdir, symlink, readdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { publishCloudflare } from "../packaging/output.mjs";
const fixture = async (run) => {
  const root = await mkdtemp(join(tmpdir(), "hd-sdk-owned-output-"));
  try {
    await run(join(root, "output"));
  } finally {
    await rm(root, { recursive: true, force: true });
  }
};
const input = (output, version = "one") => ({
  outputDirectory: output,
  files: new Map([
    ["entry.mjs", Buffer.from("export default " + JSON.stringify(version) + ";\n")],
    ["sdk/module.mjs", Buffer.from("export const value=" + JSON.stringify(version) + ";\n")],
    ["app.wasm", new Uint8Array([0, 97, 115, 109, 1, 0, 0, 0])],
  ]),
  configuration: { name: "example", compatibility_date: "2026-10-23" },
});
const selected = async (output) =>
  JSON.parse(await readFile(join(output, "wrangler.json"), "utf8"));

test("config publication selects complete generations and keeps an old reader valid", () =>
  fixture(async (output) => {
    const first = await publishCloudflare(input(output));
    const config = await selected(output),
      old = await readFile(join(output, config.main), "utf8");
    assert.equal(old, 'export default "one";\n');
    const second = await publishCloudflare(input(output, "two"));
    assert.notEqual(first.generation, second.generation);
    const next = await selected(output);
    assert.notEqual(next.main, config.main);
    assert.equal(await readFile(join(output, next.main), "utf8"), 'export default "two";\n');
    assert.equal(await readFile(join(output, config.main), "utf8"), old);
    assert.equal((await readdir(join(output, "generations"))).length, 2);
    assert.equal((await publishCloudflare(input(output, "two"))).generation, second.generation);
    assert.equal((await readdir(join(output, "generations"))).length, 2);
  }));

test("preexisting unowned output is preserved, including an empty directory", () =>
  fixture(async (output) => {
    await mkdir(output);
    await assert.rejects(publishCloudflare(input(output)));
    assert.deepEqual(await readdir(output), []);
    await writeFile(join(output, "sentinel"), "unrelated");
    await assert.rejects(publishCloudflare(input(output)));
    assert.equal(await readFile(join(output, "sentinel"), "utf8"), "unrelated");
  }));

test("changed payloads, added files and changed config refuse replacement", async () => {
  for (const kind of ["payload", "extra", "config"])
    await fixture(async (output) => {
      await publishCloudflare(input(output));
      const before = await readFile(join(output, "wrangler.json"), "utf8");
      const config = JSON.parse(before),
        base = join(output, "generations", config.main.split("/")[1]);
      const target =
        kind === "payload"
          ? join(base, "entry.mjs")
          : kind === "extra"
            ? join(base, "unexpected")
            : kind === "config"
              ? join(output, "wrangler.json")
              : join(output, "unexpected");
      await writeFile(target, "user-owned change");
      await assert.rejects(publishCloudflare(input(output, "two")));
      assert.equal(await readFile(target, "utf8"), "user-owned change");
      if (kind !== "config")
        assert.equal(await readFile(join(output, "wrangler.json"), "utf8"), before);
    });
});

test("platform caches and unrelated root files survive subsequent publication", () =>
  fixture(async (output) => {
    await publishCloudflare(input(output));
    await mkdir(join(output, ".wrangler"));
    await writeFile(join(output, ".wrangler", "cache"), "platform cache");
    await writeFile(join(output, "notes.txt"), "user notes");
    await publishCloudflare(input(output, "two"));
    assert.equal(await readFile(join(output, ".wrangler", "cache"), "utf8"), "platform cache");
    assert.equal(await readFile(join(output, "notes.txt"), "utf8"), "user notes");
  }));

test("symlinked output, payload, generation store and marker are refused", async () => {
  for (const kind of ["root", "payload", "store", "marker"])
    await fixture(async (output) => {
      const outside = output + "-unrelated";
      await mkdir(outside);
      await writeFile(join(outside, "sentinel"), "keep");
      if (kind === "root") await symlink(outside, output, "dir");
      else {
        await publishCloudflare(input(output));
        const config = await selected(output);
        const target =
          kind === "payload"
            ? join(output, config.main)
            : kind === "store"
              ? join(output, "generations")
              : join(output, ".hd-server-sdk.json");
        await rm(target, { recursive: true });
        await symlink(
          kind === "marker" ? join(outside, "sentinel") : outside,
          target,
          kind === "marker" ? "file" : "dir",
        );
      }
      await assert.rejects(publishCloudflare(input(output, "two")));
      assert.equal(await readFile(join(outside, "sentinel"), "utf8"), "keep");
    });
});

test("invalid captured paths and missing entries never create output", async () => {
  for (const name of [
    "../outside",
    "/outside",
    "a/../b",
    "a\\b",
    "C:/outside",
    ".hd-generation.json",
    "a//b",
  ])
    await fixture(async (output) => {
      const options = input(output);
      options.files.set(name, new Uint8Array([1]));
      await assert.rejects(publishCloudflare(options));
      assert.equal(existsSync(output), false);
    });
  await fixture(async (output) => {
    const options = input(output);
    options.files.delete("entry.mjs");
    await assert.rejects(publishCloudflare(options));
    assert.equal(existsSync(output), false);
  });
});

test("cancelled writes keep the old configuration and a reusable complete store", () =>
  fixture(async (output) => {
    await publishCloudflare(input(output));
    const before = await readFile(join(output, "wrangler.json"), "utf8");
    const controller = new AbortController();
    controller.abort();
    await assert.rejects(publishCloudflare({ ...input(output, "two"), signal: controller.signal }));
    assert.equal(await readFile(join(output, "wrangler.json"), "utf8"), before);
    const next = new AbortController();
    const check = next.signal.throwIfAborted.bind(next.signal);
    next.signal.throwIfAborted = () => {
      if (existsSync(join(output, "generations"))) {
        const names = JSON.parse(readFileSyncCompat(join(output, ".hd-server-sdk.json")));
        assert.equal(names.platform, "cloudflare");
      }
      if (
        existsSync(output + ".hd-lock") &&
        readdirSyncCompat(join(output, "generations")).length === 2
      )
        next.abort(new Error("cancel after complete inactive generation"));
      check();
    };
    await assert.rejects(
      publishCloudflare({ ...input(output, "two"), signal: next.signal }),
      /cancel after complete/,
    );
    assert.equal(await readFile(join(output, "wrangler.json"), "utf8"), before);
    await publishCloudflare(input(output, "two"));
    assert.notEqual((await selected(output)).main, JSON.parse(before).main);
  }));
import { readFileSync as readFileSyncCompat, readdirSync as readdirSyncCompat } from "node:fs";

test("literal __proto__ files are included in generation integrity checks", () =>
  fixture(async (output) => {
    await publishCloudflare(input(output));
    const config = await selected(output);
    const file = join(output, "generations", config.main.split("/")[1], "__proto__");
    await writeFile(file, "unrelated literal file");
    await assert.rejects(publishCloudflare(input(output, "two")), /added or changed/);
    assert.equal(await readFile(file, "utf8"), "unrelated literal file");
  }));

test("a concurrently created unowned empty output is never claimed by rename", () =>
  fixture(async (output) => {
    let created = false;
    const signal = {
      throwIfAborted() {
        if (!created && existsSync(output + ".hd-lock")) {
          mkdirSyncCompat(output);
          created = true;
        }
      },
    };
    await assert.rejects(publishCloudflare({ ...input(output), signal }));
    assert.equal(created, true);
    assert.deepEqual(await readdir(output), []);
  }));
import { mkdirSync as mkdirSyncCompat } from "node:fs";

test("an interrupted fresh claim retries without selecting partial output", () =>
  fixture(async (output) => {
    const signal = {
      throwIfAborted() {
        if (existsSync(join(output, "generations"))) throw new Error("stop before first selector");
      },
    };
    await assert.rejects(
      publishCloudflare({ ...input(output), signal }),
      /stop before first selector/,
    );
    assert.equal(existsSync(join(output, "wrangler.json")), false);
    assert.equal(existsSync(join(output, ".hd-server-sdk.json")), true);
    await publishCloudflare(input(output));
    const config = await selected(output);
    assert.equal(await readFile(join(output, config.main), "utf8"), 'export default "one";\n');
  }));

test("unrelated empty directories in immutable generations are preserved and refused", () =>
  fixture(async (output) => {
    await publishCloudflare(input(output));
    const config = await selected(output);
    const directory = join(output, "generations", config.main.split("/")[1], "unrelated-empty");
    await mkdir(directory);
    await assert.rejects(publishCloudflare(input(output, "two")), /unrelated directory/);
    assert.deepEqual(await readdir(directory), []);
  }));
