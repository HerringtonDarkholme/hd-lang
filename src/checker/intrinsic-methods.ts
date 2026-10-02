// The operation intrinsics of `lib/std` (spec/std/README.md#standard-library-primitives):
// bodiless `@intrinsic` trait methods, mostly in numeric-family
// implementations such as `impl[N < Num] Add for N`
// (spec/lang/09-traits.md#r-trait.target.numeric-family). Before the loader
// parses a std module, this pass rewrites its text:
//
// - a numeric-family implementation becomes one implementation per type
//   that its parameters' bounds list, as `impl Add for i8:` ... `impl Add
//   for f64:`, read from `lib/std/num.hd`'s `impl Num for T` lines;
// - an `@intrinsic` method gets the body the compiler supplies: the
//   operator on primitive operands, which the prototype compiles inline
//   (spec/lang/09-traits.md#r-trait.impl.intrinsic.inline), so no body
//   calls itself.

/** The sealed numeric traits a family implementation's parameter is bounded by. */
const FAMILY_TRAITS = new Set(["Num", "Integer", "Float"]);

/**
 * The body of each operation intrinsic, by method name, over the receiver
 * `self` and the argument `R`. `partial_cmp` makes a NaN operand unordered.
 */
const INTRINSIC_BODIES: Readonly<Record<string, readonly string[]>> = {
  add: ["self + R"],
  sub: ["self - R"],
  mul: ["self * R"],
  div: ["self / R"],
  rem: ["self % R"],
  neg: ["-self"],
  bit_and: ["self & R"],
  bit_or: ["self | R"],
  bit_xor: ["self ^ R"],
  not: ["~self"],
  shl: ["self << R"],
  shr: ["self >> R"],
  eq: ["self == R"],
  partial_cmp: [
    "if self < R:",
    "    return .Some(.Less)",
    "if self > R:",
    "    return .Some(.Greater)",
    "if self == R:",
    "    return .Some(.Equal)",
    ".None",
  ],
  cmp: ["if self < R:", "    return .Less", "if self > R:", "    return .Greater", ".Equal"],
};

/** The types each sealed numeric trait is implemented for, from `std.num`'s `impl Trait for T` lines. */
function familyTypes(numSource: string): ReadonlyMap<string, readonly string[]> {
  const families = new Map<string, string[]>();
  for (const [, trait, type] of numSource.matchAll(/^impl (\w+) for (\w+)\b/gm))
    if (FAMILY_TRAITS.has(trait!)) families.set(trait!, [...(families.get(trait!) ?? []), type!]);
  return families;
}

function wordPattern(names: readonly string[]): RegExp {
  return new RegExp(`\\b(${names.join("|")})\\b`, "g");
}

/** Every assignment of a listed type to each parameter, in order. */
function assignments(
  parameters: readonly (readonly [string, readonly string[]])[],
): Map<string, string>[] {
  let result: Map<string, string>[] = [new Map()];
  for (const [name, types] of parameters)
    result = result.flatMap((partial) => types.map((type) => new Map([...partial, [name, type]])));
  return result;
}

/** One implementation per type for each `impl[N < Num, ...] Trait for N:` block. */
function expandFamilies(source: string, families: ReadonlyMap<string, readonly string[]>): string {
  const lines = source.split("\n");
  const output: string[] = [];
  for (let index = 0; index < lines.length; index++) {
    const header = /^impl\[([^\]]+)\] (.+) for (\w+):$/.exec(lines[index]!);
    const parameters = header?.[1]!.split(",").map((item) => {
      const [name, bound] = item.trim().split(/\s*<\s*/);
      return [name!, families.get(bound ?? "")] as const;
    });
    if (
      !header ||
      !parameters ||
      parameters.some(([, types]) => types === undefined) ||
      !parameters.some(([name]) => name === header[3])
    ) {
      output.push(lines[index]!);
      continue;
    }
    const body: string[] = [];
    while (index + 1 < lines.length && /^\s+\S/.test(lines[index + 1]!)) body.push(lines[++index]!);
    const bound = parameters as readonly (readonly [string, readonly string[]])[];
    const pattern = wordPattern(bound.map(([name]) => name));
    for (const assignment of assignments(bound)) {
      const substitute = (text: string): string =>
        text.replace(pattern, (name) => assignment.get(name) ?? name);
      if (output.length > 0 && output.at(-1) !== "") output.push("");
      output.push(`impl ${substitute(`${header[2]} for ${header[3]}`)}:`, ...body.map(substitute));
    }
  }
  return output.join("\n");
}

/** Gives each bodiless `@intrinsic` method the body the compiler supplies. */
function supplyBodies(source: string): string {
  return source.replace(
    /^( +)@intrinsic\n( +fn (\w+)\(self(?:, (\w+): [^)]*)?\)[^\n:]*)$/gm,
    (_whole, indent: string, signature: string, method: string, argument?: string) => {
      const body = INTRINSIC_BODIES[method];
      if (!body) throw new Error(`no intrinsic method '${method}' (spec/std/README.md)`);
      const lines = body.map(
        (line) => `${indent}    ${line.replaceAll(/\bR\b/g, argument ?? "R")}`,
      );
      return `${signature}:\n${lines.join("\n")}`;
    },
  );
}

/** A std module's source with its operation intrinsics expanded for the loader. */
export function withIntrinsicMethods(source: string, numSource: () => string): string {
  if (!source.includes("@intrinsic\n")) return source;
  const supplied = supplyBodies(expandFamilies(source, familyTypes(numSource())));
  if (supplied.includes("@intrinsic\n"))
    throw new Error("an `@intrinsic` line that is not before a bodiless method with a receiver");
  return supplied;
}
