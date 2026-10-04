import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { after, before, describe, test } from "node:test";
import { fileURLToPath } from "node:url";

import { buildSite, PAGES_BASE } from "../build.ts";
import { claimCard } from "../src/claim.ts";
import type { GrammarIndex } from "../src/ebnf.ts";
import { FEATURES, featureSource } from "../src/features.ts";
import { checkHdBlocksParse, LEARN_PAGE } from "../src/learn-check.ts";
import { renderLayout } from "../src/layout.ts";
import { checkLinks } from "../src/links.ts";
import { createMarkdown, type RenderEnv } from "../src/markdown.ts";
import { PAGES, PLAYGROUND_PAGE } from "../src/pages.ts";
import { editedCode, loadTour, parseTourPage } from "../src/tour-pages.ts";
import { tourEditor } from "../src/tour.ts";

const REPO_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const EXAMPLES_DIR = join(REPO_DIR, "website/playground/examples");

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

  test("builds the landing page from the README and the playground examples", async () => {
    const home = await readFile(join(scratch, "pages", "index.html"), "utf8");
    const readme = await readFile(join(REPO_DIR, "README.md"), "utf8");
    const slogan = /^\*\*(.+)\*\*$/m.exec(readme)![1]!;
    assert.match(home, new RegExp(`<h1 id="hero-title">${slogan.replaceAll(".", "\\.")}</h1>`));
    assert.match(home, /<div class="layout layout-home">/);
    assert.match(home, /<a class="button button-primary" href="\/hd-lang\/tour\/">Take the tour/);
    assert.match(home, /<a class="button" href="\/hd-lang\/playground\.html">/);
    assert.match(home, /href="\/hd-lang\/guide\/learn-in-10-minutes\.html">Learn in 10 minutes/);
    // The hero sample is the README's own code, highlighted.
    assert.match(
      home,
      /<span class="hl-keyword">trait<\/span> <span class="hl-type">Mailer<\/span>/,
    );
    // Each claim card opens its whole example in the playground.
    const examples = [
      ...home.matchAll(/<a href="\/hd-lang\/playground\.html#code=([A-Za-z0-9_-]+)">Run the full/g),
    ].map((match) => Buffer.from(match[1]!, "base64url").toString("utf8"));
    assert.equal(examples.length, FEATURES.length);
    for (const [index, feature] of FEATURES.entries())
      assert.equal(
        examples[index],
        await readFile(join(EXAMPLES_DIR, `${feature.example}.hd`), "utf8"),
      );
    // A punchline line is marked, and a diagnostic sits in an error pane.
    assert.match(
      home,
      /<span class="line-mark"><span class="hl-keyword">fn<\/span> <span class="hl-function">welcome!<\/span>/,
    );
    assert.match(home, /<figure class="claim-output claim-output-error">/);
    // The README's sections follow the hero.
    assert.match(home, /<h2 id="why-hd">/);
  });

  test("claim outputs are real compiler output", () => {
    const dir = mkdtempSync(join(tmpdir(), "hd-claims-"));
    try {
      for (const feature of FEATURES) {
        const { output } = feature;
        if (!output) continue;
        if (output.source === "comment") {
          const source = readFileSync(join(EXAMPLES_DIR, `${feature.example}.hd`), "utf8");
          for (const line of output.lines)
            assert.ok(source.includes(line), `${feature.title}: ${line}`);
          continue;
        }
        const file = join(dir, "main.hd");
        writeFileSync(file, featureSource(feature, EXAMPLES_DIR));
        const result = spawnSync(
          process.execPath,
          ["--experimental-strip-types", join(REPO_DIR, "bin/hd.js"), "check", file],
          { encoding: "utf8" },
        );
        const printed = `${result.stdout}${result.stderr}`.replaceAll(`${dir}/`, "");
        for (const line of output.lines)
          assert.ok(printed.includes(line), `${feature.title}: ${printed}`);
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("claim cards cap the snippet and mark its punchline", () => {
    const card = claimCard({
      title: "T",
      blurb: "Uses `x`.",
      code: "fn f() -> i32:\n    1",
      marks: ["1"],
      href: "#",
    });
    assert.match(card, /<p class="claim-blurb">Uses <code>x<\/code>\.<\/p>/);
    assert.match(
      card,
      /\n<span class="line-mark">    <span class="hl-number">1<\/span><\/span><\/code>/,
    );
    const long = { title: "T", blurb: "", code: "1\n".repeat(9).trim(), marks: ["1"], href: "#" };
    assert.throws(() => claimCard(long), /at most 8/);
    assert.throws(() => claimCard({ ...long, code: "1", marks: ["2"] }), /not in the snippet/);
  });

  test("renders the split shell with prose and an aside", () => {
    const html = renderLayout({
      base: "/b/",
      output: "tour/1-hello/index.html",
      title: "Tour",
      description: "",
      body: "<h1>Step</h1>",
      headings: [],
      shape: "split",
      aside: '<iframe title="editor"></iframe>',
    });
    assert.match(
      html,
      /<div class="split">\s*<article class="prose split-prose">\s*<h1>Step<\/h1>[\s\S]*<\/article>\s*<aside class="split-aside" aria-label="Live editor">\s*<iframe title="editor">/,
    );
    assert.match(html, /<a href="\/b\/tour\/" aria-current="page">Tour<\/a>/);
  });

  test("renders the tour: contents, one split page per file, and a static listing offline", async () => {
    const outDir = join(scratch, "pages");
    const tour = loadTour(REPO_DIR);
    assert.ok(tour.length >= 6);
    const index = await readFile(join(outDir, "tour/index.html"), "utf8");
    assert.match(index, /<a href="\/hd-lang\/tour\/" aria-current="page">Tour<\/a>/);
    for (const page of tour) {
      assert.match(index, new RegExp(`<a href="/hd-lang/tour/${page.number}-${page.slug}/">`));
      const html = await readFile(
        join(outDir, `tour/${page.number}-${page.slug}/index.html`),
        "utf8",
      );
      assert.match(html, /<div class="layout layout-split">/);
      assert.match(
        html,
        new RegExp(`<p class="tour-step">.*· ${page.number} of ${tour.length}</p>`),
      );
      assert.match(
        html,
        new RegExp(`aria-current="page"><span class="tour-number">${page.number}<`),
      );
      // Without the playground build there is no editor script, only the listing.
      assert.match(html, /class="tour-editor tour-offline"/);
      assert.doesNotMatch(html, /tour\.js|id="tour-run"/);
      // The edit and output blocks are check data, not page text.
      assert.doesNotMatch(html, /replace: |```/);
      if (page.number > 1) assert.match(html, /<a class="tour-prev" rel="prev"/);
      if (page.number < tour.length) assert.match(html, /<a class="tour-next" rel="next"/);
    }
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
    // A tour page gets the editor script and the worker it runs code in.
    const tourPage = await readFile(join(outDir, "tour/1-hello/index.html"), "utf8");
    assert.match(tourPage, /<script type="module" src="\/assets\/tour\.js"><\/script>/);
    assert.match(tourPage, /data-worker="\/playground\/assets\/worker\.js"/);
    assert.match(tourPage, /<script type="application\/json" id="tour-source">"# Press Run/);
    assert.ok(existsSync(join(outDir, "assets/tour.js")));
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

describe("tour pages", () => {
  test("tour pages are well formed, and an edit must name one line", () => {
    const text =
      "---\ntitle: T\n---\n\n# Claim\n\nProse.\n\n```hd\na\nb\n```\n\n```output\nx\n```\n";
    const page = parseTourPage("website/tour/01-t.md", text, 1);
    assert.deepEqual(
      [page.title, page.claim, page.prose, page.code],
      ["T", "Claim", "Prose.", "a\nb\n"],
    );
    assert.deepEqual(page.output, ["x"]);
    assert.throws(() => editedCode(page), /has no edit/);
    const edit = text.replace(
      "```output",
      "```edit\nreplace: c\nwith: d\nerror: e\n```\n\n```output",
    );
    assert.throws(() => editedCode(parseTourPage("website/tour/01-t.md", edit, 1)), /0 times/);
    assert.throws(() => parseTourPage("website/tour/01-t.md", text.replace("```hd", "```js"), 1));
    assert.throws(() => parseTourPage("website/tour/t.md", text, 1), /NN-slug/);
    // A tests block may stand in for the output block; an empty with: deletes the line.
    const tested = text.replace(
      "```output\nx\n```",
      "```edit\nreplace: a\nwith:\nfailure: f\n```\n\n```tests\n2 tests passed\n```",
    );
    const testPage = parseTourPage("website/tour/01-t.md", tested, 1);
    assert.deepEqual([testPage.output, testPage.tests], [undefined, "2 tests passed"]);
    assert.deepEqual(testPage.edit, { replace: "a", with: "", failure: "f" });
    assert.equal(editedCode(testPage), "\nb\n");
    assert.throws(
      () => parseTourPage("website/tour/01-t.md", edit.replace("error: e", "failure: f"), 1),
      /needs a ```tests block/,
    );
    assert.throws(
      () =>
        parseTourPage("website/tour/01-t.md", edit.replace("error: e", "error: e\nfailure: f"), 1),
      /one of error: or failure:/,
    );
    assert.throws(
      () => parseTourPage("website/tour/01-t.md", text.replace("```output\nx\n```", ""), 1),
      /```output or ```tests block/,
    );
  });

  test("a tour page's buttons follow its blocks: Run for output, Test for tests", () => {
    const env: RenderEnv = {
      source: "website/tour/01-t.md",
      resolveLink: (href) => href,
      playgroundUrl: () => "",
      headings: [],
      slugCounts: new Map(),
    };
    const render = {
      md: createMarkdown(),
      env,
      pageUrl: (output: string) => output,
      workerUrl: "w.js",
    };
    const page = (blocks: string) =>
      parseTourPage(
        "website/tour/01-t.md",
        `---\ntitle: T\n---\n\n# C\n\n\`\`\`hd\na\n\`\`\`\n\n${blocks}`,
        1,
      );
    const ran = tourEditor(page("```output\nx\n```\n"), render);
    assert.match(ran, /data-primary="run"/);
    assert.match(ran, /class="button button-primary" id="tour-run"/);
    assert.doesNotMatch(ran, /id="tour-test"/);
    const both = tourEditor(page("```output\nx\n```\n\n```tests\n1 test passed\n```\n"), render);
    assert.match(both, /id="tour-run".*class="button" id="tour-test" title="Test"/);
    const tested = tourEditor(page("```tests\n1 test passed\n```\n"), render);
    assert.match(tested, /data-primary="test"/);
    assert.match(
      tested,
      /class="button button-primary" id="tour-test" title="Test \(Ctrl\+Enter\)"/,
    );
    assert.doesNotMatch(tested, /id="tour-run"/);
    assert.match(tested, /Press Test to run the tests\./);
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
  test("every hd block parses with the compiler", async () => {
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
