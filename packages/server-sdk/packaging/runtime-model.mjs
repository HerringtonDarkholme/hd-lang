import { createHash } from "node:crypto";

// The SDK snapshot contains this exact platform-neutral graph and its ABI model.
export const runtimeSources = Object.freeze({
  "core.mjs": "host/core.mjs",
  "checked-report.mjs": "host/checked-report.mjs",
  "float.mjs": "host/float.mjs",
  "journal.mjs": "host/journal.mjs",
  "wire.mjs": "host/wire.mjs",
  "package-providers.mjs": "host/package-providers.mjs",
  "gen/abi.mjs": "host/gen/abi.mjs",
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
