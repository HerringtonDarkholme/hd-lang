//! `hd_web`'s integration tests as one binary. Each separate test binary is a
//! full debug link of the compiler, so one per crate is the cheap layout.

mod compile;
mod repl;
mod thin_driver;
