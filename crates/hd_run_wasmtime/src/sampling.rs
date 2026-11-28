//! Optional Wasmtime guest sampling, using the existing process epoch clock.

use hd_base::HashMap;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::engine::{Module, WasmtimeEngine};
use crate::host::Host;
use crate::runtime::epochs;

pub(crate) type Collector = Arc<Mutex<Option<wasmtime::GuestProfiler>>>;

pub(crate) struct Sampling {
    pub(crate) profiler: Collector,
    output: tempfile::NamedTempFile,
    destination: PathBuf,
    names: hd_run::frames::LineTable,
}

impl Sampling {
    pub(crate) fn start(
        engine: &WasmtimeEngine,
        module: &Module,
        name: &str,
        path: &Path,
        metadata: &[u8],
    ) -> Result<Self, String> {
        // cost: one explicitly requested output; samples visit only this
        // module's live stack, never accumulated runs or cached modules.
        // Publish by replacement: even an output naming the selected native
        // image never truncates bytes while Wasmtime has them mapped.
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let output = tempfile::Builder::new()
            .prefix(".hd-profile-")
            .tempfile_in(parent)
            .map_err(|error| format!("profile {}: {error}", path.display()))?;
        let profiler = wasmtime::GuestProfiler::new(
            &engine.engine,
            name,
            epochs::INTERVAL,
            [(name.to_owned(), module.0.as_ref().clone())],
        )
        .map_err(|error| format!("cannot start guest profiling: {error:#}"))?;
        let profiler = Arc::new(Mutex::new(Some(profiler)));
        let names = hd_run::frames::LineTable::parse(metadata)?;
        Ok(Self {
            profiler,
            output,
            destination: path.to_path_buf(),
            names,
        })
    }

    pub(crate) fn finish(self) -> Result<(), String> {
        let profiler = self
            .profiler
            .lock()
            .map_err(|_| "guest profiler lock poisoned".to_owned())?
            .take()
            .ok_or_else(|| "guest profiler was already finished".to_owned())?;
        let mut output = self.output;
        // Wasmtime's fallback labels use defined indices, excluding imports
        // (runtime/profiling.rs::module_symbols). Resolve those labels from
        // existing metadata only for this requested profile. Cost: linear
        // work in profile bytes and referenced names; artifacts stay unchanged.
        let mut bytes = Vec::new();
        profiler
            .finish(&mut bytes)
            .map_err(|error| format!("cannot write guest profile: {error:#}"))?;
        let mut profile: serde_json::Value = serde_json::from_slice(&bytes)
            .map_err(|error| format!("cannot read guest profile: {error}"))?;
        drop(bytes);
        symbolize(&mut profile, &self.names)?;
        {
            let mut writer = BufWriter::new(output.as_file_mut());
            serde_json::to_writer(&mut writer, &profile)
                .map_err(|error| format!("cannot write named guest profile: {error}"))?;
            writer
                .flush()
                .map_err(|error| format!("cannot flush guest profile: {error}"))?;
        }
        output
            .persist(&self.destination)
            .map(|_| ())
            .map_err(|error| format!("profile {}: {error}", self.destination.display()))
    }
}

/// Resolve function references while preserving the shared string entries
/// that module labels can also reference.
fn symbolize(
    profile: &mut serde_json::Value,
    symbols: &hd_run::frames::LineTable,
) -> Result<(), String> {
    let threads = profile["threads"]
        .as_array_mut()
        .ok_or("guest profile has no threads")?;
    for thread in threads {
        let serde_json::Value::Array(mut strings) = thread["stringArray"].take() else {
            return Err("guest profile has no string table".into());
        };
        let mut resolved = HashMap::<usize, usize>::default();
        for table in ["funcTable", "nativeSymbols"] {
            let entries = thread[table]["name"]
                .as_array_mut()
                .ok_or("guest profile has no function names")?;
            for entry in entries {
                let index = entry
                    .as_u64()
                    .and_then(|index| usize::try_from(index).ok())
                    .ok_or("invalid guest profile string index")?;
                if let Some(&replacement) = resolved.get(&index) {
                    *entry = serde_json::Value::from(replacement);
                    continue;
                }
                let label = strings
                    .get(index)
                    .and_then(serde_json::Value::as_str)
                    .ok_or("missing guest profile string")?;
                let defined = label
                    .strip_prefix("<wasm function ")
                    .and_then(|name| name.strip_suffix('>'))
                    .and_then(|index| index.parse::<u32>().ok());
                if let Some(name) = defined.and_then(|index| symbols.defined_function_name(index)) {
                    let replacement = strings.len();
                    strings.push(serde_json::Value::String(name.to_owned()));
                    resolved.insert(index, replacement);
                    *entry = serde_json::Value::from(replacement);
                }
            }
        }
        thread["stringArray"] = serde_json::Value::Array(strings);
    }
    Ok(())
}

/// Install sampling before instantiation, so a Wasm start function is covered.
/// The existing deadline/interrupt check stays authoritative on every tick.
pub(crate) fn attach(
    engine: &Arc<wasmtime::Engine>,
    store: &mut wasmtime::Store<Host>,
    samples: Collector,
) -> Result<(), String> {
    epochs::register(engine)?;
    store.epoch_deadline_callback(move |context| {
        let mut guard = samples
            .lock()
            .map_err(|_| wasmtime::Error::msg("guest profiler lock poisoned"))?;
        if let Some(profiler) = guard.as_mut() {
            profiler.sample(&context, Duration::ZERO);
        }
        drop(guard);
        context.data().check_deadline()?;
        Ok(wasmtime::UpdateDeadline::Continue(1))
    });
    store.set_epoch_deadline(1);
    Ok(())
}
