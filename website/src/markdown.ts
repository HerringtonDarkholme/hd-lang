import { posix } from "node:path";

import markdownIt, { type MarkdownIt, type Token } from "markdown-it";
import footnote from "markdown-it-footnote";

import { isErrorExample, RULE_ID, ruleIdAnchor } from "../../spec/tools/spec-prose.ts";
import { classify } from "../../src/highlight.ts";
import { parse } from "../../src/parser/index.ts";
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
  /** Repository-relative path of the Markdown source, such as `spec/lang/04-type-system.md`. */
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
function githubSlug(text: string): string {
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

/**
 * Whether the compiler's parser accepts `code` as a complete source file. The
 * site runs its examples on this compiler (the REPL and the playground), so it
 * asks the same parser in process; one CLI call per block would be too slow.
 */
function parsesAsHd(code: string): boolean {
  let parses = parsesCache.get(code);
  if (parses === undefined) {
    try {
      parses = parse(code).program !== undefined;
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
  /:=|->|=>|^\s*(?:fn|pub|data|enum|trait|impl|let|use|match|type|test|for|while|if|return)\b|^\s*@[A-Za-z]|\bfn\s*\(|\$\.|\b[A-Z][A-Za-z0-9]*\[/m;

/**
 * Whether a fenced block holds hd source. ```hd always does; a ```text block
 * does when the compiler's parser accepts it or it contains an hd construct.
 */
function isHdBlock(info: string, code: string): boolean {
  if (info === "hd") return true;
  if (info !== "text") return false;
  return parsesAsHd(code) || HD_MARKERS.test(code) || isErrorExample(code);
}

/** A line an error example marks as rejected: it ends in `# error` or `# error: CODE`. */
const ERROR_LINE = /#\s*error(?::\s*[a-z0-9-]+)?\s*$/;

/**
 * Renders hd source as spans carrying the repository highlighter's token
 * classes. A line that an error example marks as rejected is wrapped in a
 * `line-error` span, and its marker comment gets `hl-error-marker`.
 */
export function highlightHd(code: string): string {
  return code
    .split("\n")
    .map((line) => {
      const rejected = ERROR_LINE.test(line);
      const html = classify(line)
        .map(({ text, kind }) => {
          if (kind === "plain") return escapeHtml(text);
          const marker = rejected && kind === "comment" && ERROR_LINE.test(text);
          const classes = marker ? `hl-${kind} hl-error-marker` : `hl-${kind}`;
          return `<span class="${classes}">${escapeHtml(text)}</span>`;
        })
        .join("");
      return rejected ? `<span class="line-error">${html}</span>` : html;
    })
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
function highlightEbnf(code: string, env: RenderEnv): string {
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

/** Gives each body cell of a table its column's header text, for the phone layout. */
function labelCells(tableTokens: Token[]): void {
  const headers: string[] = [];
  let column = 0;
  for (const [index, token] of tableTokens.entries()) {
    if (token.type === "th_open") headers.push(tableTokens[index + 1]!.content.replaceAll("`", ""));
    if (token.type === "tr_open") column = 0;
    if (token.type === "td_open") {
      const header = headers[column++];
      if (header) token.attrSet("data-label", header);
    }
  }
}

/** A rule ID marker opening a block or table cell, as spec/STYLE.md defines it. */
const RULE_MARKER = /^r\[([^\]\s]+)\]\s*/;

/**
 * Turns each rule ID marker that opens a paragraph, list item, or table cell
 * into a `rule_id` token, which renders as a linkable anchor. A table with a
 * rule in it becomes a rule table. A block quote that opens with a bold
 * "Why." or "Note." becomes a callout.
 */
function markRulesAndCallouts(tokens: Token[], env: RenderEnv, TokenClass: typeof Token): void {
  let table: Token | undefined;
  let tableStart = 0;
  for (const [index, token] of tokens.entries()) {
    if (token.type === "table_open") {
      table = token;
      tableStart = index;
    }
    if (token.type === "table_close") {
      if (table?.attrGet("class") === "rule-table") labelCells(tokens.slice(tableStart, index));
      table = undefined;
    }
    if (token.type === "blockquote_open") {
      const inline = tokens[index + 2];
      const callout = /^\*\*(Why|Note)\.\*\*/.exec(inline?.content ?? "");
      if (tokens[index + 1]?.type === "paragraph_open" && callout)
        token.attrSet("class", `callout callout-${callout[1]!.toLowerCase()}`);
    }
    if (token.type !== "inline") continue;
    const opener = tokens[index - 1]?.type;
    if (opener !== "paragraph_open" && opener !== "td_open" && opener !== "th_open") continue;
    const marker = RULE_MARKER.exec(token.content);
    if (!marker || !RULE_ID.test(marker[1]!)) continue;
    const id = ruleIdAnchor(marker[1]!);
    if (env.slugCounts.has(id)) throw new Error(`${env.source}: duplicate rule ID ${marker[1]}`);
    env.slugCounts.set(id, 1);
    // Drop the marker's text from the leading text children, then put the anchor first.
    let remaining = marker[0].length;
    const children = token.children ?? [];
    while (remaining > 0 && children[0]?.type === "text") {
      const child = children[0];
      if (child.content.length <= remaining) {
        remaining -= child.content.length;
        children.shift();
      } else {
        child.content = child.content.slice(remaining);
        remaining = 0;
      }
    }
    const anchor = new TokenClass("rule_id", "", 0);
    anchor.meta = { id: marker[1] };
    children.unshift(anchor);
    token.children = children;
    if (opener !== "paragraph_open" && table) table.attrSet("class", "rule-table");
  }
}

/** Whether `code` declares `main`, which makes it a whole program rather than REPL input. */
function isWholeProgram(code: string): boolean {
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

/**
 * The anchor suffix of a footnote: its label when the label is a plain word,
 * as in `[^miku]`, else its number. A second reference to the same footnote
 * adds its index, so each reference has its own back-link target.
 */
function footnoteName(token: Token, withSubId: boolean): string {
  const { id, label, subId } = token.meta as { id: number; label?: string; subId?: number };
  const name = label !== undefined && /^[A-Za-z0-9_-]+$/.test(label) ? label : String(id + 1);
  return withSubId && subId ? `${name}-${subId + 1}` : name;
}

/**
 * Renders footnotes quietly: a small superscript number links to the note,
 * and the notes gather in a footnotes section at the end of the page, each
 * with a back-link to where it is referenced.
 */
function renderFootnotes(md: MarkdownIt): void {
  md.use(footnote);
  const rules = md.renderer.rules;
  /** Claims anchor `id` on the page, failing when a heading slug or rule anchor holds it. */
  const claim = (env: unknown, id: string): string => {
    const renderEnv = asRenderEnv(env);
    if (renderEnv.slugCounts.has(id))
      throw new Error(`${renderEnv.source}: footnote anchor ${id} collides with another anchor`);
    renderEnv.slugCounts.set(id, 1);
    return escapeHtml(id);
  };
  rules.footnote_ref = (tokens, index, _options, env) => {
    const token = tokens[index]!;
    const note = escapeHtml(`fn-${footnoteName(token, false)}`);
    const ref = claim(env, `fnref-${footnoteName(token, true)}`);
    const number = Number((token.meta as { id: number }).id) + 1;
    return `<sup class="footnote-ref"><a href="#${note}" id="${ref}" aria-label="Footnote ${number}">${number}</a></sup>`;
  };
  rules.footnote_block_open = () =>
    '<section class="footnotes" aria-label="Footnotes">\n<ol class="footnotes-list">\n';
  rules.footnote_block_close = () => "</ol>\n</section>\n";
  rules.footnote_open = (tokens, index, _options, env) =>
    `<li id="${claim(env, `fn-${footnoteName(tokens[index]!, false)}`)}" class="footnote-item">`;
  rules.footnote_close = () => "</li>\n";
  rules.footnote_anchor = (tokens, index) => {
    const ref = escapeHtml(footnoteName(tokens[index]!, true));
    // U+FE0E keeps the arrow from rendering as an emoji on iOS.
    return ` <a href="#fnref-${ref}" class="footnote-backref" aria-label="Back to the reference">\u21a9\ufe0e</a>`;
  };
}

export function createMarkdown(): MarkdownIt {
  const md = markdownIt({ html: false, linkify: false, typographer: false });
  renderFootnotes(md);
  md.core.ruler.push("heading_ids", (state) => {
    assignHeadingIds(state.tokens, asRenderEnv(state.env));
  });
  md.core.ruler.push("rule_ids_and_callouts", (state) => {
    markRulesAndCallouts(state.tokens, asRenderEnv(state.env), state.Token);
  });

  md.renderer.rules.rule_id = (tokens, index) => {
    const id = String(tokens[index]!.meta?.id);
    const anchor = escapeHtml(ruleIdAnchor(id));
    return `<a class="rule-id" id="${anchor}" href="#${anchor}" title="Rule ${escapeHtml(id)}">${escapeHtml(id)}</a>`;
  };

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

  md.renderer.rules.table_open = (tokens, index) => {
    const kind = tokens[index]!.attrGet("class");
    return `<div class="table-wrap"><table${kind ? ` class="${escapeHtml(String(kind))}"` : ""}>\n`;
  };
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
    if (isErrorExample(code))
      return `<div class="code-block error-example"><div class="example-label">Error example</div><pre class="code hd"><code class="language-hd">${highlightHd(code)}</code></pre>${tryAction(code, asRenderEnv(env))}</div>\n`;
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
