// Where a runtime panic happened (spec/lang/06-control-flow.md#r-flow.panic.report).
// The emitter annotates the program's operations as panic sites
// (emitter/panic-sites.ts), and assembly maps their code offsets
// (src/wasm.ts). A panic raised in a host import is located here, from the
// Wasm frames under the import in a stack trace.

import type { SourceSpan } from "./diagnostics.ts";
import { fallbackNote, RuntimePanicError, type PanicSite } from "./runtime-panic.ts";
import { siteAt, type SiteMap } from "./wasm.ts";

type HostImport = (...arguments_: unknown[]) => unknown;

/** A compilation's panic sites, and where its Wasm code puts them. */
interface LocatedArtifact {
  readonly sites?: readonly PanicSite[];
  readonly siteMap?: SiteMap;
}

/**
 * Wraps a host import so that a panic it raises names the program operation
 * that called it: `where` shows a span as `file:line:col`. An import that
 * cannot panic needs no wrapper.
 */
export function panicLocator(
  artifact: LocatedArtifact,
  where: ((span: SourceSpan) => string) | undefined,
): (host: HostImport) => HostImport {
  const show = where ?? ((span: SourceSpan) => `${span.start.line}:${span.start.column}`);
  const locate = (error: unknown): void => {
    const { siteMap, sites } = artifact;
    if (!(error instanceof RuntimePanicError) || error.site || !siteMap || !sites) return;
    // A panic is rare, so a deep trace costs nothing a run notices.
    const limit = Error.stackTraceLimit;
    Error.stackTraceLimit = 200;
    const stack = new Error().stack ?? "";
    Error.stackTraceLimit = limit;
    const site = stackPanicSite(stack, siteMap, sites);
    if (!site) return;
    const fallback = error.code === "integer-overflow" ? site.fallback : undefined;
    error.locate(site, show(site.span), fallback ? [fallbackNote(fallback, show)] : []);
  };
  return (host) =>
    (...arguments_) => {
      try {
        return host(...arguments_);
      } catch (error) {
        locate(error);
        throw error;
      }
    };
}

/**
 * The panic site of the innermost frame of the program's own Wasm code in
 * `stack`, a V8 or SpiderMonkey stack trace taken inside a host import. The
 * frames name their byte offset in the module, which `map` maps to a site.
 */
function stackPanicSite(
  stack: string,
  map: SiteMap,
  sites: readonly PanicSite[],
): PanicSite | undefined {
  let inWasm = false;
  for (const line of stack.split("\n")) {
    const frame = /wasm-function\[\d+\]:0x([0-9a-f]+)/.exec(line);
    // The Wasm frames under the import are this module's; a host frame below
    // them is the caller of its export.
    if (!frame) {
      if (inWasm) return undefined;
      continue;
    }
    inWasm = true;
    const site = siteAt(map, Number.parseInt(frame[1]!, 16));
    if (site !== undefined) return sites[site];
  }
  return undefined;
}
