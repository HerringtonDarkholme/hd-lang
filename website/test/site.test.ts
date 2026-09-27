import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { after, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";

import { buildSite, PAGES_BASE } from "../build.ts";
import { checkHdBlocksParse, LEARN_PAGE } from "../src/learn-check.ts";
import { checkLinks } from "../src/links.ts";
import { PAGES, PLAYGROUND_PAGE } from "../src/pages.ts";

const REPO_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

let scratch = "";

before(async () => {
  scratch = await mkdtemp(join(tmpdir(), "hd-website-"));
});

after(async () => {
  await rm(scratch, { recursive: true, force: true });
});

describe("website build", () => {
  test("renders every spec chapter and guide file under the Pages base", async () => {
    const outDir = join(scratch, "pages");
    const result = await buildSite({
      base: PAGES_BASE,
      outDir,
      playgroundDist: join(scratch, "missing"),
    });
    assert.equal(result.playground, false);
    const sources = new Set(PAGES.map((entry) => entry.source));
    for (const directory of ["spec", "guide"])
      for (const name of await readdir(join(REPO_DIR, directory)))
        if (name.endsWith(".md"))
          assert.ok(sources.has(`${directory}/${name}`), `${directory}/${name} is rendered`);
    assert.ok(existsSync(join(outDir, "index.html")));
    for (const entry of PAGES) assert.ok(existsSync(join(outDir, entry.output)), entry.output);

    const types = await readFile(join(outDir, "spec/04-type-system.html"), "utf8");
    assert.match(types, /<span class="hl-keyword">fn<\/span>/);
    assert.match(types, /href="\/hd-lang\/spec\/09-traits\.html#[a-z0-9-]+"/);
    assert.match(types, /href="\/hd-lang\/assets\/style\.css"/);
    assert.match(types, /href="https:\/\/github\.com\/HerringtonDarkholme\/hd-lang"/);
    assert.match(types, /id="mutable-paths"/);

    const playground = await readFile(join(outDir, PLAYGROUND_PAGE), "utf8");
    assert.match(playground, /playground is not part of this build/);
    assert.ok(!existsSync(join(outDir, "playground")));
  });

  test("serves a playground build at playground/ when one exists", async () => {
    const playgroundDist = join(scratch, "playground-dist");
    await mkdir(join(playgroundDist, "assets"), { recursive: true });
    await writeFile(join(playgroundDist, "index.html"), '<script src="./assets/app.js"></script>');
    await writeFile(join(playgroundDist, "assets/app.js"), "");
    const outDir = join(scratch, "with-playground");
    const result = await buildSite({ base: "/", outDir, playgroundDist });
    assert.equal(result.playground, true);
    assert.ok(existsSync(join(outDir, "playground/index.html")));
    assert.ok(existsSync(join(outDir, "playground/assets/app.js")));
    const page = await readFile(join(outDir, PLAYGROUND_PAGE), "utf8");
    assert.match(page, /<iframe id="playground-frame" src="\/playground\/"/);
  });

  test("links code blocks to the playground with base64url source", async () => {
    const outDir = join(scratch, "pages");
    const learn = await readFile(join(outDir, "guide/learn-in-10-minutes.html"), "utf8");
    const match = /href="\/hd-lang\/playground\.html#code=([A-Za-z0-9_-]+)"/.exec(learn);
    assert.ok(match, "a Try in playground link exists");
    assert.equal(
      Buffer.from(match[1]!, "base64url").toString("utf8"),
      '# hello.hd\nprintln("hello, hd-lang")',
    );
  });
});

describe("learn page", () => {
  test("every hd block parses with the reference parser", async () => {
    assert.deepEqual(await checkHdBlocksParse(REPO_DIR, LEARN_PAGE), []);
  });
});

describe("link checker", () => {
  test("reports missing pages, missing anchors, and URLs outside the base", async () => {
    const outDir = join(scratch, "links");
    await mkdir(join(outDir, "spec"), { recursive: true });
    await writeFile(join(outDir, "spec/a.html"), '<h2 id="here">Here</h2>');
    await writeFile(
      join(outDir, "index.html"),
      [
        '<a href="/b/spec/a.html#here">ok</a>',
        '<a href="/b/spec/">dir</a>',
        '<a href="#top" id="top">self</a>',
        '<a href="/b/playground.html#code=abc">state</a>',
        '<a href="https://example.com/x">external</a>',
        '<a href="/b/spec/a.html#gone">anchor</a>',
        '<a href="/b/spec/missing.html">page</a>',
        '<a href="/elsewhere/a.html">base</a>',
        '<a href="#nowhere">local</a>',
      ].join("\n"),
    );
    await writeFile(join(outDir, "playground.html"), "");
    await writeFile(join(outDir, "spec/index.html"), "");
    const failures = await checkLinks(outDir, "/b/", ["index.html"]);
    assert.deepEqual(failures, [
      "index.html: /b/spec/a.html#gone names a missing anchor #gone",
      "index.html: /b/spec/missing.html does not resolve to a generated file",
      "index.html: /elsewhere/a.html is not under the site base /b/",
      "index.html: #nowhere names a missing anchor #nowhere",
    ]);
  });
});
