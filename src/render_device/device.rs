//! The [`RenderDevice`] trait that Rust-side device implementations satisfy,
//! plus the handle / desc / binding plain-data types that flow through it.
//!
//! Implement [`RenderDevice`] to back a Noesis view with your own GPU backend
//! (a `wgpu` device, say). This layer is pure Rust — no FFI. A C++ shim
//! translates Noesis's pure-virtual `RenderDevice` surface into calls on this
//! trait through a vtable of `extern "C"` trampolines.

use core::num::NonZeroU64;

use crate::render_device::types::{Batch, DeviceCaps, TextureFormat, Tile};

// ────────────────────────────────────────────────────────────────────────────
// Handles — opaque IDs the device owns. NonZeroU64 lets `Option<Handle>` reuse
// the handle's niche.
// ────────────────────────────────────────────────────────────────────────────

/// Opaque identifier for a Rust-owned texture resource (e.g. a `wgpu::Texture`
/// in the eventual Bevy impl). Allocated by [`RenderDevice::create_texture`];
/// released by [`RenderDevice::drop_texture`] when the C++ `RustTexture`
/// wrapper is destroyed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureHandle(pub NonZeroU64);

/// Opaque identifier for a Rust-owned render-target resource. Allocated by
/// [`RenderDevice::create_render_target`] / [`RenderDevice::clone_render_target`];
/// released by [`RenderDevice::drop_render_target`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderTargetHandle(pub NonZeroU64);

/// Sub-rectangle of a texture mip level; used by
/// [`RenderDevice::update_texture`].
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

// ────────────────────────────────────────────────────────────────────────────
// Texture creation
// ────────────────────────────────────────────────────────────────────────────

/// Inputs to [`RenderDevice::create_texture`]. Borrowed for the duration of
/// the call only.
#[derive(Debug)]
pub struct TextureDesc<'a> {
    /// Debug label; passed straight through to GPU debug-marker APIs.
    pub label: &'a str,
    pub width: u32,
    pub height: u32,
    /// Mip level count. `1` = no mipmaps.
    pub num_levels: u32,
    pub format: TextureFormat,
    /// Initial contents. `None` marks a dynamic texture (subsequent
    /// [`RenderDevice::update_texture`] calls fill it). `Some` requires
    /// exactly `num_levels` tightly-packed byte slices, ordered mip-0 first.
    pub data: Option<&'a [&'a [u8]]>,
}

/// Returned from [`RenderDevice::create_texture`]. The C++ `RustTexture`
/// wrapper stores the metadata and delegates `GetWidth` / `GetHeight` /
/// `HasMipMaps` / `IsInverted` / `HasAlpha` to it without further round-trips.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureBinding {
    pub handle: TextureHandle,
    pub width: u32,
    pub height: u32,
    pub has_mipmaps: bool,
    /// `true` when the texture's V coordinate runs bottom-to-top (the GL
    /// convention). wgpu textures are top-to-bottom → set to `false`.
    pub inverted: bool,
    /// Hint for the renderer: `false` means alpha is uniformly opaque and
    /// blending can be skipped. Conservative choice is `true`.
    pub has_alpha: bool,
}

// ────────────────────────────────────────────────────────────────────────────
// Render-target creation
// ────────────────────────────────────────────────────────────────────────────

/// Inputs to [`RenderDevice::create_render_target`].
#[derive(Debug)]
pub struct RenderTargetDesc<'a> {
    pub label: &'a str,
    pub width: u32,
    pub height: u32,
    /// MSAA sample count. `1` = no multisampling.
    pub sample_count: u32,
    /// Whether Noesis needs a stencil buffer attached (always true when
    /// rendering paths with masks).
    pub needs_stencil: bool,
}

/// Returned from [`RenderDevice::create_render_target`] and
/// [`RenderDevice::clone_render_target`].
///
/// `resolve_texture.handle` may be the same as a freshly-created texture's
/// handle (cloned RTs may share the underlying resolve resource); it is the
/// Rust impl's choice. The C++ `RustRenderTarget::GetTexture` returns the
/// `RustTexture` instance built from this binding.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderTargetBinding {
    pub handle: RenderTargetHandle,
    pub resolve_texture: TextureBinding,
}

/// Rust-side implementation of `Noesis::RenderDevice`.
///
/// Method order mirrors the frame protocol documented at the top of
/// `NsRender/RenderDevice.h`:
///
/// ```text
/// // Per-frame (texture phase, before any rendering):
/// for each dirty dynamic texture:
///     update_texture()
/// end_updating_textures()
///
/// // Offscreen phase (when needed):
/// begin_offscreen_render()
///     for each render target:
///         set_render_target()
///         for each tile:
///             begin_tile()
///                 map_vertices() / map_indices()
///                 draw_batch() ...
///             end_tile()
///         resolve_render_target()
/// end_offscreen_render()
///
/// // Onscreen phase:
/// begin_onscreen_render()
///     map_vertices() / map_indices()
///     draw_batch() ...
/// end_onscreen_render()
/// ```
///
/// Noesis calls every method on a single thread (the render thread). `&mut`
/// receivers reflect that; impls do not need internal locking.
///
/// The `Send + Sync` supertrait bounds make the boxed impl behind the
/// [`Registered`] guard `Send` (so the guard can be *moved* to the thread
/// that owns the Noesis view) and let Noesis invoke the device trampolines
/// from its render thread. The guard itself is `Send` but **not** `Sync` —
/// it exposes `&self` accessors (e.g. [`Registered::offscreen_width`]) that
/// read live Noesis state, so it must not be shared across threads. See the
/// crate-level "Thread affinity" docs; store it in a `NonSend` resource, not
/// a plain `Send + Sync` Bevy `Resource`.
///
/// [`Registered`]: crate::render_device::Registered
/// [`Registered::offscreen_width`]: crate::render_device::Registered::offscreen_width
pub trait RenderDevice: Send + Sync + 'static {
    /// Downcast escape hatch used by [`Registered::device_mut`] so callers
    /// can reach back into their concrete impl after registration. Standard
    /// one-line body for every impl:
    ///
    /// ```ignore
    /// fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    /// ```
    ///
    /// [`Registered::device_mut`]: crate::render_device::Registered::device_mut
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Static device capabilities. Queried once early in setup, so returning a
    /// cached value is fine.
    fn caps(&self) -> DeviceCaps;

    // ─── Texture lifecycle ────────────────────────────────────────────────

    /// Allocate a texture matching `desc`. The returned binding's metadata
    /// is exposed verbatim through the C++ `RustTexture` wrapper to Noesis.
    fn create_texture(&mut self, desc: TextureDesc<'_>) -> TextureBinding;

    /// Replace a region of a dynamic texture mip level. `data` is tightly
    /// packed (no extra pitch); its length matches `rect.width * rect.height *
    /// bytes_per_pixel(format)`.
    fn update_texture(&mut self, handle: TextureHandle, level: u32, rect: TextureRect, data: &[u8]);

    /// Called after a batch of [`update_texture`](Self::update_texture) calls,
    /// before any rendering uses the affected textures. The impl can issue
    /// barriers / flushes / state transitions here.
    fn end_updating_textures(&mut self, textures: &[TextureHandle]);

    /// Release the resource the C++ `RustTexture` wrapper held. Called from
    /// the wrapper's destructor when Noesis releases its `Ptr<Texture>`.
    fn drop_texture(&mut self, handle: TextureHandle);

    // ─── Render-target lifecycle ──────────────────────────────────────────

    /// Allocate a render target matching `desc`. The returned binding carries
    /// the resolve texture's own binding alongside the render-target handle.
    fn create_render_target(&mut self, desc: RenderTargetDesc<'_>) -> RenderTargetBinding;

    /// Create a render target that reuses transient buffers (stencil, MSAA
    /// color) of `src`. Useful for ping-pong post-processing chains.
    fn clone_render_target(&mut self, label: &str, src: RenderTargetHandle) -> RenderTargetBinding;

    /// Release the resource the C++ `RustRenderTarget` wrapper held.
    fn drop_render_target(&mut self, handle: RenderTargetHandle);

    // ─── Frame phases ─────────────────────────────────────────────────────

    /// Open the offscreen phase, in which Noesis renders to its own render
    /// targets before compositing onto the screen.
    fn begin_offscreen_render(&mut self);
    /// Close the offscreen phase.
    fn end_offscreen_render(&mut self);
    /// Open the onscreen phase, in which Noesis renders to the back buffer.
    fn begin_onscreen_render(&mut self);
    /// Close the onscreen phase.
    fn end_onscreen_render(&mut self);

    // ─── Render-target binding & tile sub-passes ──────────────────────────

    /// Bind `handle` as the active render target, set viewport to cover the
    /// surface, and discard any existing contents (do NOT clear).
    fn set_render_target(&mut self, handle: RenderTargetHandle);

    /// Begin a sub-pass restricted to `tile`. Until [`end_tile`](Self::end_tile),
    /// all draws affect only that region. Good place to enable scissor.
    fn begin_tile(&mut self, handle: RenderTargetHandle, tile: Tile);

    /// End the sub-pass opened by [`begin_tile`](Self::begin_tile).
    fn end_tile(&mut self, handle: RenderTargetHandle);

    /// Resolve the listed `tiles` of an MSAA render target into its resolve
    /// texture; transient stencil/color buffers may be discarded after.
    fn resolve_render_target(&mut self, handle: RenderTargetHandle, tiles: &[Tile]);

    // ─── Streaming geometry buffers ───────────────────────────────────────

    /// Reserve `bytes` of vertex storage and return a writable slice.
    /// `bytes` never exceeds Noesis's 512 KiB dynamic vertex-buffer cap. Each
    /// frame issues at least one pair of map / unmap. The slice must remain
    /// valid until [`unmap_vertices`].
    ///
    /// [`unmap_vertices`]: Self::unmap_vertices
    fn map_vertices(&mut self, bytes: u32) -> &mut [u8];
    /// Finish the write started by [`map_vertices`](Self::map_vertices) and
    /// release the slice back to the device.
    fn unmap_vertices(&mut self);

    /// Same as [`map_vertices`](Self::map_vertices), but for 16-bit indices.
    /// `bytes` never exceeds the 128 KiB dynamic index-buffer cap.
    fn map_indices(&mut self, bytes: u32) -> &mut [u8];
    /// Finish the write started by [`map_indices`](Self::map_indices) and
    /// release the slice back to the device.
    fn unmap_indices(&mut self);

    /// Draw the indexed-triangle batch described by `batch`. The vertex /
    /// index data lives in the most recently mapped buffers; texture pointers
    /// reference `RustTexture` instances allocated through
    /// [`create_texture`](Self::create_texture).
    fn draw_batch(&mut self, batch: &Batch);
}
