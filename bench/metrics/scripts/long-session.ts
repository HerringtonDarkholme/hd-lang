// long-session: memory over a long REPL session.
//
// The REPL needs a terminal (bare `hd` with piped text runs a program), so
// this metric does not start `hd`. It runs the in-process test
// `repl::tests::long_session_plain` of `hd_cli` (cargo, with the `wasmtime`
// feature, in the repository root; the `--hd` command is not used). The
// test drives the session API the REPL uses (`Goal::ReplInput` builds over
// the kept session, `WasmtimeSession::run_input`): 1,000 inputs, where even
// input i binds `wI := +(1000000 + i)` and odd input i shows `w(i-1) + 1`,
// so every input shows the value 1000000 + i. Every 50 inputs it prints the
// process RSS, the build and run time of the input just done and the
// journal size. The growth is the largest RSS from input 100 on, minus the
// RSS at input 100.
// Target (owner, 2026-10-13): under 100 KiB per kept input on 16 KiB pages,
// scaled by the page size (the code mapping of each input's module is
// rounded to pages), so 87.9 MiB from input 100 to input 1,000 on 16 KiB
// pages and a quarter of that on 4 KiB pages (unmeasured on Linux).
// n/a: without cargo.

import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { runProcess } from "../lib/hd.ts";
import { failed, formatValue, judge, notApplicable, type Metric } from "../lib/metric.ts";
import { makeTempDir } from "../lib/tmp.ts";

const NAME = "long-session";
const INPUTS = 1000;
const STEP = 50;
const BASELINE_INPUT = 100;
const MARK = 1_000_000;
const TEST_TIMEOUT_MS = 30 * 60_000;
/** The owner's accepted cost of a kept input (2026-10-13), on 16 KiB pages. */
const KIB_PER_INPUT_16K = 100;
const DEFAULT_PAGE = 16 * 1024;

/** One line of the test's output: the state after `inputs` inputs. */
export interface Sample {
  readonly inputs: number;
  readonly rssBytes: number;
  readonly buildUs: number;
  readonly runUs: number;
  readonly journalBytes: number;
}

/** Input i of the session; each shows the value MARK + i. The test writes the same text. */
export function replInput(index: number): string {
  return index % 2 === 0 ? `w${index} := +${MARK + index}` : `w${index - 1} + 1`;
}

/** The samples in the test's output, in order. */
export function parseSamples(text: string): Sample[] {
  const samples: Sample[] = [];
  for (const line of text.split("\n")) {
    if (!line.startsWith("LONG-SESSION ")) {
      continue;
    }
    const field = (key: string) => Number(new RegExp(`\\b${key}=(\\d+)`).exec(line)?.[1]);
    const sample = {
      inputs: field("inputs"),
      rssBytes: field("rss_kib") * 1024,
      buildUs: field("mean_build_us"),
      runUs: field("mean_run_us"),
      journalBytes: field("journal"),
    };
    if (Object.values(sample).every(Number.isFinite)) {
      samples.push(sample);
    }
  }
  return samples;
}

/** The bound on growth from input `from` to input `to`: the accepted cost per input, scaled by the page size. */
export function growthBound(from: number, to: number, pageBytes: number): number {
  return (to - from) * KIB_PER_INPUT_16K * 1024 * (pageBytes / DEFAULT_PAGE);
}

/** The largest RSS from `from` inputs on, minus the RSS at `from`; NaN when `from` has no sample. */
export function growthFrom(samples: readonly Sample[], from: number): number {
  const base = samples.find((s) => s.inputs === from);
  const later = samples.filter((s) => s.inputs >= from);
  if (!base || later.length === 0) {
    return Number.NaN;
  }
  return Math.max(...later.map((s) => s.rssBytes)) - base.rssBytes;
}

async function run(command: string, args: string[]): Promise<string | undefined> {
  try {
    return (await promisify(execFile)(command, args)).stdout.trim();
  } catch {
    return undefined;
  }
}

async function pageSize(): Promise<number> {
  const size = Number(await run("getconf", ["PAGESIZE"]));
  return Number.isFinite(size) && size > 0 ? size : DEFAULT_PAGE;
}

export const longSession: Metric = {
  name: NAME,
  pillar: 2,
  summary: "RSS across 1,000 REPL inputs on the session API (in-process, cargo test)",
  async run(context) {
    const page = await pageSize();
    const bound = growthBound(BASELINE_INPUT, INPUTS, page);
    const label = `RSS growth, REPL inputs ${BASELINE_INPUT} to ${INPUTS}`;
    const target = `≤ ${formatValue(bound, "MB")}`;
    if ((await run("cargo", ["--version"])) === undefined) {
      return [notApplicable(NAME, label, target, "no cargo to run the in-process session test")];
    }
    context.log(
      `${NAME}: ${INPUTS} inputs in steps of ${STEP}, in process (${page / 1024} KiB pages)`,
    );
    const result = await runProcess(
      [
        "cargo",
        "test",
        "-q",
        "-p",
        "hd_cli",
        "--features",
        "wasmtime",
        "long_session_plain",
        "--",
        "--ignored",
        "--nocapture",
      ],
      {
        cwd: context.repoRoot,
        timeoutMs: TEST_TIMEOUT_MS,
        env: { HD_CACHE: makeTempDir(`${NAME}-cache`) },
      },
    );
    const samples = parseSamples(result.stdout);
    const last = samples.at(-1);
    if (result.timedOut) {
      return [failed(NAME, label, target, "the session test timed out")];
    }
    if (result.status !== 0 || !last) {
      return [failed(NAME, label, target, `the session test failed: ${result.stderr.slice(-300)}`)];
    }
    const at = (inputs: number) => samples.find((s) => s.inputs === inputs);
    const mb = (s: Sample | undefined) => (s ? formatValue(s.rssBytes, "MB") : "?");
    const ms = (s: Sample | undefined) => (s ? `${(s.buildUs / 1000).toFixed(0)} ms` : "?");
    const detail =
      `RSS ${mb(at(BASELINE_INPUT))} at input ${BASELINE_INPUT}, ${mb(last)} at input ${last.inputs}; ` +
      `build per input ${ms(at(STEP))} at ${STEP}, ${ms(last)} at ${last.inputs}; ` +
      `${page / 1024} KiB pages`;
    const growth = growthFrom(samples, BASELINE_INPUT);
    if (Number.isNaN(growth)) {
      return [failed(NAME, label, target, "no sample at the baseline input")];
    }
    return [judge(NAME, label, growth, bound, "MB", "at-most", detail)];
  },
};
