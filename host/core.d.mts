// Hand-written declarations for the public exports of core.mjs that the
// TypeScript side uses. The host itself is plain JavaScript.

export interface Sink {
  out(text: string): void;
  err(text: string): void;
}

/** What the providers hand out; the fields a platform layer reads
 * (stdin, tempDir, snapshot, programs) are in `HostEnv` of node.d.mts. */
export interface CoreEnv {
  args?: string[];
  program?: string;
  grants?: Record<string, false | string[]>;
  seed?: number | null;
  runner?: unknown;
  providers?:
    | ((io: { put: (e: unknown) => number; Enc: unknown }) => Record<string, unknown>)
    | null;
}

export interface RunResult {
  status: number;
  trapped: boolean;
  ended?: boolean;
  frames?: [number, number][];
}

export interface Host {
  imports: WebAssembly.Imports;
  run(instance: WebAssembly.Instance, init: string, poll: string): Promise<RunResult>;
  close(): void;
  args: string[];
  program: string;
  grant: unknown;
  seed: number | null;
  tempDir?: () => string;
}

export class Deadlock extends Error {}
export class EndCase extends Error {}
export class HostContract extends Error {}
export function timeLimitReport(ms: number): string;

export function createCoreHost(sink: Sink, env?: CoreEnv, layer?: unknown): Host;
