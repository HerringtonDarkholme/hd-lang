/**
 * The Daily Metrics page: trend charts drawn from the metrics-data branch
 * (bench/metrics/README.md describes what it holds). Each target gets a
 * small line chart of its value across runs, its goal as a dashed line, and
 * its status as an icon and a label; a table view follows the charts.
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
  if (!match) return null;
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
    for (const result of run.results)
      goals.set(JSON.stringify([result.metric, result.name]), result.target);
  }
  const series: Series[] = [];
  for (const key of header) {
    if (!key.startsWith("value ")) continue;
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

const STATUS_LABEL: Record<Status, string> = { pass: "✓ Pass", fail: "✗ Fail", "n/a": "– n/a" };

function shortDate(iso: string): string {
  return iso.slice(5, 16).replace("T", " ");
}

/** One target's trend: a 2px line over the runs, the goal dashed, a dot per run. */
function sparkline(series: Series): string {
  const width = 280;
  const height = 72;
  const pad = { left: 6, right: 6, top: 8, bottom: 8 };
  const values = series.points.map((point) => point.amount);
  const known = values.filter((value): value is number => value !== null);
  if (known.length === 0) return `<div class="metric-empty">No numeric value recorded yet.</div>`;
  const scale = [...known, ...(series.goal === null ? [] : [series.goal])];
  let low = Math.min(...scale);
  let high = Math.max(...scale);
  if (high === low) {
    high += Math.abs(high) * 0.1 || 1;
    low -= Math.abs(low) * 0.1 || 1;
  }
  const n = series.points.length;
  const x = (i: number): number =>
    n === 1 ? width / 2 : pad.left + (i * (width - pad.left - pad.right)) / (n - 1);
  const y = (value: number): number =>
    pad.top + ((high - value) * (height - pad.top - pad.bottom)) / (high - low);
  // A missing value breaks the line rather than bridging it.
  const segments: string[] = [];
  let current: string[] = [];
  values.forEach((value, i) => {
    if (value === null) {
      if (current.length > 1) segments.push(current.join(" "));
      current = [];
    } else current.push(`${x(i).toFixed(1)},${y(value).toFixed(1)}`);
  });
  if (current.length > 1) segments.push(current.join(" "));
  const goal =
    series.goal === null
      ? ""
      : `<line class="metric-goal" x1="0" x2="${width}" y1="${y(series.goal).toFixed(1)}" y2="${y(series.goal).toFixed(1)}"/>`;
  const lines = segments
    .map((points) => `<polyline class="metric-line" points="${points}"/>`)
    .join("");
  const dots = series.points
    .map((point, i) =>
      point.amount === null
        ? ""
        : `<g class="metric-dot ${point.status === "pass" ? "is-pass" : point.status === "fail" ? "is-fail" : ""}"><circle class="metric-hit" cx="${x(i).toFixed(1)}" cy="${y(point.amount).toFixed(1)}" r="9"/><circle cx="${x(i).toFixed(1)}" cy="${y(point.amount).toFixed(1)}" r="3.5"/><title>${escapeHtml(`${shortDate(point.finished)} · ${point.value} · ${point.status} · ${point.commit}`)}</title></g>`,
    )
    .join("");
  const label = `${series.name}: ${series.points.length} runs, latest ${series.points.at(-1)?.value ?? "-"}, goal ${series.target}`;
  return `<svg class="metric-chart" viewBox="0 0 ${width} ${height}" role="img" aria-label="${escapeHtml(label)}">${goal}${lines}${dots}</svg>`;
}

function card(series: Series): string {
  const last = series.points.at(-1);
  const status = last?.status ?? "n/a";
  const first = series.points[0];
  return `<figure class="metric-card is-${status === "n/a" ? "na" : status}">
<figcaption><span class="metric-name">${escapeHtml(series.name)}</span><span class="metric-script">${escapeHtml(series.metric)}</span></figcaption>
<div class="metric-head"><span class="metric-value">${escapeHtml(last?.value ?? "-")}</span><span class="metric-status">${STATUS_LABEL[status]}</span></div>
<div class="metric-goal-text">goal ${escapeHtml(series.target || "—")}</div>
${sparkline(series)}
<div class="metric-axis"><span>${first ? escapeHtml(shortDate(first.finished)) : ""}</span><span>${last && series.points.length > 1 ? escapeHtml(shortDate(last.finished)) : ""}</span></div>
</figure>`;
}

const PILLARS: readonly [string, string][] = [
  ["1", "Pillar 1"],
  ["2", "Pillar 2"],
  ["3", "Pillar 3"],
  ["gate", "Correctness gate"],
  ["other", "Other"],
];

/** The page body and its headings, or null when the build has no metrics data. */
export function metricsPage(dir: string | undefined): { body: string; headings: Heading[] } {
  const headings: Heading[] = [{ level: 1, id: "daily-goal-metrics", text: "Daily goal metrics" }];
  if (!dir || !existsSync(join(dir, "history.tsv")))
    return {
      headings,
      body: `<h1 id="daily-goal-metrics">Daily goal metrics</h1><p>This build has no metrics data. The runs are on the <a href="${BRANCH}#readme">metrics-data branch</a>.</p>`,
    };
  const { series, latest } = loadSeries(dir);
  const pillarOf = new Map(METRICS.map((metric) => [metric.name, String(metric.pillar)]));
  const lastStatus = (s: Series): Status => s.points.at(-1)?.status ?? "n/a";
  const count = (status: Status): number => series.filter((s) => lastStatus(s) === status).length;
  const parts: string[] = [
    `<h1 id="daily-goal-metrics">Daily goal metrics</h1>`,
    `<p>Each target's value over every recorded run, with its goal as a dashed line. Runs happen at 00:00 and 12:00 UTC; hover a point for its run. Latest run: ${escapeHtml(latest.replace("T", " ").slice(0, 16))} UTC. Data and history: the <a href="${BRANCH}#readme">metrics-data branch</a>.</p>`,
    `<p class="metric-summary"><span class="is-pass">✓ ${count("pass")} pass</span> <span class="is-fail">✗ ${count("fail")} fail</span> <span>– ${count("n/a")} n/a</span></p>`,
  ];
  for (const [pillar, title] of PILLARS) {
    const group = series.filter((s) => (pillarOf.get(s.metric) ?? "other") === pillar);
    if (group.length === 0) continue;
    const id = `pillar-${pillar}`;
    headings.push({ level: 2, id, text: title });
    parts.push(
      `<h2 id="${id}">${title}</h2><div class="metric-grid">${group.map(card).join("")}</div>`,
    );
  }
  headings.push({ level: 2, id: "table-view", text: "Table view" });
  const rows = series
    .map((s) => {
      const last = s.points.at(-1);
      return `<tr><td>${escapeHtml(s.metric)}</td><td>${escapeHtml(s.name)}</td><td>${escapeHtml(last?.value ?? "-")}</td><td>${escapeHtml(s.target)}</td><td>${STATUS_LABEL[last?.status ?? "n/a"]}</td></tr>`;
    })
    .join("");
  parts.push(
    `<h2 id="table-view">Table view</h2><details><summary>All targets, latest run</summary><table><thead><tr><th>Metric</th><th>Target</th><th>Value</th><th>Goal</th><th>Status</th></tr></thead><tbody>${rows}</tbody></table></details>`,
  );
  return { body: parts.join("\n"), headings };
}
