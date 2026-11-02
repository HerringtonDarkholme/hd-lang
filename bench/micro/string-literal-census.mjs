// Fixed string-literal sites in the benchmark and example programs. Audit
// microcases marked with `@dump` are excluded so they do not bias the corpus.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const ROOT = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
const ROOTS = [
  "bench/study",
  "bench/runtime/progs",
  "bench/micro",
  "bench/math",
  "bench/records",
  "bench/tokenizer",
  "bench/traits",
  "examples",
  "website/playground/examples",
];

function filesBelow(path) {
  if (!statSync(path).isDirectory()) {
    return path.endsWith(".hd") ? [path] : [];
  }
  return readdirSync(path).flatMap((name) => filesBelow(join(path, name)));
}

function interpolated(text) {
  for (let i = 0; i < text.length; i++) {
    if (text[i] === "\\") {
      i++;
    } else if (text[i] === "$" && (text[i + 1] === "{" || /[_\p{L}]/u.test(text[i + 1] ?? ""))) {
      return true;
    }
  }
  return false;
}

function cookedBytes(text) {
  let cooked = "";
  for (let i = 0; i < text.length; i++) {
    if (text[i] !== "\\") {
      cooked += text[i];
      continue;
    }
    const escape = text[++i];
    if (escape === "u" && text[i + 1] === "{") {
      const end = text.indexOf("}", i + 2);
      cooked += String.fromCodePoint(Number.parseInt(text.slice(i + 2, end), 16));
      i = end;
    } else {
      cooked += { n: "\n", r: "\r", t: "\t", 0: "\0" }[escape] ?? escape;
    }
  }
  return Buffer.byteLength(cooked);
}

function literals(source) {
  const found = [];
  for (let i = 0; i < source.length;) {
    if (source[i] === "#") {
      i = source.indexOf("\n", i);
      if (i < 0) {
        break;
      }
      continue;
    }
    if (source[i] !== '"') {
      i++;
      continue;
    }
    const triple = source.slice(i, i + 3) === '"""';
    const width = triple ? 3 : 1;
    const start = i + width;
    i = start;
    let escaped = false;
    while (i < source.length) {
      if (!escaped && source.slice(i, i + width) === '"'.repeat(width)) {
        break;
      }
      escaped = !escaped && source[i] === "\\";
      if (escaped) {
        i++;
        continue;
      }
      escaped = false;
      i++;
    }
    found.push({ text: source.slice(start, i), triple });
    i += width;
  }
  return found;
}

const paths = ROOTS.flatMap((root) => filesBelow(join(ROOT, root))).sort();
const lengths = [];
let dynamic = 0;
let multiline = 0;
let includedFiles = 0;
for (const path of paths) {
  const source = readFileSync(path, "utf8");
  if (path.includes("/bench/micro/") && source.startsWith("# @dump:")) {
    continue;
  }
  includedFiles++;
  for (const literal of literals(source)) {
    if (literal.triple) {
      multiline++;
    } else if (interpolated(literal.text)) {
      dynamic++;
    } else {
      lengths.push(cookedBytes(literal.text));
    }
  }
}

console.log(`files\t${includedFiles}`);
console.log(`fixed\t${lengths.length}`);
console.log(`interpolated\t${dynamic}`);
console.log(`multiline\t${multiline}`);
for (const limit of [6, 8, 10, 12]) {
  const count = lengths.filter((length) => length <= limit).length;
  console.log(`<=${limit}\t${count}\t${((100 * count) / lengths.length).toFixed(1)}%`);
}
