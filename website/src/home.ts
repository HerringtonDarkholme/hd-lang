// The landing page shows only what sets hd apart: the hero, the three
// agentic pillars, a walk through
// the three features that emulate algebraic effects, typed derivation, the
// standard library's testing support, and how to install it. Each claim is a
// few lines that run in the playground. The README is the source of the
// slogan, pitch, pillars, and install commands, so the two cannot drift; the
// page renders no other README text.

import type { MarkdownIt } from "markdown-it";

import { type Claim, claimCard } from "./claim.ts";
import { DERIVATION, EFFECT_STEPS, featureClaims, STD_FEATURES } from "./features.ts";
import { logoSvg } from "./logo.ts";
import { escapeHtml, highlightHd, type RenderEnv } from "./markdown.ts";
import { PILLARS } from "./metrics.ts";
import { REPOSITORY_URL } from "./pages.ts";

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

const ARROW_ICON =
  '<svg viewBox="0 0 24 24" width="16" height="16" aria-hidden="true"><path fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" d="M5 12h14M13 6l6 6-6 6"/></svg>';

/** How many of the slogan's last words the hero lights in the logo's colors. */
const SLOGAN_GLOW_WORDS = 2;

/** The README's heading over the three pillars. */
const PILLARS_HEADING = "Algebraic effects, emulated by three pillars";

/** Each pillar's mark, in the README's order. */
const PILLAR_GLYPHS = ["$", "$.with", "!"];

/**
 * How hd meets each pillar, by the metrics page's pillar key: the README's
 * pillar list and the goals doc's per-pillar features, in short.
 */
const PILLAR_HOW: Readonly<Record<string, string>> = {
  "1": "Explicit signatures let the checker skip dependency bodies, and checking is incremental per module. Diagnostics are compact, with JSON fix-its. Tests, property tests, and `require`/`ensure` contracts are built in.",
  "2": "Check results and compiled code are cached by content and shared across processes, and std is checked once, inside the binary. A program reaches the host only through the capabilities it is granted.",
  "3": "Code is generated per value layout, suspension compiles to state machines, and only reachable code goes into the Wasm module. Time, randomness, and I/O sit behind capabilities, so a test can fix them.",
};

/** One explorer tab: its label, an optional mark before it, and its claim. */
interface ExplorerItem {
  readonly label: string;
  readonly glyph?: string;
  readonly claim: Claim;
}

/**
 * Splits the README into the hero's parts and its `##` sections. The slogan
 * is the bold paragraph and the pitch the plain one before the first `##`
 * heading.
 */
function readmeParts(readme: string): {
  slogan: string;
  pitch: string;
  sections: ReadonlyMap<string, string>;
} {
  const split = readme.search(/^## /m);
  const intro = split < 0 ? readme : readme.slice(0, split);
  const paragraphs = intro
    .split(/\n\s*\n/)
    .map((block) => block.trim())
    .filter((block) => block !== "" && !block.startsWith("#"));
  const slogan = /^\*\*(.+)\*\*$/.exec(paragraphs[0] ?? "")?.[1];
  const pitch = paragraphs[1];
  if (!slogan || !pitch) {
    throw new Error("landing page: README.md must open with a bold slogan and a pitch paragraph");
  }
  const sections = new Map<string, string>();
  for (const block of (split < 0 ? "" : readme.slice(split)).split(/^(?=## )/m)) {
    const newline = block.indexOf("\n");
    sections.set(block.slice(3, newline).trim(), block.slice(newline + 1));
  }
  return { slogan, pitch: pitch.replaceAll(/\s+/g, " "), sections };
}

function section(sections: ReadonlyMap<string, string>, heading: string): string {
  const body = sections.get(heading);
  if (body === undefined) {
    throw new Error(`landing page: README.md has no "## ${heading}" section`);
  }
  return body;
}

/**
 * The first list in `markdown` whose items `marker` starts, each with its bold
 * label split off. Indented continuation lines join their item.
 */
function labeledItems(markdown: string, marker: RegExp): { label: string; text: string }[] {
  const items: string[] = [];
  for (const line of markdown.split("\n")) {
    if (marker.test(line)) {
      items.push(line.replace(marker, ""));
    } else if (items.length > 0 && /^\s+\S/.test(line)) {
      items[items.length - 1] += ` ${line.trim()}`;
    } else if (items.length > 0 && line.trim() !== "") {
      break;
    }
  }
  return items.map((item) => {
    const match = /^\*\*(.+?)\*\*\s*(.*)$/.exec(item);
    if (!match) {
      throw new Error(`landing page: README list item has no bold label: ${item}`);
    }
    return { label: match[1]!.replace(/[.:]$/, ""), text: match[2]! };
  });
}

/** The slogan as the hero's heading, its last words lit in the logo's colors. */
function sloganHtml(slogan: string, md: MarkdownIt, env: RenderEnv): string {
  const words = slogan.split(" ");
  if (words.length <= SLOGAN_GLOW_WORDS) {
    return md.renderInline(slogan, env);
  }
  const lead = words.slice(0, -SLOGAN_GLOW_WORDS).join(" ");
  const tail = words.slice(-SLOGAN_GLOW_WORDS).join(" ");
  return `${md.renderInline(lead, env)} <span class="hero-glow">${md.renderInline(tail, env)}</span>`;
}

/** A section's spaced label, title, and optional lead paragraph. */
function sectionHead(id: string, eyebrow: string, title: string, lead = ""): string {
  return `<header class="home-head">
<p class="eyebrow">${escapeHtml(eyebrow)}</p>
<h2 id="${id}">${escapeHtml(title)}</h2>
${lead ? `<p class="section-lead">${escapeHtml(lead)}</p>` : ""}
</header>`;
}

/**
 * Claims beside their tabs, one shown at a time. Without scripts, every
 * panel shows. `id` prefixes the tab and panel ids.
 */
function explorer(id: string, label: string, items: readonly ExplorerItem[]): string {
  const tabs = items
    .map(
      (item, index) =>
        `<button class="explorer-tab" type="button" role="tab" id="${id}-tab-${index}" aria-controls="${id}-${index}" aria-selected="${index === 0}"${index === 0 ? "" : ' tabindex="-1"'}><span class="explorer-index">${String(index + 1).padStart(2, "0")}</span>${item.glyph ? `<code class="explorer-glyph">${escapeHtml(item.glyph)}</code>` : ""}<span>${escapeHtml(item.label)}</span></button>`,
    )
    .join("\n");
  const panels = items
    .map(
      (item, index) =>
        `<div class="explorer-panel" role="tabpanel" id="${id}-${index}" aria-labelledby="${id}-tab-${index}">
${claimCard(item.claim)}
</div>`,
    )
    .join("\n");
  return `<div class="explorer-body" data-explorer>
<div class="explorer-tabs" role="tablist" aria-label="${escapeHtml(label)}">
${tabs}
</div>
<div class="explorer-panels">
${panels}
</div>
</div>`;
}

export function renderHome(input: HomeInput): string {
  const { md, env, pageUrl } = input;
  const { slogan, pitch, sections } = readmeParts(input.readme);
  const inline = (text: string): string => md.renderInline(text, env);

  const hero = `<section class="hero" aria-labelledby="hero-title">
<div class="hero-sky" aria-hidden="true"></div>
<div class="hero-text">
<p class="hero-chip"><span aria-hidden="true"></span>Compiles to WebAssembly</p>
<h1 id="hero-title">${sloganHtml(slogan, md, env)}</h1>
<p class="hero-pitch">${inline(pitch)}</p>
<div class="hero-actions">
<a class="button button-primary" href="${pageUrl("tour/index.html")}">Take the tour</a>
<a class="button" href="${pageUrl("playground.html")}">Open the playground</a>
</div>
<p class="hero-links"><a href="${pageUrl("guide/learn-in-10-minutes.html")}">Learn in 10 minutes</a><a href="${pageUrl("spec/index.html")}">Read the spec</a></p>
</div>
<div class="hero-art">
${logoSvg({ id: "hero-logo", variant: "full", className: "hero-logo" })}
</div>
</section>`;

  // Why hd: the three pillars of the Agentic Programming Language Arena, as
  // the metrics page defines them (correctness is a gate, not a pillar):
  // what each measures, and how hd meets it.
  const agentCards = PILLARS.filter((pillar) => pillar.key !== "gate")
    .map(
      (
        pillar,
        index,
      ) => `<article class="agent-pillar" data-widen-item style="view-transition-name: pillar-card-${index + 1}">
<p class="agent-pillar-name">${String(index + 1).padStart(2, "0")} · ${escapeHtml(pillar.title)}</p>
<h3>${escapeHtml(pillar.question)}</h3>
<div class="agent-pillar-body">
<span class="agent-pillar-label">Measures</span>
<button class="agent-pillar-more" type="button" aria-controls="pillar-how-${index + 1}" aria-expanded="false" style="view-transition-name: pillar-more-${index + 1}">How hd gets there<span class="agent-pillar-arrow" aria-hidden="true"> →</span></button>
<p class="agent-pillar-measures">${escapeHtml(pillar.explain)}</p>
<p class="agent-pillar-how" id="pillar-how-${index + 1}"><span class="agent-pillar-how-title">How hd gets there</span> ${inline(PILLAR_HOW[pillar.key] ?? "")}</p>
</div>
</article>`,
    )
    .join("\n");
  const agentSection = `<section class="home-section agents" aria-labelledby="agents-title">
<header class="agents-head">
<h2 id="agents-title">Cheap · Scalable · Fast</h2>
<a class="eyebrow" href="${pageUrl("metrics.html")}">Built for agents · daily metrics →</a>
</header>
<div class="agent-grid" data-widen>
${agentCards}
</div>
</section>`;

  // Algebraic effects: one step per pillar. The README names each step; the
  // playground example shows it at work.
  const pillarsBody = section(sections, PILLARS_HEADING);
  const pillars = labeledItems(pillarsBody, /^\d+\.\s+/);
  if (pillars.length !== PILLAR_GLYPHS.length || pillars.length !== EFFECT_STEPS.length) {
    throw new Error(`landing page: the README must list ${PILLAR_GLYPHS.length} pillars`);
  }
  const errorsNote = /^Errors stay plain values.*$/m.exec(pillarsBody)?.[0];
  const steps = featureClaims(EFFECT_STEPS, input.examplesDir, env.playgroundUrl).map(
    (claim, index): ExplorerItem => ({
      label: claim.title,
      glyph: PILLAR_GLYPHS[index]!,
      claim: { ...claim, title: pillars[index]!.label },
    }),
  );
  const effectsSection = `<section class="home-section effects" aria-labelledby="effects-title">
${sectionHead("effects-title", "The core", PILLARS_HEADING, "Three plain features stand in for an effect handler runtime. Walk through them in order.")}
${explorer("effect", "Steps", steps)}
${errorsNote ? `<p class="section-note">${inline(errorsNote)}</p>` : ""}
</section>`;

  const [derivation] = featureClaims([DERIVATION], input.examplesDir, env.playgroundUrl);
  const derivationSection = `<section class="home-section derivation" aria-labelledby="derivation-title">
<div class="showcase" data-links>
<div class="showcase-text">
${sectionHead("derivation-title", "Typed derivation", "Behavior derived from your types", "Declare serialization, validation, and schemas on the type. Libraries derive them in plain hd from its fields.")}
<ul class="showcase-list" data-link-items>
${(DERIVATION.links ?? [])
  .map(
    (link) =>
      `<li data-link="${link.key}" tabindex="0"><p class="showcase-line"><code class="hd">${highlightHd(link.label)}</code><span>${inline(link.summary)}</span></p><div class="showcase-more"><span>${inline(link.detail)}</span></div></li>`,
  )
  .join("\n")}
</ul>
</div>
<div class="showcase-card">
${claimCard(derivation!)}
</div>
</div>
</section>`;

  const stdItems = featureClaims(STD_FEATURES, input.examplesDir, env.playgroundUrl).map(
    (claim): ExplorerItem => ({ label: claim.title, claim }),
  );
  const stdSection = `<section class="home-section std" aria-labelledby="std-title">
${sectionHead("std-title", "Standard library", "A standard library for getting it right", "Its effects show in your signatures, and it ships a fake for each one. Property tests and contracts check the cases you would not think to write.")}
${explorer("std", "Standard library", stdItems)}
</section>`;

  const installBlock = /^```sh\n[\s\S]*?^```/m.exec(section(sections, "Install"))?.[0];
  if (!installBlock) {
    throw new Error('landing page: README.md\'s "## Install" section has no ```sh block');
  }
  const install = `<section class="home-section install" aria-labelledby="install-title">
<div class="install-card">
<div class="install-text">
<h2 id="install-title">Start building with hd</h2>
<p>hd installs from source with the Rust toolchain, and programs run on Node or wasmtime.</p>
<div class="install-actions">
<a class="button button-primary" href="${pageUrl("guide/learn-in-10-minutes.html")}">Learn hd in 10 minutes ${ARROW_ICON}</a>
<a class="button" href="${REPOSITORY_URL}">View on GitHub</a>
</div>
</div>
<div class="install-code">
${md.render(installBlock, env)}
</div>
</div>
</section>`;

  const footer = `<footer class="home-footer">
<a class="home-footer-brand" href="${pageUrl("index.html")}">${logoSvg({ id: "footer-logo", variant: "mark", className: "home-footer-mark" })}hd-lang</a>
<p>${inline(slogan)}</p>
<nav aria-label="Footer"><a href="${pageUrl("guide/learn-in-10-minutes.html")}">Learn</a><a href="${pageUrl("tour/index.html")}">Tour</a><a href="${pageUrl("playground.html")}">Playground</a><a href="${pageUrl("spec/index.html")}">Spec</a><a href="${REPOSITORY_URL}">GitHub</a></nav>
</footer>`;

  return [hero, agentSection, effectsSection, derivationSection, stdSection, install, footer].join(
    "\n",
  );
}
