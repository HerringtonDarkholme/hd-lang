// Node-specific path, host-name and file-operation helpers. This module is
// deliberately below `node.ts`: capability workers import it without
// evaluating the host that starts those workers.
import { FsError, METHODS, Path } from "./gen/abi.ts";
import { Err, Ok, variant } from "./wire.ts";
import type { Result, TaggedValue } from "./wire.ts";
import type { BigIntStats } from "node:fs";

const {
  appendFileSync,
  closeSync,
  copyFileSync,
  existsSync,
  fstatSync,
  lstatSync,
  mkdirSync,
  openSync,
  readdirSync,
  readFileSync,
  readlinkSync,
  readSync,
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
export function resolvePath(path: string, cwd = process.cwd(), links = 0): string {
  let full: string;
  if (sep === "\\") {
    full = resolve(cwd, path);
  } else {
    full = isAbsolute(path) ? path : `${cwd}/${path}`;
  }
  const { root } = parse(full);
  const parts = full
    .slice(root.length)
    .split(sep)
    .filter((part) => part !== "" && part !== ".");
  let start = parts.length;
  let out: string | null = realOf(root + parts.join(sep));
  if (out === null) {
    // The longest prefix the OS resolves, by binary search (#1901): a
    // prefix resolves only when every shorter one does, since the OS walks
    // each name's directory before the name. Invariant: prefix `start`
    // resolves (to `found`, or is the root), prefix `end` does not.
    // cost: O(log n) realpath calls, each O(n), for n names.
    let end = parts.length;
    let found: string | null = null;
    start = 0;
    while (end - start > 1) {
      const mid = (start + end) >>> 1;
      const real = realOf(root + parts.slice(0, mid).join(sep));
      if (real === null) {
        end = mid;
      } else {
        [start, found] = [mid, real];
      }
    }
    out = found ?? realOf(root) ?? root;
  }
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
function realOf(path: string): string | null {
  try {
    return realpathSync.native(path);
  } catch {
    return null;
  }
}

// `path`, whose parent is resolved, resolved by the OS, or through the
// symbolic link it names when the link's target does not exist; `null`
// when no such file or link exists.
function linkStep(path: string, links: number): string | null {
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
export function validHost(host: string): boolean {
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
} as const;

type FsTag = (typeof FS_CODES)[keyof typeof FS_CODES] | "InvalidUtf8" | "NotGranted";
interface ErrorLike {
  code?: string;
  message?: unknown;
}

const errorLike = (error: unknown): ErrorLike =>
  typeof error === "object" && error !== null ? (error as ErrorLike) : {};

// `.Err(FsError)` of the variant `tag` for a file operation on `path`.
export const fsErr = (path: string, tag: FsTag): Result<never, TaggedValue> =>
  Err(variant(FsError, tag, path));
const fsFail = (path: string, error: unknown): Result<never, TaggedValue> => {
  const detail = errorLike(error);
  const tag = FS_CODES[detail.code as keyof typeof FS_CODES];
  return tag === undefined
    ? Err({ tag: "Other", message: String(detail.message ?? error) })
    : fsErr(path, tag);
};

const belowFile = (path: string): boolean => {
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
const opening = <Value>(path: string, open: () => Value): Value => {
  try {
    return open();
  } catch (error) {
    const detail = errorLike(error);
    if ((detail.code === "ENOENT" || detail.code === "ENOTDIR") && belowFile(path)) {
      throw Object.assign(error as object, { code: "ENOTDIR" });
    }
    throw error;
  }
};

// The `FsError` variant for `path` when a name before one of its `..` is
// not an existing directory, as in `data/missing/../a.txt`: NotFound, or
// NotADirectory for a file; `undefined` otherwise. Unix fails such a path
// itself, but Windows resolves `..` as text and skips the missing name, so
// the host checks it on every OS (cli.engine.same-behavior, #277). The
// prefix is joined by hand: `path.join` would resolve the `..` away.
export const beforeDotDot = (path: string): "NotFound" | "NotADirectory" | undefined => {
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

// One decoder for every `read_text!` (#1901): a fatal decoder throws on
// invalid UTF-8, and `decode` without `stream` keeps no state between calls.
const utf8 = new TextDecoder("utf-8", { fatal: true });

// `left` before `right` in the byte order of their UTF-8 encodings, without
// encoding them (#1901). UTF-8 orders as code points; UTF-16 code units
// order the same except that a surrogate (a code point from U+10000) must
// sort after U+E000..U+FFFF, so where both differing units are at or past
// U+D800 they are moved: U+E000..U+FFFF down by 0x800, surrogates up by
// 0x2000. This is ICU's code point order fix-up (`u_strCompare`).
// cost: O(the common prefix), no allocation.
export const byteOrder = (left: string, right: string): number => {
  const shorter = Math.min(left.length, right.length);
  for (let i = 0; i < shorter; i++) {
    let mine = left.charCodeAt(i);
    let theirs = right.charCodeAt(i);
    if (mine !== theirs) {
      if (mine >= 0xd800 && theirs >= 0xd800) {
        mine += mine >= 0xe000 ? -0x800 : 0x2000;
        theirs += theirs >= 0xe000 ? -0x800 : 0x2000;
      }
      return mine - theirs;
    }
  }
  return left.length - right.length;
};

// An `Entry`: path, kind, size, and an optional Unix-millisecond timestamp.
const entry = (path: string, st: BigIntStats) => {
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

// `path`'s bytes. `size` is a regular file's size from the stat the
// provider just made (#1901): the read then opens, reads and closes, and
// makes no second stat, as readFileSync's fstat would. Like readFileSync it
// reads `size` bytes, stops at the end of a file that shrank, and reads to
// the end when the size is 0, as a Linux /proc file reports. Without
// `size` (the special-file helper) it is readFileSync.
// Node reads at most 2 GiB - 1 bytes in one call (readFileSync's
// kIoMaxLength); past it readFileSync rejects from the stat with
// "File size (N) is greater than 2 GiB", reading nothing, which the
// wasmtime host matches (core_parity::filesystem_sparse_file_size_bound).
const IO_MAX_LENGTH = 2 ** 31 - 1;

const readBytes = (path: string, size?: number): Uint8Array => {
  if (size === undefined || size === 0 || size > IO_MAX_LENGTH) {
    return opening(path, () => readFileSync(path));
  }
  const fd = opening(path, () => openSync(path, "r"));
  try {
    const bytes = Buffer.allocUnsafeSlow(size);
    let filled = 0;
    // cost: one read call for a file that keeps its size.
    while (filled < size) {
      const read = readSync(fd, bytes, filled, size - filled, null);
      if (read === 0) {
        return bytes.subarray(0, filled);
      }
      filled += read;
    }
    return bytes;
  } finally {
    closeSync(fd);
  }
};

// The most bytes one `readSync` of an open file asks for; a read of more
// makes more calls, so a chunk is never allocated ahead of its bytes.
const READ_CHUNK = 65536;

// `File.read`: the next bytes of the open file `fd`, at most `max`,
// all that are left when fewer, as the Rust host reads a regular file.
// cost: one host call per chunk, ceil(n / READ_CHUNK) `readSync` calls,
// and O(n) memory for the n bytes read.
export const readOpen = (fd: number, max: number): Uint8Array => {
  const parts: Buffer[] = [];
  let left = max;
  while (left > 0) {
    const chunk = Buffer.allocUnsafe(Math.min(left, READ_CHUNK));
    const count = readSync(fd, chunk, 0, chunk.length, null);
    if (count === 0) {
      break;
    }
    parts.push(chunk.subarray(0, count));
    left -= count;
  }
  return parts.length === 1 ? parts[0]! : Buffer.concat(parts);
};

// `File.read` of a special file `fd` (std-fs.open.special-file): one
// `readSync`, which waits for any bytes and returns those it has, at most
// `max` and at most READ_CHUNK, or none at the end (libuv retries an
// interrupted read). It runs on a thread of the helper child that holds
// the file (fs.ts), as the Rust host's `read_once` runs on its blocking
// pool. A short read is copied out, since posting a view would copy its
// whole buffer. cost: one system call and O(min(max, READ_CHUNK)) memory.
export const readSpecial = (fd: number, max: number): Result<Uint8Array, TaggedValue> => {
  const chunk = new Uint8Array(Math.min(max, READ_CHUNK));
  try {
    const count = readSync(fd, chunk, 0, chunk.length, null);
    return Ok(count === chunk.length ? chunk : chunk.slice(0, count));
  } catch (error) {
    return Err({ tag: "Other", message: `read: ${(error as Error).message}` });
  }
};

export const fileOperations = {
  "hd:FsRead": {
    // A file opened for reading: its descriptor, which `File.dispose`
    // closes (std-fs.open.dispose). A directory is `IsADirectory`, as its
    // read would be.
    open: (path: string) => {
      const fd = opening(path, () => openSync(path, "r"));
      if (fstatSync(fd).isDirectory()) {
        closeSync(fd);
        return fsErr(path, "IsADirectory");
      }
      return Ok(fd);
    },
    read_bytes: (path: string, size?: number) => Ok(readBytes(path, size)),
    read_text: (path: string, size?: number) => {
      const bytes = readBytes(path, size);
      try {
        return Ok(utf8.decode(bytes));
      } catch {
        return fsErr(path, "InvalidUtf8");
      }
    },
    // The entries in the byte order of their names; each path is the
    // directory's, `/`, and the name (as `MemoryFs` lists them).
    list_dir: (path: string) => {
      // cost: O(n log n) comparisons for n names, none allocating.
      const names = opening(path, () => readdirSync(path)).sort(byteOrder);
      const under = path.endsWith("/") ? path : `${path}/`;
      const found = [];
      for (const count of names) {
        try {
          found.push(entry(under + count, lstatSync(under + count, { bigint: true })));
        } catch {
          // Gone since the listing.
        }
      }
      return Ok(found);
    },
    stat: (path: string) => {
      let stats: BigIntStats;
      try {
        stats = lstatSync(path, { bigint: true });
      } catch (error) {
        const detail = errorLike(error);
        if (detail.code === "ENOENT" || detail.code === "ENOTDIR") {
          return Ok(null);
        }
        throw error;
      }
      return Ok(entry(path, stats));
    },
  },
  "hd:FsWrite": {
    write_bytes: (path: string, bytes: Uint8Array) =>
      Ok(opening(path, () => writeFileSync(path, bytes))),
    write_text: (path: string, text: string) => Ok(opening(path, () => writeFileSync(path, text))),
    append_text: (path: string, text: string) =>
      Ok(opening(path, () => appendFileSync(path, text))),
    create_dir_all: {
      run: (path: string) => {
        if (existsSync(path) && !statSync(path).isDirectory()) {
          return fsErr(path, "NotADirectory");
        }
        mkdirSync(path, { recursive: true });
        return Ok(undefined);
      },
      create: true,
    },
    remove: (path: string) => {
      if (lstatSync(path).isDirectory()) {
        rmdirSync(path);
      } else {
        unlinkSync(path);
      }
      return Ok(undefined);
    },
    // cli.cap.scope.rename: the grant covers both paths.
    rename: (from: string, to: string) => Ok(renameSync(from, to)),
    copy: (from: string, to: string) => {
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
type FileOperation = ((...args: unknown[]) => unknown) | { run: (...args: unknown[]) => unknown };

export interface FileJob {
  module: string;
  name: string;
  args: unknown[];
}

export function fileOperation(module: string, name: string, args: unknown[]): unknown {
  const modules = fileOperations as unknown as Record<string, Record<string, FileOperation>>;
  const op = modules[module]![name]!;
  try {
    return (typeof op === "function" ? op : op.run)(...args);
  } catch (error) {
    const methods = METHODS as unknown as Record<string, Record<string, { params: unknown[] }>>;
    const method = methods[module]?.[name];
    const paths = args.filter((_, index) => method?.params[index] === Path) as string[];
    return fsFail(paths[paths.length - 1]!, error);
  }
}
