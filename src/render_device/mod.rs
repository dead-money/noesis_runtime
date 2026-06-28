//! Rust-side machinery for implementing `Noesis::RenderDevice`.
//!
//! Phase plan in `../../docs/PHASE_1_PLAN.md`. Layers, in dependency order:
//!
//! - [`types`] — `#[repr(C)]` mirrors of the public Noesis types in
//!   `Include/NsRender/RenderDevice.h`. ABI surface; layout-checked at
//!   compile time.
//! - [`device`] — the [`RenderDevice`] trait that Rust-side device impls
//!   satisfy, plus its handle / desc / binding plain-data types.
//! - [`ffi`] — Rust mirrors of the C ABI types in `cpp/noesis_shim.h`,
//!   plus `extern "C"` decls for the factory and helpers.
//! - [`vtable`] — `extern "C"` trampolines + the [`register`] entry point
//!   that owns the boxed impl and the C++ `RustRenderDevice` handle.

pub mod device;
pub mod ffi;
pub mod types;
pub mod vtable;

pub use device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect,
};
pub use vtable::{Registered, register};
