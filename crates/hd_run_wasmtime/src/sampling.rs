//! Optional Wasmtime guest sampling, using the existing process epoch clock.

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
}

impl Sampling {
    pub(crate) fn start(
        engine: &WasmtimeEngine,
        module: &Module,
        name: &str,
        path: &Path,
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
        Ok(Self {
            profiler,
            output,
            destination: path.to_path_buf(),
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
        {
            let mut writer = BufWriter::new(output.as_file_mut());
            profiler
                .finish(&mut writer)
                .map_err(|error| format!("cannot write guest profile: {error:#}"))?;
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
