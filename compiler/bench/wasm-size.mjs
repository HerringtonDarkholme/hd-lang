#!/usr/bin/env node

import { readFileSync } from "node:fs";

const file = process.argv[2];
if (!file) {
  console.error("usage: node compiler/bench/wasm-size.mjs FILE.wasm");
  process.exit(2);
}

const bytes = readFileSync(file);
if (bytes.length < 8 || bytes.subarray(0, 4).toString("hex") !== "0061736d") {
  throw new Error(`${file}: not a Wasm module`);
}

function u32(cursor) {
  let value = 0;
  let shift = 0;
  for (;;) {
    const byte = bytes[cursor.at++];
    value |= (byte & 0x7f) << shift;
    if ((byte & 0x80) === 0) return value >>> 0;
    shift += 7;
    if (shift > 35) throw new Error("invalid u32 LEB");
  }
}

function string(cursor) {
  const length = u32(cursor);
  const start = cursor.at;
  cursor.at += length;
  return bytes.subarray(start, start + length).toString("utf8");
}

function limits(cursor) {
  const flags = u32(cursor);
  u32(cursor);
  if (flags & 1) u32(cursor);
  if (flags & 4) u32(cursor);
}

function skipRefType(cursor) {
  const first = bytes[cursor.at++];
  if (first === 0x63 || first === 0x64) u32(cursor);
}

const sectionNames = [
  "custom", "type", "import", "function", "table", "memory", "global",
  "export", "start", "element", "code", "data", "data_count", "tag",
];
const sections = [];
const byId = new Map();
const custom = [];
let cursor = { at: 8 };
while (cursor.at < bytes.length) {
  const start = cursor.at;
  const id = bytes[cursor.at++];
  const payloadBytes = u32(cursor);
  const payloadStart = cursor.at;
  const end = payloadStart + payloadBytes;
  if (end > bytes.length) throw new Error("section exceeds module");
  let name = sectionNames[id] ?? `section_${id}`;
  if (id === 0) {
    const c = { at: payloadStart };
    name = `custom:${string(c)}`;
    custom.push({ name, payloadStart, end });
  } else {
    byId.set(id, { payloadStart, end });
  }
  sections.push({ name, offset: start, payloadBytes, encodedBytes: end - start });
  cursor.at = end;
}

const imports = [];
const importSection = byId.get(2);
if (importSection) {
  cursor = { at: importSection.payloadStart };
  const count = u32(cursor);
  for (let i = 0; i < count; i++) {
    const module = string(cursor);
    const name = string(cursor);
    const kind = bytes[cursor.at++];
    if (kind === 0) {
      u32(cursor);
      imports.push({ index: imports.length, name: `${module}/${name}` });
    } else if (kind === 1) {
      skipRefType(cursor);
      limits(cursor);
    } else if (kind === 2) {
      limits(cursor);
    } else if (kind === 3) {
      skipRefType(cursor);
      cursor.at++;
    } else {
      throw new Error(`unsupported import kind ${kind}`);
    }
  }
}

const functionNames = new Map(imports.map(({ index, name }) => [index, name]));
const nameSection = custom.find((section) => section.name === "custom:name");
if (nameSection) {
  cursor = { at: nameSection.payloadStart };
  string(cursor);
  while (cursor.at < nameSection.end) {
    const kind = bytes[cursor.at++];
    const size = u32(cursor);
    const end = cursor.at + size;
    if (kind === 1) {
      const count = u32(cursor);
      for (let i = 0; i < count; i++) functionNames.set(u32(cursor), string(cursor));
    }
    cursor.at = end;
  }
}

const exports = [];
const exportSection = byId.get(7);
if (exportSection) {
  cursor = { at: exportSection.payloadStart };
  const count = u32(cursor);
  for (let i = 0; i < count; i++) {
    const name = string(cursor);
    const kind = bytes[cursor.at++];
    const index = u32(cursor);
    exports.push({ name, kind, index });
  }
}

function paddedIndex(at, end) {
  if (at + 5 > end) return undefined;
  if (
    (bytes[at] & 0x80) === 0 || (bytes[at + 1] & 0x80) === 0 ||
    (bytes[at + 2] & 0x80) === 0 || (bytes[at + 3] & 0x80) === 0 ||
    (bytes[at + 4] & 0x80) !== 0
  ) return undefined;
  const c = { at };
  return u32(c);
}

const functions = [];
const codeSection = byId.get(10);
if (codeSection) {
  cursor = { at: codeSection.payloadStart };
  const count = u32(cursor);
  for (let i = 0; i < count; i++) {
    const encodedStart = cursor.at;
    const codeBytes = u32(cursor);
    const bodyStart = cursor.at;
    const bodyEnd = bodyStart + codeBytes;
    const index = imports.length + i;
    const edges = [];
    for (let at = bodyStart; at < bodyEnd - 5; at++) {
      const opcode = bytes[at];
      if (opcode !== 0x10 && opcode !== 0xd2) continue;
      const target = paddedIndex(at + 1, bodyEnd);
      if (target !== undefined) {
        edges.push({ kind: opcode === 0x10 ? "call" : "ref.func", target });
        at += 5;
      }
    }
    functions.push({
      index,
      name: functionNames.get(index) ?? `func[${index}]`,
      bodyBytes: codeBytes,
      encodedBytes: bodyEnd - encodedStart,
      edges,
    });
    cursor.at = bodyEnd;
  }
}

const dataSegments = [];
const dataSection = byId.get(11);
if (dataSection) {
  cursor = { at: dataSection.payloadStart };
  const count = u32(cursor);
  for (let i = 0; i < count; i++) {
    const flags = u32(cursor);
    if (flags === 0 || flags === 2) {
      if (flags === 2) u32(cursor);
      while (bytes[cursor.at++] !== 0x0b) {}
    }
    const length = u32(cursor);
    const start = cursor.at;
    cursor.at += length;
    dataSegments.push({
      index: i,
      mode: flags === 1 ? "passive" : "active",
      bytes: length,
      preview: bytes.subarray(start, start + Math.min(length, 200)).toString("utf8"),
    });
  }
}

const root = exports.find((item) => item.kind === 0 && item.name === "main")?.index;
const parents = new Map(root === undefined ? [] : [[root, null]]);
const edgeKinds = new Map();
const queue = root === undefined ? [] : [root];
const functionByIndex = new Map(functions.map((fn) => [fn.index, fn]));
while (queue.length) {
  const from = queue.shift();
  const fn = functionByIndex.get(from);
  if (!fn) continue;
  for (const edge of fn.edges) {
    if (parents.has(edge.target)) continue;
    parents.set(edge.target, from);
    edgeKinds.set(edge.target, edge.kind);
    queue.push(edge.target);
  }
}

function chain(index) {
  if (!parents.has(index)) return "reachable through a table, adapter, or linker root";
  const path = [];
  for (let at = index; at !== null; at = parents.get(at)) path.push(at);
  path.reverse();
  return path.map((item, i) => {
    const name = functionNames.get(item) ?? `func[${item}]`;
    return i === 0 ? name : `${edgeKinds.get(item) ?? "call"} ${name}`;
  }).join(" -> ");
}

const output = {
  file,
  bytes: bytes.length,
  sections,
  exports,
  functions: [
    ...imports.map((item) => ({ ...item, bodyBytes: 0, encodedBytes: 0, why: "host import reached by emitted code" })),
    ...functions.map((fn) => ({ ...fn, why: chain(fn.index) })),
  ],
  dataSegments,
};
console.log(JSON.stringify(output, null, 2));
