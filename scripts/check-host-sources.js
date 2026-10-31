import { execFileSync } from "node:child_process";

const files = execFileSync("git", ["ls-files", "-z", "host"], { encoding: "utf8" })
  .split("\0")
  .filter(Boolean);
const forbidden = files.filter(
  (file) => /\.(?:mjs|mts|cjs|cts)$/.test(file) || file.endsWith(".d.ts"),
);

if (forbidden.length) {
  process.stderr.write(
    `host sources must be plain .ts (or non-source assets):\n${forbidden.join("\n")}\n`,
  );
  process.exitCode = 1;
}
