// Hand-written declarations for the public exports of core.mjs that the
// TypeScript side uses. The host itself is plain JavaScript.

import type { End } from "./journal.mjs";

/** Whether text can be an `Http` request URL: printable ASCII, and no
 * percent-encoded host (std-http.url.valid). */
export function isAsciiUrl(text: string): boolean;
/** The lowercase host as written, without URL parser normalization. */
export function writtenHost(text: string): string;

/** Read-only Http authority and raw-URL helpers from the owning runtime. */
export interface ProviderHttpPolicy {
  readonly coversHost: (host: string, port: number) => boolean;
  readonly isAsciiUrl: (text: string) => boolean;
  readonly writtenHost: (text: string) => string;
}

/** Read-only Console authority from the owning runtime. */
export interface ProviderConsolePolicy {
  readonly allows: () => boolean;
}

export interface Sink {
  out(text: string): void;
  err(text: string): void;
}

/** What the providers hand out; the fields a platform layer reads
 * (stdin, tempDir, snapshot, programs) are in `HostEnv` of node.d.mts. */
export interface CoreEnv {
  args?: string[];
  program?: string;
  grants?: Record<string, boolean | string[]>;
  module?: WebAssembly.Module;
  packageBindings?: ((runtime: unknown) => WebAssembly.Imports) | null;
  seed?: number | null;
  runner?: unknown;
  providers?:
    | ((io: {
        put: (e: unknown) => number;
        Enc: unknown;
        pending: (at: number, done: (() => void) | null) => [number, number];
        finish: (h: number) => void;
      }) => Record<string, unknown>)
    | null;
}

/** The checked nominal interface that owns a provider method. */
export interface ProviderInterfaceIdentity {
  readonly package: string;
  readonly version: string;
  readonly trait_path: string;
}

/** The trailing argument supplied to a generated package-provider method. */
export interface ProviderContext {
  readonly interfaceIdentity: ProviderInterfaceIdentity;
  readonly http: ProviderHttpPolicy;
  readonly console: ProviderConsolePolicy;
  signal: AbortSignal;
  instanceSignal: AbortSignal;
  /** Owned readiness, cancellation and delivery hooks; no retained Promise is required. */
  pending<T>(
    poll: () => boolean,
    done: () => T,
    abort?: (() => void) | null,
    finish?: (() => T | undefined) | null,
  ): unknown;
}

/** A pollable operation accepted by builtin and package providers in this core. */
export function pendingOperation<T>(
  poll: () => boolean,
  done: () => T,
  abort?: (() => void) | null,
  finish?: (() => T | undefined) | null,
): unknown;

export interface RunResult {
  status: number;
  trapped: boolean;
  ended?: boolean;
  frames?: [number, number][];
  /** A trap's panic as the host recorded it, or null. */
  panic?: { category: string; message: string } | null;
}

export interface Host {
  imports: WebAssembly.Imports;
  run(instance: WebAssembly.Instance, init: string, poll: string): Promise<RunResult>;
  /** Select synchronous, ready-only driving for this host; creates no Promise or timer. */
  createDriver(instance: WebAssembly.Instance, init?: string, poll?: string): StepDriver;
  prime(instance: WebAssembly.Instance): void;
  close(): void;
  /** The host handles still open, as nonzero `[kind, count]` pairs. */
  leaks(): [string, number][];
  args: string[];
  program: string;
  grant: unknown;
  seed: number | null;
  tempDir?: () => string;
}

export interface StepDriver {
  /** Poll once, then publish ready wakes. A nonzero wake count calls for another turn. */
  step(): { status: number; wakes: number };
}

export class Deadlock extends Error {}
export class EndCase extends Error {}
export class HostContract extends Error {}
export function timeLimitReport(ms: number): string;

export function createCoreHost(sink: Sink, env?: CoreEnv, layer?: unknown): Host;

/** How a REPL session's new input ended (`createSession`'s `run`). */
export interface InputOutcome {
  /** 0, the input's own status, or 3 after a panic; null when it failed to run. */
  status: number | null;
  /** Its `End` row. */
  end: End;
  /** Whether it ended with status 0 and joined the session. */
  joined: boolean;
  /** Why it could not run, or null. */
  error?: string | null;
}

/** What one run of a session program replayed, skipped and dropped, by input number. */
export interface Ran {
  kept: number[];
  skipped: number[];
  dropped: number[];
  reason: string | null;
  /** How the new input ended; null when none was asked for or it did not run. */
  live: InputOutcome | null;
}

export interface Session {
  run(
    wasm: Uint8Array,
    sources: Iterable<readonly [number, string]>,
    live?: number | null,
    journal?: Uint8Array | null,
  ): Promise<Ran>;
  /** Forget initialized guest inputs and history, retaining this provider. */
  clear(): void;
  journal(): Uint8Array | null;
  setJournalCap(cap: number): void;
  takeJournalNotice(): boolean;
  stop(): void;
  hostCalls(): number;
  checkpointStats(): {
    epochs: number;
    archivedJournals: number;
    archivedBytes: number;
    snapshotBytes: number;
    activeJournalBytes: number;
  };
  close(): void;
}

export function createSession(
  sink: Sink,
  env?: CoreEnv,
  layer?: unknown,
  options?: { journalCap?: number },
): Session;
