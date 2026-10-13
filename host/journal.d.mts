// Hand-written declarations for the exports of journal.mjs that the
// TypeScript side uses.

/** A reply value: void, an i64, the bits of an f64, or a status and value. */
export type Value =
  | { tag: 0 }
  | { tag: 1; v: bigint }
  | { tag: 2; v: bigint }
  | { tag: 3; v: number; w: number };

/** How an input ended, as its `End` row says. */
export type End =
  | { kind: "exit"; status: number }
  | { kind: "trapped" | "stopped" | "cancelled" | "failed"; status?: undefined };

export type Request = { args: Uint8Array } | { len: number; digest: bigint };

export type Event =
  | { kind: "header"; format: bigint; execution: Uint8Array }
  | { kind: "input"; number: number; source: bigint; module: bigint }
  | {
      kind: "call";
      module: string;
      name: string;
      request: Request;
      value: Value;
      result: Uint8Array | null;
    }
  | { kind: "finish"; handle: number; value: Value; result: Uint8Array | null }
  | { kind: "wake"; poll: bigint; handles: number[] }
  | { kind: "block"; handles: number[] }
  | { kind: "abort"; handle: number }
  | { kind: "cancel"; poll: bigint }
  | { kind: "end"; end: End };

export interface Row {
  kind: number;
  index: number;
  payload: Uint8Array;
}

export interface RecordedInput {
  number: number;
  source: bigint;
  module: bigint;
  events: Event[];
  end: End | null;
}

export const KIND: Record<
  "header" | "input" | "call" | "finish" | "wake" | "block" | "abort" | "cancel" | "end",
  number
>;
export const FORMAT: number;
export const SESSION_CAP: number;

export function xxh3_128(bytes: Uint8Array): bigint;
export function crc32(bytes: Uint8Array): number;
export function readRows(bytes: Uint8Array): { rows: Row[]; valid: number };
export function decodeRows(rows: Row[]): Event[];
export function requestDigest(args: Uint8Array): bigint;
export function requestOf(args: Uint8Array): Request;
export function callArgs(
  values: readonly (number | bigint)[],
  types: readonly string[],
  tail: Uint8Array | null,
): Uint8Array;
export function ended(end: End): string;
export function recorded(bytes: Uint8Array): {
  execution: Uint8Array;
  inputs: RecordedInput[];
};
export function ran(input: RecordedInput, source: string): boolean;
export function importTypes(
  bytes: Uint8Array | ArrayBuffer,
): Map<string, { params: string[]; results: string[] } | null>;

export class Diverged extends Error {}

export class Recorder {
  constructor(cap: number, execution: Uint8Array);
  journal(): Uint8Array | null;
  readonly cap: number;
  setCap(cap: number): void;
  takeNotice(): boolean;
  call(
    module: string,
    name: string,
    request: Uint8Array,
    value: Value,
    result: Uint8Array | null,
  ): void;
  finish(handle: number, value: Value, result: Uint8Array | null): void;
  block(handles: readonly number[]): void;
  abort(handle: number): void;
  poll(): void;
  wake(handles: readonly number[]): void;
  cancel(): void;
  input(number: number, source: string, module: Uint8Array): void;
  end(end: End): void;
}

export class Replay {
  constructor(input: RecordedInput);
  readonly diverged: string | null;
  call(module: string, name: string, args: Uint8Array): { value: Value; result: Uint8Array | null };
  finish(handle: number): { value: Value; result: Uint8Array | null };
  block(): number[] | null;
  abort(handle: number): void;
  poll(): void;
  wakeNext(): boolean;
  wake(): number[];
  end(end: End, recorded: End | null): void;
}
