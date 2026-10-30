import { createHash } from "node:crypto";

// The SDK snapshot contains this exact platform-neutral graph and its ABI model.
export const runtimeSources = Object.freeze({
  "core.mjs": "host/core.mjs",
  "session.mjs": "host/session.mjs",
  "checked-report.mjs": "host/checked-report.mts",
  "float.mjs": "host/float.mts",
  "journal.mjs": "host/journal.mjs",
  "run-support.mjs": "host/run-support.mts",
  "wire.mjs": "host/wire.mts",
  "package-contract.mjs": "host/package-contract.mts",
  "package-providers.mjs": "host/package-providers.mts",
  "gen/abi.mjs": "host/gen/abi.mts",
  "abi-model.json": "host/gen/abi.json",
});
export const digest = (text) => createHash("sha256").update(text, "utf8").digest("hex");
export function graphFingerprint(files, packageHostABI, baseRevision) {
  return digest(
    `packageHostABI:${packageHostABI}\nbaseRevision:${baseRevision}\n` +
      Object.keys(files)
        .sort()
        .map((name) => `${name}\0${digest(files[name])}\n`)
        .join(""),
  );
}
