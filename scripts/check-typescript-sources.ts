import { execFileSync } from "node:child_process";

const files = execFileSync("git", ["ls-files", "-z", "--", "*.mjs", "*.mts", "*.cjs", "*.cts"], {
  encoding: "utf8",
})
  .split("\0")
  .filter(Boolean)
  .filter((file) => !file.startsWith("packages/server-sdk/packaging/vendor/"));

if (files.length) {
  process.stderr.write(`module sources must be plain .ts:\n${files.join("\n")}\n`);
  process.exitCode = 1;
}
