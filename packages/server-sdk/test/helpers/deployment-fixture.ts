import { execFileSync } from "node:child_process";
import { cpSync, mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve, basename } from "node:path";
import { fileURLToPath } from "node:url";

export const repository = fileURLToPath(new URL("../../../../", import.meta.url));
export const hd = process.env.HD_SDK_TEST_HD ?? resolve(repository, "target/debug/hd");
export function environment(cache) {
  const env = { PATH: process.env.PATH, HD_CACHE: cache };
  if (process.platform === "win32") {
    env.SystemRoot = process.env.SystemRoot;
  }
  return env;
}
export function fixture({ probe = false } = {}) {
  const directory = mkdtempSync(join(repository, ".cache-muse-sdk-deployment-"));
  const installation = join(directory, "installed"),
    sdk = join(installation, "sdk"),
    app = join(installation, "app");
  const filter = (path) =>
    !["build", "cloudflare", ".vercel", "node_modules", ".git"].includes(basename(path));
  cpSync(join(repository, "packages/server-sdk"), sdk, { recursive: true, filter });
  cpSync(join(repository, "examples/server-api"), app, { recursive: true, filter });
  const manifest = join(app, "hd.toml");
  writeFileSync(
    manifest,
    readFileSync(manifest, "utf8").replace('"../../packages/server-sdk"', '"../sdk"'),
  );
  if (probe) {
    writeFileSync(
      join(app, "src/main.hd"),
      readFileSync(join(sdk, "test/fixtures/server-schema-probe.hd")),
    );
  }
  const cache = join(directory, "cache");
  const run = (args, cwd = app) =>
    execFileSync(hd, args, { cwd, env: environment(cache), stdio: "pipe", timeout: 30000 });
  run(["build", "--engine", "node"]);
  const build = join(app, "build/debug"),
    wasmPath = join(build, "server_api.wasm"),
    bindingsPath = join(build, "server_api.host.mjs");
  return {
    directory,
    installation,
    app,
    sdk,
    cache,
    run,
    manifest,
    wasmPath,
    bindingsPath,
    wasm: readFileSync(wasmPath),
    companion: readFileSync(bindingsPath, "utf8"),
  };
}
