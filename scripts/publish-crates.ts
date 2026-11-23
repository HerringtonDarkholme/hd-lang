// Packages the crates that `cargo install hd-lang` builds, checks that each
// one builds from its own package, and with --publish uploads them to
// crates.io (crates/README.md, "Publishing").
//
// A published crate holds only the files in its own directory, and two of
// ours read files outside theirs: hd_driver embeds `lib/std`, and hd_command
// embeds the Node host that Node strips from `host/*.ts`. So this script
// stages a copy of the workspace under target/publish/stage, puts `lib/std`
// in the copy of hd_driver as `std/` and the stripped host in the copy of
// hd_command as `host/`, and runs cargo there. Each crate's build.rs reads
// its staged directory when it has one. The source tree is never written, so
// a checkout never builds from a stale staged copy.
//
// cargo then packages every publishable crate and builds the hd-lang
// package with every other crate taken from its package, not from the
// workspace paths. A crate that still reads outside its directory fails here. Last, the `hd` built
// from the hd-lang package runs a program on Wasmtime, with no `node` on
// PATH.
//
// Uploading goes in dependency order and skips a version crates.io already
// has, so a run that stops can be run again. crates.io limits new crate
// names (a burst of 5, then one every 10 minutes), which the first release
// of about 26 crates meets; the script waits and retries.
//
// Cost per run: one copy of each source file under crates/ (about 5,600),
// one download and one dev build from nothing of the publishable crates and
// their dependencies, each compiled once (a run keeps no cargo state, see
// `cargoHome`), and
// with --publish one index request and one upload per crate.
//
// Usage: node scripts/publish-crates.ts             stage, package, verify
//        node scripts/publish-crates.ts --publish   the same, then upload

import { execFileSync, spawnSync } from "node:child_process";
import {
  copyFileSync,
  existsSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { dirname, join } from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

const root = join(import.meta.dirname, "..");
/** Everything a run writes. A run starts by deleting it. */
const work = join(root, "target", "publish");
const stage = join(work, "stage");
const targetDir = join(work, "target");
/**
 * The verifying cargo's own home. cargo takes a registry package of one
 * name and version as never changing: it unpacks it once under its home and
 * compiles it once into a target directory. A second run with the same
 * version then builds against the first run's packages (2026-11-20: hd_wasm
 * was checked against the hd_tir of an older commit, and failed). With the
 * home and the target directory inside `work`, nothing outlives a run, and
 * nothing is written outside target/.
 */
const cargoHome = join(work, "cargo-home");
const manifest = join(stage, "Cargo.toml");

/** The crate users install, and the program its package must run. */
const INSTALLED = "hd-lang";
const SMOKE = "examples/hello/hello.hd";

/** crates.io refills one new crate name every 10 minutes. */
const RATE_LIMIT_WAIT_MS = 10 * 60 * 1000;
/** Enough waits for 40 new names; a first release has about 26. */
const RATE_LIMIT_TRIES = 40;

interface Package {
  name: string;
  version: string;
  /** `null`: any registry. `[]`: `publish = false`. */
  publish: string[] | null;
  dependencies: { name: string; kind: string | null }[];
}

function git(...args: string[]): string {
  // cost: one git process per call; the publish run makes a fixed handful.
  return execFileSync("git", args, { cwd: root, encoding: "utf8", maxBuffer: 1 << 28 });
}

/** The files git tracks or would track under `paths`, as the tree has them now. */
function sourceFiles(...paths: string[]): string[] {
  return git("ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", ...paths)
    .split("\0")
    .filter((file) => file && existsSync(join(root, file)));
}

function copyInto(files: string[], destination: (file: string) => string): void {
  const made = new Set<string>();
  for (const file of files) {
    const to = destination(file);
    const directory = dirname(to);
    if (!made.has(directory)) {
      mkdirSync(directory, { recursive: true });
      made.add(directory);
    }
    copyFileSync(join(root, file), to);
  }
}

function stageWorkspace(): void {
  rmSync(work, { recursive: true, force: true });
  copyInto(
    sourceFiles("Cargo.toml", "Cargo.lock", "README.md", "rust-toolchain.toml", "crates"),
    (file) => join(stage, file),
  );
  const std = sourceFiles("lib/std").filter((file) => file.endsWith(".hd"));
  if (std.length === 0) {
    throw new Error("lib/std holds no .hd files");
  }
  copyInto(std, (file) => join(stage, "crates", "hd_driver", "std", file.slice("lib/std/".length)));
  const host = join(stage, "crates", "hd_command", "host");
  // cost: one node process, once per publish run, listing the host files.
  const listing = execFileSync(
    process.execPath,
    ["--no-warnings", join(root, "scripts", "ship-host.ts"), host],
    { cwd: root, encoding: "utf8" },
  );
  // `src` lines name the checkout's files, which a package does not have.
  const kept = listing.split("\n").filter((line) => line && !line.startsWith("src "));
  writeFileSync(join(host, "ship-host.txt"), kept.join("\n") + "\n");
}

/** The publishable crates, each after the crates it depends on. */
function publishOrder(): Package[] {
  // cost: one `cargo metadata` process, once per publish run.
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--no-deps", "--format-version", "1", "--manifest-path", manifest],
      { encoding: "utf8", maxBuffer: 1 << 28 },
    ),
  ) as { packages: Package[] };
  const byName = new Map(metadata.packages.map((crate) => [crate.name, crate]));
  const publishable = (crate: Package) => crate.publish === null || crate.publish.length > 0;
  const order: Package[] = [];
  const state = new Map<string, "visiting" | "done">();
  // An explicit stack: depth is the dependency chain's, bounded by the
  // number of crates, but the worklist form costs nothing more.
  for (const start of metadata.packages.filter(publishable)) {
    const stack: { crate: Package; next: number }[] = [{ crate: start, next: 0 }];
    while (stack.length > 0) {
      const top = stack[stack.length - 1];
      if (top.next === 0) {
        if (state.has(top.crate.name)) {
          stack.pop();
          continue;
        }
        state.set(top.crate.name, "visiting");
      }
      // A dev-dependency names a path only, and cargo drops it on publish.
      const needed = top.crate.dependencies.filter(
        (needs) => needs.kind !== "dev" && byName.has(needs.name),
      );
      if (top.next < needed.length) {
        const dependency = byName.get(needed[top.next++].name)!;
        if (!publishable(dependency)) {
          throw new Error(
            `${top.crate.name} depends on ${dependency.name}, which is not published`,
          );
        }
        if (state.get(dependency.name) === "visiting") {
          throw new Error(`${top.crate.name} and ${dependency.name} depend on each other`);
        }
        stack.push({ crate: dependency, next: 0 });
      } else {
        state.set(top.crate.name, "done");
        order.push(top.crate);
        stack.pop();
      }
    }
  }
  return order;
}

/** Where each package is unpacked, as `<name>-<version>/`, to be built. */
const unpacked = join(work, "unpacked");

function cargo(args: string[], cwd: string): void {
  const result = spawnSync("cargo", args, {
    cwd,
    stdio: "inherit",
    // cargo still reads the user's configuration, from the .cargo
    // directories above the stage.
    env: { ...process.env, CARGO_HOME: cargoHome },
  });
  if (result.status !== 0) {
    throw new Error(`cargo ${args[0]} failed`);
  }
}

/**
 * Packages every crate, then builds the hd-lang package with every other
 * crate taken from its own package. hd-lang depends on all of them, so
 * this one build compiles each package once; a crate it leaves out fails
 * the run.
 * `cargo publish --dry-run` built each package by itself and then again as
 * the next one's dependency, so every crate, and Wasmtime, compiled twice
 * (5 min, run 45186991942).
 */
function verify(order: Package[], installed: Package): void {
  const selected = order.flatMap((crate) => ["-p", crate.name]);
  // cost: one cargo process packaging every crate together.
  cargo(
    ["package", "--no-verify", "--manifest-path", manifest, "--target-dir", targetDir, ...selected],
    stage,
  );
  const directory = (crate: Package) => join(unpacked, `${crate.name}-${crate.version}`);
  mkdirSync(unpacked, { recursive: true });
  for (const crate of order) {
    const file = join(targetDir, "package", `${crate.name}-${crate.version}.crate`);
    // cost: one tar process per crate, about 26.
    execFileSync("tar", ["-xzf", file, "-C", unpacked]);
  }
  // A package is its own workspace, not a member of the checkout's above it.
  const top = join(directory(installed), "Cargo.toml");
  writeFileSync(top, readFileSync(top, "utf8") + "\n[workspace]\n");
  const patches = order
    .filter((crate) => crate !== installed)
    .flatMap((crate) => [
      "--config",
      `patch.crates-io.${crate.name}.path=${JSON.stringify(directory(crate))}`,
    ]);
  // cost: one build of every package and its dependencies.
  cargo(["build", "--manifest-path", top, "--target-dir", targetDir, ...patches], unpacked);
  // The target directory is new each run, so a fingerprint is this build's.
  const built = new Set(
    readdirSync(join(targetDir, "debug", ".fingerprint")).map((entry) =>
      entry.slice(0, entry.lastIndexOf("-")),
    ),
  );
  const missed = order.filter((crate) => !built.has(crate.name)).map((crate) => crate.name);
  if (missed.length > 0) {
    throw new Error(`${INSTALLED} does not build ${missed.join(", ")}; verify it another way`);
  }
}

/** Where cargo leaves the `hd` it builds from the hd-lang package. */
const packagedHd = join(targetDir, "debug", process.platform === "win32" ? "hd.exe" : "hd");

/** Runs the `hd` that cargo built from the hd-lang package, with no Node. */
function smoke(): void {
  const hd = packagedHd;
  if (!existsSync(hd)) {
    throw new Error(`cargo left no hd at ${hd}`);
  }
  // cost: one run of the packaged `hd` on one small program, once per publish run.
  const result = spawnSync(hd, ["--engine", "wasmtime", SMOKE], {
    cwd: root,
    encoding: "utf8",
    env: { PATH: "", HOME: process.env.HOME ?? "" },
    timeout: 120_000,
  });
  if (result.status !== 0 || result.stdout !== "42\n") {
    throw new Error(
      `the packaged hd did not run ${SMOKE}: status ${result.status}, ` +
        `stdout ${JSON.stringify(result.stdout)}, stderr ${JSON.stringify(result.stderr)}`,
    );
  }
  process.stdout.write(`the packaged hd ran ${SMOKE} on Wasmtime without Node\n`);
}

/** Whether crates.io's index already lists this version of the crate. */
async function published(crate: Package): Promise<boolean> {
  // The sparse index path of a name of 4 or more characters.
  const name = crate.name.toLowerCase();
  if (name.length < 4) {
    throw new Error(`${crate.name}: index paths of short names are not handled`);
  }
  const url = `https://index.crates.io/${name.slice(0, 2)}/${name.slice(2, 4)}/${name}`;
  const response = await fetch(url, { signal: AbortSignal.timeout(30_000) });
  if (response.status === 404) {
    return false;
  }
  if (!response.ok) {
    throw new Error(`${url}: HTTP ${response.status}`);
  }
  const lines = (await response.text()).split("\n").filter(Boolean);
  return lines.some((line) => (JSON.parse(line) as { vers: string }).vers === crate.version);
}

async function upload(order: Package[]): Promise<void> {
  let waits = 0;
  for (const crate of order) {
    for (;;) {
      if (await published(crate)) {
        process.stdout.write(`${crate.name} ${crate.version} is on crates.io already\n`);
        break;
      }
      // --no-verify: `verify` built these same packages just now.
      const args = ["publish", "--no-verify", "--manifest-path", manifest, "-p", crate.name];
      // cost: one index request and one cargo process per crate, plus one
      // more of each per rate-limit wait (at most RATE_LIMIT_TRIES in a run).
      const result = spawnSync("cargo", [...args, "--target-dir", targetDir], {
        cwd: stage,
        encoding: "utf8",
        stdio: ["ignore", "inherit", "pipe"],
      });
      process.stderr.write(result.stderr);
      if (result.status === 0) {
        break;
      }
      if (!/429|too many|try again after/i.test(result.stderr) || ++waits > RATE_LIMIT_TRIES) {
        throw new Error(`cargo publish -p ${crate.name} failed; run the script again to resume`);
      }
      process.stdout.write(
        `crates.io limited the upload of ${crate.name}; trying again in 10 minutes ` +
          `(help@crates.io can raise the limit)\n`,
      );
      await sleep(RATE_LIMIT_WAIT_MS);
    }
  }
}

const options = process.argv.slice(2);
const unknown = options.filter((option) => option !== "--publish");
if (unknown.length > 0) {
  throw new Error(`usage: node scripts/publish-crates.ts [--publish] (got ${unknown.join(" ")})`);
}
const publish = options.includes("--publish");
if (publish && git("status", "--porcelain").trim() !== "") {
  throw new Error("--publish uploads what a commit holds: commit or stash the changes first");
}
stageWorkspace();
const order = publishOrder();
const installed = order.find((crate) => crate.name === INSTALLED);
if (!installed) {
  throw new Error(`${INSTALLED} is not a publishable crate of the workspace`);
}
process.stdout.write(`${order.length} crates: ${order.map((crate) => crate.name).join(" ")}\n`);
verify(order, installed);
smoke();
if (publish) {
  await upload(order);
  process.stdout.write(`published; install with: cargo install ${INSTALLED}\n`);
} else {
  process.stdout.write("dry run passed; --publish uploads these crates\n");
}
