// The landing page: a hero with the README's slogan, pitch, and code sample,
// claim cards that each open a playground example, and the rest of the README.

import { readFileSync } from "node:fs";
import { join } from "node:path";

import type { MarkdownIt } from "markdown-it";

import { claimCard, markedCode } from "./claim.ts";
import { featureClaims } from "./features.ts";
import { escapeHtml, type RenderEnv } from "./markdown.ts";

interface HomeInput {
  readonly md: MarkdownIt;
  readonly env: RenderEnv;
  /** README.md's text. */
  readonly readme: string;
  /** Directory of the playground's example programs. */
  readonly examplesDir: string;
  /** Site URL of a page given its output path. */
  readonly pageUrl: (output: string) => string;
}

const ARROW = '<span aria-hidden="true">→</span>';

/** The hero sample's punchline: the README function's signature. */
const HERO_MARK = /^fn \w+!\(.*\$ /;

/**
 * Splits the README into the hero's parts and the sections after them. The
 * slogan is the bold paragraph and the pitch the plain one before the first
 * `##` heading.
 */
function readmeParts(readme: string): { slogan: string; pitch: string; rest: string } {
  const split = readme.search(/^## /m);
  const intro = split < 0 ? readme : readme.slice(0, split);
  const paragraphs = intro
    .split(/\n\s*\n/)
    .map((block) => block.trim())
    .filter((block) => block !== "" && !block.startsWith("#"));
  const slogan = /^\*\*(.+)\*\*$/.exec(paragraphs[0] ?? "")?.[1];
  const pitch = paragraphs[1];
  if (!slogan || !pitch)
    throw new Error("landing page: README.md must open with a bold slogan and a pitch paragraph");
  return {
    slogan,
    pitch: pitch.replaceAll(/\s+/g, " "),
    rest: split < 0 ? "" : readme.slice(split),
  };
}

/** The README's first hd block, trimmed to its `trait` and the function after it. */
function heroSample(readme: string): string {
  const block = /^```hd\n([\s\S]*?)^```/m.exec(readme)?.[1];
  if (!block) throw new Error("landing page: README.md has no ```hd block");
  const lines = block.replace(/\n$/, "").split("\n");
  const start = lines.findIndex((line) => line.startsWith("trait "));
  const end = lines.findIndex((line, index) => index > start && line.startsWith("data "));
  if (start < 0 || end < 0) return lines.join("\n");
  return lines.slice(start, end).join("\n").trimEnd();
}

function codeWindow(title: string, code: string, action: string): string {
  const marks = code.split("\n").filter((line) => HERO_MARK.test(line));
  return `<figure class="code-window">
<figcaption class="code-window-bar"><span class="code-window-dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="code-window-title">${escapeHtml(title)}</span>${action}</figcaption>
${markedCode(code, marks)}
</figure>`;
}

export function renderHome(input: HomeInput): string {
  const { md, env, pageUrl } = input;
  const { slogan, pitch, rest } = readmeParts(input.readme);
  const requirements = readFileSync(join(input.examplesDir, "requirements.hd"), "utf8");

  const hero = `<section class="hero" aria-labelledby="hero-title">
<div class="hero-text">
<p class="hero-eyebrow">hd-lang</p>
<h1 id="hero-title">${md.renderInline(slogan, env)}</h1>
<p class="hero-pitch">${md.renderInline(pitch, env)}</p>
<div class="hero-actions">
<a class="button button-primary" href="${pageUrl("playground.html")}">Open the playground</a>
<a class="button" href="${pageUrl("guide/learn-in-10-minutes.html")}">Learn in 10 minutes</a>
<a class="button button-quiet" href="${pageUrl("spec/index.html")}">Read the spec</a>
</div>
</div>
<div class="hero-code">
${codeWindow(
  "welcome.hd",
  heroSample(input.readme),
  `<a class="code-window-action" href="${escapeHtml(env.playgroundUrl(requirements))}">Run it ${ARROW}</a>`,
)}
</div>
</section>`;

  const cards = featureClaims(input.examplesDir, env.playgroundUrl).map((claim) =>
    claimCard(claim),
  );
  const features = `<section class="features" aria-labelledby="features-title">
<h2 id="features-title">See it run</h2>
<p class="section-lead">One claim per card, the lines that prove it, and the full example one click away in the playground.</p>
<div class="claim-grid">
${cards.join("\n")}
</div>
</section>`;

  const readme = `<section class="home-readme prose" aria-label="About hd-lang">
${md.render(rest, env)}
</section>`;
  return `${hero}\n${features}\n${readme}`;
}
