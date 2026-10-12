// Hand-written declarations for the public exports of web.mjs that the
// TypeScript side uses.

import type { CoreEnv, Host, Sink } from "./core.mjs";

export type { Host, Sink } from "./core.mjs";

export interface BrowserRequest {
  method: string;
  url: string;
  headers: readonly (readonly [string, string])[];
  body: Uint8Array;
  /** Milliseconds, or `null` for none. */
  timeout: number | null;
}

export type BrowserAnswer =
  | { status: number; headers: [string, string][]; body: Uint8Array; error?: undefined }
  | { error: string; text: string };

export function browserSend(request: BrowserRequest): BrowserAnswer;
export function createWebHost(sink: Sink, env?: CoreEnv): Host;
export function unboundTraits(module: WebAssembly.Module, host: Host): string[];
