export interface BaselineCase {
  readonly scales: readonly [number, number];
  readonly scores: readonly [number, number];
  readonly ratio: number;
  readonly codes: readonly string[];
}

export interface Baseline {
  readonly note: string;
  readonly recorded: {
    readonly date: string;
    readonly node: string;
    readonly platform: string;
    readonly runs?: number;
    readonly percentile?: number;
  };
  readonly referenceMs: number;
  readonly cases: Record<string, BaselineCase>;
}

interface GateCase {
  readonly name: string;
  readonly scales: readonly [number, number];
}

/** The nearest-rank percentile: with five samples, p90 deliberately takes the highest. */
export function percentile(values: readonly number[], percent: number): number {
  const sorted = values.toSorted((left, right) => left - right);
  return sorted[Math.max(0, Math.ceil((percent / 100) * sorted.length) - 1)]!;
}

export function mergedBaseline(
  inputs: readonly Baseline[],
  percent: number,
  gate: readonly GateCase[],
): Baseline {
  if (inputs.length < 2) throw new Error("--merge needs at least two baseline files");
  const first = inputs[0]!;
  for (const input of inputs.slice(1)) {
    if (input.recorded.node !== first.recorded.node)
      throw new Error(
        `merged baselines use different Node versions: ${first.recorded.node} and ${input.recorded.node}`,
      );
    if (input.recorded.platform !== first.recorded.platform)
      throw new Error(
        `merged baselines use different platforms: ${first.recorded.platform} and ${input.recorded.platform}`,
      );
  }
  const cases: Record<string, BaselineCase> = {};
  for (const entry of gate) {
    const samples = inputs.map((input, index) => {
      const sample = input.cases[entry.name];
      if (!sample) throw new Error(`merged baseline ${index + 1} has no case '${entry.name}'`);
      if (sample.scales[0] !== entry.scales[0] || sample.scales[1] !== entry.scales[1])
        throw new Error(`merged baseline ${index + 1} has different scales for '${entry.name}'`);
      return sample;
    });
    const codes = samples[0]!.codes;
    for (const [index, sample] of samples.slice(1).entries())
      if (sample.codes.join(",") !== codes.join(","))
        throw new Error(
          `merged baseline ${index + 2} has different result codes for '${entry.name}'`,
        );
    cases[entry.name] = {
      scales: entry.scales,
      scores: [
        Number(
          percentile(
            samples.map((sample) => sample.scores[0]),
            percent,
          ).toFixed(4),
        ),
        Number(
          percentile(
            samples.map((sample) => sample.scores[1]),
            percent,
          ).toFixed(4),
        ),
      ],
      ratio: Number(
        percentile(
          samples.map((sample) => sample.ratio),
          percent,
        ).toFixed(2),
      ),
      codes,
    };
  }
  return {
    note: `Merged from ${inputs.length} update runs at the nearest-rank ${percent}th percentile; see README.md.`,
    recorded: {
      date: new Date().toISOString().slice(0, 10),
      node: first.recorded.node,
      platform: first.recorded.platform,
      runs: inputs.length,
      percentile: percent,
    },
    referenceMs: Number(
      percentile(
        inputs.map((input) => input.referenceMs),
        percent,
      ).toFixed(1),
    ),
    cases,
  };
}
