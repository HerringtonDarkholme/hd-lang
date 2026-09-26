// 5.3: value survival through erased positions, generic vs concrete equality,
// and identity of boxed primitives in trait values.
//   node --experimental-strip-types audit/scripts/arch/erasure-values.ts
import { writeFileSync } from "node:fs";
import { callExport, instantiatePlain, readProbe, ROOT } from "./alloc-lib.ts";

const lines: string[] = [
  `# commit bd985d7; command: node --experimental-strip-types audit/scripts/arch/erasure-values.ts; date: ${new Date().toISOString()}`,
];
const log = (line: string): void => {
  lines.push(line);
  console.log(line);
};
const bits = (value: number): string => {
  const view = new DataView(new ArrayBuffer(8));
  view.setFloat64(0, value);
  return view.getBigUint64(0).toString(16).padStart(16, "0");
};
let failures = 0;
const check = (
  name: string,
  actual: number,
  expected: number,
  render = String,
): void => {
  const ok = Object.is(actual, expected);
  if (!ok) failures += 1;
  log(
    `${ok ? "ok  " : "FAIL"}\t${name}\tactual=${render(actual)}\texpected=${render(expected)}`,
  );
};

const values = await instantiatePlain(
  readProbe("audit/probes/arch/erasure/erasure-values.hd"),
);
const f64 = (value: number): string => `${value} bits=${bits(value)}`;
log("\n## f64 / i32 survival through erased positions");
const nanBits = bits(callExport(values, "nan"));
for (const via of ["identity", "box", "list", "tuple", "optional", "closure"]) {
  const result = callExport(values, `nan_${via}`);
  const ok = Number.isNaN(result) && bits(result) === nanBits;
  if (!ok) failures += 1;
  log(
    `${ok ? "ok  " : "FAIL"}\tnan_${via}\tactual=${f64(result)}\texpected=NaN bits=${nanBits}`,
  );
  check(`zero_${via}`, callExport(values, `zero_${via}`), -0, f64);
}
check("max_box", callExport(values, "max_box"), Number.MAX_VALUE, f64);
check("tiny_list", callExport(values, "tiny_list"), Number.MIN_VALUE, f64);
check("min_tuple", callExport(values, "min_tuple"), -2147483648);
check("max_optional", callExport(values, "max_optional"), 2147483647);

log(
  "\n## equality (1 = true). IEEE: NaN != NaN, -0 == 0; spec 05: map equality ignores order",
);
const eq: [string, number][] = [
  ["concrete_nan_eq", 0],
  ["generic_nan_eq", 0],
  ["concrete_zero_eq", 1],
  ["generic_zero_eq", 1],
  ["list_nan_eq", 0],
  ["list_zero_eq", 1],
  ["tuple_nan_eq", 0],
  ["optional_nan_eq", 0],
  ["boxed_same_value_eq", 1],
  ["map_value_nan_eq", 0],
  ["map_order_eq", 1],
];
for (const [name, expected] of eq)
  check(name, callExport(values, name), expected);

const identity = await instantiatePlain(
  readProbe("audit/probes/arch/erasure/erasure-identity.hd"),
);
log("\n## identity of trait values (spec 05 `is` rules; 1 = same identity)");
const ids: [string, number][] = [
  ["prim_alias", 1],
  ["prim_pass", 1],
  ["prim_twice", 0],
  ["data_twice", 1],
  ["generic_alias", 1],
  ["generic_twice", 0],
];
for (const [name, expected] of ids)
  check(name, callExport(identity, name), expected);
log(`\nfailures: ${failures}`);
writeFileSync(
  `${ROOT}audit/evidence/05-object-model/erasure-values.txt`,
  lines.join("\n") + "\n",
);
