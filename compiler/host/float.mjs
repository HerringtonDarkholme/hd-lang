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
