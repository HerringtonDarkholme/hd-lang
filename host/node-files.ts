// Node-specific path, host-name and file-operation helpers. This module is
// deliberately below `node.ts`: capability workers import it without
// evaluating the host that starts those workers.
import { FsError, METHODS, Path } from "./gen/abi.ts";
import { Err, Ok, variant } from "./wire.ts";

const {
  appendFileSync,
  closeSync,
  copyFileSync,
  existsSync,
  lstatSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  realpathSync,
  renameSync,
  rmdirSync,
  statSync,
  unlinkSync,
  writeFileSync,
} = process.getBuiltinModule("node:fs");
const { dirname, isAbsolute, join, parse, resolve, sep } = process.getBuiltinModule("node:path");

export const pathSeparator = sep;

// A path resolved as the OS resolves it, so no path escapes a granted
// directory (cli.cap.scope.path.resolved); `hd_run::resolve_path` on the
// Rust side. The longest existing prefix goes to the OS, then each further
// name follows a symbolic link there (even one whose target does not
// exist) before the `..` after it, as a write or `create_dir_all` would. A
// name that does not exist is kept as it is, and a `..` after it removes
// it. Windows resolves every `..` by its text before it follows any link,
// so there the path is made lexical first. A relative path is from `cwd`.
// `realpathSync.native`, like Rust's canonicalize, asks the OS, so on Windows
// both expand 8.3 short names (RUNNER~1) and agree on the drive letter case.
export function resolvePath(path, cwd = process.cwd(), links = 0) {
  let full;
  if (sep === "\\") {
    full = resolve(cwd, path);
  } else {
    full = isAbsolute(path) ? path : `${cwd}/${path}`;
  }
  const { root } = parse(full);
  const parts = full
    .slice(root.length)
    .split(sep)
    .filter((p) => p !== "" && p !== ".");
  let start = parts.length;
  let out = realOf(root + parts.join(sep));
  while (out === null && start > 0) {
    start -= 1;
    out = realOf(root + parts.slice(0, start).join(sep));
  }
  out ??= root;
  // How many names at the end of `out` do not exist.
  let missing = 0;
  for (const name of parts.slice(start)) {
    if (name === "..") {
      out = dirname(out);
      missing = Math.max(0, missing - 1);
      continue;
    }
    const next = join(out, name);
    const resolved = missing === 0 ? linkStep(next, links) : null;
    if (resolved === null) {
      missing += 1;
    }
    out = resolved ?? next;
  }
  return out;
}

// How many symbolic links one resolution follows before it stops, as the
// OS does with `ELOOP`; the access then fails on its own.
const MAX_LINKS = 40;

// `path` resolved by the OS, or `null` when it does not exist.
function realOf(path) {
  try {
    return realpathSync.native(path);
  } catch {
    return null;
  }
}

// `path`, whose parent is resolved, resolved by the OS, or through the
// symbolic link it names when the link's target does not exist; `null`
// when no such file or link exists.
function linkStep(path, links) {
  const real = realOf(path);
  if (real !== null || links >= MAX_LINKS) {
    return real;
  }
  let target;
  try {
    target = readlinkSync(path);
  } catch {
    return null;
  }
  return resolvePath(target, dirname(path), links + 1);
}

// Whether `host` is a name or an IP address
// (std-net.error.invalid-address).
export function validHost(host) {
  const name =
    /^(?=.{1,253}$)[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?(?:\.[A-Za-z0-9](?:[A-Za-z0-9-]*[A-Za-z0-9])?)*\.?$/;
  return process.getBuiltinModule("node:net").isIP(host) !== 0 || name.test(host);
}

// The `FsError` variant of a failed file operation's error code.
const FS_CODES = {
  ENOENT: "NotFound",
  EACCES: "PermissionDenied",
  EPERM: "PermissionDenied",
  EEXIST: "AlreadyExists",
  ENOTDIR: "NotADirectory",
  EISDIR: "IsADirectory",
};

// `.Err(FsError)` of the variant `tag` for a file operation on `path`.
export const fsErr = (path, tag) => Err(variant(FsError, tag, path));
const fsFail = (path, err) => {
  const tag = FS_CODES[err?.code];
  return tag === undefined
    ? Err({ tag: "Other", message: String(err?.message ?? err) })
    : fsErr(path, tag);
};

const belowFile = (path) => {
  for (let up = dirname(path); ; up = dirname(up)) {
    try {
      return !statSync(up).isDirectory();
    } catch {
      if (dirname(up) === up) {
        return false;
      }
    }
  }
};

// Windows reports a path below a file as ENOENT, or as ENOTDIR on some
// libuv paths, so the code is not stable there. A failed open whose nearest
// existing ancestor is not a directory is `NotADirectory`
// (std-fs.memory.error.not-directory); a missing ancestor chain stays
// `NotFound`. The Rust host's `below_a_file` walks the same way.
const opening = (path, open) => {
  try {
    return open();
  } catch (err) {
    if ((err?.code === "ENOENT" || err?.code === "ENOTDIR") && belowFile(path)) {
      throw Object.assign(err, { code: "ENOTDIR" });
    }
    throw err;
  }
};

// The `FsError` variant for `path` when a name before one of its `..` is
// not an existing directory, as in `data/missing/../a.txt`: NotFound, or
// NotADirectory for a file; `undefined` otherwise. Unix fails such a path
// itself, but Windows resolves `..` as text and skips the missing name, so
// the host checks it on every OS (cli.engine.same-behavior, #277). The
// prefix is joined by hand: `path.join` would resolve the `..` away.
export const beforeDotDot = (path) => {
  const parts = path.split(/[\\/]/);
  let prefix = parts[0];
  for (const part of parts.slice(1)) {
    if (part === ".." && prefix !== "" && prefix !== ".") {
      let st;
      try {
        st = statSync(prefix);
      } catch {
        return "NotFound";
      }
      if (!st.isDirectory()) {
        return "NotADirectory";
      }
    }
    prefix = `${prefix}/${part}`;
  }
  return undefined;
};

// An `Entry`: path, kind, size, and an optional Unix-millisecond timestamp.
const entry = (path, st) => {
  let kind = "File";
  if (st.isSymbolicLink()) {
    kind = "Symlink";
  } else if (st.isDirectory()) {
    kind = "Directory";
  }
  const millis = typeof st.mtimeNs === "bigint" ? st.mtimeNs / 1_000_000n : null;
  const modified =
    millis === null || millis < -9_223_372_036_854_775_808n || millis > 9_223_372_036_854_775_807n
      ? null
      : { millis };
  return { path, kind: { tag: kind }, size: kind === "Directory" ? 0n : st.size, modified };
};

export const fileOperations = {
  "hd:FsRead": {
    read_bytes: (p) => Ok(opening(p, () => readFileSync(p))),
    read_text: (p) => {
      const bytes = opening(p, () => readFileSync(p));
      try {
        return Ok(new TextDecoder("utf-8", { fatal: true }).decode(bytes));
      } catch {
        return fsErr(p, "InvalidUtf8");
      }
    },
    // The entries in the byte order of their names; each path is the
    // directory's, `/`, and the name (as `MemoryFs` lists them).
    list_dir: (p) => {
      const names = opening(p, () => readdirSync(p)).sort((a, b) =>
        Buffer.compare(Buffer.from(a), Buffer.from(b)),
      );
      const under = p.endsWith("/") ? p : `${p}/`;
      const found = [];
      for (const n of names) {
        try {
          found.push(entry(under + n, lstatSync(under + n, { bigint: true })));
        } catch {
          // Gone since the listing.
        }
      }
      return Ok(found);
    },
    stat: (p) => {
      let st;
      try {
        st = lstatSync(p, { bigint: true });
      } catch (err) {
        if (err.code === "ENOENT" || err.code === "ENOTDIR") {
          return Ok(null);
        }
        throw err;
      }
      return Ok(entry(p, st));
    },
  },
  "hd:FsWrite": {
    write_bytes: (p, bytes) => Ok(opening(p, () => writeFileSync(p, bytes))),
    write_text: (p, text) => Ok(opening(p, () => writeFileSync(p, text))),
    append_text: (p, text) => Ok(opening(p, () => appendFileSync(p, text))),
    create_dir_all: {
      run: (p) => {
        if (existsSync(p) && !statSync(p).isDirectory()) {
          return fsErr(p, "NotADirectory");
        }
        mkdirSync(p, { recursive: true });
        return Ok(undefined);
      },
      create: true,
    },
    remove: (p) => {
      if (lstatSync(p).isDirectory()) {
        rmdirSync(p);
      } else {
        unlinkSync(p);
      }
      return Ok(undefined);
    },
    // cli.cap.scope.rename: the grant covers both paths.
    rename: (from, to) => Ok(renameSync(from, to)),
    copy: (from, to) => {
      // Validate the source as a read before attributing destination errors.
      try {
        if (opening(from, () => statSync(from)).isDirectory()) {
          return fsErr(from, "IsADirectory");
        }
        closeSync(opening(from, () => openSync(from, "r")));
      } catch (err) {
        return fsFail(from, err);
      }
      if (existsSync(to) && statSync(to).isDirectory()) {
        return fsErr(to, "IsADirectory");
      }
      return Ok(opening(to, () => copyFileSync(from, to)));
    },
  },
};

// Shared by the inline provider and its isolated special-file helper;
// checks stay in the provider, and both paths keep the same result/errors.
export function fileOperation(module, name, args) {
  const op = fileOperations[module][name];
  try {
    return (typeof op === "function" ? op : op.run)(...args);
  } catch (err) {
    const m = METHODS[module][name];
    const ps = args.filter((_, i) => m.params[i] === Path);
    return fsFail(ps[ps.length - 1], err);
  }
}
