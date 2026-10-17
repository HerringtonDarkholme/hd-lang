//! One process-wide epoch ticker. Wall-clock checks remain authoritative:
//! a tick may arrive before a store's requested duration has elapsed.

use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};
use std::time::Duration;

type Engines = Arc<Mutex<Vec<Weak<wasmtime::Engine>>>>;

pub fn register(engine: &Arc<wasmtime::Engine>) -> Result<(), String> {
    static ENGINES: OnceLock<Result<Engines, String>> = OnceLock::new();
    let engines = ENGINES.get_or_init(|| {
        let engines: Engines = Arc::default();
        let ticking = Arc::clone(&engines);
        std::thread::Builder::new()
            .name("hd-epochs".into())
            .spawn(move || {
                loop {
                    std::thread::sleep(Duration::from_millis(10));
                    ticking
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .retain(|weak| match weak.upgrade() {
                            Some(engine) => {
                                engine.increment_epoch();
                                true
                            }
                            None => false,
                        });
                }
            })
            .map_err(|error| format!("the epoch ticker cannot start: {error}"))?;
        Ok(engines)
    });
    let engines = engines.as_ref().map_err(Clone::clone)?;
    let mut engines = engines.lock().unwrap_or_else(PoisonError::into_inner);
    let weak = Arc::downgrade(engine);
    if !engines.iter().any(|registered| registered.ptr_eq(&weak)) {
        engines.push(weak);
    }
    Ok(())
}
