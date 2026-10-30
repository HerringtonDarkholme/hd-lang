// Hand-written declarations for the public exports of node.ts that the
// TypeScript side uses.

import type { CoreEnv, Host, Session, Sink } from "./core.ts";

export { Deadlock, EndCase, timeLimitReport } from "./core.ts";
export type { Host, Session, Sink } from "./core.ts";

export interface HostEnv extends CoreEnv {
  /** A file descriptor, or `null` for a closed standard input. */
  stdin?: number | null;
  /** Explicit hd executable used for Windows Process Job ownership. */
  processLauncher?: string;
  tempDir?: string;
  snapshot?: { dir: string; shown?: string; slug: string; update: boolean };
  programs?: Record<string, { host: string; wasm: string; config: string; cwd: string }>;
}

export function createHost(sink: Sink, env?: HostEnv): Host;
export function createNodeSession(
  sink: Sink,
  env?: HostEnv,
  options?: { journalCap?: number },
): Session;
