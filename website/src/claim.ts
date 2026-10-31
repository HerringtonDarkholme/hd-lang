// The "claim -> minimal code -> run" card: a benefit headline, a short snippet
// with its punchline lines marked, an optional compiler-output pane, and a link
// that runs the full example. The landing page uses it, and a tour page can
// reuse claimCard() or markedCode() as is.

import { escapeHtml, highlightHd, inlineCode } from "./markdown.ts";

/** A snippet longer than this is a tutorial, not a claim. */
export const CLAIM_MAX_LINES = 8;

/** At most this many punchline lines, so the eye finds them at once. */
export const CLAIM_MAX_MARKS = 2;

export interface ClaimOutput {
  /** What produced the output, such as "hd check, after adding a variant". */
  readonly caption: string;
  /** The output text, one entry per line, copied from the compiler. */
  readonly lines: readonly string[];
  /** `error` tints the pane as a diagnostic. */
  readonly kind: "error" | "output";
}

/**
 * Code and output lines that belong to one item outside the card, such as an
 * entry in a list beside it. site.js tints an item and its lines together.
 */
export interface ClaimLink {
  readonly key: string;
  /** Code lines, matched by their trimmed text. */
  readonly code: readonly string[];
  /** Output lines, matched exactly. */
  readonly output?: readonly string[];
}

export interface Claim {
  /** The benefit headline. Backticks mark code. */
  readonly title: string;
  /** One sentence that says what the snippet proves. Backticks mark code. */
  readonly blurb: string;
  /** An optional one-line contrast, such as how another language does it. Backticks mark code. */
  readonly contrast?: string;
  readonly code: string;
  /**
   * The punchline lines, matched against each code line's trimmed text. A
   * claim with links may have none: the linked lines take their place.
   */
  readonly marks: readonly string[];
  readonly links?: readonly ClaimLink[];
  readonly output?: ClaimOutput;
  /** Where "Run the full example" goes. */
  readonly href: string;
}

/**
 * `lines`, each wrapped in a span carrying the keys of the links that name
 * it (in `data-link`, space-separated); `html` is the lines as rendered.
 * Code lines match a link's `code` by their trimmed text, and output lines
 * match its `output` exactly. Fails when a linked line is not there.
 */
function linkedLines(
  lines: readonly string[],
  html: readonly string[],
  links: readonly ClaimLink[],
  part: "code" | "output",
): string[] {
  const text = (line: string): string => (part === "code" ? line.trim() : line);
  for (const link of links) {
    for (const wanted of link[part] ?? []) {
      if (!lines.some((line) => text(line) === wanted)) {
        throw new Error(`linked line not in the snippet: ${wanted}`);
      }
    }
  }
  return lines.map((line, index) => {
    const keys = links
      .filter((link) => (link[part] ?? []).includes(text(line)))
      .map((link) => link.key);
    return keys.length === 0
      ? html[index]!
      : `<span class="line-link" data-link="${keys.join(" ")}">${html[index]!}</span>`;
  });
}

/**
 * hd code as a highlighted `pre`, with each line whose trimmed text is in
 * `marks` wrapped in a `line-mark` span, and each line a link names wrapped
 * in a `line-link` span. Fails when a mark or a link matches no line.
 */
export function markedCode(
  code: string,
  marks: readonly string[],
  className = "",
  links: readonly ClaimLink[] = [],
): string {
  const lines = code.split("\n");
  for (const mark of marks) {
    if (!lines.some((line) => line.trim() === mark)) {
      throw new Error(`marked line not in the snippet: ${mark}`);
    }
  }
  const marked = lines.map((line) => {
    const highlighted = highlightHd(line);
    return marks.includes(line.trim())
      ? `<span class="line-mark">${highlighted}</span>`
      : highlighted;
  });
  const html = linkedLines(lines, marked, links, "code").join("\n");
  const classes = ["code", "hd", "marked-code", className].filter(Boolean).join(" ");
  return `<pre class="${classes}"><code class="language-hd">${html}</code></pre>`;
}

export function claimCard(claim: Claim, headingLevel: 2 | 3 = 3): string {
  const count = claim.code.split("\n").length;
  if (count > CLAIM_MAX_LINES) {
    throw new Error(
      `${claim.title}: ${count} lines; a claim snippet has at most ${CLAIM_MAX_LINES}`,
    );
  }
  const links = claim.links ?? [];
  if ((claim.marks.length === 0 && links.length === 0) || claim.marks.length > CLAIM_MAX_MARKS) {
    throw new Error(`${claim.title}: mark 1 to ${CLAIM_MAX_MARKS} punchline lines, or link some`);
  }
  const headingTag = `h${headingLevel}`;
  const contrast = claim.contrast
    ? `<p class="claim-contrast">${inlineCode(claim.contrast)}</p>`
    : "";
  const output = claim.output
    ? `<figure class="claim-output claim-output-${claim.output.kind}">
<figcaption>${escapeHtml(claim.output.caption)}</figcaption>
<pre><code>${linkedLines(claim.output.lines, claim.output.lines.map(escapeHtml), links, "output").join("\n")}</code></pre>
</figure>`
    : "";
  return `<article class="claim">
<${headingTag} class="claim-title">${inlineCode(claim.title)}</${headingTag}>
<p class="claim-blurb">${inlineCode(claim.blurb)}</p>
${markedCode(claim.code, claim.marks, "claim-code", links)}
${output}${contrast}
<p class="claim-run"><a href="${escapeHtml(claim.href)}">Run the full example <span aria-hidden="true">→</span></a></p>
</article>`;
}
