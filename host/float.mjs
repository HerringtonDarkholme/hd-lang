// Float text for the host primitives `format_f64`, `format_f32` and
// `format_f64_fixed` (spec/std/README.md#standard-library-primitives).
// The text is fixed by the spec and never taken from the host's own
// number printing:
//
// - `shortest` writes the Display text of
//   spec/lang/04-type-system.md#numeric-display: the shortest decimal
//   digits that read back as the same value at the value's own width (so
//   an `f32` gets its `f32` digits), the closest of those, ties to even;
//   fixed notation for a normalized decimal exponent in [-6, 21) and
//   lowercase scientific notation otherwise; `NaN`, `inf`, `-inf` and
//   `-0.0`.
// - `fixed` writes `to_fixed` (spec/std/num.md#fixed-point-text): the
//   multiple of 10 to the power -digits nearest the exact binary value,
//   ties to even, never an exponent, the sign bit kept.
//
// Both work on the exact significand and exponent with BigInt, so the
// result owes nothing to `Number.prototype.toString` or `toFixed`.

// The two widths: the significand's stored bits and the exponent bias.
const WIDTH = {
  32: { mantissa: 23, bias: 127 },
  64: { mantissa: 52, bias: 1023 },
};

// The sign bit, and the value `f * 2**e` of a finite `value` with the
// significand `f` (an integer) and the smallest exponent `min` of its
// width, which the subnormals share.
function split(value, width) {
  const { mantissa, bias } = WIDTH[width];
  const view = new DataView(new ArrayBuffer(8));
  let bits;
  let expBits;
  if (width === 32) {
    view.setFloat32(0, value);
    bits = BigInt(view.getUint32(0));
    expBits = 8;
  } else {
    view.setFloat64(0, value);
    bits = view.getBigUint64(0);
    expBits = 11;
  }
  const fraction = bits & ((1n << BigInt(mantissa)) - 1n);
  const biased = Number((bits >> BigInt(mantissa)) & ((1n << BigInt(expBits)) - 1n));
  const negative = (bits >> BigInt(mantissa + expBits)) !== 0n;
  const min = 1 - bias - mantissa;
  if (biased === 0) return { negative, f: fraction, e: min, min, hidden: 1n << BigInt(mantissa) };
  return {
    negative,
    f: fraction | (1n << BigInt(mantissa)),
    e: biased - bias - mantissa,
    min,
    hidden: 1n << BigInt(mantissa),
  };
}

// The shortest digits that read back as `f * 2**e`, and the exponent `k`
// with the value near 0.DIGITS * 10**k (Steele and White, in the form of
// Burger and Dybvig's free-format printing). The interval of values that
// read back as this one reaches halfway to each neighbor, and includes
// its ends exactly when the significand is even (round-to-nearest-even
// reading).
function digits({ f, e, min, hidden }) {
  // The value is r / s; `up` and `down` are the distances to the halfway
  // points above and below, in the same unit.
  let r;
  let s;
  let up;
  let down;
  const narrow = f === hidden && e !== min; // the gap below is half the gap above
  if (e >= 0) {
    const scale = 1n << BigInt(e);
    if (!narrow) {
      [r, s, up, down] = [f * scale * 2n, 2n, scale, scale];
    } else {
      [r, s, up, down] = [f * scale * 4n, 4n, scale * 2n, scale];
    }
  } else if (!narrow) {
    [r, s, up, down] = [f * 2n, (1n << BigInt(-e)) * 2n, 1n, 1n];
  } else {
    [r, s, up, down] = [f * 4n, (1n << BigInt(-e + 1)) * 2n, 2n, 1n];
  }
  const closed = (f & 1n) === 0n;
  // `k` is the least exponent with the high end of the interval below
  // 10**k (at most 10**k when the ends are inside).
  const reaches = () => (closed ? r + up >= s : r + up > s);
  const fits = () => (closed ? (r + up) * 10n < s : (r + up) * 10n <= s);
  let k = Math.ceil(Math.log10(Number(f) * 2 ** e) - 1e-9);
  if (k >= 0) s *= 10n ** BigInt(k);
  else {
    const scale = 10n ** BigInt(-k);
    r *= scale;
    up *= scale;
    down *= scale;
  }
  while (reaches()) {
    s *= 10n;
    k += 1;
  }
  while (fits()) {
    r *= 10n;
    up *= 10n;
    down *= 10n;
    k -= 1;
  }
  let out = "";
  for (;;) {
    r *= 10n;
    up *= 10n;
    down *= 10n;
    let d = r / s;
    r %= s;
    const low = closed ? r <= down : r < down;
    const high = closed ? r + up >= s : r + up > s;
    if (!low && !high) {
      out += d;
      continue;
    }
    if (low && high) {
      const twice = r * 2n;
      if (twice > s || (twice === s && (d & 1n) === 1n)) d += 1n;
    } else if (high) d += 1n;
    out += d;
    return { digits: out, k };
  }
}

// The Display text of a float of the given width (32 or 64).
export function shortest(value, width) {
  if (value !== value) return "NaN";
  if (value === Infinity) return "inf";
  if (value === -Infinity) return "-inf";
  const parts = split(value, width);
  const sign = parts.negative ? "-" : "";
  if (parts.f === 0n) return `${sign}0.0`;
  const { digits: ds, k } = digits(parts);
  const exponent = k - 1;
  if (exponent >= -6 && exponent < 21) {
    if (exponent < 0) return `${sign}0.${"0".repeat(-exponent - 1)}${ds}`;
    const whole = ds.length > exponent + 1 ? ds.slice(0, exponent + 1) : ds.padEnd(exponent + 1, "0");
    const rest = ds.slice(exponent + 1);
    return `${sign}${whole}.${rest === "" ? "0" : rest}`;
  }
  const mantissa = ds.length > 1 ? `${ds[0]}.${ds.slice(1)}` : ds;
  return `${sign}${mantissa}e${exponent < 0 ? "-" : "+"}${Math.abs(exponent)}`;
}

// The `to_fixed` text of an `f64` with `places` digits after the point.
export function fixed(value, places) {
  if (value !== value) return "NaN";
  if (value === Infinity) return "inf";
  if (value === -Infinity) return "-inf";
  const { negative, f, e } = split(value, 64);
  const n = BigInt(places);
  let scaled;
  if (e >= 0) scaled = f * (1n << BigInt(e)) * 10n ** n;
  else {
    const numerator = f * 10n ** n;
    const denominator = 1n << BigInt(-e);
    scaled = numerator / denominator;
    const twice = (numerator % denominator) * 2n;
    if (twice > denominator || (twice === denominator && (scaled & 1n) === 1n)) scaled += 1n;
  }
  const text = scaled.toString().padStart(places + 1, "0");
  const whole = text.slice(0, text.length - places);
  return `${negative ? "-" : ""}${whole}${places > 0 ? "." + text.slice(text.length - places) : ""}`;
}

// The unsigned decimal number of spec/std/num.md#r-std-num.parse-f64.decimal:
// digits, then optionally `.` and more digits (either side may be empty,
// but not both), then an optional exponent.
const DECIMAL = /^(?:([0-9]+)(?:\.([0-9]*))?|\.([0-9]+))(?:[eE]([+-]?[0-9]+))?$/;

function bitLength(n) {
  return n === 0n ? 0 : n.toString(2).length;
}

// The `f64` nearest the unsigned decimal number `text`, a tie going to the
// even significand: positive infinity past the range and `0.0` below it
// (the `parse_f64` primitive, spec/std/README.md#standard-library-
// primitives). The value is the exact rational `digits * 10^exponent`, so
// the result owes nothing to `Number` or `parseFloat`, which may round
// after the twentieth digit. Text outside the grammar is a bug in std.
export function parse(text) {
  const m = DECIMAL.exec(text);
  if (m === null) throw new Error(`parse_f64: text outside the grammar: ${text}`);
  const whole = m[1] ?? "";
  const fraction = m[2] ?? m[3] ?? "";
  const digits = (whole + fraction).replace(/^0+/, "");
  if (digits === "") return 0;
  const exponent = BigInt(m[4] ?? "0") - BigInt(fraction.length);
  // The value is in [10^(n-1+exponent), 10^(n+exponent)).
  const magnitude = BigInt(digits.length) + exponent;
  if (magnitude > 310n) return Infinity;
  if (magnitude < -330n) return 0;
  const d = BigInt(digits);
  const e = Number(exponent);
  let num = d;
  let den = 1n;
  if (e >= 0) num = d * 10n ** BigInt(e);
  else den = 10n ** BigInt(-e);
  // Scale so the quotient has 54 to 56 bits, then round it to the width
  // of the result.
  const shift = 54 - (bitLength(num) - bitLength(den));
  if (shift >= 0) num <<= BigInt(shift);
  else den <<= BigInt(-shift);
  const q = num / den;
  const rest = num % den;
  const top = bitLength(q) - 1 - shift;
  // The exponent of the last place: 52 below the top bit, or -1074 for a
  // subnormal.
  const unit = Math.max(top - 52, -1074);
  const drop = BigInt(unit + shift);
  let significand = q >> drop;
  const low = q & ((1n << drop) - 1n);
  const half = 1n << (drop - 1n);
  if (low > half || (low === half && (rest > 0n || (significand & 1n) === 1n))) significand += 1n;
  const x = Number(significand);
  return unit >= -1000 ? x * 2 ** unit : x * 2 ** -1000 * 2 ** (unit + 1000);
}

// Unicode Default Case Conversion with full mappings
// (spec/std/text.md#r-std-text.string.lower): `toLowerCase` and
// `toUpperCase` are that conversion with no locale, `ß` giving `SS`, and
// the final sigma by its context.
export function lower(text) {
  return text.toLowerCase();
}

export function upper(text) {
  return text.toUpperCase();
}
