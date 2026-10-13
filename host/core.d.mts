// Hand-written declarations for the public exports of core.mjs that the
// TypeScript side uses. The host itself is plain JavaScript.

import type { End } from "./journal.mjs";

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
  prime(instance: WebAssembly.Instance): void;
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

/** How a REPL session's input ended (`createSession`). */
export interface InputOutcome {
  /** 0, the input's own status, or 3 after a panic; null when it did not run. */
  status: number | null;
  /** Its `End` row; null for a module the session could not link. */
  end: End | null;
  /** Whether it ended with status 0 and joined the session. */
  joined: boolean;
  /** Why it could not run, or null. */
  error?: string | null;
}

/** Builds a session's inputs again for a rebuild; input 0 is the prelude. */
export interface Rebuilder {
  build(
    number: number,
  ): { source: string; wasm: Uint8Array } | Promise<{ source: string; wasm: Uint8Array }>;
  replayed?(number: number, joined: boolean): void | Promise<void>;
}

/** What a rebuild kept, skipped and dropped, by input number. */
export interface Rebuilt {
  kept: number[];
  skipped: number[];
  dropped: number[];
  reason: string | null;
}

export interface Session {
  input(number: number, source: string, wasm: Uint8Array): Promise<InputOutcome>;
  rebuild(rebuilder: Rebuilder, journal?: Uint8Array | null): Promise<Rebuilt>;
  journal(): Uint8Array | null;
  setJournalCap(cap: number): void;
  takeJournalNotice(): boolean;
  stop(): void;
  hostCalls(): number;
  close(): void;
}

export function isSessionKey(name: string): boolean;
export function createSession(
  sink: Sink,
  env?: CoreEnv,
  layer?: unknown,
  options?: { journalCap?: number },
): Session;
