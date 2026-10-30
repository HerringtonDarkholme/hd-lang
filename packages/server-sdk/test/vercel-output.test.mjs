import assert from "node:assert/strict";
import { test } from "node:test";
import { existsSync, readdirSync } from "node:fs";
import { mkdtemp, mkdir, readFile, writeFile, readdir, rm, symlink } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { publishVercelNode, publishCloudflare } from "../packaging/output.mjs";

async function fixture(run) {
  const root = await mkdtemp(join(tmpdir(), "hd-vercel-output-"));
  try {
    await run(join(root, "output"), root);
  } finally {
    await rm(root, { recursive: true, force: true });
  }
}
const input = (output, version = "one") => ({
  outputDirectory: output,
  configuration: { name: "api" },
  files: new Map([
    ["index.mjs", Buffer.from("export default " + JSON.stringify(version) + ";\n")],
    ["sdk/value.mjs", Buffer.from(version)],
    [".vc-config.json", Buffer.from('{"runtime":"nodejs24.x","handler":"index.mjs"}\n')],
  ]),
});
const selected = async (output) => JSON.parse(await readFile(join(output, "config.json"), "utf8"));
const active = (output, config) =>
  join(output, "functions", config.routes[0].dest.slice(1) + ".func");

test("Vercel v3 selector names complete immutable functions and keeps old readers and platform caches", () =>
  fixture(async (output) => {
    const first = await publishVercelNode(input(output)),
      old = await selected(output);
    const record = JSON.parse(
      await readFile(join(active(output, old), ".hd-generation.json"), "utf8"),
    );
    assert.equal(record.generation, first.generation);
    assert.match(record.generation, /^[0-9a-f]{64}$/);
    assert.deepEqual(old, {
      version: 3,
      routes: [
        {
          src: "/(.*)",
          dest: `/api-${first.generation.slice(0, 48)}`,
          transforms: [{ type: "request.path", op: "set", args: "/$1" }],
        },
      ],
    });
    await mkdir(join(output, ".vercel-cache"));
    await writeFile(join(output, ".vercel-cache/private"), "platform cache");
    await writeFile(join(output, "notes.txt"), "user notes");
    const second = await publishVercelNode(input(output, "two")),
      next = await selected(output);
    assert.notEqual(first.generation, second.generation);
    assert.equal(
      await readFile(join(active(output, old), "index.mjs"), "utf8"),
      'export default "one";\n',
    );
    assert.equal(
      await readFile(join(active(output, next), "index.mjs"), "utf8"),
      'export default "two";\n',
    );
    assert.equal(await readFile(join(active(output, next), "sdk/value.mjs"), "utf8"), "two");
    assert.equal(await readFile(join(output, ".vercel-cache/private"), "utf8"), "platform cache");
    assert.equal(await readFile(join(output, "notes.txt"), "utf8"), "user notes");
    assert.equal((await publishVercelNode(input(output, "two"))).generation, second.generation);
    assert.equal((await readdir(join(output, "functions"))).length, 2);
  }));

test("Vercel refuses unowned roots and another platform's owned output", async () => {
  for (const kind of ["empty", "sentinel", "cloudflare"]) {
    await fixture(async (output) => {
      if (kind === "cloudflare") {
        const cloudflare = {
          outputDirectory: output,
          configuration: { name: "example" },
          files: new Map([["entry.mjs", Buffer.from("old platform")]]),
        };
        const receipt = await publishCloudflare(cloudflare);
        const marker = join(output, "generations", receipt.generation, ".hd-generation.json");
        const legacy = JSON.parse(await readFile(marker, "utf8"));
        delete legacy.generation;
        await writeFile(marker, JSON.stringify(legacy));
        assert.equal(
          (await publishCloudflare(cloudflare)).generation,
          receipt.generation,
          "Cloudflare records from before the full-digest field remain valid",
        );
      } else {
        await mkdir(output);
        if (kind === "sentinel") {
          await writeFile(join(output, "sentinel"), "keep");
        }
      }
      const before = await readdir(output);
      await assert.rejects(publishVercelNode(input(output)), /ownership|ownership record/);
      assert.deepEqual(await readdir(output), before);
      if (kind === "sentinel") {
        assert.equal(await readFile(join(output, "sentinel"), "utf8"), "keep");
      }
      if (kind === "cloudflare") {
        assert.equal(existsSync(join(output, "wrangler.json")), true);
      }
    });
  }
});

test("changed selectors, function bytes and extra functions are preserved and refused", async () => {
  for (const kind of ["selector", "payload", "extra", "full-digest", "missing-digest"]) {
    await fixture(async (output) => {
      await publishVercelNode(input(output));
      const before = await readFile(join(output, "config.json"), "utf8"),
        config = JSON.parse(before);
      const target =
        kind === "selector"
          ? join(output, "config.json")
          : kind === "payload"
            ? join(active(output, config), "sdk/value.mjs")
            : ["full-digest", "missing-digest"].includes(kind)
              ? join(active(output, config), ".hd-generation.json")
              : join(output, "functions/unrelated.func");
      let edited = "unrelated edit";
      if (["full-digest", "missing-digest"].includes(kind)) {
        const record = JSON.parse(await readFile(target, "utf8"));
        if (kind === "full-digest") {
          // Retain all 48 path-prefix digits while corrupting only the full identity.
          record.generation =
            record.generation.slice(0, 63) + (record.generation.endsWith("0") ? "1" : "0");
        } else {
          delete record.generation;
        }
        edited = JSON.stringify(record);
      }
      if (kind === "extra") {
        await mkdir(target);
      } else {
        await writeFile(target, edited);
      }
      await assert.rejects(publishVercelNode(input(output, "two")));
      if (kind === "extra") {
        assert.deepEqual(await readdir(target), []);
      } else {
        assert.equal(await readFile(target, "utf8"), edited);
      }
      if (kind !== "selector") {
        assert.equal(await readFile(join(output, "config.json"), "utf8"), before);
      }
    });
  }
});

test("symlinked Vercel roots, stores, functions and selectors are refused without following them", async () => {
  for (const kind of ["root", "store", "function", "selector", "marker"]) {
    await fixture(async (output, root) => {
      const outside = join(root, "outside");
      await mkdir(outside);
      await writeFile(join(outside, "sentinel"), "keep");
      if (kind === "root") {
        await symlink(outside, output, "dir");
      } else {
        await publishVercelNode(input(output));
        const config = await selected(output);
        const target =
          kind === "store"
            ? join(output, "functions")
            : kind === "function"
              ? active(output, config)
              : kind === "selector"
                ? join(output, "config.json")
                : join(output, ".hd-server-sdk.json");
        const isFile = ["selector", "marker"].includes(kind);
        await rm(target, { recursive: true });
        await symlink(
          isFile ? join(outside, "sentinel") : outside,
          target,
          isFile ? "file" : "dir",
        );
      }
      await assert.rejects(publishVercelNode(input(output, "two")));
      assert.equal(await readFile(join(outside, "sentinel"), "utf8"), "keep");
    });
  }
});

test("cancellation after a complete inactive function preserves selection and permits retry", () =>
  fixture(async (output) => {
    await publishVercelNode(input(output));
    const before = await readFile(join(output, "config.json"), "utf8");
    const signal = {
      throwIfAborted() {
        if (
          existsSync(join(output, "functions")) &&
          readdirSync(join(output, "functions")).length === 2
        ) {
          throw new Error("cancel after complete function");
        }
      },
    };
    await assert.rejects(
      publishVercelNode({ ...input(output, "two"), signal }),
      /cancel after complete function/,
    );
    assert.equal(await readFile(join(output, "config.json"), "utf8"), before);
    assert.equal(existsSync(output + ".hd-lock"), false);
    await publishVercelNode(input(output, "two"));
    assert.notEqual(await readFile(join(output, "config.json"), "utf8"), before);
    assert.equal((await readdir(join(output, "functions"))).length, 2);
  }));

test("caller mutation and overlapping SDK writers cannot publish mixed functions", () =>
  fixture(async (output) => {
    const options = input(output),
      pending = publishVercelNode(options);
    options.files.get("index.mjs").fill(120);
    options.files.set("late.mjs", Buffer.from("late"));
    options.configuration.name = "mutated";
    await pending;
    const first = await selected(output);
    assert.match(first.routes[0].dest, /^\/api-/);
    assert.equal(
      await readFile(join(active(output, first), "index.mjs"), "utf8"),
      'export default "one";\n',
    );
    assert.equal(existsSync(join(active(output, first), "late.mjs")), false);
    const results = await Promise.allSettled([
      publishVercelNode(input(output, "left")),
      publishVercelNode(input(output, "right")),
    ]);
    const winners = results.filter((r) => r.status === "fulfilled"),
      losers = results.filter((r) => r.status === "rejected");
    assert.equal(winners.length, 1);
    assert.equal(losers.length, 1);
    assert.equal(losers[0].reason.code, "EEXIST");
    const config = await selected(output),
      base = active(output, config);
    const version = await readFile(join(base, "sdk/value.mjs"), "utf8");
    assert.ok(["left", "right"].includes(version));
    assert.equal(
      await readFile(join(base, "index.mjs"), "utf8"),
      "export default " + JSON.stringify(version) + ";\n",
    );
    assert.equal(existsSync(output + ".hd-lock"), false);
  }));

test("invalid Vercel function configuration and escaping payload paths fail before output", async () => {
  for (const patch of [{ name: "../escape" }, { name: "Uppercase" }, { name: "api", routes: [] }]) {
    await fixture(async (output) => {
      await assert.rejects(
        publishVercelNode({ ...input(output), configuration: patch }),
        /SDK output:/,
      );
      assert.equal(existsSync(output), false);
    });
  }
  for (const name of ["../escape", "a\\b", "/absolute", ".hd-generation.json", "C:/absolute"]) {
    await fixture(async (output) => {
      const options = input(output);
      options.files.set(name, Buffer.from("escape"));
      await assert.rejects(publishVercelNode(options), /SDK output:/);
      assert.equal(existsSync(output), false);
    });
  }
});

test("maximum function names use a bounded basename while retaining full generation identity", () =>
  fixture(async (output) => {
    const options = input(output);
    options.configuration.name = "a".repeat(63);
    const receipt = await publishVercelNode(options),
      config = await selected(output);
    assert.match(receipt.generation, /^[0-9a-f]{64}$/);
    const name = config.routes[0].dest.slice(1) + ".func";
    assert.equal(name, "a".repeat(63) + "-" + receipt.generation.slice(0, 48) + ".func");
    assert.ok(name.length <= 128);
    assert.equal(
      await readFile(join(output, "functions", name, "index.mjs"), "utf8"),
      'export default "one";\n',
    );
    assert.equal((await publishVercelNode(options)).generation, receipt.generation);
  }));
