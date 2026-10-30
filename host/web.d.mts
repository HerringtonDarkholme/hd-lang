// Hand-written declarations for the public exports of web.mjs that the
// TypeScript side uses.

import type { TestFiles } from "./cases.mjs";
import type { CoreEnv, Host, Session, Sink } from "./core.mjs";

export type { Host, Session, Sink } from "./core.mjs";

export interface BrowserRequest {
  method: string;
  url: string;
  headers: readonly (readonly [string, string])[];
  body: Uint8Array;
  /** Milliseconds, or `null` for none. */
  timeout: number | null;
  /** A derived provider's transport settings, which the browser refuses. */
  settings?: {
    dnsServers: string[] | null;
    tlsRoots: string[];
    tlsIdentity: { certificateChain: string; privateKey: string } | null;
    connectTimeout: number | null;
    tlsHandshakeTimeout: number | null;
    responseHeaderTimeout: number | null;
    bodyReadTimeout: number | null;
  };
}

export type BrowserAnswer =
  | { status: number; headers: [string, string][]; body: Uint8Array; error?: undefined }
  | { error: string; text: string };

export function browserSend(request: BrowserRequest): BrowserAnswer;
/** A test case's `tempDir` and the run's `files` add to `CoreEnv`. */
export function createWebHost(
  sink: Sink,
  env?: CoreEnv & { tempDir?: string | null; files?: TestFiles },
): Host;
/** A REPL session in the browser, over the providers of `createWebHost`. */
export function createWebSession(
  sink: Sink,
  env?: CoreEnv,
  options?: { journalCap?: number },
): Session;
export function memoryFiles(): TestFiles;
export function unboundTraits(module: WebAssembly.Module, host: Host): string[];
