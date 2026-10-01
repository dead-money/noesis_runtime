//! The [`RenderDevice`] trait and the handle and descriptor types passed
//! through it.
//!
//! Implement [`RenderDevice`] to render Noesis with your own GPU backend, then
//! hand it to [`register`](crate::render_device::register). A C++ shim forwards
//! each call Noesis makes on its `RenderDevice` to your implementation.

use core::num::NonZeroU64;

use crate::render_device::types::{Batch, DeviceCaps, TextureFormat, Tile};

/// Your device's identifier for a texture. You choose the value in
/// [`RenderDevice::create_texture`]; Noesis passes it back on updates and on
/// [`RenderDevice::drop_texture`]. Zero is reserved.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureHandle(pub NonZeroU64);

/// Your device's identifier for a render target. You choose the value in
/// [`RenderDevice::create_render_target`] or
/// [`RenderDevice::clone_render_target`]; it is released by
/// [`RenderDevice::drop_render_target`]. Zero is reserved.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderTargetHandle(pub NonZeroU64);

/// A region of a texture mip level, in texels, passed to
/// [`RenderDevice::update_texture`]. `(x, y)` is the top-left corner.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Parameters for [`RenderDevice::create_texture`]. Borrowed data is valid only
/// during the call.
#[derive(Debug)]
pub struct TextureDesc<'a> {
    /// Debug label for GPU debugging tools.
    pub label: &'a str,
    /// Width of mip level 0, in texels.
    pub width: u32,
    /// Height of mip level 0, in texels.
    pub height: u32,
    /// Number of mip levels; `1` means no mipmaps.
    pub num_levels: u32,
    pub format: TextureFormat,
    /// Initial contents. `None` means a dynamic texture that Noesis fills
    /// later through [`RenderDevice::update_texture`]. `Some` holds one tightly
    /// packed slice per mip level, level 0 first.
    pub data: Option<&'a [&'a [u8]]>,
}

/// A created texture, returned from [`RenderDevice::create_texture`]. Noesis
/// caches these fields and does not query the device for them again.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct TextureBinding {
    pub handle: TextureHandle,
    pub width: u32,
    pub height: u32,
    pub has_mipmaps: bool,
    /// `true` when rows are stored bottom-to-top (the OpenGL convention).
    /// `false` for top-to-bottom APIs such as wgpu, Vulkan, Metal and D3D.
    pub inverted: bool,
    /// `false` if every texel is opaque, which lets Noesis skip blending. Use
    /// `true` when unsure.
    pub has_alpha: bool,
}

/// Parameters for [`RenderDevice::create_render_target`].
#[derive(Debug)]
pub struct RenderTargetDesc<'a> {
    /// Debug label for GPU debugging tools.
    pub label: &'a str,
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// MSAA sample count; `1` means no multisampling.
    pub sample_count: u32,
    /// Whether to attach a stencil buffer.
    pub needs_stencil: bool,
}

/// A created render target, returned from
/// [`RenderDevice::create_render_target`] and
/// [`RenderDevice::clone_render_target`]. `resolve_texture` describes the
/// texture Noesis samples from once the target is resolved.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct RenderTargetBinding {
    pub handle: RenderTargetHandle,
    pub resolve_texture: TextureBinding,
}

/// A GPU backend for Noesis, the Rust counterpart of `Noesis::RenderDevice`.
///
/// Noesis calls the methods in this order each frame:
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
/// Noesis calls every method from one thread, its render thread, so
/// implementations need no internal locking. The `Send + Sync` bounds let that
/// thread differ from the one that registered the device.
///
/// The [`Registered`] guard is `Send` but not `Sync`: its `&self` accessors,
/// such as [`Registered::offscreen_width`], read live Noesis state. Keep it on
/// the view's thread (in Bevy, a `NonSend` resource). See the crate-level
/// "Thread affinity" docs.
///
/// [`Registered`]: crate::render_device::Registered
/// [`Registered::offscreen_width`]: crate::render_device::Registered::offscreen_width
pub trait RenderDevice: Send + Sync + 'static {
    /// Lets [`Registered::device_mut`] downcast back to your concrete type.
    /// Every implementation is the same one line:
    ///
    /// ```ignore
    /// fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    /// ```
    ///
    /// [`Registered::device_mut`]: crate::render_device::Registered::device_mut
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Device capabilities. Queried once during setup.
    fn caps(&self) -> DeviceCaps;

    /// Creates a texture matching `desc`.
    fn create_texture(&mut self, desc: TextureDesc<'_>) -> TextureBinding;

    /// Copies `data` into `rect` of mip `level` of a dynamic texture. `data`
    /// is tightly packed: `rect.width * rect.height` texels in the texture's
    /// format, no row padding. Never called during a render pass.
    fn update_texture(&mut self, handle: TextureHandle, level: u32, rect: TextureRect, data: &[u8]);

    /// Ends a block of [`update_texture`](Self::update_texture) calls and lists
    /// the textures it modified, before any render pass uses them. Issue
    /// barriers or state transitions here.
    fn end_updating_textures(&mut self, textures: &[TextureHandle]);

    /// Releases a texture once Noesis drops its last reference to it.
    fn drop_texture(&mut self, handle: TextureHandle);

    /// Creates a render target matching `desc`.
    fn create_render_target(&mut self, desc: RenderTargetDesc<'_>) -> RenderTargetBinding;

    /// Creates a render target that shares the transient buffers (stencil and
    /// MSAA color) of `src`.
    fn clone_render_target(&mut self, label: &str, src: RenderTargetHandle) -> RenderTargetBinding;

    /// Releases a render target once Noesis drops its last reference to it.
    fn drop_render_target(&mut self, handle: RenderTargetHandle);

    /// Starts the offscreen phase, where Noesis renders into its own render
    /// targets.
    fn begin_offscreen_render(&mut self);
    /// Ends the offscreen phase.
    fn end_offscreen_render(&mut self);
    /// Starts the onscreen phase, which draws into whatever target the host
    /// has bound.
    fn begin_onscreen_render(&mut self);
    /// Ends the onscreen phase.
    fn end_onscreen_render(&mut self);

    /// Binds `handle` as the active render target with a viewport covering
    /// the whole surface. Existing contents may be discarded; do not clear.
    /// Drawing into it always happens between [`begin_tile`](Self::begin_tile)
    /// and [`end_tile`](Self::end_tile).
    fn set_render_target(&mut self, handle: RenderTargetHandle);

    /// Until [`end_tile`](Self::end_tile), draws only touch the region `tile`.
    /// A good place to enable scissoring.
    fn begin_tile(&mut self, handle: RenderTargetHandle, tile: Tile);

    /// Ends the tile started by [`begin_tile`](Self::begin_tile).
    fn end_tile(&mut self, handle: RenderTargetHandle);

    /// Resolves the listed `tiles` of a multisampled render target into its
    /// resolve texture. Only those regions need resolving. Discard the
    /// transient stencil and MSAA color buffers afterwards.
    fn resolve_render_target(&mut self, handle: RenderTargetHandle, tiles: &[Tile]);

    /// Returns a writable slice of at least `bytes` bytes of vertex storage,
    /// valid until [`unmap_vertices`]. `bytes` is at most 512 KiB. Called at
    /// least once per frame; GPU-mapped memory is the fast path.
    ///
    /// [`unmap_vertices`]: Self::unmap_vertices
    fn map_vertices(&mut self, bytes: u32) -> &mut [u8];
    /// Ends the write started by [`map_vertices`](Self::map_vertices).
    fn unmap_vertices(&mut self);

    /// Like [`map_vertices`](Self::map_vertices), for 16-bit indices. `bytes`
    /// is at most 128 KiB.
    fn map_indices(&mut self, bytes: u32) -> &mut [u8];
    /// Ends the write started by [`map_indices`](Self::map_indices).
    fn unmap_indices(&mut self);

    /// Draws the indexed triangles described by `batch`, reading from the most
    /// recently mapped vertex and index buffers. Recover the handles of its
    /// textures with [`Batch::pattern_handle`] and its siblings.
    fn draw_batch(&mut self, batch: &Batch);
}
