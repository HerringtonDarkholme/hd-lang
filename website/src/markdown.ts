import { posix } from "node:path";

import markdownIt, { type MarkdownIt, type Token } from "markdown-it";

import { parseSource } from "../../spec/reference-parser/parser.ts";
import { classify } from "../../src/highlight.ts";
import { type GrammarIndex, ruleAnchor, ruleTarget, tokenizeEbnf } from "./ebnf.ts";

/** A heading found while rendering, used for the page outline and search. */
export interface Heading {
  readonly level: number;
  readonly id: string;
  readonly text: string;
}

/** Per-render state: the source being rendered and what rendering found. */
// A type alias, not an interface, so it satisfies markdown-it's indexable `Env`.
export type RenderEnv = {
  /** Repository-relative path of the Markdown source, such as `spec/04-type-system.md`. */
  readonly source: string;
  /** Rewrites one link destination written in `source` to a site URL. */
  readonly resolveLink: (href: string, source: string) => string;
  /** Builds the playground URL that opens `code`. */
  readonly playgroundUrl: (code: string) => string;
  readonly headings: Heading[];
  readonly slugCounts: Map<string, number>;
  /** The site-wide rule index that ```ebnf rule references link through. */
  readonly grammar?: GrammarIndex;
};

/**
 * GitHub's heading anchor algorithm, as `spec/check-spec-anchors.ts` applies
 * it to the raw heading text: the specification's cross-links depend on it.
 */
export function githubSlug(text: string): string {
  return text
    .replaceAll(/<[^>]+>/g, "")
    .trim()
    .toLowerCase()
    .replaceAll(/[`*_~]/g, "")
    .replaceAll(/[^\p{L}\p{N}_\- ]/gu, "")
    .replaceAll(" ", "-");
}

export function escapeHtml(text: string): string {
  return text
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

const parsesCache = new Map<string, boolean>();

/** Whether the reference parser accepts `code` as a complete source file. */
export function parsesAsHd(code: string): boolean {
  let parses = parsesCache.get(code);
  if (parses === undefined) {
    try {
      parses = parseSource(code).length === 0;
    } catch {
      parses = false;
    }
    parsesCache.set(code, parses);
  }
  return parses;
}

// hd constructs that do not occur in the prose, diagnostics, and file
// listings that the sources also put in ```text fences.
const HD_MARKERS =
  /:=|->|=>|^\s*(?:fn|pub|data|enum|trait|impl|let|use|match|type|annotate|test|for|while|if|return)\b|^\s*@[A-Za-z]|\bfn\s*\(|\$\.|\b[A-Z][A-Za-z0-9]*\[/m;

/**
 * Whether a fenced block holds hd source. ```hd always does; a ```text block
 * does when the reference parser accepts it or it contains an hd construct.
 */
export function isHdBlock(info: string, code: string): boolean {
  if (info === "hd") return true;
  if (info !== "text") return false;
  return parsesAsHd(code) || HD_MARKERS.test(code);
}

/** Renders hd source as spans carrying the repository highlighter's token classes. */
export function highlightHd(code: string): string {
  return code
    .split("\n")
    .map((line) =>
      classify(line)
        .map(({ text, kind }) =>
          kind === "plain"
            ? escapeHtml(text)
            : `<span class="hl-${kind}">${escapeHtml(text)}</span>`,
        )
        .join(""),
    )
    .join("\n");
}

/** The URL of rule `name`'s definition on page `target`, from the page `env` renders. */
function ruleUrl(env: RenderEnv, target: string, name: string): string {
  const fragment = `#${ruleAnchor(name)}`;
  if (target === env.source) return fragment;
  const relative = posix.relative(posix.dirname(env.source), target);
  return env.resolveLink(relative + fragment, env.source);
}

/**
 * Renders an EBNF block as spans with `eb-*` token classes. A rule's first
 * definition on a page carries its anchor and links to the canonical
 * definition (itself, on the page that owns the rule). A name in a rule body
 * links to its definition on this page, else to the canonical one.
 */
export function highlightEbnf(code: string, env: RenderEnv): string {
  return tokenizeEbnf(code)
    .map(({ text, kind }) => {
      const html = escapeHtml(text);
      if (kind === "plain") return html;
      if (kind === "definition") {
        const id = ruleAnchor(text);
        if (env.headings.some((heading) => heading.id === id))
          throw new Error(`${env.source}: heading id ${id} collides with a grammar rule anchor`);
        const first = !env.slugCounts.has(id);
        if (first) env.slugCounts.set(id, 1);
        const canonical = env.grammar?.definitions.get(text)?.[0] ?? env.source;
        const restated = canonical !== env.source;
        const href = restated ? ruleUrl(env, canonical, text) : `#${id}`;
        const idAttribute = first ? ` id="${escapeHtml(id)}"` : "";
        const title = restated ? ` title="Canonical definition: ${escapeHtml(canonical)}"` : "";
        return `<a class="eb-definition" href="${escapeHtml(href)}"${idAttribute}${title}>${html}</a>`;
      }
      if (kind === "reference" || kind === "token") {
        const target = env.grammar && ruleTarget(env.grammar, text, env.source);
        if (target !== undefined)
          return `<a class="eb-${kind}" href="${escapeHtml(ruleUrl(env, target, text))}">${html}</a>`;
      }
      return `<span class="eb-${kind}">${html}</span>`;
    })
    .join("");
}

function inlineText(token: Token): string {
  return (token.children ?? [])
    .filter((child) => child.type === "text" || child.type === "code_inline")
    .map((child) => child.content)
    .join("");
}

function assignHeadingIds(tokens: Token[], env: RenderEnv): void {
  for (const [index, token] of tokens.entries()) {
    if (token.type !== "heading_open") continue;
    const inline = tokens[index + 1]!;
    const base = githubSlug(inline.content);
    const seen = env.slugCounts.get(base) ?? 0;
    env.slugCounts.set(base, seen + 1);
    const id = seen === 0 ? base : `${base}-${seen}`;
    token.attrSet("id", id);
    env.headings.push({ level: Number(token.tag.slice(1)), id, text: inlineText(inline) });
  }
}

/** Whether `code` declares `main`, which makes it a whole program rather than REPL input. */
export function isWholeProgram(code: string): boolean {
  return /^(?:pub\s+)?fn\s+main!?\s*\(/m.test(code);
}

/**
 * The action on a code block that parses. A snippet gets a "Try in REPL"
 * button, which the REPL panel script reveals and handles; it evaluates the
 * snippet as REPL input. A whole program cannot be REPL input, because the
 * REPL supplies its own `main`, so it links to the playground instead.
 */
function tryAction(code: string, env: RenderEnv): string {
  if (!parsesAsHd(code)) return "";
  if (isWholeProgram(code))
    return `<a class="try-link try-playground" href="${escapeHtml(env.playgroundUrl(code))}">Open in playground</a>`;
  return '<button type="button" class="try-link try-repl" title="Evaluate this code in the REPL panel">Try in REPL</button>';
}

// markdown-it types the render environment loosely; every render here passes a RenderEnv.
const asRenderEnv = (env: unknown): RenderEnv => env as RenderEnv;

export function createMarkdown(): MarkdownIt {
  const md = markdownIt({ html: false, linkify: false, typographer: false });
  md.core.ruler.push("heading_ids", (state) => {
    assignHeadingIds(state.tokens, asRenderEnv(state.env));
  });

  md.renderer.rules.heading_open = (tokens, index, options, _env, self) => {
    const id = String(tokens[index]!.attrGet("id") ?? "");
    const anchor = `<a class="heading-anchor" href="#${escapeHtml(id)}" aria-label="Link to this section">#</a>`;
    return self.renderToken(tokens, index, options) + anchor;
  };

  md.renderer.rules.link_open = (tokens, index, options, env, self) => {
    const token = tokens[index]!;
    const renderEnv = asRenderEnv(env);
    const href = token.attrGet("href");
    if (href !== null) token.attrSet("href", renderEnv.resolveLink(String(href), renderEnv.source));
    return self.renderToken(tokens, index, options);
  };

  md.renderer.rules.table_open = () => '<div class="table-wrap"><table>\n';
  md.renderer.rules.table_close = () => "</table></div>\n";

  md.renderer.rules.fence = (tokens, index, _options, env) => {
    const token = tokens[index]!;
    const info = token.info.trim().split(/\s+/, 1)[0] ?? "";
    const code = token.content.replace(/\n$/, "");
    if (info === "ebnf")
      return `<pre class="code ebnf"><code class="language-ebnf">${highlightEbnf(code, asRenderEnv(env))}</code></pre>\n`;
    if (!isHdBlock(info, code)) {
      const language = info === "" ? "" : ` class="language-${escapeHtml(info)}"`;
      return `<pre class="code"><code${language}>${escapeHtml(code)}</code></pre>\n`;
    }
    return `<div class="code-block"><pre class="code hd"><code class="language-hd">${highlightHd(code)}</code></pre>${tryAction(code, asRenderEnv(env))}</div>\n`;
  };
  return md;
}

/** Extracts every fenced block with its info string and starting line. */
export function fencedBlocks(
  md: MarkdownIt,
  markdown: string,
): { info: string; code: string; line: number }[] {
  return md
    .parse(markdown, { headings: [], slugCounts: new Map() })
    .filter((token: Token) => token.type === "fence")
    .map((token: Token) => ({
      info: token.info.trim().split(/\s+/, 1)[0] ?? "",
      code: token.content,
      line: (token.map?.[0] ?? 0) + 1,
    }));
}
