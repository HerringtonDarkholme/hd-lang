/**
 * The Daily Metrics page, from the metrics-data branch
 * (bench/metrics/README.md): the pillars and their philosophy, one trend
 * chart of targets met per pillar, one goal-distance chart per pillar (a
 * bar per metric), and a table of every target.
 */
import { existsSync, readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { METRICS } from "../../bench/metrics/scripts/index.ts";
import { escapeHtml, type Heading } from "./markdown.ts";
import { REPOSITORY_URL } from "./pages.ts";

const BRANCH = `${REPOSITORY_URL}/tree/metrics-data`;

type Status = "pass" | "fail" | "n/a";

interface Point {
  readonly finished: string;
  readonly commit: string;
  readonly value: string;
  readonly status: Status;
  /** The value's first quantity in its normalized unit, when it has one. */
  readonly amount: number | null;
}

interface Series {
  readonly metric: string;
  readonly name: string;
  readonly target: string;
  readonly unit: string | null;
  readonly goal: number | null;
  readonly points: readonly Point[];
}

// The units the metrics runner prints, scaled to one unit per kind, as the
// runner's own comparison does.
const UNITS: Record<string, [number, string]> = {
  s: [1000, "ms"],
  ms: [1, "ms"],
  µs: [0.001, "ms"],
  ns: [0.000001, "ms"],
  B: [1, "B"],
  MB: [1048576, "B"],
  "M/s": [1000000, "/s"],
  "k/s": [1000, "/s"],
};

/** The first number of `text` and its normalized unit, or null when it has none. */
function quantity(text: string): { amount: number; unit: string } | null {
  const match =
    /(-?(?:\d+(?:\.\d*)?|\.\d+))\s*(ms|s|ns|µs|MB\/s|M\/s|k\/s|\/s|MB|B|tokens|%|x|B\/iter)?/.exec(
      text.split(", ")[0] ?? "",
    );
  if (!match) {
    return null;
  }
  const unit = match[2] ?? "";
  const [scale, normalized] = UNITS[unit] ?? [1, unit];
  return { amount: Number(match[1]) * scale, unit: normalized };
}

function parseHistory(text: string): { header: string[]; rows: string[][] } {
  const [header = [], ...rows] = text
    .trimEnd()
    .split("\n")
    .map((line) => line.split("\t"));
  return { header, rows };
}

/** Every target's values across the recorded runs, oldest first. */
function loadSeries(dir: string): { series: Series[]; latest: string } {
  const { header, rows } = parseHistory(readFileSync(join(dir, "history.tsv"), "utf8"));
  const col = (key: string): number => header.indexOf(key);
  rows.sort((a, b) => (a[col("finished")] ?? "").localeCompare(b[col("finished")] ?? ""));
  // Goals live in the run files; the newest one names the current targets.
  const daily = join(dir, "daily");
  const newest = existsSync(daily)
    ? readdirSync(daily)
        .filter((name) => name.endsWith(".json"))
        .sort()
        .at(-1)
    : undefined;
  const goals = new Map<string, string>();
  if (newest) {
    const run = JSON.parse(readFileSync(join(daily, newest), "utf8")) as {
      results: { metric: string; name: string; target: string }[];
    };
    for (const result of run.results) {
      goals.set(JSON.stringify([result.metric, result.name]), result.target);
    }
  }
  const series: Series[] = [];
  for (const key of header) {
    if (!key.startsWith("value ")) {
      continue;
    }
    const id = key.slice(6);
    const [metric, name] = JSON.parse(id) as [string, string];
    const statusCol = col("status " + id);
    const points = rows
      .filter((row) => (row[col(key)] ?? "") !== "")
      .map((row): Point => {
        const value = row[col(key)] ?? "-";
        const parsed = value === "-" ? null : quantity(value);
        return {
          finished: row[col("finished")] ?? "",
          commit: (row[col("commit")] ?? "").slice(0, 9),
          value,
          status: (row[statusCol] as Status) ?? "n/a",
          amount: parsed?.amount ?? null,
        };
      });
    const target = goals.get(id) ?? "";
    const units = new Set(
      points.map((point) => quantity(point.value)?.unit).filter((unit) => unit !== undefined),
    );
    const unit = units.size === 1 ? [...units][0]! : null;
    const goal = quantity(target);
    series.push({
      metric,
      name,
      target,
      unit,
      goal: goal && goal.unit === unit ? goal.amount : null,
      points,
    });
  }
  return { series, latest: rows.at(-1)?.[col("finished")] ?? "" };
}

const STATUS_LABEL: Record<Status, string> = { pass: "✓ met", fail: "✗ missed", "n/a": "– n/a" };

interface Pillar {
  readonly key: string;
  readonly title: string;
  readonly question: string;
  readonly explain: string;
}

// The Agentic Programming Language Arena (docs/compiler/goals.md).
const PILLARS: readonly Pillar[] = [
  {
    key: "1",
    title: "Single-agent development cost",
    question: "How cheaply does one agent get a correct program?",
    explain:
      "Compile and test latency is agent wait time; clear, short, well-placed diagnostics and fix-its that resolve mistakes are fewer retries and fewer tokens.",
  },
  {
    key: "2",
    title: "Agent scalability",
    question: "How much useful work do N agents do on one machine?",
    explain:
      "CPU, memory and disk per check, startup, behaviour under concurrency, and caches that are shared, bounded and safe to write at once.",
  },
  {
    key: "3",
    title: "Artifact quality",
    question: "How good is the program the agents produced?",
    explain:
      "The user's program, not the compiler: test speed, runtime against Node, allocations, binary size, host-call and serde cost, memory over time.",
  },
  {
    key: "gate",
    title: "Correctness gate",
    question: "Is the compiler right?",
    explain:
      "Conformance and incremental soundness. A faster wrong result is not better, so this is a prerequisite rather than a pillar.",
  },
];

// Slots 1-4 of the dataviz reference palette, light and dark; pillars keep
// their colour on every chart.
const PILLAR_COLOUR = ["metric-c1", "metric-c2", "metric-c3", "metric-c4"];

/** How far a value is from its goal: 1 is the goal, below 1 meets it. */
function goalRatio(series: Series): number | null {
  const last = series.points.at(-1);
  if (!last || last.amount === null || series.goal === null || series.goal === 0) {
    return null;
  }
  const atLeast = series.target.trim().startsWith("≥");
  const ratio = atLeast ? series.goal / last.amount : last.amount / series.goal;
  return Number.isFinite(ratio) ? ratio : null;
}

interface Group {
  readonly metric: string;
  readonly targets: readonly Series[];
  readonly status: Status;
  /** The worst target's ratio to its goal, when any target has one. */
  readonly ratio: number | null;
  readonly worst: Series;
}

/** One row per metric script: missed if any target missed, its worst target's ratio. */
function groups(series: readonly Series[]): Group[] {
  const by = new Map<string, Series[]>();
  for (const s of series) {
    by.set(s.metric, [...(by.get(s.metric) ?? []), s]);
  }
  return [...by.entries()].map(([metric, targets]) => {
    const statuses = targets.map((t) => t.points.at(-1)?.status ?? "n/a");
    const status: Status = statuses.includes("fail")
      ? "fail"
      : statuses.includes("pass")
        ? "pass"
        : "n/a";
    let worst = targets[0]!;
    let ratio: number | null = null;
    for (const t of targets) {
      const r = goalRatio(t);
      if (r !== null && (ratio === null || r > ratio)) {
        ratio = r;
        worst = t;
      }
    }
    if (ratio === null) {
      worst = targets.find((t) => t.points.at(-1)?.status === "fail") ?? worst;
    }
    return { metric, targets, status, ratio, worst };
  });
}

/** Share of a pillar's targets met in each run, oldest first. */
function metShare(series: readonly Series[], runs: readonly string[]): (number | null)[] {
  return runs.map((run) => {
    const statuses = series
      .map((s) => s.points.find((p) => p.finished === run)?.status)
      .filter((s): s is Status => s === "pass" || s === "fail");
    return statuses.length === 0
      ? null
      : (100 * statuses.filter((s) => s === "pass").length) / statuses.length;
  });
}

function shortDate(iso: string): string {
  return iso.slice(5, 16).replace("T", " ");
}

/** One line per pillar: the share of its targets met, run by run. */
function trendChart(
  lines: { pillar: Pillar; colour: string; values: (number | null)[] }[],
  runs: readonly string[],
): string {
  const width = 720;
  const height = 220;
  const pad = { left: 40, right: 150, top: 12, bottom: 28 };
  const n = runs.length;
  const x = (i: number): number =>
    n === 1
      ? pad.left + (width - pad.left - pad.right) / 2
      : pad.left + (i * (width - pad.left - pad.right)) / (n - 1);
  const y = (v: number): number => pad.top + ((100 - v) * (height - pad.top - pad.bottom)) / 100;
  const grid = [0, 50, 100]
    .map(
      (v) =>
        `<line class="metric-grid-line" x1="${pad.left}" x2="${width - pad.right}" y1="${y(v)}" y2="${y(v)}"/><text class="metric-tick" x="${pad.left - 6}" y="${y(v) + 4}" text-anchor="end">${v}%</text>`,
    )
    .join("");
  const axis = n
    ? `<text class="metric-tick" x="${x(0)}" y="${height - 8}" text-anchor="${n === 1 ? "middle" : "start"}">${escapeHtml(shortDate(runs[0]!))}</text>${n > 1 ? `<text class="metric-tick" x="${x(n - 1)}" y="${height - 8}" text-anchor="end">${escapeHtml(shortDate(runs[n - 1]!))}</text>` : ""}`
    : "";
  const labels: { x: number; y: number; text: string }[] = [];
  const marks = lines
    .map(({ pillar, colour, values }) => {
      const pts = values
        .map((v, i) => (v === null ? null : `${x(i).toFixed(1)},${y(v).toFixed(1)}`))
        .filter((p): p is string => p !== null);
      const last = values.findLastIndex((v) => v !== null);
      const line =
        pts.length > 1
          ? `<polyline class="metric-series ${colour}" points="${pts.join(" ")}"/>`
          : "";
      const dots = values
        .map((v, i) =>
          v === null
            ? ""
            : `<g class="metric-point ${colour}"><circle class="metric-hit" cx="${x(i).toFixed(1)}" cy="${y(v).toFixed(1)}" r="9"/><circle cx="${x(i).toFixed(1)}" cy="${y(v).toFixed(1)}" r="4"/><title>${escapeHtml(`${pillar.title}, ${shortDate(runs[i]!)}: ${Math.round(v)}% of targets met`)}</title></g>`,
        )
        .join("");
      if (last >= 0) {
        labels.push({
          x: x(last) + 10,
          y: y(values[last]!) + 4,
          text: `${pillar.title} ${Math.round(values[last]!)}%`,
        });
      }
      return line + dots;
    })
    .join("");
  // Direct labels at the line ends, pushed apart so they never overlap.
  labels.sort((a, b) => a.y - b.y);
  for (let i = 1; i < labels.length; i++) {
    labels[i]!.y = Math.max(labels[i]!.y, labels[i - 1]!.y + 15);
  }
  const text = labels
    .map(
      (l) =>
        `<text class="metric-label" x="${l.x.toFixed(1)}" y="${l.y.toFixed(1)}">${escapeHtml(l.text)}</text>`,
    )
    .join("");
  return `<svg class="metric-trend" viewBox="0 0 ${width} ${height}" role="img" aria-label="Share of targets met per pillar, per run">${grid}${axis}${marks}${text}</svg>`;
}

/** One bar per metric script: its worst target's distance from the goal. */
function goalChart(rows: readonly Group[]): string {
  const cap = 3;
  const rowHeight = 26;
  const width = 720;
  const labelWidth = 170;
  const valueWidth = 230;
  const plot = width - labelWidth - valueWidth;
  const height = rows.length * rowHeight + 24;
  const x = (r: number): number => labelWidth + (Math.min(r, cap) / cap) * plot;
  const goalX = x(1);
  const bars = rows
    .map((row, i) => {
      const top = 8 + i * rowHeight;
      const value = row.worst.points.at(-1)?.value ?? "-";
      const note =
        row.ratio === null
          ? row.status === "pass"
            ? "met"
            : row.status === "fail"
              ? "missed"
              : "n/a"
          : `${value} vs ${row.worst.target}${row.ratio > cap ? ` (${row.ratio.toFixed(1)}× goal)` : ""}`;
      const bar =
        row.ratio === null
          ? ""
          : `<rect class="metric-bar is-${row.status === "n/a" ? "na" : row.status}" x="${labelWidth}" y="${top + 4}" width="${Math.max(2, x(row.ratio) - labelWidth).toFixed(1)}" height="${rowHeight - 10}" rx="4"/>`;
      return `<g class="metric-row"><title>${escapeHtml(`${row.metric}: ${row.targets.length} target(s); worst ${row.worst.name}: ${note}`)}</title><text class="metric-row-label" x="${labelWidth - 10}" y="${top + rowHeight / 2 + 3}" text-anchor="end">${escapeHtml(row.metric)}</text>${bar}<text class="metric-row-note is-${row.status === "n/a" ? "na" : row.status}" x="${labelWidth + plot + 10}" y="${top + rowHeight / 2 + 3}">${row.status === "pass" ? "✓" : row.status === "fail" ? "✗" : "–"} ${escapeHtml(note)}</text></g>`;
    })
    .join("");
  const goal = `<line class="metric-goal" x1="${goalX}" x2="${goalX}" y1="2" y2="${height - 18}"/><text class="metric-tick" x="${goalX}" y="${height - 4}" text-anchor="middle">goal</text><text class="metric-tick" x="${x(cap)}" y="${height - 4}" text-anchor="end">${cap}× or worse</text>`;
  return `<svg class="metric-goals" viewBox="0 0 ${width} ${height}" role="img" aria-label="Each metric's worst target as a multiple of its goal">${goal}${bars}</svg>`;
}

/** The page body and its headings. */
export function metricsPage(dir: string | undefined): { body: string; headings: Heading[] } {
  const headings: Heading[] = [];
  const h = (level: number, id: string, text: string): string => {
    headings.push({ level, id, text });
    return `<h${level} id="${id}">${escapeHtml(text)}</h${level}>`;
  };
  const intro = `${h(1, "daily-goal-metrics", "Daily goal metrics")}
<p>hd is built for programs written by coding agents, so we judge it with the <a href="https://x.com/hd_nvim/status/2107634125083451468"><em>Agentic Programming Language Arena</em></a>: three pillars that ask what agent-driven development actually costs and produces. Each pillar is kept on its own and compared on a Pareto frontier rather than folded into one score, because a faster compiler that makes agents retry more is not a win. Compiler correctness is a gate in front of all three. Every metric is a script with a fixed target; no AI or agent run is involved, and the same scripts run against any <code>hd</code> binary. They run twice a day, at 00:00 and 12:00 UTC.</p>`;
  if (!dir || !existsSync(join(dir, "history.tsv"))) {
    return {
      headings,
      body: `${intro}<p>This build has no metrics data. The runs are on the <a href="${BRANCH}#readme">metrics-data branch</a>.</p>`,
    };
  }
  const { series, latest } = loadSeries(dir);
  const pillarOf = new Map(METRICS.map((metric) => [metric.name, String(metric.pillar)]));
  const inPillar = (key: string): Series[] =>
    series.filter((s) => (pillarOf.get(s.metric) ?? "") === key);
  const runs = [...new Set(series.flatMap((s) => s.points.map((p) => p.finished)))].sort();
  const parts = [intro];

  const card = (p: Pillar, i: number): string => {
    const targets = inPillar(p.key);
    const met = targets.filter((s) => s.points.at(-1)?.status === "pass").length;
    return `<div class="metric-pillar"><dt><span class="metric-swatch ${PILLAR_COLOUR[i]}"></span>${escapeHtml(p.title)}<span class="metric-count">${met} of ${targets.length} met</span></dt><dd><strong>${escapeHtml(p.question)}</strong> ${escapeHtml(p.explain)}</dd></div>`;
  };
  const gate = PILLARS.findIndex((p) => p.key === "gate");
  parts.push(
    h(2, "correctness-first", "First: correctness"),
    `<dl class="metric-pillars">${card(PILLARS[gate]!, gate)}</dl>`,
    h(2, "the-pillars", "The three pillars"),
    `<dl class="metric-pillars">${PILLARS.map((p, i) => (p.key === "gate" ? "" : card(p, i))).join("")}</dl>`,
  );

  parts.push(
    h(2, "trend", "Trend"),
    `<p class="metric-explain">The share of each pillar's targets that the run met. A line going up is progress; a drop after a commit is a regression worth reading. Hover a point for its run.</p>`,
    trendChart(
      PILLARS.map((pillar, i) => ({
        pillar,
        colour: PILLAR_COLOUR[i]!,
        values: metShare(inPillar(pillar.key), runs),
      })),
      runs,
    ),
  );

  PILLARS.forEach((pillar) => {
    const rows = groups(inPillar(pillar.key)).sort((a, b) => (b.ratio ?? -1) - (a.ratio ?? -1));
    if (rows.length === 0) {
      return;
    }
    parts.push(
      h(2, `pillar-${pillar.key}`, pillar.title),
      `<p class="metric-explain">${escapeHtml(pillar.question)} Each bar is one metric, drawn at its worst target's distance from the goal: left of the dashed line meets it, right of it misses by that multiple. Metrics without a numeric goal show only met or missed. The full list of targets is in the table below.</p>`,
      goalChart(rows),
    );
  });

  headings.push({ level: 2, id: "all-targets", text: "All targets" });
  const rows = series
    .map((s) => {
      const last = s.points.at(-1);
      const pillar =
        PILLARS.find((p) => p.key === (pillarOf.get(s.metric) ?? ""))?.title ?? "Other";
      return `<tr><td>${escapeHtml(pillar)}</td><td>${escapeHtml(s.metric)}</td><td>${escapeHtml(s.name)}</td><td>${escapeHtml(last?.value ?? "-")}</td><td>${escapeHtml(s.target)}</td><td>${STATUS_LABEL[last?.status ?? "n/a"]}</td></tr>`;
    })
    .join("");
  parts.push(
    `<h2 id="all-targets">All targets</h2><p class="metric-explain">Latest run: ${escapeHtml(latest.replace("T", " ").slice(0, 16))} UTC. History and raw data: the <a href="${BRANCH}#readme">metrics-data branch</a>.</p><details><summary>Every target, latest run</summary><table><thead><tr><th>Pillar</th><th>Metric</th><th>Target</th><th>Value</th><th>Goal</th><th>Status</th></tr></thead><tbody>${rows}</tbody></table></details>`,
  );
  return { body: parts.join("\n"), headings };
}
