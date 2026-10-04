// Mixed-script identifier detection under UTS 39 Restriction Level 4,
// Moderately Restrictive (spec/lang/01-lexical-structure.md
// #r-lex.ident.mixed-script.warning). The Script data lives in
// script-data.ts; this file holds the detection over it.

import { SCRIPT_AUGMENT_OVERRIDES, SCRIPT_NAMES, SCRIPT_RANGES } from "./script-data.ts";

/**
 * UAX 31 Table 5 Recommended Scripts (Unicode 17): scripts in widespread
 * modern customary use. A remainder that resolves to one of these, other
 * than Cyrillic or Greek, is Moderately Restrictive and passes.
 */
const RECOMMENDED_SCRIPTS: ReadonlySet<string> = new Set([
  "Zyyy",
  "Zinh",
  "Arab",
  "Armn",
  "Beng",
  "Cyrl",
  "Deva",
  "Ethi",
  "Geor",
  "Grek",
  "Gujr",
  "Guru",
  "Hang",
  "Hani",
  "Hebr",
  "Hira",
  "Kana",
  "Knda",
  "Khmr",
  "Laoo",
  "Latn",
  "Mlym",
  "Mymr",
  "Orya",
  "Sinh",
  "Taml",
  "Telu",
  "Thaa",
  "Thai",
  "Tibt",
]);

/**
 * UTS 39 section 5.1 writing-system additions: a character whose augmented
 * set holds the key also belongs to each listed system (Hanb is Han with
 * Bopomofo, Hntl Traditional Han with Latin, Jpan Japanese, Kore Korean).
 */
const AUGMENT_ADDITIONS: Readonly<Record<string, readonly string[]>> = {
  Hani: ["Hanb", "Hntl", "Jpan", "Kore"],
  Hira: ["Jpan"],
  Kana: ["Jpan"],
  Hang: ["Kore"],
  Bopo: ["Hanb"],
  Latn: ["Hntl"],
};

const AUGMENT_BY_CODE: ReadonlyMap<number, readonly string[]> = new Map(
  SCRIPT_AUGMENT_OVERRIDES.map(([code, ...scripts]) => [code, scripts]),
);

/** A character's Script value, or `Zzzz` (Unknown) past the table. */
function scriptOf(code: number): string {
  let low = 0;
  let high = SCRIPT_RANGES.length - 1;
  while (low <= high) {
    const middle = (low + high) >> 1;
    const [first, last, script] = SCRIPT_RANGES[middle]!;
    if (code < first) high = middle - 1;
    else if (code > last) low = middle + 1;
    else return script;
  }
  return "Zzzz";
}

/**
 * A character's UTS 39 augmented script set, or null for ALL: sets holding
 * Common or Inherited cover every script (UTS 39 section 5.1 step 2).
 */
function augmentedScripts(code: number): ReadonlySet<string> | null {
  const override = AUGMENT_BY_CODE.get(code);
  if (override !== undefined) return override.includes("ALL") ? null : new Set(override);
  const script = scriptOf(code);
  if (script === "Zyyy" || script === "Zinh") return null;
  return new Set([script, ...(AUGMENT_ADDITIONS[script] ?? [])]);
}

function sameScripts(left: ReadonlySet<string>, right: ReadonlySet<string>): boolean {
  return left.size === right.size && [...left].every((script) => right.has(script));
}

function scriptDisplayName(script: string): string {
  return SCRIPT_NAMES[script] ?? script;
}

/**
 * The `mixed-script-identifier` warning message for `text`, or undefined
 * when the identifier is ASCII-only, single-script, or covered through
 * Moderately Restrictive. Identity never changes: this only describes.
 */
export function mixedScriptWarning(text: string): string | undefined {
  let ascii = true;
  for (let index = 0; index < text.length; index += 1) {
    if (text.charCodeAt(index)! > 0x7f) {
      ascii = false;
      break;
    }
  }
  if (ascii) return undefined;
  // The set of script sets (SOSS): one distinct augmented set per
  // character, ignoring ALL. Its intersection is the resolved script set.
  const sets: Set<string>[] = [];
  for (const character of text) {
    const augmented = augmentedScripts(character.codePointAt(0)!);
    if (augmented !== null && !sets.some((known) => sameScripts(known, augmented)))
      sets.push(new Set(augmented));
  }
  const resolved = sets.reduce<Set<string> | undefined>(
    (found, set) => (found === undefined ? set : new Set([...found].filter((s) => set.has(s)))),
    undefined,
  );
  if (sets.length === 0 || (resolved !== undefined && resolved.size > 0)) return undefined;
  // Every entry still holds Latin, so the resolved set above held Latin:
  // unreachable, but a single script passes by definition.
  const rest = sets.filter((set) => !set.has("Latn"));
  if (rest.length === 0) return undefined;
  for (const cover of ["Kore", "Hanb", "Jpan"])
    if (rest.every((set) => set.has(cover))) return undefined;
  const remainder = rest.reduce<Set<string>>(
    (found, set) => new Set([...found].filter((s) => set.has(s))),
    new Set(rest[0]),
  );
  for (const script of remainder)
    if (script !== "Cyrl" && script !== "Grek" && RECOMMENDED_SCRIPTS.has(script)) return undefined;
  const mixed = [...new Set([...text].map((c) => scriptOf(c.codePointAt(0)!)))]
    .filter((script) => script !== "Zyyy" && script !== "Zinh")
    .map(scriptDisplayName)
    .sort();
  return `identifier '${text}' mixes ${mixed.join(" and ")} scripts; use one script`;
}
