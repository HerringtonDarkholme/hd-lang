#!/usr/bin/env python3
"""Generate the musl libm test vectors for std.num's float methods (#2407).

Usage: python3 scripts/generate-libm-vectors.py MUSL_TARBALL [--check]
Input: musl-1.2.5.tar.gz from https://musl.libc.org/releases/ (MIT), checked
against its SHA-256 below. The script compiles musl's own src/math C files
with the host C compiler, with contraction off and no fast FMA, as a Wasm
build of musl compiles them, then runs each function on fixed inputs.
Output: crates/hd_driver/tests/cases/std-num/libm-musl-vectors.hd, a stage
case whose tests compare every result by its bits (a NaN result as NaN).
Cost: one compile of about 70 C files and one run over a few thousand
inputs. The inputs come from fixed lists and a seeded generator, so the
output is deterministic.
"""
import argparse
import hashlib
import math
import random
import struct
import subprocess
import tarfile
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "crates/hd_driver/tests/cases/std-num/libm-musl-vectors.hd"
MUSL = "musl-1.2.5"
MUSL_SHA256 = "a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4"

# musl's sources for each function, with the helpers and tables they call.
SOURCES = """
exp exp_data log log_data log2 log2_data log10 pow pow_data exp2 expm1 log1p
sin cos tan __sin __cos __tan __rem_pio2 __rem_pio2_large asin acos atan atan2
sinh cosh tanh __expo2 asinh acosh atanh cbrt hypot fma scalbn
expf exp2f exp2f_data logf logf_data log2f log2f_data log10f powf powf_data
expm1f log1pf sinf cosf tanf __sindf __cosdf __tandf __rem_pio2f asinf acosf
atanf atan2f sinhf coshf tanhf __expo2f asinhf acoshf atanhf cbrtf hypotf fmaf
scalbnf __math_oflow __math_uflow __math_xflow __math_invalid __math_divzero
__math_oflowf __math_uflowf __math_xflowf __math_invalidf __math_divzerof
""".split()

# hd method name, C function, argument count.
FUNCTIONS = [
    ("exp", "exp", 1), ("exp2", "exp2", 1), ("exp_m1", "expm1", 1),
    ("ln", "log", 1), ("log2", "log2", 1), ("log10", "log10", 1),
    ("ln_1p", "log1p", 1), ("sin", "sin", 1), ("cos", "cos", 1),
    ("tan", "tan", 1), ("asin", "asin", 1), ("acos", "acos", 1),
    ("atan", "atan", 1), ("sinh", "sinh", 1), ("cosh", "cosh", 1),
    ("tanh", "tanh", 1), ("asinh", "asinh", 1), ("acosh", "acosh", 1),
    ("atanh", "atanh", 1), ("cbrt", "cbrt", 1), ("powf", "pow", 2),
    ("atan2", "atan2", 2), ("hypot", "hypot", 2), ("mul_add", "fma", 3),
]

HARNESS = r"""
#include <stdint.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
#define D1(n) if (!strcmp(name, #n)) { double r = n(da); memcpy(&o, &r, 8); }
#define D2(n) if (!strcmp(name, #n)) { double r = n(da, db); memcpy(&o, &r, 8); }
#define F1(n) if (!strcmp(name, #n)) { float r = n(fa); uint32_t w; memcpy(&w, &r, 4); o = w; }
#define F2(n) if (!strcmp(name, #n)) { float r = n(fa, fb); uint32_t w; memcpy(&w, &r, 4); o = w; }
int main(void) {
    char name[32];
    unsigned long long a, b, c;
    int count;
    while (scanf("%31s %d %llx %llx %llx", name, &count, &a, &b, &c) == 5) {
        double da, db, dc; float fa, fb, fc;
        uint32_t wa = (uint32_t)a, wb = (uint32_t)b, wc = (uint32_t)c;
        memcpy(&da, &a, 8); memcpy(&db, &b, 8); memcpy(&dc, &c, 8);
        memcpy(&fa, &wa, 4); memcpy(&fb, &wb, 4); memcpy(&fc, &wc, 4);
        uint64_t o = 0;
        D1(exp) D1(exp2) D1(expm1) D1(log) D1(log2) D1(log10) D1(log1p)
        D1(sin) D1(cos) D1(tan) D1(asin) D1(acos) D1(atan) D1(sinh) D1(cosh)
        D1(tanh) D1(asinh) D1(acosh) D1(atanh) D1(cbrt) D2(pow) D2(atan2)
        D2(hypot)
        F1(expf) F1(exp2f) F1(expm1f) F1(logf) F1(log2f) F1(log10f) F1(log1pf)
        F1(sinf) F1(cosf) F1(tanf) F1(asinf) F1(acosf) F1(atanf) F1(sinhf)
        F1(coshf) F1(tanhf) F1(asinhf) F1(acoshf) F1(atanhf) F1(cbrtf)
        F2(powf) F2(atan2f) F2(hypotf)
        if (!strcmp(name, "fma")) { double r = fma(da, db, dc); memcpy(&o, &r, 8); }
        if (!strcmp(name, "fmaf")) { float r = fmaf(fa, fb, fc); uint32_t w; memcpy(&w, &r, 4); o = w; }
        printf("%llx\n", (unsigned long long)o);
    }
    return 0;
}
"""

SHIMS = {
    "features.h": "#define hidden\n",
    "endian.h": "#define __LITTLE_ENDIAN 1234\n#define __BIG_ENDIAN 4321\n"
                "#define __BYTE_ORDER __LITTLE_ENDIAN\n",
    "fp_arch.h": "",
    "atomic.h": "static inline int a_clz_64(unsigned long long x) "
                "{ return __builtin_clzll(x); }\n",
}


def build(tarball, work):
    digest = hashlib.sha256(tarball.read_bytes()).hexdigest()
    if digest != MUSL_SHA256:
        raise SystemExit(f"{tarball}: SHA-256 {digest}, expected {MUSL_SHA256}")
    with tarfile.open(tarball) as archive:
        members = [m for m in archive.getmembers()
                   if m.name.startswith(f"{MUSL}/src/math/") and "/" not in m.name[len(f"{MUSL}/src/math/"):]
                   or m.name == f"{MUSL}/src/internal/libm.h"]
        archive.extractall(work, members=members, filter="data")
    shim = work / "shim"
    shim.mkdir()
    for name, text in SHIMS.items():
        (shim / name).write_text(text)
    # musl's own math.h brings `hidden` in through features.h; the host's does not.
    libm = (work / MUSL / "src/internal/libm.h").read_text()
    (shim / "libm.h").write_text('#include "features.h"\n' + libm)
    (work / "harness.c").write_text(HARNESS)
    math_dir = work / MUSL / "src/math"
    files = [str(math_dir / f"{name}.c") for name in SOURCES] + [str(work / "harness.c")]
    # No contraction and no fast FMA: the operations musl writes, each
    # rounded once, as on Wasm. No builtins, so every call reaches musl.
    flags = ["-std=c99", "-O1", "-ffp-contract=off", "-fno-builtin",
             "-U__FP_FAST_FMA", "-U__FP_FAST_FMAF", "-w",
             "-I", str(shim), "-I", str(math_dir)]
    binary = work / "harness"
    subprocess.run(["cc", *flags, *files, "-o", str(binary), "-lm"], check=True, timeout=300)
    # Every function must be musl's, not the host's libm.
    symbols = subprocess.run(["nm", str(binary)], capture_output=True, text=True, check=True,
                             timeout=60).stdout.split()
    for _, c_name, _ in FUNCTIONS:
        for name in (c_name, c_name + "f"):
            if f"_{name}" not in symbols and name not in symbols:
                raise SystemExit(f"{name} did not link from musl's source")
    (work / "dump.c").write_text(DUMP)
    data = [str(math_dir / f"{name}.c") for name in SOURCES if name.endswith("_data")]
    dumper = work / "dump"
    subprocess.run(["cc", *flags, *data, str(work / "dump.c"), "-o", str(dumper)], check=True,
                   timeout=300)
    return binary, dumper


# Prints every field of musl's shared math tables as `name hexbits`.
DUMP = r"""
#include <stdio.h>
#include <string.h>
#include "libm.h"
#include "exp_data.h"
#include "log_data.h"
#include "log2_data.h"
#include "pow_data.h"
#include "exp2f_data.h"
#include "logf_data.h"
#include "log2f_data.h"
#include "powf_data.h"
static void d(const char *n, double v) { unsigned long long b; memcpy(&b, &v, 8); printf("%s %llx\n", n, b); }
static void w(const char *n, unsigned long long v) { printf("%s %llx\n", n, v); }
#define EACH(n, k, e) for (int i = 0; i < (k); i++) e
int main(void) {
    d("exp.invln2N", __exp_data.invln2N); d("exp.shift", __exp_data.shift);
    d("exp.negln2hiN", __exp_data.negln2hiN); d("exp.negln2loN", __exp_data.negln2loN);
    EACH(p, 4, d("exp.poly", __exp_data.poly[i]));
    d("exp.exp2_shift", __exp_data.exp2_shift);
    EACH(p, 5, d("exp.exp2_poly", __exp_data.exp2_poly[i]));
    EACH(t, 256, w("exp.tab", __exp_data.tab[i]));
    d("log.ln2hi", __log_data.ln2hi); d("log.ln2lo", __log_data.ln2lo);
    EACH(p, 5, d("log.poly", __log_data.poly[i]));
    EACH(p, 11, d("log.poly1", __log_data.poly1[i]));
    EACH(t, 128, (d("log.tab", __log_data.tab[i].invc), d("log.tab", __log_data.tab[i].logc),
                  d("log.tab", __log_data.tab2[i].chi), d("log.tab", __log_data.tab2[i].clo)));
    d("log2.invln2hi", __log2_data.invln2hi); d("log2.invln2lo", __log2_data.invln2lo);
    EACH(p, 6, d("log2.poly", __log2_data.poly[i]));
    EACH(p, 10, d("log2.poly1", __log2_data.poly1[i]));
    EACH(t, 64, (d("log2.tab", __log2_data.tab[i].invc), d("log2.tab", __log2_data.tab[i].logc),
                 d("log2.tab", __log2_data.tab2[i].chi), d("log2.tab", __log2_data.tab2[i].clo)));
    d("pow.ln2hi", __pow_log_data.ln2hi); d("pow.ln2lo", __pow_log_data.ln2lo);
    EACH(p, 7, d("pow.poly", __pow_log_data.poly[i]));
    EACH(t, 128, (d("pow.tab", __pow_log_data.tab[i].invc), d("pow.tab", __pow_log_data.tab[i].logc),
                  d("pow.tab", __pow_log_data.tab[i].logctail)));
    EACH(t, 32, w("exp2f.tab", __exp2f_data.tab[i]));
    d("exp2f.shift_scaled", __exp2f_data.shift_scaled);
    EACH(p, 3, d("exp2f.poly", __exp2f_data.poly[i]));
    d("exp2f.shift", __exp2f_data.shift); d("exp2f.invln2_scaled", __exp2f_data.invln2_scaled);
    EACH(p, 3, d("exp2f.poly_scaled", __exp2f_data.poly_scaled[i]));
    EACH(t, 16, (d("logf.tab", __logf_data.tab[i].invc), d("logf.tab", __logf_data.tab[i].logc)));
    d("logf.ln2", __logf_data.ln2);
    EACH(p, 3, d("logf.poly", __logf_data.poly[i]));
    EACH(t, 16, (d("log2f.tab", __log2f_data.tab[i].invc), d("log2f.tab", __log2f_data.tab[i].logc)));
    EACH(p, 4, d("log2f.poly", __log2f_data.poly[i]));
    EACH(t, 16, (d("powf.tab", __powf_log2_data.tab[i].invc), d("powf.tab", __powf_log2_data.tab[i].logc)));
    EACH(p, 5, d("powf.poly", __powf_log2_data.poly[i]));
    return 0;
}
"""


def packed_word(value):
    # Eleven characters, six bits each from the least significant, as `?`
    # (63) plus the bits: lib/std/num.hd's `libm_word` reads them back.
    text = "".join(chr(63 + ((value >> (6 * j)) & 63)) for j in range(11))
    return text.replace("\\", "\\\\")


# Every shared table is packed static text, reached only by its users.
STRING_TABLES = ["exp.tab", "pow.tab", "log.tab", "log2.tab", "exp2f.tab", "logf.tab", "log2f.tab"]


def musl_tables(dumper):
    """musl's scalars as text, and each table as num.hd writes it."""
    fields = {}
    out = subprocess.run([str(dumper)], capture_output=True, text=True, check=True, timeout=60).stdout
    for line in out.splitlines():
        name, bits_text = line.split()
        fields.setdefault(name, []).append(int(bits_text, 16))
    if fields["log2f.tab"] != fields["powf.tab"]:
        raise SystemExit("log2f's and powf's tables differ; num.hd shares one")
    lines = []
    for name, values in fields.items():
        if not name.endswith(".tab"):
            lines.append(f"{name}: " + ", ".join(repr(from_f64(v)) for v in values))
            continue
        if name in STRING_TABLES:
            lines.append(f"{name} ({len(values)} words): \"{''.join(packed_word(v) for v in values)}\"")
    return lines


def f64_bits(value):
    return struct.unpack("<Q", struct.pack("<d", value))[0]


def f32_bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


def from_f64(bits):
    return struct.unpack("<d", struct.pack("<Q", bits))[0]


def next_up(value, steps=1):
    return math.nextafter(value, math.inf, steps=steps)


def next_down(value, steps=1):
    return math.nextafter(value, -math.inf, steps=steps)


SPECIAL_F64 = [0x0, 0x8000000000000000, 0x7ff0000000000000, 0xfff0000000000000,
               0x7ff8000000000000, 0x1, 0x8000000000000001, 0x000fffffffffffff,
               0x0010000000000000, 0x7fefffffffffffff, 0xffefffffffffffff]
SPECIAL_F32 = [0x0, 0x80000000, 0x7f800000, 0xff800000, 0x7fc00000, 0x1, 0x80000001,
               0x007fffff, 0x00800000, 0x7f7fffff, 0xff7fffff]
COMMON = [1.0, -1.0, next_down(1.0), next_up(1.0), next_down(1.0, 3), next_up(1.0, 3), 0.5,
          -0.5, 2.0, -2.0, 3.0, 10.0, 0.1, -0.1, 0.75, 1.5, -1.5, 100.0, -100.0, 1e-10, 1e10,
          math.pi, math.pi / 2, math.pi / 4, math.e, 2.0 ** -30, 2.0 ** -60, 1e-300, 1e300,
          -1e300, 1e22, 2.0 ** 1000, 1e-40, 1e30, 3.0e38, -3.0e38, 1e-45]

# Domain points per function: edges of each function's argument reduction,
# its overflow and underflow thresholds, and values near its poles.
EDGES = {
    "exp": [709.782712893384, next_up(709.782712893384), 709.79, -745.1332191019411,
            -745.13321910194122, -745.2, -708.39, -708.4, -720.0, -740.0, 512.0, 1024.0, 50.5,
            -50.5, 2.0 ** -54, -(2.0 ** -54), 88.72283, 88.7228394, -103.97, -87.33655, 1e-20],
    "exp2": [1023.99, 1024.0, -1022.0, -1074.0, -1075.0, -1074.5, -1060.3, -1022.5, 127.99,
             128.0, -126.0, -149.0, -150.0, -149.5, 0.5, 2.0 ** -60, -0.75, 64.25],
    "exp_m1": [709.78, 710.0, 2.0 ** -54, 1e-5, -1e-5, 0.34657359027997264, 0.35, -0.35,
               1.0397207708399179, -38.0, -40.0, 56.0, 57.0, 88.7, 89.0, -17.0, -18.0, 1e-20],
    "ln": [next_up(1.0, 1000), next_down(1.0, 1000), 0.9375, 1.0625, 1.03125, 0.96875,
           1.000001, 0.999999, 1 + 2.0 ** -20, 1 - 2.0 ** -20, -1.0, -1e-300],
    "ln_1p": [-1.0, -2.0, next_up(-1.0), -0.999999, -0.5, 0.41421356237309503,
              -0.2928932188134524, 2.0 ** -29, 2.0 ** -54, -(2.0 ** -54), 1e-10, -1e-10, 2.0 ** 53,
              2.0 ** 24, 1e-5, -1e-5],
    "sin": [math.pi, 2 * math.pi, 3 * math.pi / 2, 1e6, 1e15, 1e22, 2.0 ** 1023, 1e300,
            1647099.3291652855, 5e8, 2.0 ** -27, 2.0 ** -26, 0.785398, 2.356194, 3.926990,
            5.497787, 7.0685834705770345, 9e5, 1e9, 12.0],
    "asin": [0.5, next_down(0.5), next_up(0.5), 0.975, 0.97500002, 0.9999, -0.9999, next_up(1.0),
             2.0 ** -26, 2.0 ** -27, 0.7, -0.7, 0.25],
    "atan": [0.4375, 0.6875, 1.1875, 2.4375, 2.0 ** 66, -(2.0 ** 66), 2.0 ** -27, 1e-9, 39.0,
             2.0 ** 26, 2.0 ** 27, 1e16],
    "sinh": [710.4758600739439, 710.476, 709.78, 711.0, -711.0, 22.0, 21.9, 0.6931471805599453,
             2.0 ** -26, 2.0 ** -27, 88.7, 89.5, 90.0, 9.0, -9.0, 0.3, 12.0],
    "asinh": [2.0 ** 26, 2.0 ** 27, 2.0 ** 28, 2.0 ** -26, 2.0 ** -27, 2.0, 2.5, 1e10, 1e-10,
              2.0 ** 12, 2.0 ** -12],
    "acosh": [1.0, next_up(1.0), 2.0, 2.5, 1e10, 2.0 ** 26, 2.0 ** 28, 2.0 ** 13, 1.5, 1.1, 0.5],
    "atanh": [0.5, 0.25, 0.99999, -0.99999, 1e-10, 2.0 ** -26, 2.0 ** -27, next_down(1.0), 0.9],
    "cbrt": [27.0, -8.0, 1e-310, -1e-310, 3e-320, 2.0 ** -1060, 64.0, 0.001, 1e-45, 7e-46],
}
EDGES["cos"] = EDGES["sin"]
EDGES["tan"] = EDGES["sin"]
EDGES["acos"] = EDGES["asin"]
EDGES["cosh"] = EDGES["sinh"]
EDGES["tanh"] = EDGES["sinh"] + [20.0, 0.55, 2.0 ** -55]
EDGES["log2"] = EDGES["ln"]
EDGES["log10"] = EDGES["ln"] + [1000.0, 1e-5, 1e23]

# Seeded random inputs per function: (low, high, count, log scale).
RANDOM = {
    "exp": [(-745.0, 710.0, 30, False), (-1.0, 1.0, 15, False)],
    "exp2": [(-1080.0, 1025.0, 30, False), (-1.0, 1.0, 15, False)],
    "exp_m1": [(-40.0, 710.0, 25, False), (-1.0, 1.0, 20, False)],
    "ln": [(1e-308, 1e308, 30, True), (0.5, 2.0, 20, False)],
    "ln_1p": [(1e-300, 1e300, 25, True), (-0.9, 1.0, 20, False)],
    "sin": [(-10.0, 10.0, 30, False), (10.0, 1e300, 15, True)],
    "asin": [(-1.0, 1.0, 35, False)],
    "atan": [(1e-300, 1e300, 30, True), (-3.0, 3.0, 15, False)],
    "sinh": [(-30.0, 30.0, 30, False), (-1.0, 1.0, 15, False)],
    "asinh": [(1e-300, 1e300, 30, True), (-5.0, 5.0, 15, False)],
    "acosh": [(1.0, 1e300, 30, True), (1.0, 3.0, 15, False)],
    "atanh": [(-1.0, 1.0, 40, False)],
    "cbrt": [(1e-308, 1e308, 35, True)],
}
for alias, base in [("cos", "sin"), ("tan", "sin"), ("acos", "asin"), ("cosh", "sinh"),
                    ("tanh", "sinh"), ("log2", "ln"), ("log10", "ln")]:
    RANDOM[alias] = RANDOM[base]


def sample(rng, low, high, count, log_scale, signed=False):
    out = []
    for _ in range(count):
        if log_scale:
            value = math.exp(rng.uniform(math.log(low), math.log(high)))
        else:
            value = rng.uniform(low, high)
        if signed and rng.random() < 0.5:
            value = -value
        out.append(value)
    return out


def unary_inputs(name, f32, rng):
    specials = SPECIAL_F32 if f32 else SPECIAL_F64
    values = COMMON + EDGES.get(name, [])
    for low, high, count, log_scale in RANDOM.get(name, []):
        values += sample(rng, low, high, count, log_scale, signed=log_scale and name in
                         ("atan", "asinh", "cbrt", "sin", "cos", "tan"))
    return dedup(specials + [bits(v, f32) for v in values])


def bits(value, f32):
    if f32:
        if abs(value) > 3.4028235677973366e38 and not math.isinf(value):
            value = math.copysign(math.inf, value)
        return f32_bits(value)
    return f64_bits(value)


def dedup(items):
    seen = set()
    out = []
    for item in items:
        if item not in seen:
            seen.add(item)
            out.append(item)
    return out


def binary_inputs(name, f32, rng):
    inf, nan = math.inf, math.nan
    if name == "powf":
        pairs = [(2.0, 0.5), (2.0, 10.0), (-2.0, 3.0), (-2.0, 3.5), (-1.0, 1e20), (-1.0, inf),
                 (0.0, -1.0), (-0.0, -3.0), (-0.0, 3.0), (-0.0, 2.0), (inf, -1.0), (-inf, 3.0),
                 (-inf, -3.0), (-inf, 2.0), (1.0, nan), (nan, 0.0), (nan, 1.0), (1.0000001, 1e10),
                 (0.5, 1075.0), (10.0, 308.25), (2.0, 1023.999), (2.0, -1074.0), (2.0, -1075.0),
                 (5e-324, 0.5), (1e-310, 2.0), (0.9999999, -1e12), (-8.0, 1.0 / 3.0), (2.0, 1024.0),
                 (2.0, -1022.5), (1.5, 2.0 ** 31), (0.5, -(2.0 ** 31)), (-1.0, -(2.0 ** 63)),
                 (-3.0, 2.0 ** 53), (-3.0, 2.0 ** 53 + 1), (10.0, -5.0), (3.0, 0.3333333333),
                 (1e300, 1e-300), (2.5, 127.5), (2.0, 128.0), (2.0, -149.0), (2.0, -150.0),
                 (0.0, 0.0), (inf, 0.0), (1.0, inf), (0.5, inf), (0.5, -inf), (2.0, -inf)]
        for _ in range(45):
            x = math.exp(rng.uniform(math.log(1e-10), math.log(1e10)))
            pairs.append((x, rng.uniform(-50.0, 50.0)))
        for _ in range(10):
            pairs.append((-rng.uniform(0.1, 10.0), float(rng.randint(-40, 40))))
    elif name == "atan2":
        pairs = [(0.0, 0.0), (-0.0, 0.0), (0.0, -0.0), (-0.0, -0.0), (1.0, 0.0), (-1.0, -0.0),
                 (0.0, -1.0), (-0.0, -1.0), (inf, inf), (inf, -inf), (-inf, inf), (-inf, -inf),
                 (1.0, inf), (1.0, -inf), (-1.0, -inf), (inf, 1.0), (nan, 1.0), (1.0, nan),
                 (1e300, 1e-300), (1e-300, 1e300), (1e-300, -1e300), (1.0, 1.0), (-1.0, -1.0),
                 (1.0, -1.0), (3.0, 4.0), (2.0 ** 70, 1.0), (1.0, 2.0 ** 70), (1.0, -(2.0 ** 70)),
                 (5e-324, 1.0), (1.0, 5e-324)]
        for _ in range(45):
            pairs.append((rng.uniform(-10.0, 10.0), rng.uniform(-10.0, 10.0)))
        for _ in range(15):
            pairs.append((math.exp(rng.uniform(-200.0, 200.0)) * rng.choice([1, -1]),
                          math.exp(rng.uniform(-200.0, 200.0)) * rng.choice([1, -1])))
    else:  # hypot
        pairs = [(3.0, 4.0), (1e300, 1e300), (1e-300, 1e-300), (5e-324, 5e-324), (inf, nan),
                 (nan, inf), (-inf, nan), (nan, 1.0), (1.0, nan), (0.0, 0.0), (-0.0, -0.0),
                 (1.0, 1e-20), (1e200, 1.0), (2.0 ** 500, 2.0 ** 500), (2.0 ** -500, 2.0 ** -540),
                 (1.0, 1.0), (-3.0, 4.0), (1.7e308, 1.7e308), (3e38, 3e38), (1e-40, 1e-40),
                 (1e19, 1e19), (1e-19, 1e-19)]
        for _ in range(40):
            pairs.append((rng.uniform(-100.0, 100.0), rng.uniform(-100.0, 100.0)))
        for _ in range(15):
            pairs.append((math.exp(rng.uniform(-700.0, 700.0)), math.exp(rng.uniform(-700.0, 700.0))))
    return dedup([(bits(a, f32), bits(b, f32)) for a, b in pairs])


def fma_inputs(f32, rng):
    inf, nan = math.inf, math.nan
    triples = [(1.0, 2.0, 3.0), (inf, 0.0, 1.0), (1.0, 1.0, inf), (inf, 1.0, -inf),
               (nan, 1.0, 1.0), (1.0, nan, 1.0), (1.0, 1.0, nan), (0.0, 0.0, -0.0),
               (-0.0, 0.0, 0.0), (-0.0, 0.0, -0.0), (1e300, 1e300, -inf), (1e300, 1e10, 1.0),
               (1e-300, 1e-300, 0.0), (1e-200, 1e-130, 5e-324), (2.0 ** -1022, 0.5, 0.0),
               (5e-324, 0.5, 0.0), (5e-324, 0.5, 5e-324), (1e-40, 1e-6, 0.0), (1e-30, 1e-15, 1e-45),
               (1.0, 1.0, -1.0)]
    if f32:
        e = 2.0 ** -12
        triples += [(1 + e, 1 - e, -1.0), (1 + 2.0 ** -23, 1 + 2.0 ** -23, -1.0),
                    (0.1, 10.0, -1.0), (3.0e38, 2.0, -3.0e38), (1.0 + 2.0 ** -23, 1.0, 2.0 ** -49),
                    (1.0, 1.0, 2.0 ** -24), (1.0, 1.0, 2.0 ** -24 + 2.0 ** -60),
                    (1.0, 1.0, -(2.0 ** -25)), (2.0 ** -75, 2.0 ** -75, 2.0 ** -149)]
    else:
        e = 2.0 ** -30
        triples += [(1 + e, 1 - e, -1.0), (1 + 2.0 ** -52, 1 + 2.0 ** -52, -1.0),
                    (0.1, 10.0, -1.0), (1.7e308, 2.0, -1.7e308), (1.0 + 2.0 ** -52, 1.0, 2.0 ** -106),
                    (1.0, 1.0, 2.0 ** -53), (1.0, 1.0, 2.0 ** -53 + 2.0 ** -100),
                    (1.0, 1.0, -(2.0 ** -54)), (2.0 ** -537, 2.0 ** -537, 5e-324)]
    for _ in range(40):
        a, b = rng.uniform(-10.0, 10.0), rng.uniform(-10.0, 10.0)
        product = a * b
        if f32:
            a, b = from_f32(f32_bits(a)), from_f32(f32_bits(b))
            product = from_f32(f32_bits(a * b))
        triples.append((a, b, -product if rng.random() < 0.5 else rng.uniform(-100.0, 100.0)))
    for _ in range(10):
        triples.append((math.exp(rng.uniform(-300, 300)), math.exp(rng.uniform(-300, 300)),
                        -math.exp(rng.uniform(-300, 300))))
    return dedup([(bits(a, f32), bits(b, f32), bits(c, f32)) for a, b, c in triples])


def from_f32(bits_value):
    return struct.unpack("<f", struct.pack("<I", bits_value))[0]


def run(binary, queries):
    text = "".join(f"{name} {len(args)} " + " ".join(f"{a:x}" for a in (list(args) + [0, 0, 0])[:3]) + "\n"
                   for name, args in queries)
    result = subprocess.run([str(binary)], input=text, capture_output=True, text=True,
                            check=True, timeout=120)
    return [int(line, 16) for line in result.stdout.split()]


def is_nan(value, f32):
    if f32:
        return (value & 0x7fffffff) > 0x7f800000
    return (value & 0x7fffffffffffffff) > 0x7ff0000000000000


def render(tables):
    out = [
        "# @dump: tests",
        "# Generated by scripts/generate-libm-vectors.py from musl 1.2.5's src/math;",
        "# do not edit. Each float method gives musl's result bit for bit",
        "# (std-num.float-math.musl): a line is the input bits, then the result",
        "# bits, or `nan` where musl's result is a NaN. A failure lists every",
        "# input whose result differs.",
        "",
    ]
    for width in ("f64", "f32"):
        digits = 16 if width == "f64" else 8
        out += [
            f"fn check_{width}(name: string, table: string) -> List[string]:",
            "    words := table.split_whitespace()",
            "    arity := if name == \"powf\" || name == \"atan2\" || name == \"hypot\": usize(2)"
            " else if name == \"mul_add\": usize(3) else: usize(1)",
            "    let mut wrong: List[string] = []",
            "    let i = usize(0)",
            "    while i < words.len():",
            f"        a := {width}::from_bits({ 'u64' if width == 'f64' else 'u32'}::parse_radix(words[i], 16).unwrap())",
            f"        b := if arity > 1: {width}::from_bits({ 'u64' if width == 'f64' else 'u32'}::parse_radix(words[i + 1], 16).unwrap()) else: {width}(0.0)",
            f"        c := if arity > 2: {width}::from_bits({ 'u64' if width == 'f64' else 'u32'}::parse_radix(words[i + 2], 16).unwrap()) else: {width}(0.0)",
            "        expected := words[i + arity]",
            "        got := match name:",
        ]
        for method, _, arity in FUNCTIONS:
            call = {1: f"a.{method}()", 2: f"a.{method}(b)", 3: f"a.{method}(b, c)"}[arity]
            out.append(f"            \"{method}\" => {call}")
        out += [
            "            _ => panic(\"unknown function $name\")",
            f"        text := if got.is_nan(): \"nan\" else: got.to_bits().to_radix(16).pad_start({digits}, '0')",
            "        if text != expected:",
            "            wrong.push(\"${words.slice(i, i + arity).join(\" \")} -> $text, musl $expected\")",
            "        i += arity + 1",
            "    wrong",
            "",
        ]
    out.append("tests:")
    for (method, width), lines in tables:
        out.append(f"    it(\"{method} {width}\"):")
        out.append("        table := \"\"\"")
        out += ["            " + line for line in lines]
        out.append("            \"\"\"")
        out.append(f"        assert_equal(check_{width}(\"{method}\", table), [], reason=\"musl {width} {method}\")")
    return "\n".join(out) + "\n"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("tarball", type=Path)
    parser.add_argument("--check", action="store_true", help="fail when the case would change")
    parser.add_argument("--tables", action="store_true",
                        help="print musl's tables and constants as lib/std/num.hd writes them")
    args = parser.parse_args()
    with tempfile.TemporaryDirectory() as temp:
        binary, dumper = build(args.tarball.resolve(), Path(temp))
        lines = musl_tables(dumper)
        if args.tables:
            print("\n".join(lines))
            return
        if args.check:
            source = (ROOT / "lib/std/num.hd").read_text()
            for line in lines:
                if " words): " in line and line.split(" words): ", 1)[1] not in source:
                    raise SystemExit(f"lib/std/num.hd lacks musl's table {line.split()[0]}")
        rng = random.Random(2407)
        plan = []
        for width in ("f64", "f32"):
            f32 = width == "f32"
            for method, c_name, arity in FUNCTIONS:
                if arity == 1:
                    inputs = [(v,) for v in unary_inputs(method, f32, rng)]
                elif arity == 2:
                    inputs = binary_inputs(method, f32, rng)
                else:
                    inputs = fma_inputs(f32, rng)
                plan.append((method, width, c_name + ("f" if f32 else ""), inputs, f32))
        queries = [(c_name, operands) for _, _, c_name, inputs, _ in plan for operands in inputs]
        results = iter(run(binary, queries))
        tables = []
        counts = []
        for method, width, _, inputs, f32 in plan:
            digits = 8 if f32 else 16
            lines = []
            for operands in inputs:
                result = next(results)
                shown = "nan" if is_nan(result, f32) else f"{result:0{digits}x}"
                lines.append(" ".join(f"{a:0{digits}x}" for a in operands) + " " + shown)
            tables.append(((method, width), lines))
            counts.append(f"{method} {width}: {len(lines)}")
    text = render(tables)
    if args.check:
        if OUT.read_text() != text:
            raise SystemExit(f"{OUT} is out of date")
    else:
        OUT.write_text(text)
    print("\n".join(counts))
    print(f"total: {sum(len(lines) for _, lines in tables)}")


if __name__ == "__main__":
    main()
