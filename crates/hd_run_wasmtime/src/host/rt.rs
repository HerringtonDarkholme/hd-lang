//! The runtime imports, which no host trait owns: `hd:rt`'s `block`,
//! `abort`, `stderr`, `stdout` and `panic`, and the `hd:prim` primitives,
//! whose text and float work is `prim.rs`.

use wasmtime::{Caller, Val};

use super::kind::{Prim, Rt};
use super::link::deadlock;
use super::reply::{Ret, complete, float_arg, handle, int_arg, text};
use super::{Control, Host, Stop};
use crate::prim;

/// An `hd:rt` import.
pub(super) fn rt(
    import: Rt,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match import {
        Rt::Stderr => {
            let t = text(caller, int_arg(params, 0)?)?;
            caller.data_mut().guest_err(&t);
            Ret::void()
        }
        Rt::Panic => {
            caller.data_mut().begin_panic();
            Ret::void()
        }
        // Held with `Console`'s output, so a later `stderr` write puts it
        // first.
        Rt::Stdout => {
            let t = text(caller, int_arg(params, 0)?)?;
            let host = caller.data_mut();
            host.out.extend_from_slice(t.as_bytes());
            host.flush_over_bound();
            Ret::void()
        }
        Rt::Block => loop {
            let handles = complete(caller)?;
            if !handles.is_empty() {
                return Ret::batch(&handles);
            }
            if !caller.data_mut().wait()? {
                return Err(deadlock(caller.data_mut()));
            }
        },
        Rt::Abort => {
            let h = int_arg(params, 0)?;
            caller.data_mut().control(Control::Abort(handle(h)));
            Ret::void()
        }
    })
}

/// An `hd:prim` import: answered from its arguments alone.
pub(super) fn prim(
    import: Prim,
    caller: &mut Caller<'_, Host>,
    params: &[Val],
) -> wasmtime::Result<Ret> {
    Ok(match import {
        Prim::FormatF64 => Ret::value(&prim::shortest_f64(float_arg(params, 0)?)),
        Prim::FormatF32 => {
            let s = match params.first() {
                Some(Val::F32(bits)) => prim::shortest_f32(f32::from_bits(*bits)),
                _ => return Err(Stop::Contract("format_f32 takes an f32".into()).into()),
            };
            Ret::value(&s)
        }
        Prim::FormatFixed => {
            let value = float_arg(params, 0)?;
            let places = u32::try_from(int_arg(params, 1)?)
                .map_err(|_| Stop::Internal("format_f64_fixed: digits out of range".into()))?;
            let s = prim::fixed(value, places);
            Ret::value(&s)
        }
        Prim::Lower | Prim::Upper => {
            let t = text(caller, int_arg(params, 0)?)?;
            let s = if matches!(import, Prim::Lower) {
                prim::lower(&t)
            } else {
                prim::upper(&t)
            };
            Ret::value(&s)
        }
        Prim::ParseF64 => {
            let t = text(caller, int_arg(params, 0)?)?;
            match prim::parse(&t) {
                Some(v) => Ret::float(v),
                None => {
                    return Err(Stop::Internal(format!(
                        "parse_f64: text outside the grammar: {t}"
                    ))
                    .into());
                }
            }
        }
    })
}
