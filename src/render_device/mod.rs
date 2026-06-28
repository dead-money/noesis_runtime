//! Implement a custom GPU backend for Noesis by writing a Rust `Noesis::RenderDevice`.
//!
//! Reach for this module when you want Noesis to render through your own
//! graphics API instead of a bundled backend: implement the [`RenderDevice`]
//! trait, then hand it to [`register`] to wire it up across the C ABI.
//!
//! The pieces, in dependency order:
//!
//! - [`types`]: `#[repr(C)]` mirrors of the public Noesis types in
//!   `Include/NsRender/RenderDevice.h`. These are the ABI surface and their
//!   layouts are checked against the C++ headers at compile time.
//! - [`device`]: the [`RenderDevice`] trait your device impl satisfies, plus
//!   its handle / desc / binding plain-data types.
//! - [`ffi`]: Rust mirrors of the C ABI types in `cpp/noesis_shim.h`, plus the
//!   `extern "C"` declarations for the factory and helpers.
//! - [`vtable`]: the `extern "C"` trampolines and the [`register`] entry point
//!   that owns the boxed impl and the C++ `RustRenderDevice` handle.

pub mod device;
// Not part of the stable API; no semver guarantees.
#[doc(hidden)]
pub mod ffi;
pub mod types;
// Not part of the stable API; no semver guarantees.
#[doc(hidden)]
pub mod vtable;

pub use device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect,
};
pub use vtable::{Registered, register};
