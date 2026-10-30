// Hand-written declarations for the public exports of cases.ts that the
// TypeScript side uses.

import type { Host, Sink } from "./core.ts";

/** The files of a test run: temporary directories, snapshots, regression streams. */
export interface TestFiles {
  join(dir: string, name: string): string;
  /** The text, or `null` when there is no such file. */
  read(path: string): string | null;
  /** Writes the file, making its directory. */
  write(path: string, text: string): void;
  remove(dir: string): void;
}

/** One case of a run's configuration (test.ts has the format). */
export interface CaseConfig {
  test: number;
  init: number;
  kind: string;
  tempDir: string | null;
  seed: number | null;
  grants: Record<string, false | string[]>;
  snapshot?: { dir: string; shown?: string | null; slug: string; update: boolean } | null;
  regression?: { file: string; shown?: string } | null;
}

export interface RunConfig {
  program: string;
  args: string[];
  programs?: null;
  cases: CaseConfig[];
}

/** One result: a case's, or a row's with `row` and `rows`. An empty table has no `status`. */
export interface CaseRecord {
  test: number;
  row?: number;
  rows?: number;
  status?: number;
  trapped?: boolean;
  stdout?: string;
  stderr?: string;
  us?: number;
  frames?: [number, number][];
  /** A failed property case's input, shrunk when applicable. */
  input?: string;
  failure?: string;
  /** A trapped run's panic as the host recorded it. */
  category?: string;
  message?: string;
}

export interface RunCasesOptions {
  module: WebAssembly.Module;
  config: RunConfig;
  createHost(sink: Sink, env: Record<string, unknown>): Host;
  files: TestFiles;
  notify(message: { type: string }): void;
  emit(record: CaseRecord): void;
}

export function runCases(
  options: RunCasesOptions,
  start?: { case: number; row: number; rows: number },
): Promise<void>;

export interface Panic {
  category: string;
  message: string;
  debug: string[];
}

/** The fields of a result record that judging it reads. */
export interface JudgedRecord {
  failure?: string;
  status: number;
  trapped: boolean;
  stderr: string;
  category?: string;
  message?: string;
}

export function panicOf(record: JudgedRecord): Panic;

export type Verdict =
  | { outcome: "passed" }
  | { outcome: "failed"; message: string }
  | ({ outcome: "panicked" } & Panic);

export function judge(record: JudgedRecord, expectPanic: string | null): Verdict;
