use crate::host::Stop;

pub fn random(count: usize) -> wasmtime::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(count)
        .map_err(|_| Stop::HeapExhausted)?;
    bytes.resize(count, 0);
    getrandom::fill(&mut bytes).map_err(|e| Stop::Internal(e.to_string()))?;
    Ok(bytes)
}
