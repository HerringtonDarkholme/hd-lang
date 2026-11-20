//! `hd_resolve`'s integration tests as one binary. Each separate test binary is a
//! full debug link of the compiler, so one per crate is the cheap layout.

mod const_generics;
mod iface_codec;
mod known_items;
mod show_ty;
