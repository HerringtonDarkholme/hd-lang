//! Node as an `hd_run::Engine` (build-order.md §22: until wasmtime is an
//! approved dependency, `hd run` runs on V8 through Node). The JS host
//! (`compiler/host/run.mjs`) is embedded, so the binary is relocatable.

use std::io::Write as _;
use std::path::PathBuf;
use std::process::Command;
use std::task::Poll;

use hd_cache::CacheStore;
use hd_run::{Engine, HostSetup, Instance, LoadError, Outcome, StartError};

/// The JS host: the prelude imports, then the module's `main` export.
pub const HOST: &str = include_str!("../../../host/run.mjs");

pub struct NodeEngine;

pub struct NodeModule {
    wasm: Vec<u8>,
}

struct NodeInstance {
    wasm: Vec<u8>,
    done: Option<Outcome>,
}

impl Engine for NodeEngine {
    type Module = NodeModule;
    fn load(&self, wasm: &[u8], _cache: &dyn CacheStore) -> Result<NodeModule, LoadError> {
        if wasm.get(..4) != Some(b"\0asm") {
            return Err(LoadError::NotHd("not a Wasm module".into()));
        }
        Ok(NodeModule {
            wasm: wasm.to_vec(),
        })
    }
    fn instantiate(
        &self,
        m: &NodeModule,
        _host: &HostSetup,
    ) -> Result<Box<dyn Instance>, StartError> {
        Ok(Box::new(NodeInstance {
            wasm: m.wasm.clone(),
            done: None,
        }))
    }
}

fn scratch() -> PathBuf {
    std::env::temp_dir().join(format!("hd-run-{}", std::process::id()))
}

impl NodeInstance {
    /// Runs the whole program in one Node process; output goes to ours.
    fn run(&self) -> Outcome {
        let dir = scratch();
        if let Err(e) = std::fs::create_dir_all(&dir) {
            return Outcome::Internal(format!("{}: {e}", dir.display()));
        }
        let wasm = dir.join("main.wasm");
        let host = dir.join("host.mjs");
        if let Err(e) = std::fs::write(&wasm, &self.wasm).and_then(|()| std::fs::write(&host, HOST))
        {
            return Outcome::Internal(format!("{}: {e}", dir.display()));
        }
        let out = Command::new("node").arg(&host).arg(&wasm).output();
        let _ = std::fs::remove_dir_all(&dir);
        match out {
            Ok(o) => {
                let _ = std::io::stdout().write_all(&o.stdout);
                if o.status.success() {
                    Outcome::Exit(0)
                } else {
                    let _ = std::io::stderr().write_all(&o.stderr);
                    Outcome::Exit(
                        u8::try_from(o.status.code().unwrap_or(1).clamp(1, 255)).unwrap_or(1),
                    )
                }
            }
            Err(e) => Outcome::Internal(format!("node: {e}")),
        }
    }
}

impl Instance for NodeInstance {
    fn init(&mut self) -> Outcome {
        let o = self.run();
        let r = o.clone();
        self.done = Some(o);
        r
    }
    fn poll(&mut self) -> Poll<Outcome> {
        Poll::Ready(self.done.clone().unwrap_or(Outcome::Exit(0)))
    }
    fn wake(&mut self, _: &[u32]) {}
    fn call_test(&mut self, _: u32) -> Poll<Outcome> {
        Poll::Ready(Outcome::Internal(
            "tests do not run on the Node engine yet".into(),
        ))
    }
    fn completed(&mut self) -> Vec<u32> {
        Vec::new()
    }
}
