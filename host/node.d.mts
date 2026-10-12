// Hand-written declarations for the public exports of node.mjs that the
// TypeScript side uses.

import type { CoreEnv, Host, Sink } from "./core.mjs";

export { Deadlock, EndCase, timeLimitReport } from "./core.mjs";
export type { Host, Sink } from "./core.mjs";

export interface HostEnv extends CoreEnv {
  /** A file descriptor, or `null` for a closed standard input. */
  stdin?: number | null;
  tempDir?: string;
  snapshot?: { dir: string; shown?: string; slug: string; update: boolean };
  programs?: Record<string, { host: string; wasm: string; config: string; cwd: string }>;
}

export function createHost(sink: Sink, env?: HostEnv): Host;
