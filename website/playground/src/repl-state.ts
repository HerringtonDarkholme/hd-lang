// What a REPL session keeps across a page reload and in a share link
// (`f1c55020:future-work/compiler/repl-plan.md` job W6): the files it started over, each kept input's number
// and text, the journal of its host calls, and the transcript the page
// showed. A rebuild replays the journal without repeating a host call
// (repl-client.ts), but it drops the output, so the transcript travels too.
//
// Saved sessions live in IndexedDB. Every access is wrapped: a private
// window or blocked storage answers "unavailable" and the page starts a
// fresh session. A share link is `#repl=<mode>.<base64url>`: mode `z` is
// deflate-raw compressed JSON, `j` is plain JSON (a browser without
// CompressionStream).

/** One line of the transcript, by the CSS class the panel gave it. */
export interface TranscriptEntry {
  readonly c: string;
  readonly t: string;
  /** The input number of an echo. */
  readonly n?: number;
}

export interface SessionState {
  /** The session's package files; none on the website. */
  readonly files: Readonly<Record<string, string>>;
  /** Each kept input's number and text, in order; the prelude (0) is not listed. */
  readonly inputs: readonly (readonly [number, string])[];
  /** The number the next input gets. */
  readonly next: number;
  readonly journal: Uint8Array | null;
  readonly transcript: readonly TranscriptEntry[];
}

/** Transcript classes that carry a session's output; notes are left out. */
const SAVED_CLASSES = new Set([
  "repl-echo",
  "repl-out",
  "repl-error",
  "repl-debug",
  "repl-warning",
]);

export const keepsLine = (entry: TranscriptEntry): boolean => SAVED_CLASSES.has(entry.c);

/** At most this many bytes of journal, files, inputs and transcript are saved. */
export const MAX_SAVED_BYTES = 4 * 1024 * 1024;
/** The most transcript lines kept. */
export const MAX_TRANSCRIPT_LINES = 2000;
/** The longest share link, in characters of its hash; longer drops the outputs. */
export const MAX_SHARE_HASH = 12_000;
/** The most a link may expand to when decoded. */
const MAX_DECODED_BYTES = 8 * 1024 * 1024;

export function stateSize(state: SessionState): number {
  let size = state.journal?.length ?? 0;
  for (const source of Object.values(state.files)) {
    size += source.length;
  }
  for (const [, text] of state.inputs) {
    size += text.length;
  }
  for (const entry of state.transcript) {
    size += entry.t.length + 16;
  }
  return size;
}

/** `state` with only the transcript lines worth keeping, the last few. */
export function trimmed(state: SessionState): SessionState {
  const lines = state.transcript.filter(keepsLine);
  return { ...state, transcript: lines.slice(-MAX_TRANSCRIPT_LINES) };
}

// ---- untrusted input -------------------------------------------------------

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

/** Checks a decoded or loaded value and returns it as a state; undefined if it is not one. */
export function asState(value: unknown): SessionState | undefined {
  if (!isRecord(value)) {
    return undefined;
  }
  const { files, inputs, next, journal, transcript } = value;
  if (!isRecord(files) || !Object.values(files).every((text) => typeof text === "string")) {
    return undefined;
  }
  if (!Array.isArray(inputs)) {
    return undefined;
  }
  const checkedInputs: [number, string][] = [];
  for (const item of inputs) {
    if (!Array.isArray(item) || item.length !== 2) {
      return undefined;
    }
    const [number, text] = item as [unknown, unknown];
    if (!Number.isInteger(number) || (number as number) < 1 || typeof text !== "string") {
      return undefined;
    }
    checkedInputs.push([number as number, text]);
  }
  if (!Number.isInteger(next) || (next as number) < 1) {
    return undefined;
  }
  if (journal !== null && !(journal instanceof Uint8Array)) {
    return undefined;
  }
  if (!Array.isArray(transcript)) {
    return undefined;
  }
  const lines: TranscriptEntry[] = [];
  for (const item of transcript) {
    if (!isRecord(item) || typeof item.c !== "string" || typeof item.t !== "string") {
      return undefined;
    }
    if (item.n !== undefined && !Number.isInteger(item.n)) {
      return undefined;
    }
    const entry: TranscriptEntry =
      item.n === undefined
        ? { c: item.c, t: item.t }
        : { c: item.c, t: item.t, n: item.n as number };
    if (keepsLine(entry)) {
      lines.push(entry);
    }
  }
  return {
    files: files as Record<string, string>,
    inputs: checkedInputs,
    next: next as number,
    journal: journal as Uint8Array | null,
    transcript: lines,
  };
}

// ---- IndexedDB -------------------------------------------------------------

const DATABASE = "hd-repl";
const STORE = "sessions";

export type SaveResult = "saved" | "too-big" | "unavailable";

const factory = (): IDBFactory | undefined => {
  try {
    return globalThis.indexedDB ?? undefined;
  } catch {
    return undefined;
  }
};

function request<T>(req: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error ?? new Error("storage failed"));
  });
}

/** Runs `body` on the sessions store; the database is open only for the call. */
async function withStore<T>(
  idb: IDBFactory,
  mode: IDBTransactionMode,
  body: (store: IDBObjectStore) => Promise<T>,
): Promise<T> {
  const opening = idb.open(DATABASE, 1);
  opening.onupgradeneeded = () => {
    if (!opening.result.objectStoreNames.contains(STORE)) {
      opening.result.createObjectStore(STORE);
    }
  };
  const db = await request(opening);
  try {
    return await body(db.transaction(STORE, mode).objectStore(STORE));
  } finally {
    db.close();
  }
}

/** Saves `state` under `key`. Never throws. */
export async function saveState(
  key: string,
  state: SessionState,
  idb: IDBFactory | undefined = factory(),
): Promise<SaveResult> {
  if (!idb) {
    return "unavailable";
  }
  const kept = trimmed(state);
  if (kept.journal === null || stateSize(kept) > MAX_SAVED_BYTES) {
    await forgetState(key, idb);
    return "too-big";
  }
  try {
    await withStore(idb, "readwrite", (store) => request(store.put(kept, key)));
    return "saved";
  } catch {
    return "unavailable";
  }
}

/** The state saved under `key`; undefined when none is, or storage fails. */
export async function loadState(
  key: string,
  idb: IDBFactory | undefined = factory(),
): Promise<SessionState | undefined> {
  if (!idb) {
    return undefined;
  }
  try {
    return asState(await withStore(idb, "readonly", (store) => request(store.get(key))));
  } catch {
    return undefined;
  }
}

/** Deletes the state saved under `key`. Never throws. */
export async function forgetState(
  key: string,
  idb: IDBFactory | undefined = factory(),
): Promise<void> {
  if (!idb) {
    return;
  }
  try {
    await withStore(idb, "readwrite", (store) => request(store.delete(key)));
  } catch {
    // Storage is a convenience; the page works without it.
  }
}

// ---- share links -----------------------------------------------------------

const SHARE = /^#?repl=([zj])\.([A-Za-z0-9_-]*)$/;

function bytesToBase64Url(bytes: Uint8Array): string {
  let binary = "";
  for (let index = 0; index < bytes.length; index += 0x8000) {
    binary += String.fromCharCode(...bytes.subarray(index, index + 0x8000));
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function base64UrlToBytes(encoded: string): Uint8Array {
  const base64 = encoded.replace(/-/g, "+").replace(/_/g, "/");
  const binary = atob(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
  return Uint8Array.from(binary, (character) => character.charCodeAt(0));
}

async function pipe(bytes: Uint8Array, stream: CompressionStream | DecompressionStream) {
  const reader = new Blob([bytes as BlobPart]).stream().pipeThrough(stream).getReader();
  const chunks: Uint8Array[] = [];
  let total = 0;
  for (;;) {
    const { done, value } = await reader.read();
    if (done) {
      break;
    }
    total += value.length;
    if (total > MAX_DECODED_BYTES) {
      await reader.cancel();
      throw new Error("the link expands too far");
    }
    chunks.push(value);
  }
  const out = new Uint8Array(total);
  let at = 0;
  for (const chunk of chunks) {
    out.set(chunk, at);
    at += chunk.length;
  }
  return out;
}

interface Payload {
  v: 1;
  files: Record<string, string>;
  inputs: [number, string][];
  next?: number;
  journal?: string;
  transcript?: { c: string; t: string; n?: number }[];
}

async function encodePayload(payload: Payload): Promise<string> {
  const json = new TextEncoder().encode(JSON.stringify(payload));
  if (typeof CompressionStream === "undefined") {
    return `#repl=j.${bytesToBase64Url(json)}`;
  }
  return `#repl=z.${bytesToBase64Url(await pipe(json, new CompressionStream("deflate-raw")))}`;
}

export interface SharedLink {
  readonly hash: string;
  /** Whether the link carries the outputs (the journal), else inputs only. */
  readonly outputs: boolean;
  /** Whether even the inputs-only link is longer than MAX_SHARE_HASH. */
  readonly long: boolean;
}

/**
 * The link for `state`: inputs, journal and transcript when that fits
 * MAX_SHARE_HASH, else the files and inputs only.
 */
export async function shareLink(state: SessionState): Promise<SharedLink> {
  const kept = trimmed(state);
  const inputs = kept.inputs.map(([n, text]): [number, string] => [n, text]);
  if (kept.journal !== null) {
    const full = await encodePayload({
      v: 1,
      files: { ...kept.files },
      inputs,
      next: kept.next,
      journal: bytesToBase64Url(kept.journal),
      transcript: kept.transcript.map((e) => ({ ...e })),
    });
    if (full.length <= MAX_SHARE_HASH) {
      return { hash: full, outputs: true, long: false };
    }
  }
  const bare = await encodePayload({ v: 1, files: { ...kept.files }, inputs });
  return { hash: bare, outputs: false, long: bare.length > MAX_SHARE_HASH };
}

/** Whether `hash` is a session link, well formed or not. */
export function isShareHash(hash: string): boolean {
  return /^#?repl=/.test(hash.trim());
}

/** Reads a `#repl=` hash; undefined for a malformed one. */
export async function sessionFromHash(hash: string): Promise<SessionState | undefined> {
  const match = SHARE.exec(hash.trim());
  if (!match) {
    return undefined;
  }
  try {
    const bytes = base64UrlToBytes(match[2]!);
    const json =
      match[1] === "z" ? await pipe(bytes, new DecompressionStream("deflate-raw")) : bytes;
    const payload: unknown = JSON.parse(new TextDecoder("utf-8", { fatal: true }).decode(json));
    if (!isRecord(payload) || payload.v !== 1) {
      return undefined;
    }
    const journal = typeof payload.journal === "string" ? base64UrlToBytes(payload.journal) : null;
    const inputs = payload.inputs;
    return asState({
      files: payload.files,
      inputs,
      next:
        payload.next ??
        (Array.isArray(inputs)
          ? Math.max(0, ...inputs.map((i) => (Array.isArray(i) ? Number(i[0]) || 0 : 0))) + 1
          : 1),
      journal,
      transcript: payload.transcript ?? [],
    });
  } catch {
    return undefined;
  }
}
