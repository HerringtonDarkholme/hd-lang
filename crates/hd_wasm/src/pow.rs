//! `Helper::PowF64`: the IEEE 754 `pow` of two `f64` values
//! (`expr.power.float.pow`), written in Wasm because the instruction set
//! has no `pow`, `exp` or `log`, and a host import would put a numeric
//! result in every host.
//!
//! The special cases follow IEEE 754-2019 clause 9.2 (C99 Annex F). The
//! general case is `exp(y * ln x)` for `x > 0`, computed in double-double
//! arithmetic (about 100 bits) so the result rounds correctly except in
//! cases too close to a rounding boundary to matter, and subnormal
//! results may differ from the correct rounding by one unit.
//!
//! `ln x` is `k ln 2 + 2 atanh(s)` with `x = m 2^k`, `m` in
//! `[sqrt(1/2), sqrt(2))` and `s = (m - 1) / (m + 1)`. `exp` reduces its
//! argument by a multiple of `ln 2` and by `2^-8`, sums the Taylor series
//! of `exp - 1`, squares it back up with `expm1(2r) = 2 expm1(r) +
//! expm1(r)^2`, and scales by a power of two.

use std::ops::{Add, Div, Mul, Neg, Sub};
use std::rc::Rc;

use crate::asm::Asm;
use crate::{Code, VT};

const SPLIT: f64 = 134_217_729.0;
const TWO53: f64 = 9_007_199_254_740_992.0;
const TWO54: f64 = 18_014_398_509_481_984.0;
const SQRT2: f64 = std::f64::consts::SQRT_2;
const INV_LN2: f64 = std::f64::consts::LOG2_E;
const LN2_HI: f64 = std::f64::consts::LN_2;
const LN2_LO: f64 = 2.319_046_813_846_299_6e-17;
/// Series lengths: the `atanh` series in `s^2 <= 0.03` and the `exp - 1`
/// series in `|r| <= 2^-8 ln 2 / 2`, each to well past 110 bits.
const LOG_TERMS: i32 = 24;
const EXP_TERMS: i32 = 10;
/// The reduction `2^-8` of the exponential's argument, and its squarings.
const EXP_HALVINGS: i32 = 8;
/// How close to a midpoint, relative to the result, a value must be to
/// count as exactly on it: `2^-80`, well above the arithmetic's error.
const TIE: f64 = 8.271_806_125_530_277e-25;
/// The same in units of `2^-1074` for a subnormal result: `2^-34`.
const SUB_TIE: f64 = 5.820_766_091_346_741e-11;
/// Past these `y ln x` bounds the result is infinity or zero.
const BOUND: f64 = 800.0;

#[derive(Clone, Copy)]
enum Op {
    Add,
    Sub,
    Mul,
    Div,
    Copysign,
    Lt,
    Gt,
    Ge,
    Eq,
    Ne,
    Or,
    And,
}

#[derive(Clone, Copy)]
enum Un {
    Neg,
    Abs,
    Trunc,
    Nearest,
    Eqz,
}

enum Node {
    Loc(u32),
    F(f64),
    Bin(Op, X, X),
    Un(Un, X),
}

/// A small expression over locals, emitted in one go. The operators are
/// untyped: the comparisons and `Or`, `And` and `Eqz` work on `i32`s.
#[derive(Clone)]
struct X(Rc<Node>);

fn l(n: u32) -> X {
    X(Rc::new(Node::Loc(n)))
}

fn k(v: f64) -> X {
    X(Rc::new(Node::F(v)))
}

impl X {
    fn bin(self, op: Op, o: X) -> X {
        X(Rc::new(Node::Bin(op, self, o)))
    }
    fn un(self, op: Un) -> X {
        X(Rc::new(Node::Un(op, self)))
    }
    /// Pushes the value.
    fn emit(self, a: &mut Asm) {
        put(a, &self);
    }
    fn abs(self) -> X {
        self.un(Un::Abs)
    }
    fn trunc(self) -> X {
        self.un(Un::Trunc)
    }
    fn nearest(self) -> X {
        self.un(Un::Nearest)
    }
    fn eqz(self) -> X {
        self.un(Un::Eqz)
    }
    fn copysign(self, o: X) -> X {
        self.bin(Op::Copysign, o)
    }
    fn flt(self, o: X) -> X {
        self.bin(Op::Lt, o)
    }
    fn fgt(self, o: X) -> X {
        self.bin(Op::Gt, o)
    }
    fn fge(self, o: X) -> X {
        self.bin(Op::Ge, o)
    }
    fn feq(self, o: X) -> X {
        self.bin(Op::Eq, o)
    }
    fn fne(self, o: X) -> X {
        self.bin(Op::Ne, o)
    }
    fn bor(self, o: X) -> X {
        self.bin(Op::Or, o)
    }
    fn band(self, o: X) -> X {
        self.bin(Op::And, o)
    }
}

impl Add for X {
    type Output = X;
    fn add(self, o: X) -> X {
        self.bin(Op::Add, o)
    }
}

impl Sub for X {
    type Output = X;
    fn sub(self, o: X) -> X {
        self.bin(Op::Sub, o)
    }
}

impl Mul for X {
    type Output = X;
    fn mul(self, o: X) -> X {
        self.bin(Op::Mul, o)
    }
}

impl Div for X {
    type Output = X;
    fn div(self, o: X) -> X {
        self.bin(Op::Div, o)
    }
}

impl Neg for X {
    type Output = X;
    fn neg(self) -> X {
        self.un(Un::Neg)
    }
}

fn put(a: &mut Asm, x: &X) {
    match &*x.0 {
        Node::Loc(n) => a.raw_get(*n),
        Node::F(v) => {
            a.s().f64_const((*v).into());
        }
        Node::Bin(op, p, q) => {
            put(a, p);
            put(a, q);
            let mut s = a.s();
            match op {
                Op::Add => s.f64_add(),
                Op::Sub => s.f64_sub(),
                Op::Mul => s.f64_mul(),
                Op::Div => s.f64_div(),
                Op::Copysign => s.f64_copysign(),
                Op::Lt => s.f64_lt(),
                Op::Gt => s.f64_gt(),
                Op::Ge => s.f64_ge(),
                Op::Eq => s.f64_eq(),
                Op::Ne => s.f64_ne(),
                Op::Or => s.i32_or(),
                Op::And => s.i32_and(),
            };
        }
        Node::Un(op, p) => {
            put(a, p);
            let mut s = a.s();
            match op {
                Un::Neg => s.f64_neg(),
                Un::Abs => s.f64_abs(),
                Un::Trunc => s.f64_trunc(),
                Un::Nearest => s.f64_nearest(),
                Un::Eqz => s.i32_eqz(),
            };
        }
    }
}

/// A double-double number: the sum of two `f64` locals, `l` small against
/// `h`.
#[derive(Clone, Copy)]
struct Dd {
    h: u32,
    l: u32,
}

struct G {
    a: Asm,
}

impl G {
    fn f64(&mut self) -> u32 {
        self.a.local(VT::F64)
    }

    fn i32(&mut self) -> u32 {
        self.a.local(VT::I32)
    }

    fn assign(&mut self, dst: u32, x: X) {
        x.emit(&mut self.a);
        self.a.set(dst);
    }

    fn tmp(&mut self, x: X) -> u32 {
        let t = self.f64();
        self.assign(t, x);
        t
    }

    fn itmp(&mut self, x: X) -> u32 {
        let t = self.i32();
        self.assign(t, x);
        t
    }

    fn ret(&mut self, x: X) {
        x.emit(&mut self.a);
        self.a.s().return_();
    }

    fn iff(&mut self, c: X, then: impl FnOnce(&mut G)) {
        c.emit(&mut self.a);
        self.a.if_();
        then(self);
        self.a.end();
    }

    fn if_else(&mut self, c: X, then: impl FnOnce(&mut G), els: impl FnOnce(&mut G)) {
        c.emit(&mut self.a);
        self.a.if_();
        then(self);
        self.a.else_();
        els(self);
        self.a.end();
    }

    /// Runs `body` `n` times (`n >= 1`).
    fn repeat(&mut self, n: i32, body: impl FnOnce(&mut G)) {
        let c = self.i32();
        self.a.i32(n);
        self.a.set(c);
        self.a.loop_();
        body(self);
        self.a.get(c);
        self.a.i32(1);
        self.a.s().i32_sub();
        self.a.set(c);
        self.a.get(c);
        self.a.br_if(0);
        self.a.end();
    }

    fn two_sum(&mut self, a: X, b: X) -> Dd {
        let s = self.tmp(a.clone() + b.clone());
        let bb = self.tmp(l(s) - a.clone());
        let e = self.tmp((a - (l(s) - l(bb))) + (b - l(bb)));
        Dd { h: s, l: e }
    }

    /// `two_sum` when `|a| >= |b|`.
    fn quick_two_sum(&mut self, a: X, b: X) -> Dd {
        let s = self.tmp(a.clone() + b.clone());
        let e = self.tmp(b - (l(s) - a));
        Dd { h: s, l: e }
    }

    /// Veltkamp's split of `a` into two 26-bit halves.
    fn split(&mut self, a: u32) -> (u32, u32) {
        let t = self.tmp(k(SPLIT) * l(a));
        let hi = self.tmp(l(t) - (l(t) - l(a)));
        let lo = self.tmp(l(a) - l(hi));
        (hi, lo)
    }

    /// Dekker's exact product: `a * b = h + l`.
    fn two_prod(&mut self, a: u32, b: u32) -> Dd {
        let p = self.tmp(l(a) * l(b));
        let (ah, al) = self.split(a);
        let (bh, bl) = self.split(b);
        let e = self.tmp(((l(ah) * l(bh) - l(p)) + l(ah) * l(bl) + l(al) * l(bh)) + l(al) * l(bl));
        Dd { h: p, l: e }
    }

    fn dd_add(&mut self, a: Dd, b: Dd) -> Dd {
        let s = self.two_sum(l(a.h), l(b.h));
        let t = self.two_sum(l(a.l), l(b.l));
        let s2 = self.tmp(l(s.l) + l(t.h));
        let r = self.quick_two_sum(l(s.h), l(s2));
        let r2 = self.tmp(l(r.l) + l(t.l));
        self.quick_two_sum(l(r.h), l(r2))
    }

    fn dd_add_f(&mut self, a: Dd, f: u32) -> Dd {
        let s = self.two_sum(l(a.h), l(f));
        let s2 = self.tmp(l(s.l) + l(a.l));
        self.quick_two_sum(l(s.h), l(s2))
    }

    fn dd_sub(&mut self, a: Dd, b: Dd) -> Dd {
        let nh = self.tmp(-l(b.h));
        let nl = self.tmp(-l(b.l));
        self.dd_add(a, Dd { h: nh, l: nl })
    }

    fn dd_mul(&mut self, a: Dd, b: Dd) -> Dd {
        let p = self.two_prod(a.h, b.h);
        let p2 = self.tmp(l(p.l) + (l(a.h) * l(b.l) + l(a.l) * l(b.h)));
        self.quick_two_sum(l(p.h), l(p2))
    }

    fn dd_mul_f(&mut self, a: Dd, f: u32) -> Dd {
        let p = self.two_prod(a.h, f);
        let p2 = self.tmp(l(p.l) + l(a.l) * l(f));
        self.quick_two_sum(l(p.h), l(p2))
    }

    fn dd_div_f(&mut self, a: Dd, f: u32) -> Dd {
        let q1 = self.tmp(l(a.h) / l(f));
        let p = self.two_prod(q1, f);
        let s = self.two_sum(l(a.h), -l(p.h));
        let e = self.tmp(l(s.l) - l(p.l) + l(a.l));
        let q2 = self.tmp((l(s.h) + l(e)) / l(f));
        self.quick_two_sum(l(q1), l(q2))
    }

    fn dd_div(&mut self, a: Dd, b: Dd) -> Dd {
        let q1 = self.tmp(l(a.h) / l(b.h));
        let p1 = self.dd_mul_f(b, q1);
        let r1 = self.dd_sub(a, p1);
        let q2 = self.tmp(l(r1.h) / l(b.h));
        let p2 = self.dd_mul_f(b, q2);
        let r2 = self.dd_sub(r1, p2);
        let q3 = self.tmp(l(r2.h) / l(b.h));
        let q = self.quick_two_sum(l(q1), l(q2));
        self.dd_add_f(q, q3)
    }

    /// Fresh locals holding `src`: the variables of a loop.
    fn dd_var(&mut self, src: Dd) -> Dd {
        let h = self.tmp(l(src.h));
        let lo = self.tmp(l(src.l));
        Dd { h, l: lo }
    }

    fn dd_set(&mut self, dst: Dd, src: Dd) {
        self.assign(dst.h, l(src.h));
        self.assign(dst.l, l(src.l));
    }
}

/// `2^n` for an `i32` local `n` in the normal exponent range.
fn pow2(g: &mut G, n: u32) -> u32 {
    let out = g.f64();
    g.a.get(n);
    g.a.i32(1023);
    g.a.s().i32_add().i64_extend_i32_u();
    g.a.i64(52);
    g.a.s().i64_shl().f64_reinterpret_i64();
    g.a.set(out);
    out
}

/// The special cases of `pow(x, y)` that need no logarithm. On return,
/// `x > 0` is finite, `y` is finite and nonzero, `sign` is the result's
/// sign (`-1.0` for a negative base with an odd exponent), and `x` has been
/// replaced by its magnitude, in the returned local.
fn special_cases(g: &mut G, x: u32, y: u32) -> (u32, u32) {
    // `pow(x, +-0)` and `pow(1, y)` are 1, even for a NaN.
    g.iff(l(y).feq(k(0.0)), |g| g.ret(k(1.0)));
    g.iff(l(x).feq(k(1.0)), |g| g.ret(k(1.0)));
    g.iff(l(x).fne(l(x)).bor(l(y).fne(l(y))), |g| g.ret(l(x) + l(y)));
    let yabs = g.tmp(l(y).abs());
    // `y` is an integer, and an odd one (every `|y| >= 2^53` is even).
    let small = g.itmp(l(yabs).flt(k(TWO53)));
    let yint = g.itmp(l(yabs).fge(k(TWO53)).bor(l(y).trunc().feq(l(y))));
    let yodd = g.itmp(
        l(yint)
            .band(l(small))
            .band(((l(y) * k(0.5)).trunc() * k(2.0)).fne(l(y))),
    );
    g.iff(l(yabs).feq(k(f64::INFINITY)), |g| {
        let ax = g.tmp(l(x).abs());
        g.iff(l(ax).feq(k(1.0)), |g| g.ret(k(1.0)));
        // `|x| > 1` with `y = +inf` and `|x| < 1` with `y = -inf` give
        // `+inf`; the other two give `+0`.
        g.if_else(
            l(ax).fgt(k(1.0)),
            |g| {
                g.if_else(
                    l(y).fgt(k(0.0)),
                    |g| g.ret(k(f64::INFINITY)),
                    |g| g.ret(k(0.0)),
                );
            },
            |g| {
                g.if_else(
                    l(y).fgt(k(0.0)),
                    |g| g.ret(k(0.0)),
                    |g| g.ret(k(f64::INFINITY)),
                );
            },
        );
    });
    // A zero base: infinity for a negative exponent, else zero, with the
    // base's sign for an odd exponent.
    g.iff(l(x).feq(k(0.0)), |g| {
        let r = g.f64();
        g.if_else(
            l(y).flt(k(0.0)),
            |g| g.assign(r, k(f64::INFINITY)),
            |g| g.assign(r, k(0.0)),
        );
        g.iff(l(yodd), |g| g.ret(l(r).copysign(l(x))));
        g.ret(l(r));
    });
    // An infinite base: zero for a negative exponent, else infinity,
    // negative for a negative base with an odd exponent.
    g.iff(l(x).abs().feq(k(f64::INFINITY)), |g| {
        let r = g.f64();
        g.if_else(
            l(y).flt(k(0.0)),
            |g| g.assign(r, k(0.0)),
            |g| g.assign(r, k(f64::INFINITY)),
        );
        g.iff(l(x).flt(k(0.0)).band(l(yodd)), |g| g.ret(-l(r)));
        g.ret(l(r));
    });
    // A negative base needs an integer exponent.
    let sign = g.tmp(k(1.0));
    g.iff(l(x).flt(k(0.0)), |g| {
        g.iff(l(yint).eqz(), |g| g.ret(k(f64::NAN)));
        g.iff(l(yodd), |g| g.assign(sign, k(-1.0)));
    });
    let xa = g.tmp(l(x).abs());
    (xa, sign)
}

/// `ln x` as a double-double, for a positive finite `x`.
fn log(g: &mut G, x: u32) -> Dd {
    let kf = g.tmp(k(0.0));
    // A subnormal is scaled up first.
    g.iff(l(x).flt(k(f64::MIN_POSITIVE)), |g| {
        g.assign(x, l(x) * k(TWO54));
        g.assign(kf, k(-54.0));
    });
    // `x = m 2^k` with `m` in `[1, 2)`, from the bits.
    let bits = g.a.local(VT::I64);
    g.a.get(x);
    g.a.s().i64_reinterpret_f64();
    g.a.set(bits);
    let e = g.f64();
    g.a.get(bits);
    g.a.i64(52);
    g.a.s().i64_shr_u().f64_convert_i64_u();
    g.a.set(e);
    g.assign(kf, l(kf) + l(e) - k(1023.0));
    let m = g.f64();
    g.a.get(bits);
    g.a.i64(0x000f_ffff_ffff_ffff);
    g.a.s().i64_and();
    g.a.i64(0x3ff0_0000_0000_0000);
    g.a.s().i64_or().f64_reinterpret_i64();
    g.a.set(m);
    // `m` in `[sqrt(1/2), sqrt(2))`.
    g.iff(l(m).fgt(k(SQRT2)), |g| {
        g.assign(m, l(m) * k(0.5));
        g.assign(kf, l(kf) + k(1.0));
    });
    // `ln m = 2 atanh(s)`, `s = (m - 1) / (m + 1)`, both exact sums.
    let num = g.tmp(l(m) - k(1.0));
    let zero = g.tmp(k(0.0));
    let den = g.two_sum(l(m), k(1.0));
    let s = g.dd_div(Dd { h: num, l: zero }, den);
    let s2 = g.dd_mul(s, s);
    let sum = g.dd_var(s);
    let pw = g.dd_var(s);
    let j = g.tmp(k(1.0));
    g.repeat(LOG_TERMS, |g| {
        let p = g.dd_mul(pw, s2);
        g.dd_set(pw, p);
        g.assign(j, l(j) + k(2.0));
        let t = g.dd_div_f(pw, j);
        let acc = g.dd_add(sum, t);
        g.dd_set(sum, acc);
    });
    let lh = g.tmp(k(2.0) * l(sum.h));
    let ll = g.tmp(k(2.0) * l(sum.l));
    let ln2 = Dd {
        h: g.tmp(k(LN2_HI)),
        l: g.tmp(k(LN2_LO)),
    };
    let kln2 = g.dd_mul_f(ln2, kf);
    g.dd_add(kln2, Dd { h: lh, l: ll })
}

/// The code of `PowF64`: `(x: f64, y: f64) -> f64`.
#[must_use]
pub(crate) fn pow_f64_code() -> Code {
    let mut g = G {
        a: Asm::new(vec![VT::F64, VT::F64]),
    };
    let (x, y) = (0, 1);
    let (xa, sign) = special_cases(&mut g, x, y);
    let t = log(&mut g, xa);
    // Past `BOUND` the result is infinity or zero: this also keeps the
    // product below finite in the double-double arithmetic.
    let approx = g.tmp(l(y) * l(t.h));
    g.iff(l(approx).fgt(k(BOUND)), |g| {
        g.ret(k(f64::INFINITY).copysign(l(sign)));
    });
    g.iff(l(approx).flt(k(-BOUND)), |g| {
        g.ret(k(0.0).copysign(l(sign)));
    });
    // `exp(p) = exp(r) 2^n`, `r = p - n ln 2`.
    let p = g.dd_mul_f(t, y);
    let nf = g.tmp((l(p.h) * k(INV_LN2)).nearest());
    let ln2 = Dd {
        h: g.tmp(k(LN2_HI)),
        l: g.tmp(k(LN2_LO)),
    };
    let nln2 = g.dd_mul_f(ln2, nf);
    let r = g.dd_sub(p, nln2);
    let scale = k(1.0 / f64::from(1 << EXP_HALVINGS));
    let rh = g.tmp(l(r.h) * scale.clone());
    let rl = g.tmp(l(r.l) * scale);
    let r = Dd { h: rh, l: rl };
    // `exp(r) - 1` by its Taylor series, then back up by squarings.
    let term = g.dd_var(r);
    let sum = g.dd_var(r);
    let j = g.tmp(k(1.0));
    g.repeat(EXP_TERMS, |g| {
        g.assign(j, l(j) + k(1.0));
        let tr = g.dd_mul(term, r);
        let next = g.dd_div_f(tr, j);
        g.dd_set(term, next);
        let acc = g.dd_add(sum, term);
        g.dd_set(sum, acc);
    });
    g.repeat(EXP_HALVINGS, |g| {
        let dh = g.tmp(k(2.0) * l(sum.h));
        let dl = g.tmp(k(2.0) * l(sum.l));
        let sq = g.dd_mul(sum, sum);
        let acc = g.dd_add(Dd { h: dh, l: dl }, sq);
        g.dd_set(sum, acc);
    });
    let one = g.tmp(k(1.0));
    let f = g.dd_add_f(sum, one);
    let n = g.i32();
    g.a.get(nf);
    g.a.s().i32_trunc_f64_s();
    g.a.set(n);
    // The result `f 2^n` is subnormal when `n < -1022`, or `n = -1022`
    // with `f < 1`.
    let subnormal = g.itmp(
        l(nf)
            .flt(k(-1022.0))
            .bor(l(nf).feq(k(-1022.0)).band(l(f.h).flt(k(1.0)))),
    );
    g.iff(l(subnormal), |g| subnormal_result(g, n, f, sign));
    let rounded = tie_to_even(&mut g, f);
    // The scale `2^n` in two steps, so neither overflows on its own.
    let n1 = g.i32();
    g.a.get(n);
    g.a.i32(2);
    g.a.s().i32_div_s();
    g.a.set(n1);
    let n2 = g.i32();
    g.a.get(n);
    g.a.get(n1);
    g.a.s().i32_sub();
    g.a.set(n2);
    let s1 = pow2(&mut g, n1);
    let s2 = pow2(&mut g, n2);
    g.ret((l(rounded) * l(s1) * l(s2)).copysign(l(sign)));
    g.a.finish(vec![VT::F64])
}

/// The correct rounding of `f.h + f.l` to a double (`f.h` is the nearest
/// double already). When `f.l` is within `2^-80` of half the gap to the
/// neighbor it points to, the exact value is taken to be that midpoint, as
/// it is for a result of exactly 54 significant bits, and the neighbor
/// with the even significand wins. Without this, an exact tie would round
/// either way.
fn tie_to_even(g: &mut G, f: Dd) -> u32 {
    let bits = g.a.local(VT::I64);
    g.a.get(f.h);
    g.a.s().i64_reinterpret_f64();
    g.a.set(bits);
    // The neighbor of `f.h` on the side of `f.l`.
    let other_bits = g.a.local(VT::I64);
    g.a.get(bits);
    g.a.i64(1);
    g.a.i64(-1);
    l(f.l).fgt(k(0.0)).emit(&mut g.a);
    g.a.s().select().i64_add();
    g.a.set(other_bits);
    let other = g.f64();
    g.a.get(other_bits);
    g.a.s().f64_reinterpret_i64();
    g.a.set(other);
    let half = g.tmp((l(other) - l(f.h)) * k(0.5));
    let near = g.itmp((l(f.l).abs() - l(half).abs()).abs().flt(l(f.h) * k(TIE)));
    let odd = g.i32();
    g.a.get(bits);
    g.a.i64(1);
    g.a.s().i64_and().i32_wrap_i64();
    g.a.set(odd);
    let out = g.tmp(l(f.h));
    g.iff(l(near).band(l(odd)), |g| g.assign(out, l(other)));
    out
}

/// Returns `f 2^n` for a subnormal result, rounded once to a multiple of
/// `2^-1074`: the scaled value `u + w` (both exact) goes to the nearest
/// integer, a tie to the even one.
fn subnormal_result(g: &mut G, n: u32, f: Dd, sign: u32) {
    let shift = g.i32();
    g.a.get(n);
    g.a.i32(1074);
    g.a.s().i32_add();
    g.a.set(shift);
    let scale = pow2(g, shift);
    let u = g.tmp(l(f.h) * l(scale));
    let w = g.tmp(l(f.l) * l(scale));
    let r = g.tmp(l(u).nearest());
    let d = g.tmp((l(u) - l(r)) + l(w));
    let step = g.tmp(k(1.0).copysign(l(d)));
    let even = g.itmp(((l(r) * k(0.5)).trunc() * k(2.0)).feq(l(r)));
    let tie = g.itmp((l(d).abs() - k(0.5)).abs().flt(k(SUB_TIE)));
    let out = g.tmp(l(r));
    g.if_else(
        l(tie),
        |g| g.iff(l(even).eqz(), |g| g.assign(out, l(r) + l(step))),
        |g| g.iff(l(d).abs().fgt(k(0.5)), |g| g.assign(out, l(r) + l(step))),
    );
    let half = k(2.0_f64.powi(-537));
    g.ret((l(out) * half.clone() * half).copysign(l(sign)));
}
