//! Render-device FFI surface: Rust mirrors of the C ABI types declared in
//! `cpp/noesis_shim.h`, plus extern decls for the factory and helpers.
//!
//! Lifecycle FFI lives at the top of the crate in `crate::ffi`; this module
//! is render-device-specific.

use core::mem::{align_of, size_of};
use std::os::raw::{c_char, c_void};

use crate::render_device::types::DeviceCaps;

/// Mirror of `noesis_texture_binding`. `handle == 0` is reserved invalid;
/// the trampoline panics on zero on the way back up to a Rust [`TextureHandle`].
///
/// [`TextureHandle`]: crate::render_device::TextureHandle
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct TextureBindingFfi {
    pub handle: u64,
    pub width: u32,
    pub height: u32,
    pub has_mipmaps: bool,
    pub inverted: bool,
    pub has_alpha: bool,
    pub pad: u8,
}

/// Mirror of `noesis_render_target_binding`.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default)]
pub struct RenderTargetBindingFfi {
    pub handle: u64,
    pub resolve_texture: TextureBindingFfi,
}

const _: () = assert!(size_of::<TextureBindingFfi>() == 24);
const _: () = assert!(align_of::<TextureBindingFfi>() == 8);
const _: () = assert!(size_of::<RenderTargetBindingFfi>() == 32);
const _: () = assert!(align_of::<RenderTargetBindingFfi>() == 8);

/// Mirror of `noesis_render_device_vtable`. Every function pointer is
/// `unsafe extern "C"`: the trampolines dereference a raw `userdata` and trust
/// Noesis to honor the frame protocol (Map/Unmap calls never nest, etc.).
///
/// The C struct's `void*` parameters are typed here so trampolines can cast
/// directly: `out_caps` is `*mut DeviceCaps`, `tile`/`tiles` are `*const Tile`,
/// and `batch` is `*const Batch`.
#[repr(C)]
pub struct RenderDeviceVTable {
    pub get_caps: unsafe extern "C" fn(userdata: *mut c_void, out_caps: *mut DeviceCaps),

    pub create_texture: unsafe extern "C" fn(
        userdata: *mut c_void,
        label: *const c_char,
        width: u32,
        height: u32,
        num_levels: u32,
        format: u32,
        data: *const *const c_void,
        out: *mut TextureBindingFfi,
    ),
    pub update_texture: unsafe extern "C" fn(
        userdata: *mut c_void,
        handle: u64,
        level: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        format: u32,
        data: *const c_void,
    ),
    pub end_updating_textures:
        unsafe extern "C" fn(userdata: *mut c_void, handles: *const u64, count: u32),
    pub drop_texture: unsafe extern "C" fn(userdata: *mut c_void, handle: u64),

    pub create_render_target: unsafe extern "C" fn(
        userdata: *mut c_void,
        label: *const c_char,
        width: u32,
        height: u32,
        sample_count: u32,
        needs_stencil: bool,
        out: *mut RenderTargetBindingFfi,
    ),
    pub clone_render_target: unsafe extern "C" fn(
        userdata: *mut c_void,
        label: *const c_char,
        src_handle: u64,
        out: *mut RenderTargetBindingFfi,
    ),
    pub drop_render_target: unsafe extern "C" fn(userdata: *mut c_void, handle: u64),

    pub begin_offscreen_render: unsafe extern "C" fn(userdata: *mut c_void),
    pub end_offscreen_render: unsafe extern "C" fn(userdata: *mut c_void),
    pub begin_onscreen_render: unsafe extern "C" fn(userdata: *mut c_void),
    pub end_onscreen_render: unsafe extern "C" fn(userdata: *mut c_void),

    pub set_render_target: unsafe extern "C" fn(userdata: *mut c_void, handle: u64),
    pub begin_tile: unsafe extern "C" fn(
        userdata: *mut c_void,
        handle: u64,
        tile: *const crate::render_device::types::Tile,
    ),
    pub end_tile: unsafe extern "C" fn(userdata: *mut c_void, handle: u64),
    pub resolve_render_target: unsafe extern "C" fn(
        userdata: *mut c_void,
        handle: u64,
        tiles: *const crate::render_device::types::Tile,
        count: u32,
    ),

    pub map_vertices: unsafe extern "C" fn(userdata: *mut c_void, bytes: u32) -> *mut c_void,
    pub unmap_vertices: unsafe extern "C" fn(userdata: *mut c_void),
    pub map_indices: unsafe extern "C" fn(userdata: *mut c_void, bytes: u32) -> *mut c_void,
    pub unmap_indices: unsafe extern "C" fn(userdata: *mut c_void),

    pub draw_batch: unsafe extern "C" fn(
        userdata: *mut c_void,
        batch: *const crate::render_device::types::Batch,
    ),
}

// Implemented in noesis_render_device.cpp.
unsafe extern "C" {
    /// Create a `RustRenderDevice` with refcount = 1. Returns
    /// `Noesis::RenderDevice*` cast to `*mut c_void`. Pair with
    /// [`noesis_render_device_destroy`] exactly once.
    pub fn noesis_render_device_create(
        vtable: *const RenderDeviceVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;

    /// Release a device from [`noesis_render_device_create`], dropping its +1
    /// refcount. Call exactly once.
    pub fn noesis_render_device_destroy(device: *mut c_void);

    /// Read the `u64` binding handle out of a `Noesis::Texture*`.
    pub fn noesis_texture_get_handle(texture: *const c_void) -> u64;
    /// Read the `u64` binding handle out of a `Noesis::RenderTarget*`.
    pub fn noesis_render_target_get_handle(surface: *const c_void) -> u64;

    // Resource sizing on the `Noesis::RenderDevice` base; set before the first
    // frame. Offscreen 0 == automatic; glyph cache defaults to 1024×1024.
    pub fn noesis_render_device_set_offscreen_width(device: *mut c_void, width: u32);
    pub fn noesis_render_device_set_offscreen_height(device: *mut c_void, height: u32);
    pub fn noesis_render_device_set_offscreen_sample_count(device: *mut c_void, count: u32);
    pub fn noesis_render_device_set_offscreen_default_num_surfaces(device: *mut c_void, num: u32);
    pub fn noesis_render_device_set_offscreen_max_num_surfaces(device: *mut c_void, num: u32);
    pub fn noesis_render_device_set_glyph_cache_width(device: *mut c_void, width: u32);
    pub fn noesis_render_device_set_glyph_cache_height(device: *mut c_void, height: u32);

    pub fn noesis_render_device_get_offscreen_width(device: *const c_void) -> u32;
    pub fn noesis_render_device_get_offscreen_height(device: *const c_void) -> u32;
    pub fn noesis_render_device_get_offscreen_sample_count(device: *const c_void) -> u32;
    pub fn noesis_render_device_get_offscreen_default_num_surfaces(device: *const c_void) -> u32;
    pub fn noesis_render_device_get_offscreen_max_num_surfaces(device: *const c_void) -> u32;
    pub fn noesis_render_device_get_glyph_cache_width(device: *const c_void) -> u32;
    pub fn noesis_render_device_get_glyph_cache_height(device: *const c_void) -> u32;
}

// Gated by the `test-utils` Cargo feature, which defines `NOESIS_TEST_UTILS`
// for the C++ build.
#[cfg(feature = "test-utils")]
unsafe extern "C" {
    /// Drive the C++ device through one representative frame (caps query,
    /// texture create + update, render target create, offscreen + onscreen
    /// passes with map/draw/unmap, RT clone) then let every `Ptr<>` die so
    /// `drop_texture` / `drop_render_target` fire on the way out. Used by
    /// `tests/render_device.rs` to assert the recorded op sequence.
    pub fn noesis_test_run_frame_scenario(device: *mut c_void);
}
