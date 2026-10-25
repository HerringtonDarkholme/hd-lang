import { createHash } from "node:crypto";
import { mkdir, lstat, readdir, readFile, writeFile, mkdtemp, rename, rm } from "node:fs/promises";
import { dirname, join, resolve } from "node:path";

const marker = ".hd-server-sdk.json";
const generationMarker = ".hd-generation.json";
const fail = (message) => new Error("SDK output: " + message);
function publisher(profile) {
  const owner = { format: 1, platform: profile.platform };
  const hash = (bytes) => createHash("sha256").update(bytes).digest("hex");
  const json = (value) => JSON.stringify(value, null, 2) + "\n";
  const same = (a, b) => JSON.stringify(a) === JSON.stringify(b);
  const regular = async (file) => {
    const stat = await lstat(file);
    if (!stat.isFile() || stat.isSymbolicLink()) throw fail("expected an owned regular file");
    return readFile(file);
  };
  async function ownershipRecord(file) {
    try {
      return JSON.parse(await regular(file));
    } catch {
      throw fail("missing or invalid output ownership record");
    }
  }
  async function inventory(directory, allowedDirectories, prefix = "") {
    const files = Object.create(null);
    for (const name of (await readdir(join(directory, prefix))).sort()) {
      const path = prefix ? prefix + "/" + name : name;
      const stat = await lstat(join(directory, path));
      if (stat.isSymbolicLink()) throw fail("output contains a symbolic link");
      if (stat.isDirectory()) {
        if (!allowedDirectories.has(path)) throw fail("generation contains an unrelated directory");
        Object.assign(files, await inventory(directory, allowedDirectories, path));
      } else if (stat.isFile() && path !== generationMarker)
        files[path] = hash(await regular(join(directory, path)));
      else if (!stat.isFile()) throw fail("output contains a non-file");
    }
    return Object.fromEntries(Object.entries(files).sort(([a], [b]) => a.localeCompare(b, "en")));
  }
  function generationId(configuration, files) {
    return hash(JSON.stringify({ configuration, files }));
  }
  async function inspect(output) {
    let stat;
    try {
      stat = await lstat(output);
    } catch (error) {
      if (error.code === "ENOENT") return null;
      throw error;
    }
    if (!stat.isDirectory() || stat.isSymbolicLink())
      throw fail("output is not an owned directory");
    if (!same(await ownershipRecord(join(output, marker)), owner))
      throw fail("refusing output without its ownership record");
    // Platform caches and unrelated root files are outside the SDK's ownership.
    // Only the selector and reserved generation store are inspected or changed.
    let active;
    try {
      active = (await regular(join(output, profile.selector))).toString("utf8");
    } catch (error) {
      if (error.code !== "ENOENT") throw error;
      active = null;
    }
    const generations = join(output, profile.store);
    let directory;
    try {
      directory = await lstat(generations);
    } catch (error) {
      if (error.code === "ENOENT" && active === null) return new Set();
      throw error;
    }
    if (!directory.isDirectory() || directory.isSymbolicLink())
      throw fail("generation store is not an owned directory");
    const known = new Set();
    let activeKnown = active === null;
    for (const name of await readdir(generations)) {
      if (!profile.validName(name)) throw fail("unrecognized generation");
      const base = join(generations, name);
      const entry = await lstat(base);
      if (!entry.isDirectory() || entry.isSymbolicLink())
        throw fail("invalid generation directory");
      const record = await ownershipRecord(join(base, generationMarker));
      const id = generationId(record.configuration, record.files);
      const allowedDirectories = new Set();
      for (const name of Object.keys(record.files ?? {})) {
        const parts = name.split("/");
        for (let n = 1; n < parts.length; n++) allowedDirectories.add(parts.slice(0, n).join("/"));
      }
      if (
        record.format !== 1 ||
        record.platform !== profile.platform ||
        ((profile.platform === "vercel-node" || Object.hasOwn(record, "generation")) &&
          record.generation !== id) ||
        typeof record.files?.[profile.entry] !== "string" ||
        !same(record.files, await inventory(base, allowedDirectories)) ||
        profile.directory(record.configuration, id) !== name
      )
        throw fail("generation contains added or changed files");
      const configuration = json(profile.configuration(record.configuration, id));
      if (active === configuration) activeKnown = true;
      known.add(id);
    }
    if (!activeKnown) throw fail("platform configuration was changed outside the SDK");
    return known;
  }

  // Publish one regular configuration file, never exchange nonempty directories.
  // Every configuration names a complete immutable generation; old readers keep
  // their original generation. Prior generations are deliberately retained.
  async function publish({ outputDirectory, files, configuration, signal }) {
    signal?.throwIfAborted();
    if (!(files instanceof Map) || !files.has(profile.entry))
      throw fail("platform entry is required");
    profile.check(configuration);
    const captured = new Map();
    for (const [name, value] of [...files].sort(([a], [b]) => a.localeCompare(b, "en"))) {
      if (
        typeof name !== "string" ||
        name
          .split("/")
          .some((part) => !part || part === "." || part === ".." || part.includes("\\")) ||
        name.startsWith("/") ||
        name.includes(":") ||
        name.split("/").includes(generationMarker) ||
        !(value instanceof Uint8Array)
      )
        throw fail("invalid captured output file");
      captured.set(name, Uint8Array.from(value));
    }
    const config = JSON.parse(JSON.stringify(configuration));
    const hashes = Object.fromEntries([...captured].map(([name, bytes]) => [name, hash(bytes)]));
    const generation = generationId(config, hashes);
    const output = resolve(outputDirectory);
    const contents = json(profile.configuration(config, generation));
    await mkdir(dirname(output), { recursive: true });
    const lock = output + ".hd-lock";
    await mkdir(lock);
    let stage;
    try {
      const known = await inspect(output);
      signal?.throwIfAborted();
      stage = await mkdtemp(join(dirname(output), ".hd-" + profile.platform + "-"));
      const payload = join(stage, profile.store, profile.directory(config, generation));
      await mkdir(payload, { recursive: true });
      for (const [name, bytes] of captured) {
        const file = join(payload, name);
        await mkdir(dirname(file), { recursive: true });
        await writeFile(file, bytes, { flag: "wx" });
      }
      await writeFile(
        join(payload, generationMarker),
        json({ ...owner, generation, configuration: config, files: hashes }),
        { flag: "wx" },
      );
      await writeFile(join(stage, marker), json(owner), { flag: "wx" });
      await writeFile(join(stage, profile.selector), contents, { flag: "wx" });
      signal?.throwIfAborted();
      if (known === null) {
        // Claim a fresh output exclusively: rename alone may replace an unowned
        // empty directory created while staging. Populate only our claimed root.
        await mkdir(output);
        await rename(join(stage, marker), join(output, marker));
        await mkdir(join(output, profile.store));
        await rename(payload, join(output, profile.store, profile.directory(config, generation)));
      } else {
        // Recheck SDK-owned paths; unrelated root files are never traversed.
        await inspect(output);
        await mkdir(join(output, profile.store), { recursive: true });
        if (!known.has(generation))
          await rename(payload, join(output, profile.store, profile.directory(config, generation)));
      }
      signal?.throwIfAborted();
      // Only cooperating SDK writers hold the lock. Other tools may own root
      // extras, but must not edit the SDK selector/generations during publishing.
      // A failed or cancelled commit leaves a valid inactive generation for retry.
      await rename(join(stage, profile.selector), join(output, profile.selector));
      return Object.freeze({ directory: output, generation, files: captured.size });
    } finally {
      if (stage) await rm(stage, { recursive: true, force: true });
      await rm(lock, { recursive: true, force: true });
    }
  }

  return publish;
}

export const publishCloudflare = publisher({
  platform: "cloudflare",
  selector: "wrangler.json",
  store: "generations",
  entry: "entry.mjs",
  validName(name) {
    return /^[0-9a-f]{64}$/.test(name);
  },
  directory(_configuration, id) {
    return id;
  },
  check(configuration) {
    if (Object.hasOwn(configuration, "main")) throw fail("the SDK chooses the entry path");
  },
  configuration(value, id) {
    return { ...value, main: `generations/${id}/entry.mjs` };
  },
});
export const publishVercelNode = publisher({
  platform: "vercel-node",
  selector: "config.json",
  store: "functions",
  entry: "index.mjs",
  validName(name) {
    return /^[a-z0-9][a-z0-9-]*-[0-9a-f]{48}\.func$/.test(name);
  },
  directory(configuration, id) {
    return `${configuration.name}-${id.slice(0, 48)}.func`;
  },
  check(configuration) {
    if (
      typeof configuration?.name !== "string" ||
      Object.keys(configuration).length !== 1 ||
      !/^[a-z0-9][a-z0-9-]{0,62}$/.test(configuration.name)
    )
      throw fail("simple Vercel function name required");
  },
  configuration(value, id) {
    return {
      version: 3,
      routes: [
        {
          src: "/(.*)",
          dest: `/${value.name}-${id.slice(0, 48)}`,
          transforms: [{ type: "request.path", op: "set", args: "/$1" }],
        },
      ],
    };
  },
});
