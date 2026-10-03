// The "claim -> minimal code -> run" card: a benefit headline, a short snippet
// with its punchline lines marked, an optional compiler-output pane, and a link
// that runs the full example. The landing page uses it, and a tour page can
// reuse claimCard() or markedCode() as is.

import { escapeHtml, highlightHd } from "./markdown.ts";

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

export interface Claim {
  /** The benefit headline. */
  readonly title: string;
  /** One sentence that says what the snippet proves. Backticks mark code. */
  readonly blurb: string;
  /** An optional one-line contrast, such as how another language does it. Backticks mark code. */
  readonly contrast?: string;
  readonly code: string;
  /** The punchline lines, matched against each code line's trimmed text. */
  readonly marks: readonly string[];
  readonly output?: ClaimOutput;
  /** Where "Run the full example" goes. */
  readonly href: string;
}

/**
 * hd code as a highlighted `pre`, with each line whose trimmed text is in
 * `marks` wrapped in a `line-mark` span. Fails when a mark matches no line.
 */
export function markedCode(code: string, marks: readonly string[], className = ""): string {
  const lines = code.split("\n");
  for (const mark of marks)
    if (!lines.some((line) => line.trim() === mark))
      throw new Error(`marked line not in the snippet: ${mark}`);
  const html = lines
    .map((line) => {
      const highlighted = highlightHd(line);
      return marks.includes(line.trim())
        ? `<span class="line-mark">${highlighted}</span>`
        : highlighted;
    })
    .join("\n");
  const classes = ["code", "hd", "marked-code", className].filter(Boolean).join(" ");
  return `<pre class="${classes}"><code class="language-hd">${html}</code></pre>`;
}

/** Escapes `text` and renders each backtick span in it as inline code. */
function inlineCode(text: string): string {
  return escapeHtml(text).replaceAll(/`([^`]+)`/g, "<code>$1</code>");
}

export function claimCard(claim: Claim, headingLevel: 2 | 3 = 3): string {
  const count = claim.code.split("\n").length;
  if (count > CLAIM_MAX_LINES)
    throw new Error(
      `${claim.title}: ${count} lines; a claim snippet has at most ${CLAIM_MAX_LINES}`,
    );
  if (claim.marks.length === 0 || claim.marks.length > CLAIM_MAX_MARKS)
    throw new Error(`${claim.title}: mark 1 to ${CLAIM_MAX_MARKS} punchline lines`);
  const h = `h${headingLevel}`;
  const contrast = claim.contrast
    ? `<p class="claim-contrast">${inlineCode(claim.contrast)}</p>`
    : "";
  const output = claim.output
    ? `<figure class="claim-output claim-output-${claim.output.kind}">
<figcaption>${escapeHtml(claim.output.caption)}</figcaption>
<pre><code>${claim.output.lines.map(escapeHtml).join("\n")}</code></pre>
</figure>`
    : "";
  return `<article class="claim">
<${h} class="claim-title">${escapeHtml(claim.title)}</${h}>
<p class="claim-blurb">${inlineCode(claim.blurb)}</p>
${markedCode(claim.code, claim.marks, "claim-code")}
${output}${contrast}
<p class="claim-run"><a href="${escapeHtml(claim.href)}">Run the full example <span aria-hidden="true">→</span></a></p>
</article>`;
}
