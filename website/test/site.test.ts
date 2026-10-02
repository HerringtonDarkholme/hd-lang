import assert from "node:assert/strict";
import { existsSync } from "node:fs";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { after, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";

import { buildSite, PAGES_BASE } from "../build.ts";
import type { GrammarIndex } from "../src/ebnf.ts";
import { checkHdBlocksParse, LEARN_PAGE } from "../src/learn-check.ts";
import { checkLinks } from "../src/links.ts";
import { createMarkdown, type RenderEnv } from "../src/markdown.ts";
import { PAGES, PLAYGROUND_PAGE } from "../src/pages.ts";

const REPO_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../..");

let scratch = "";
/** The rule index of the Pages-base build in the first test. */
let grammar: GrammarIndex | undefined;

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
    grammar = result.grammar;
    const sources = new Set(PAGES.map((entry) => entry.source));
    for (const directory of ["spec", "spec/lang", "spec/std", "spec/cli", "guide"])
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
    // Without the playground's worker there is no REPL panel.
    assert.doesNotMatch(types, /repl\.js|data-repl-worker/);
    assert.ok(!existsSync(join(outDir, "assets/repl.js")));
  });

  test("renders rule IDs, callouts, rule tables, and error examples", async () => {
    const data = await readFile(join(scratch, "pages", "spec/08-data-and-enums.html"), "utf8");
    assert.match(
      data,
      /<li><a class="rule-id" id="r-data\.field\.unique" href="#r-data\.field\.unique" title="Rule data\.field\.unique">data\.field\.unique<\/a>Field names must be unique/,
    );
    assert.doesNotMatch(data, /r\[data\./, "no marker is left as text");
    assert.match(data, /<blockquote class="callout callout-why">\s*<p><strong>Why\.<\/strong>/);
    assert.match(data, /<blockquote class="callout callout-note">/);
    assert.match(data, /<table class="rule-table">/);
    assert.match(data, /<td data-label="Rule"><a class="rule-id" id="r-data\.embed\.width"/);
    assert.match(
      data,
      /<div class="code-block error-example"><div class="example-label">Error example<\/div>/,
    );
    assert.match(
      data,
      /<span class="line-error">.*<span class="hl-comment hl-error-marker"># error: duplicate-embedded-field<\/span><\/span>/,
    );
    // The style guide's link to a rule anchor resolves on the chapter page.
    const style = await readFile(join(scratch, "pages", "spec/style.html"), "utf8");
    assert.match(style, /href="\/hd-lang\/spec\/08-data-and-enums\.html#r-data\.embed\.width"/);
    // A marker inside a fence or inline code stays text.
    assert.match(style, /language-markdown">1\. r\[data\.field\.unique\] Field names/);
    const readme = await readFile(join(scratch, "pages", "spec/index.html"), "utf8");
    assert.match(readme, /<code>r\[data\.field\.unique\]<\/code>/);
  });

  test("generates the glossary page, links it from the sidebar, and links terms to rules", async () => {
    const page = await readFile(join(scratch, "pages", "spec/glossary.html"), "utf8");
    assert.match(
      page,
      /<a href="\/hd-lang\/spec\/03-names-and-scopes\.html#r-names\.part\.depth"><strong>depth<\/strong><\/a>/,
    );
    assert.match(page, /href="\/hd-lang\/spec\/std\/iter\.html#iterator-adapters"/);
    assert.doesNotMatch(page, /View source on GitHub/, "the page has no Markdown source");
    const types = await readFile(join(scratch, "pages", "spec/04-type-system.html"), "utf8");
    assert.match(types, /<li><a href="\/hd-lang\/spec\/glossary\.html">Glossary<\/a><\/li>/);
  });

  test("renders footnotes as small references and a notes section at the page end", async () => {
    const data = await readFile(join(scratch, "pages", "spec/08-data-and-enums.html"), "utf8");
    assert.doesNotMatch(data, /\[\^miku\]/, "no footnote syntax is left as text");
    // The reference sits inside the Note callout that cites it.
    assert.match(
      data,
      /<blockquote class="callout callout-note">(?:(?!<\/blockquote>)[\s\S])*<sup class="footnote-ref"><a href="#fn-miku" id="fnref-miku" aria-label="Footnote 1">1<\/a><\/sup>/,
    );
    const notes = data.indexOf('<section class="footnotes" aria-label="Footnotes">');
    assert.ok(notes > data.lastIndexOf("r-data.unsupported"), "the notes follow the last rule");
    assert.match(
      data.slice(notes),
      /^<section class="footnotes" aria-label="Footnotes">\s*<ol class="footnotes-list">\s*<li id="fn-miku" class="footnote-item"><p>39 reads as .*<a href="#fnref-miku" class="footnote-backref" aria-label="Back to the reference">/,
    );

    const md = createMarkdown();
    const env: RenderEnv = {
      source: "spec/example.md",
      resolveLink: (href) => href,
      playgroundUrl: () => "",
      headings: [],
      slugCounts: new Map(),
    };
    const html = md.render(
      "- One.[^a]\n- Two.[^a] Three.[^2]\n\n[^a]: Note a.\n[^2]: Note b.\n",
      env,
    );
    assert.match(html, /<li>One\.<sup class="footnote-ref"><a href="#fn-a" id="fnref-a"/);
    assert.match(html, /<li>Two\.<sup class="footnote-ref"><a href="#fn-a" id="fnref-a-2"/);
    assert.match(html, /<a href="#fn-2" id="fnref-2" aria-label="Footnote 2">2<\/a>/);
    assert.match(
      html,
      /href="#fnref-a" class="footnote-backref".*href="#fnref-a-2" class="footnote-backref"/,
    );
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
    // Every page gets the REPL panel, which loads the playground's worker.
    const chapter = await readFile(join(outDir, "spec/04-type-system.html"), "utf8");
    assert.match(chapter, /<script type="module" src="\/assets\/repl\.js"><\/script>/);
    assert.match(
      chapter,
      /<body data-base="\/" data-repl-worker="\/playground\/assets\/worker\.js">/,
    );
    const panel = await readFile(join(outDir, "assets/repl.js"), "utf8");
    assert.ok(panel.length < 100_000, `the panel bundle stays small (${panel.length} bytes)`);
    assert.doesNotMatch(panel, /binaryen/i, "the compiler stays in the worker");
  });

  test("gives snippets Try in REPL and whole programs a playground link", async () => {
    const outDir = join(scratch, "pages");
    const learn = await readFile(join(outDir, "guide/learn-in-10-minutes.html"), "utf8");
    const hello = /<code class="language-hd">([^]*?)<\/code><\/pre>(.*?)<\/div>/.exec(learn);
    assert.ok(hello, "the first hd block renders");
    assert.match(hello[1]!, /hello, hd-lang/);
    assert.equal(
      hello[2],
      '<button type="button" class="try-link try-repl" title="Evaluate this code in the REPL panel">Try in REPL</button>',
    );
    const links = [
      ...learn.matchAll(
        /class="try-link try-playground" href="\/hd-lang\/playground\.html#code=([A-Za-z0-9_-]+)"/g,
      ),
    ];
    assert.equal(links.length, 1, "one whole program on the page");
    assert.match(Buffer.from(links[0]![1]!, "base64url").toString("utf8"), /^pub fn main!\(\)/m);
    assert.doesNotMatch(learn, /Try in playground/);
  });
});

describe("grammar blocks", () => {
  const RULE_LINK = /<a class="eb-(?:definition|reference|token)" href="([^"]*)"/g;

  test("indexes the specification's rules and lists names no rule defines", () => {
    assert.ok(grammar, "the Pages-base build ran");
    for (const rule of ["source_file", "trait_decl", "requirement_clause", "identifier"])
      assert.ok(grammar.definitions.has(rule), rule);
    assert.equal(grammar.definitions.get("requirement_clause")?.[0], "spec/lang/02-grammar.md");
    assert.ok(grammar.linkedCount > 500);
    assert.ok(grammar.abstractTokens.includes("NEWLINE"));
    assert.ok(grammar.abstractTokens.includes("SUITE_END"));
    for (const { name } of grammar.unresolved) assert.ok(!grammar.definitions.has(name), name);
  });

  test("anchors definitions and links references within and across pages", async () => {
    const outDir = join(scratch, "pages");
    const chapter = await readFile(join(outDir, "spec/02-grammar.html"), "utf8");
    assert.match(
      chapter,
      /<a class="eb-definition" href="#rule-trait_decl" id="rule-trait_decl">trait_decl<\/a>/,
    );
    assert.match(chapter, /<a class="eb-reference" href="#rule-trait_member">trait_member<\/a>/);
    assert.match(chapter, /<span class="eb-token">NEWLINE<\/span>/);
    assert.match(chapter, /<span class="eb-terminal">&quot;trait&quot;<\/span>/);
    assert.match(chapter, /<span class="eb-operator">\|<\/span>/);

    const gadts = await readFile(join(outDir, "spec/13-gadts.html"), "utf8");
    assert.match(
      gadts,
      /<a class="eb-reference" href="\/hd-lang\/spec\/02-grammar\.html#rule-decorator_line">decorator_line<\/a>/,
    );

    // Every rule link on every page names an anchor that exists in its target.
    let checked = 0;
    for (const entry of PAGES) {
      const html = await readFile(join(outDir, entry.output), "utf8");
      for (const [, href] of html.matchAll(RULE_LINK)) {
        const hash = href!.indexOf("#");
        const path = href!.slice(0, hash);
        const target =
          path === "" ? html : await readFile(join(outDir, path.slice(PAGES_BASE.length)), "utf8");
        assert.ok(target.includes(` id="${href!.slice(hash + 1)}"`), `${entry.output}: ${href}`);
        checked += 1;
      }
    }
    // One link per definition and one per reference that names a rule.
    assert.equal(checked, grammar!.definitionCount + grammar!.linkedCount);
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
