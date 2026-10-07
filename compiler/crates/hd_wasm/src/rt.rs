//! Runtime helpers the compiler builds (runtime-and-host.md §16.1,
//! "generated"): literal getters, number formatting, string equality, the
//! exchange-buffer copy, panic stubs, the default profile's host provider
//! stubs (from `hd_host_abi::TABLE`), vtable adapters, the list cursor and
//! the entry wrapper. Each is keyed by what it depends on, so link builds
//! it once per program.

use hd_base::wire::{Reader, Writer};
use hd_base::{Hash128, StageResult};
use wasm_encoder::BlockType;

use crate::asm::Asm;
use crate::{Code, Sym, VT, WTy, decode_vts, encode_vts, unsupported};

/// How a `T?` is built (`layout::OptShape` without its payload values).
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum OptForm {
    NullRef,
    NullStr,
    Tagged,
    Boxed(WTy),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Helper {
    /// A pooled string literal's getter (wasm-layout.md §15.4).
    Lit(Vec<u8>),
    /// `Display.to_string` of an integer: a view over a fresh buffer.
    IntToStr { wide: bool, signed: bool },
    /// String `==` over the viewed bytes.
    StrEq,
    /// Copies a string's viewed bytes to the exchange buffer; its length.
    StrToBuf,
    /// A panic stub with a fixed message (wasm-layout.md §15.5).
    Panic(String),
    /// `panic(message)`.
    PanicStr,
    /// The cursor closure of `List.iter()`.
    ListStep {
        code: WTy,
        env: WTy,
        list: WTy,
        elem: Vec<VT>,
        opt: OptForm,
    },
    /// A host method that may wait, called cold: its leaf frame.
    HostCold {
        sig: WTy,
        frame: WTy,
        poll: Box<Helper>,
    },
    /// The leaf frame's poll: `.start`, then `.finish` after a wait.
    HostPoll {
        frame: WTy,
        module: String,
        method: String,
        result: Vec<VT>,
    },
    /// A vtable slot over a concrete impl method: unerases the receiver.
    Adapter {
        sig: WTy,
        self_vts: Vec<VT>,
        target: Box<Sym>,
        params: Vec<VT>,
        results: Vec<VT>,
    },
    /// The exported `main`: builds the default profile's providers and
    /// calls the entry.
    Entry {
        main: Hash128,
        providers: Vec<(WTy, Vec<Helper>)>,
        results: u32,
    },
}

fn enc_wty(t: &WTy, w: &mut Writer) {
    encode_vts(&[VT::r(t.clone())], w);
}

fn dec_wty(r: &mut Reader<'_>) -> Option<WTy> {
    match decode_vts(r, 0)?.pop()? {
        VT::Ref(t, _) => Some(*t),
        _ => None,
    }
}

impl Helper {
    pub(crate) fn encode(&self, w: &mut Writer) {
        match self {
            Helper::Lit(b) => {
                w.u8(0);
                w.blob(b);
            }
            Helper::IntToStr { wide, signed } => {
                w.u8(1);
                w.u8(u8::from(*wide));
                w.u8(u8::from(*signed));
            }
            Helper::StrEq => w.u8(2),
            Helper::StrToBuf => w.u8(3),
            Helper::Panic(m) => {
                w.u8(4);
                w.str(m);
            }
            Helper::PanicStr => w.u8(5),
            Helper::ListStep {
                code,
                env,
                list,
                elem,
                opt,
            } => {
                w.u8(6);
                enc_wty(code, w);
                enc_wty(env, w);
                enc_wty(list, w);
                encode_vts(elem, w);
                match opt {
                    OptForm::NullRef => w.u8(0),
                    OptForm::NullStr => w.u8(1),
                    OptForm::Tagged => w.u8(2),
                    OptForm::Boxed(b) => {
                        w.u8(3);
                        enc_wty(b, w);
                    }
                }
            }
            Helper::HostCold { sig, frame, poll } => {
                w.u8(7);
                enc_wty(sig, w);
                enc_wty(frame, w);
                poll.encode(w);
            }
            Helper::HostPoll {
                frame,
                module,
                method,
                result,
            } => {
                w.u8(8);
                enc_wty(frame, w);
                w.str(module);
                w.str(method);
                encode_vts(result, w);
            }
            Helper::Adapter {
                sig,
                self_vts,
                target,
                params,
                results,
            } => {
                w.u8(9);
                enc_wty(sig, w);
                encode_vts(self_vts, w);
                target.encode(w);
                encode_vts(params, w);
                encode_vts(results, w);
            }
            Helper::Entry {
                main,
                providers,
                results,
            } => {
                w.u8(10);
                w.hash(*main);
                w.len_of(providers);
                for (vt, slots) in providers {
                    enc_wty(vt, w);
                    w.len_of(slots);
                    for s in slots {
                        s.encode(w);
                    }
                }
                w.u32(*results);
            }
        }
    }

    pub(crate) fn decode(r: &mut Reader<'_>) -> Option<Helper> {
        Some(match r.u8() {
            0 => Helper::Lit(r.blob().to_vec()),
            1 => Helper::IntToStr {
                wide: r.u8() == 1,
                signed: r.u8() == 1,
            },
            2 => Helper::StrEq,
            3 => Helper::StrToBuf,
            4 => Helper::Panic(r.str().to_owned()),
            5 => Helper::PanicStr,
            6 => Helper::ListStep {
                code: dec_wty(r)?,
                env: dec_wty(r)?,
                list: dec_wty(r)?,
                elem: decode_vts(r, 0)?,
                opt: match r.u8() {
                    0 => OptForm::NullRef,
                    1 => OptForm::NullStr,
                    2 => OptForm::Tagged,
                    _ => OptForm::Boxed(dec_wty(r)?),
                },
            },
            7 => Helper::HostCold {
                sig: dec_wty(r)?,
                frame: dec_wty(r)?,
                poll: Box::new(Helper::decode(r)?),
            },
            8 => Helper::HostPoll {
                frame: dec_wty(r)?,
                module: r.str().to_owned(),
                method: r.str().to_owned(),
                result: decode_vts(r, 0)?,
            },
            9 => Helper::Adapter {
                sig: dec_wty(r)?,
                self_vts: decode_vts(r, 0)?,
                target: Box::new(Sym::decode(r)?),
                params: decode_vts(r, 0)?,
                results: decode_vts(r, 0)?,
            },
            10 => {
                let main = r.hash();
                let n = r.count();
                let mut providers = Vec::new();
                for _ in 0..n {
                    let vt = dec_wty(r)?;
                    let m = r.count();
                    let slots = (0..m)
                        .map(|_| Helper::decode(r))
                        .collect::<Option<Vec<_>>>()?;
                    providers.push((vt, slots));
                }
                Helper::Entry {
                    main,
                    providers,
                    results: r.u32(),
                }
            }
            _ => return None,
        })
    }
}

/// The host runtime's stderr import (runtime-and-host.md §17.2).
#[must_use]
pub fn stderr_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "stderr".into(),
        params: vec![VT::I32],
        results: vec![],
    }
}

/// The host runtime's `block` import (suspension.md §14.9).
#[must_use]
pub fn block_import() -> Sym {
    Sym::Import {
        module: "hd:rt".into(),
        name: "block".into(),
        params: vec![],
        results: vec![VT::I32],
    }
}

fn str_vts() -> Vec<VT> {
    vec![VT::r(WTy::Bytes), VT::I64]
}

/// A literal getter, once link knows its segment offset and cell global.
#[must_use]
pub fn lit_code(off: u32, len: u32, global: u32) -> Code {
    let mut a = Asm::new(vec![]);
    a.s().global_get(global).ref_is_null();
    a.if_();
    a.i32(off.cast_signed());
    a.i32(len.cast_signed());
    a.array_new_data(&WTy::Bytes, 0);
    a.s().global_set(global);
    a.end();
    a.s().global_get(global).ref_as_non_null();
    a.finish(vec![VT::r(WTy::Bytes)])
}

/// The code of a helper.
pub fn helper_code(h: &Helper) -> StageResult<Code> {
    Ok(match h {
        Helper::Lit(_) => lit_code(0, 0, 0),
        Helper::IntToStr { wide, signed } => {
            let mut a = Asm::new(vec![if *wide { VT::I64 } else { VT::I32 }]);
            let (x, neg, buf, i) = (
                a.local(VT::I64),
                a.local(VT::I32),
                a.local(VT::r(WTy::Bytes)),
                a.local(VT::I32),
            );
            a.get(0);
            if !*wide {
                if *signed {
                    a.s().i64_extend_i32_s();
                } else {
                    a.s().i64_extend_i32_u();
                }
            }
            a.set(x);
            if *signed {
                a.get(x);
                a.i64(0);
                a.s().i64_lt_s();
            } else {
                a.i32(0);
            }
            a.set(neg);
            a.get(neg);
            a.if_();
            a.i64(0);
            a.get(x);
            a.s().i64_sub();
            a.set(x);
            a.end();
            a.i32(21);
            a.array_new_default(&WTy::Bytes);
            a.set(buf);
            a.i32(21);
            a.set(i);
            a.loop_();
            a.get(i);
            a.i32(1);
            a.s().i32_sub();
            a.set(i);
            a.get(buf);
            a.get(i);
            a.get(x);
            a.i64(10);
            a.s().i64_rem_u().i32_wrap_i64();
            a.i32(48);
            a.s().i32_add();
            a.array_set(&WTy::Bytes);
            a.get(x);
            a.i64(10);
            a.s().i64_div_u();
            a.set(x);
            a.get(x);
            a.i64(0);
            a.s().i64_ne();
            a.br_if(0);
            a.end();
            a.get(neg);
            a.if_();
            a.get(i);
            a.i32(1);
            a.s().i32_sub();
            a.set(i);
            a.get(buf);
            a.get(i);
            a.i32(45);
            a.array_set(&WTy::Bytes);
            a.end();
            a.get(buf);
            a.i32(21);
            a.get(i);
            a.s().i32_sub().i64_extend_i32_u();
            a.i64(32);
            a.s().i64_shl();
            a.get(i);
            a.s().i64_extend_i32_u().i64_or();
            a.finish(str_vts())
        }
        Helper::StrEq => {
            let mut a = Asm::new([str_vts(), str_vts()].concat());
            let (len, k) = (a.local(VT::I32), a.local(VT::I32));
            a.get(1);
            a.i64(32);
            a.s().i64_shr_u().i32_wrap_i64();
            a.set(len);
            a.get(len);
            a.get(3);
            a.i64(32);
            a.s().i64_shr_u().i32_wrap_i64().i32_ne();
            a.if_();
            a.i32(0);
            a.s().return_();
            a.end();
            a.block();
            a.loop_();
            a.get(k);
            a.get(len);
            a.s().i32_ge_u();
            a.br_if(1);
            a.get(0);
            a.get(1);
            a.s().i32_wrap_i64();
            a.get(k);
            a.s().i32_add();
            a.array_get(&WTy::Bytes);
            a.get(2);
            a.get(3);
            a.s().i32_wrap_i64();
            a.get(k);
            a.s().i32_add();
            a.array_get(&WTy::Bytes);
            a.s().i32_ne();
            a.if_();
            a.i32(0);
            a.s().return_();
            a.end();
            a.get(k);
            a.i32(1);
            a.s().i32_add();
            a.set(k);
            a.br(0);
            a.end();
            a.end();
            a.i32(1);
            a.finish(vec![VT::I32])
        }
        Helper::StrToBuf => {
            let mut a = Asm::new(str_vts());
            let (len, k, need) = (a.local(VT::I32), a.local(VT::I32), a.local(VT::I32));
            a.get(1);
            a.i64(32);
            a.s().i64_shr_u().i32_wrap_i64();
            a.set(len);
            // Grow the exchange buffer to fit (runtime-and-host.md §17.3).
            a.get(len);
            a.i32(65535);
            a.s().i32_add();
            a.i32(16);
            a.s().i32_shr_u();
            a.set(need);
            a.get(need);
            a.s().memory_size(0);
            a.s().i32_gt_u();
            a.if_();
            a.get(need);
            a.s().memory_size(0).i32_sub().memory_grow(0).drop();
            a.end();
            a.block();
            a.loop_();
            a.get(k);
            a.get(len);
            a.s().i32_ge_u();
            a.br_if(1);
            a.get(k);
            a.get(0);
            a.get(1);
            a.s().i32_wrap_i64();
            a.get(k);
            a.s().i32_add();
            a.array_get(&WTy::Bytes);
            a.mem8_store(0);
            a.get(k);
            a.i32(1);
            a.s().i32_add();
            a.set(k);
            a.br(0);
            a.end();
            a.end();
            a.get(len);
            a.finish(vec![VT::I32])
        }
        Helper::Panic(msg) => {
            let mut a = Asm::new(vec![]);
            write_lit(&mut a, &format!("panic: {msg}\n"));
            a.s().unreachable();
            a.finish(vec![])
        }
        Helper::PanicStr => {
            let mut a = Asm::new(str_vts());
            write_lit(&mut a, "panic: ");
            a.get(0);
            a.get(1);
            a.call(Sym::Helper(Helper::StrToBuf));
            a.call(stderr_import());
            write_lit(&mut a, "\n");
            a.s().unreachable();
            a.finish(vec![])
        }
        Helper::ListStep {
            code,
            env,
            list,
            elem,
            opt,
        } => {
            let WTy::Func(_, results) = code else {
                return unsupported("a list cursor without a code type");
            };
            let mut a = Asm::new(vec![VT::Eq]);
            let e = a.local(VT::r(env.clone()));
            let l = a.local(VT::r(list.clone()));
            let i = a.local(VT::I32);
            a.get(0);
            a.ref_cast(env, false);
            a.set(e);
            a.get(e);
            a.struct_get(env, 1);
            a.set(l);
            a.get(e);
            a.struct_get(env, 2);
            a.set(i);
            a.get(i);
            a.get(l);
            a.struct_get(list, 0);
            a.s().i32_lt_u();
            a.if_();
            a.get(e);
            a.get(i);
            a.i32(1);
            a.s().i32_add();
            a.struct_set(env, 2);
            if *opt == OptForm::Tagged {
                a.i32(1);
            }
            for (k, v) in elem.iter().enumerate() {
                let st = crate::layout::storage(v).dflt();
                let arr = WTy::Array(st.clone());
                a.get(l);
                a.struct_get(list, 1 + u32::try_from(k).expect("k"));
                a.get(i);
                a.array_get(&arr);
                let want = if *opt == OptForm::NullRef {
                    v.dflt()
                } else {
                    v.clone()
                };
                a.conv(&st, &want);
            }
            if let OptForm::Boxed(b) = opt {
                a.struct_new(b);
            }
            a.s().return_();
            a.end();
            for v in results {
                a.zero(v);
            }
            a.finish(results.clone())
        }
        Helper::HostCold { sig, frame, poll } => {
            let WTy::Func(params, results) = sig else {
                return unsupported("a host stub without a signature");
            };
            let mut a = Asm::new(params.clone());
            a.ref_func(Sym::Helper((**poll).clone()));
            for k in 1..params.len() {
                a.get(u32::try_from(k).expect("k"));
            }
            a.i32(-1);
            a.struct_new(frame);
            a.finish(results.clone())
        }
        Helper::HostPoll {
            frame,
            module,
            method,
            result,
        } => {
            if *result != [VT::I32, VT::I32] {
                return unsupported("a host result codec other than an enum of tags");
            }
            let mut res = vec![VT::I32];
            res.extend(result.iter().map(VT::dflt));
            let mut a = Asm::new(vec![VT::Eq]);
            let (f, h, n, st) = (
                a.local(VT::r(frame.clone())),
                a.local(VT::I32),
                a.local(VT::I32),
                a.local(VT::I32),
            );
            a.get(0);
            a.ref_cast(frame, false);
            a.set(f);
            a.get(f);
            a.struct_get(frame, 3);
            a.set(h);
            a.get(h);
            a.i32(0);
            a.s().i32_lt_s();
            a.if_();
            a.get(f);
            a.struct_get(frame, 1);
            a.get(f);
            a.struct_get(frame, 2);
            a.call(Sym::Helper(Helper::StrToBuf));
            a.call(Sym::Import {
                module: module.clone(),
                name: format!("{method}.start"),
                params: vec![VT::I32],
                results: vec![VT::I32, VT::I32],
            });
            a.set(n);
            a.set(st);
            a.get(st);
            a.if_();
            a.get(f);
            a.get(n);
            a.struct_set(frame, 3);
            a.i32(0);
            a.i32(0);
            a.i32(0);
            a.s().return_();
            a.end();
            a.else_();
            a.get(h);
            a.call(Sym::Import {
                module: module.clone(),
                name: format!("{method}.finish"),
                params: vec![VT::I32],
                results: vec![VT::I32],
            });
            a.set(n);
            a.end();
            // Decode an enum of tags (runtime-and-host.md §17.4): the
            // variant index, then a payload variant index when present.
            a.i32(1);
            a.i32(0);
            a.mem8_load(0);
            a.get(n);
            a.i32(1);
            a.s().i32_gt_s();
            a.s().if_(BlockType::Result(wasm_encoder::ValType::I32));
            a.i32(0);
            a.mem8_load(1);
            a.else_();
            a.i32(0);
            a.end();
            a.finish(res)
        }
        Helper::Adapter {
            sig,
            self_vts,
            target,
            params,
            results,
        } => {
            let WTy::Func(sp, sr) = sig else {
                return unsupported("an adapter without a signature");
            };
            let mut a = Asm::new(sp.clone());
            unerase(&mut a, 0, self_vts);
            for (k, (j, p)) in (self_vts.len()..).zip(sp.iter().enumerate().skip(1)) {
                a.get(u32::try_from(j).expect("j"));
                if let Some(want) = params.get(k) {
                    a.conv(p, want);
                }
            }
            a.call((**target).clone());
            let tmp: Vec<u32> = results.iter().map(|v| a.local(v.clone())).collect();
            for l in tmp.iter().rev() {
                a.set(*l);
            }
            for (l, (have, want)) in tmp.iter().zip(results.iter().zip(sr)) {
                a.get(*l);
                a.conv(have, want);
            }
            a.finish(sr.clone())
        }
        Helper::Entry {
            main,
            providers,
            results,
        } => {
            let mut a = Asm::new(vec![]);
            for (vt, slots) in providers {
                a.ref_null_eq();
                for s in slots {
                    a.ref_func(Sym::Helper(s.clone()));
                }
                a.struct_new(vt);
            }
            a.call(Sym::Inst(*main));
            for _ in 0..*results {
                a.s().drop();
            }
            a.finish(vec![])
        }
    })
}

/// Writes a fixed text to standard error through the runtime import.
fn write_lit(a: &mut Asm, text: &str) {
    a.call(Sym::Helper(Helper::Lit(text.as_bytes().to_vec())));
    a.i64(i64::try_from(text.len()).unwrap_or(0) << 32);
    a.call(Sym::Helper(Helper::StrToBuf));
    a.call(stderr_import());
}

/// Unerases parameter `p` (an `eqref`) to `vts`: one reference is cast,
/// anything else is read out of its box.
pub fn unerase(a: &mut Asm, p: u32, vts: &[VT]) {
    match vts {
        [VT::Eq] => a.get(p),
        [v @ VT::Ref(..)] => {
            a.get(p);
            a.conv(&VT::Eq, v);
        }
        _ => {
            let b = crate::layout::box_of(vts);
            for k in 0..vts.len() {
                a.get(p);
                a.ref_cast(&b, false);
                a.struct_get(&b, u32::try_from(k).expect("k"));
            }
        }
    }
}
