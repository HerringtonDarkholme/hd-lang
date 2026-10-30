import { createHash } from "node:crypto";

// The SDK snapshot contains this exact platform-neutral graph and its ABI model.
export const runtimeSources = Object.freeze({
  "core.js": "host/core.ts",
  "core-package.js": "host/core-package.ts",
  "session.js": "host/session.ts",
  "checked-report.js": "host/checked-report.ts",
  "float.js": "host/float.ts",
  "journal.js": "host/journal.ts",
  "run-support.js": "host/run-support.ts",
  "wire.js": "host/wire.ts",
  "package-contract.js": "host/package-contract.ts",
  "package-providers.js": "host/package-providers.ts",
  "gen/abi.js": "host/gen/abi.ts",
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
