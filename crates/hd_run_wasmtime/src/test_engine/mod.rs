//! Compiled test programs and pre-resolved host kinds. Workers own stores;
//! modules, import resolution, and the bounded pool are shared by clones.

pub(crate) mod engine;
pub(crate) mod runner;
#[cfg(test)]
mod tests;

pub use engine::{
    NamedProgram, PreparedTest, TestEngine, TestEnvironment, TestHostKind, TestModule,
    TestPoolConfig,
};
pub use runner::NativeBodyRunner;
