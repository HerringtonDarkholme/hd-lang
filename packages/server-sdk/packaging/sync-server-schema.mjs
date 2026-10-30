// Repository maintenance only. Installed tasks consume the resulting SDK JSON.
import { readFile, writeFile, mkdir, mkdtemp, rename, rm, open } from "node:fs/promises";
import { execFileSync } from "node:child_process";
import { resolve, join, dirname } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";
import { createHash } from "node:crypto";
import { normalizeServerContract, checkCoverage } from "./server-schema.mjs";
import { schemaFingerprint, validateSchemaAsset } from "./schema-asset.mjs";
const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
const declarations = [
  ["packages/server-sdk/src/server.hd", "HttpServer", "server.HttpServer"],
  ["lib/std/console.hd", "Console", "console.Console"],
  // The public trait is bridged over the wire trait by std code
  // (`std.http.host_provider`), so only its bodiless methods reach the host.
  ["lib/std/http.hd", "Http", "http.Http", "bridged"],
  ["lib/std/http_wire.hd", "Http", "http.Http"],
  ["lib/std/time.hd", "Clock", "time.Clock"],
];

function parenthesisDepth(line) {
  let depth = 0;
  for (const char of line) {
    if (char === "(") {
      depth += 1;
    } else if (char === ")") {
      depth -= 1;
    }
  }
  return depth;
}

function traitMethods(source, { trait, key, bridged }) {
  const lines = source.split("\n");
  const at = lines.findIndex((line) => line === `pub trait ${trait}:`);
  if (at < 0) {
    throw Error(`missing maintenance trait declaration ${key}`);
  }
  const names = [];
  let pending = null;
  let depth = 0;
  for (const line of lines.slice(at + 1)) {
    if (line && !/^[ \t#]/.test(line)) {
      break;
    }
    const method = /^    fn ([_\p{XID_Start}][_\p{XID_Continue}]*)!?(?:\[|\()/u.exec(line);
    if (line.startsWith("    fn ") && !method) {
      throw Error(`unsupported maintenance method declaration ${key}`);
    }
    if (method) {
      pending = method[1];
    }
    if (pending === null) {
      continue;
    }
    depth += parenthesisDepth(line);
    if (depth === 0) {
      if (!bridged || !line.trimEnd().endsWith(":")) {
        names.push(pending);
      }
      pending = null;
    }
  }
  if (!names.length) {
    throw Error(`empty maintenance trait ${key}`);
  }
  return names.sort();
}

export async function sourceInventory(repository) {
  const sourceHashes = {},
    coverage = {};
  for (const [file, trait, key, bridged] of declarations) {
    const source = (await readFile(join(repository, file), "utf8")).replaceAll("\r\n", "\n");
    sourceHashes[file] = hash(source);
    const names = traitMethods(source, { trait, key, bridged });
    if (coverage[key] && JSON.stringify(coverage[key]) !== JSON.stringify(names)) {
      throw Error(`public/wire maintenance method coverage differs: ${key}`);
    }
    coverage[key] = names;
  }
  return { sourceHashes, coverage };
}
export async function generateServerSchema({
  repository,
  compilerPath,
  compilerBaseRevision,
  probeFile,
  outputFile,
  evidenceDirectory,
}) {
  repository = resolve(repository);
  compilerPath = resolve(compilerPath);
  if (!/^[0-9a-f]{40,64}$/.test(compilerBaseRevision)) {
    throw Error("explicit compiler base revision required");
  }
  const { readPackageContract } = await import(
    pathToFileURL(join(repository, "host/package-providers.mts")).href
  );
  const before = await sourceInventory(repository);
  // hd embeds std. A declared compiler base with different std sources cannot
  // authenticate this checkout's method declarations merely by being executable.
  for (const file of declarations
    .map(([file]) => file)
    .filter((file) => file.startsWith("lib/std/"))) {
    const recorded = execFileSync("git", ["show", `${compilerBaseRevision}:${file}`], {
      cwd: repository,
      encoding: "utf8",
    }).replaceAll("\r\n", "\n");
    if (hash(recorded) !== before.sourceHashes[file]) {
      throw Error(`compiler base std source differs: ${file}`);
    }
  }
  const scratchRoot = join(repository, ".cache-muse-server-schema-maintenance");
  await mkdir(scratchRoot, { recursive: true });
  const work = await mkdtemp(join(scratchRoot, "probe-"));
  let output;
  try {
    await mkdir(join(work, "src"));
    await writeFile(join(work, "src/main.hd"), await readFile(probeFile));
    await writeFile(
      join(work, "hd.toml"),
      `[package]\nname="sdk_schema_probe"\n[dependencies]\nserver_sdk={path=${JSON.stringify(join(repository, "packages/server-sdk"))}}\n[capabilities]\n"dep.server_sdk.server.HttpServer"=true\n`,
    );
    const env = { PATH: process.env.PATH, HD_CACHE: join(work, "cache") };
    if (process.platform === "win32") {
      env.SystemRoot = process.env.SystemRoot;
    }
    execFileSync(compilerPath, ["build", "--engine", "node"], {
      cwd: work,
      env,
      stdio: "pipe",
      timeout: 30000,
    });
    const wasm = await readFile(join(work, "build/debug/sdk_schema_probe.wasm")),
      module = new WebAssembly.Module(wasm);
    const sections = WebAssembly.Module.customSections(module, "hd.capabilities");
    if (sections.length !== 1) {
      throw Error("reference probe must have exactly one contract");
    }
    const bytes = new Uint8Array(sections[0]),
      interfaces = readPackageContract(bytes);
    const { sourceHashes, coverage } = await sourceInventory(repository);
    if (JSON.stringify(sourceHashes) !== JSON.stringify(before.sourceHashes)) {
      throw Error("declaration sources changed during schema maintenance");
    }
    checkCoverage(interfaces, coverage);
    const roots = interfaces.filter((i) => !i.handle);
    if (roots.length !== 1 || roots[0].identity.trait_path !== "server.HttpServer") {
      throw Error("reference probe must use only SDK HttpServer");
    }
    const snapshot = {
      format: 1,
      packageHostABI: new DataView(sections[0]).getUint32(0, true),
      provenance: {
        sourceBaseRevision: execFileSync("git", ["rev-parse", "HEAD"], {
          cwd: repository,
          encoding: "utf8",
        }).trim(),
        compilerBaseRevision,
        compilerSha256: hash(await readFile(compilerPath)),
        referenceWasmSha256: hash(wasm),
        referenceContractSha256: hash(bytes),
        probeSha256: hash(await readFile(probeFile)),
        sourceHashes,
      },
      coverage,
      schema: {
        root: "server.HttpServer",
        interfaces: normalizeServerContract(interfaces, roots[0].identity),
      },
    };
    snapshot.fingerprint = schemaFingerprint(snapshot);
    validateSchemaAsset(snapshot);
    if (evidenceDirectory) {
      await mkdir(evidenceDirectory, { recursive: true });
      await writeFile(join(evidenceDirectory, "reference.wasm"), wasm);
      await writeFile(join(evidenceDirectory, "reference-contract.bin"), bytes);
      await writeFile(
        join(evidenceDirectory, "reference-contract.json"),
        JSON.stringify(interfaces, null, 2) + "\n",
      );
    }
    output = JSON.stringify(snapshot, null, 2) + "\n";
    await mkdir(dirname(resolve(outputFile)), { recursive: true });
    const pending = resolve(outputFile) + ".tmp";
    const file = await open(pending, "wx");
    let closed = false;
    try {
      await file.writeFile(output);
      await file.close();
      closed = true;
      await rename(pending, resolve(outputFile));
    } catch (error) {
      if (!closed) {
        await file.close();
      }
      await rm(pending, { force: true });
      throw error;
    }
    return snapshot;
  } finally {
    await rm(work, { recursive: true, force: true });
  }
}
if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  const [repository, compilerPath, compilerBaseRevision, probeFile, outputFile, evidenceDirectory] =
    process.argv.slice(2);
  if (!outputFile) {
    throw Error("usage: sync-server-schema.mjs REPO HD COMPILER-BASE PROBE OUTPUT");
  }
  const snapshot = await generateServerSchema({
    repository,
    compilerPath,
    compilerBaseRevision,
    probeFile,
    outputFile,
    evidenceDirectory,
  });
  console.log(
    JSON.stringify({
      interfaces: snapshot.schema.interfaces.length,
      methods: snapshot.schema.interfaces.reduce((n, i) => n + i.methods.length, 0),
      fingerprint: snapshot.fingerprint,
    }),
  );
}
