//! Render Noesis through your own GPU backend.
//!
//! Implement [`RenderDevice`] for your graphics API, pass it to [`register`],
//! and keep the returned [`Registered`] guard alive while any view renders with
//! it. Bind it to a view with
//! [`Renderer::init`](crate::view::Renderer::init). Noesis then drives the
//! trait's frame protocol: texture uploads, offscreen render-target passes, and
//! onscreen batches.
//!
//! - [`device`]: the [`RenderDevice`] trait and the handle and descriptor types
//!   passed through it.
//! - [`types`]: the data Noesis hands the device (batches, shader and render
//!   state, tiles, device caps), laid out to match Noesis's
//!   `NsRender/RenderDevice.h`.
//!
//! [`register`], [`Registered`] and the `Batch::*_handle` methods need the
//! `shim` feature. Without it, this module is the whole crate: the types and
//! the trait, for a device that a different Noesis host drives. Such a host
//! creates the textures itself, so it maps a batch's texture pointers back to
//! handles on its own.

pub mod device;
// Not part of the stable API; no semver guarantees.
#[cfg(feature = "shim")]
#[doc(hidden)]
pub mod ffi;
pub mod types;
// Not part of the stable API; no semver guarantees.
#[cfg(feature = "shim")]
#[doc(hidden)]
pub mod vtable;

pub use device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect,
};
#[cfg(feature = "shim")]
pub use vtable::{Registered, register};
