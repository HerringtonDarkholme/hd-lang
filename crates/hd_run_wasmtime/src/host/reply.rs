use wasmtime::{Caller, Val, ValType};

use super::Host;
use super::Stop;
use crate::codec::{Enc, Wire, encode};

/// Collects due handles without writing the guest's exchange buffer.
/// Reactor completion still advances operation state, holds completed
/// Console output and writes completed error lines; recording/replay of
/// those effects is a separate change.
pub fn complete(
    caller: &mut impl wasmtime::AsContextMut<Data = Host>,
) -> wasmtime::Result<Vec<u32>> {
    let mut cx = caller.as_context_mut();
    let host = cx.data_mut();
    host.check_deadline()?;
    let mut lines = Vec::new();
    let due = host.reactor.due(&mut lines);
    for (to, line) in &lines {
        host.console_line(*to, line);
    }
    host.flush_over_bound();
    Ok(due
        .into_iter()
        .map(|handle| u32::from_le_bytes(handle.to_le_bytes()))
        .collect())
}

/// Delivers precisely the supplied batch, shared by driver wake and block.
/// Collection must not preload memory: an embedding may supply a recorded
/// batch without polling live providers, or overwrite the buffer meanwhile.
pub(crate) fn deliver_batch(
    caller: &mut impl wasmtime::AsContextMut<Data = Host>,
    handles: &[u32],
) -> wasmtime::Result<i32> {
    let (count, bytes) = encode_batch(handles)?;
    if !handles.is_empty() {
        put_bytes(caller, &bytes)?;
    }
    Ok(count)
}

pub(crate) fn encode_batch(handles: &[u32]) -> wasmtime::Result<(i32, Vec<u8>)> {
    let count = i32::try_from(handles.len())
        .map_err(|_| Stop::Contract("the wake batch exceeds the i32 count range".into()))?;
    let bytes = handles
        .iter()
        .flat_map(|handle| handle.to_le_bytes())
        .collect();
    Ok((count, bytes))
}

/// Writes `bytes` at offset 0 of the exchange buffer, growing it to fit.
pub(crate) fn put_bytes(
    caller: &mut impl wasmtime::AsContextMut<Data = Host>,
    bytes: &[u8],
) -> wasmtime::Result<i64> {
    let mut cx = caller.as_context_mut();
    let Some(memory) = cx.data().memory else {
        return Err(Stop::Contract("the module has no exchange buffer".into()).into());
    };
    let size = memory.data_size(&cx);
    if bytes.len() > size {
        let pages = (bytes.len() - size).div_ceil(65536) as u64;
        if memory.grow(&mut cx, pages).is_err() {
            return Err(Stop::HeapExhausted.into());
        }
    }
    memory.data_mut(&mut cx)[..bytes.len()].copy_from_slice(bytes);
    Ok(i64::try_from(bytes.len()).unwrap_or(i64::MAX))
}

/// The exchange buffer's first `len` bytes: a host call's argument.
pub(crate) fn bytes(caller: &mut Caller<'_, Host>, len: i64) -> wasmtime::Result<Vec<u8>> {
    let Some(memory) = caller.data().memory else {
        return Err(Stop::Contract("the module has no exchange buffer".into()).into());
    };
    let data = memory.data(&*caller);
    usize::try_from(len)
        .ok()
        .and_then(|n| data.get(..n))
        .map(<[u8]>::to_vec)
        .ok_or_else(|| {
            Stop::Contract("an argument length is past the exchange buffer".into()).into()
        })
}

pub(crate) fn text(caller: &mut Caller<'_, Host>, len: i64) -> wasmtime::Result<String> {
    Ok(String::from_utf8_lossy(&bytes(caller, len)?).into_owned())
}

/// An import reply before the one dispatch-side exchange-buffer write.
pub(crate) struct Ret {
    pub(crate) value: Value,
    pub(crate) payload: Option<Vec<u8>>,
}

pub(crate) enum Value {
    Void,
    Int(i64),
    F64(f64),
    Pair(i32, i32),
}

impl Ret {
    pub(crate) fn scalar(value: Value) -> Self {
        Self {
            value,
            payload: None,
        }
    }
    pub(crate) fn void() -> Self {
        Self::scalar(Value::Void)
    }
    pub(crate) fn int(value: i64) -> Self {
        Self::scalar(Value::Int(value))
    }
    pub(crate) fn float(value: f64) -> Self {
        Self::scalar(Value::F64(value))
    }
    pub(crate) fn pair(status: i32, value: i32) -> Self {
        Self::scalar(Value::Pair(status, value))
    }
    pub(crate) fn bytes(bytes: Vec<u8>) -> Self {
        Self {
            value: Value::Int(i64::try_from(bytes.len()).unwrap_or(i64::MAX)),
            payload: Some(bytes),
        }
    }
    pub(crate) fn encoded(encoded: Enc) -> Self {
        Self::bytes(encoded.into_bytes())
    }
    /// A structured result: `value`, encoded.
    pub(crate) fn value<T: Wire>(value: &T) -> Self {
        Self::encoded(encode(value))
    }
    pub(crate) fn ready(encoded: Enc) -> wasmtime::Result<Self> {
        let bytes = encoded.into_bytes();
        let length = result_len(i64::try_from(bytes.len()).unwrap_or(i64::MAX))?;
        Ok(Self {
            value: Value::Pair(0, length),
            payload: Some(bytes),
        })
    }
    pub(crate) fn batch(handles: &[u32]) -> wasmtime::Result<Self> {
        let (count, bytes) = encode_batch(handles)?;
        Ok(Self {
            value: Value::Int(i64::from(count)),
            payload: (!handles.is_empty()).then_some(bytes),
        })
    }
}

pub(crate) fn int_arg(params: &[Val], i: usize) -> wasmtime::Result<i64> {
    match params.get(i) {
        Some(Val::I32(v)) => Ok(i64::from(*v)),
        Some(Val::I64(v)) => Ok(*v),
        _ => Err(Stop::Contract(format!("argument {i} is not an integer")).into()),
    }
}

pub(crate) fn float_arg(params: &[Val], i: usize) -> wasmtime::Result<f64> {
    match params.get(i) {
        Some(Val::F64(bits)) => Ok(f64::from_bits(*bits)),
        Some(Val::F32(bits)) => Ok(f64::from(f32::from_bits(*bits))),
        _ => Err(Stop::Contract(format!("argument {i} is not a float")).into()),
    }
}

/// The value of result type `ty` for the integer `v`.
pub(crate) fn int_val(ty: &ValType, v: i64) -> Val {
    match ty {
        ValType::I64 => Val::I64(v),
        // A narrower result keeps its low bits, as a JS number crossing to
        // an `i32` does.
        _ => Val::I32(i32::from_le_bytes(
            v.to_le_bytes()[..4].try_into().unwrap_or([0; 4]),
        )),
    }
}

fn result_len(len: i64) -> wasmtime::Result<i32> {
    i32::try_from(len).map_err(|_| Stop::HeapExhausted.into())
}

/// A handle argument; one that no `i32` holds names no operation.
pub(crate) fn handle(h: i64) -> i32 {
    i32::try_from(h).unwrap_or(-1)
}
