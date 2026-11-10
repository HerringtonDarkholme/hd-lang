//! `hd_fmt`'s integration tests as one binary. Each separate test binary is a
//! full debug link of the compiler, so one per crate is the cheap layout.

mod performance;
mod review;
mod std_projection;
mod width;
