//! `extern "C"` trampolines that bridge the C++ `RustRenderDevice` subclass
//! to a Rust-side [`RenderDevice`] trait object, plus the [`register`] entry
//! point that owns the boxed impl and the C++ device handle.
//!
//! Userdata convention: every trampoline receives a `*mut c_void` whose
//! actual type is `*mut Box<dyn RenderDevice>`. The double-`Box` gives us a
//! stable thin pointer (the inner `Box<dyn …>` is a fat pointer).

#![allow(unsafe_op_in_unsafe_fn)] // FFI sea-of-unsafe — explicit blocks add noise.

use core::num::NonZeroU64;
use core::ptr::NonNull;
use core::slice;
use std::borrow::Cow;
use std::ffi::CStr;
use std::os::raw::{c_char, c_void};

use crate::render_device::device::{
    RenderDevice, RenderTargetDesc, RenderTargetHandle, TextureDesc, TextureHandle, TextureRect,
};
use crate::render_device::ffi::{
    RenderDeviceVTable, RenderTargetBindingFfi, TextureBindingFfi, dm_noesis_render_device_create,
    dm_noesis_render_device_destroy, dm_noesis_render_device_get_glyph_cache_height,
    dm_noesis_render_device_get_glyph_cache_width,
    dm_noesis_render_device_get_offscreen_default_num_surfaces,
    dm_noesis_render_device_get_offscreen_height,
    dm_noesis_render_device_get_offscreen_max_num_surfaces,
    dm_noesis_render_device_get_offscreen_sample_count,
    dm_noesis_render_device_get_offscreen_width, dm_noesis_render_device_set_glyph_cache_height,
    dm_noesis_render_device_set_glyph_cache_width,
    dm_noesis_render_device_set_offscreen_default_num_surfaces,
    dm_noesis_render_device_set_offscreen_height,
    dm_noesis_render_device_set_offscreen_max_num_surfaces,
    dm_noesis_render_device_set_offscreen_sample_count,
    dm_noesis_render_device_set_offscreen_width,
};
use crate::render_device::types::{Batch, DeviceCaps, TextureFormat, Tile};

// ────────────────────────────────────────────────────────────────────────────
// Helpers
// ────────────────────────────────────────────────────────────────────────────

/// Decode a C `*const c_char` into a string. Empty on null. Noesis labels are
/// ASCII debug strings; decode lossily so odd input can't panic across the C
/// ABI (this runs on the render thread).
unsafe fn cstr_to_str<'a>(p: *const c_char) -> Cow<'a, str> {
    if p.is_null() {
        Cow::Borrowed("")
    } else {
        CStr::from_ptr(p).to_string_lossy()
    }
}

fn texture_format_from_raw(raw: u32) -> TextureFormat {
    match raw {
        0 => TextureFormat::Rgba8,
        1 => TextureFormat::Rgbx8,
        2 => TextureFormat::R8,
        other => panic!("unknown TextureFormat::Enum from Noesis: {other}"),
    }
}

const fn bytes_per_pixel(format: TextureFormat) -> u32 {
    match format {
        TextureFormat::Rgba8 | TextureFormat::Rgbx8 => 4,
        TextureFormat::R8 => 1,
    }
}

fn level_byte_count(format: TextureFormat, base_w: u32, base_h: u32, level: u32) -> usize {
    let w = (base_w >> level).max(1) as usize;
    let h = (base_h >> level).max(1) as usize;
    w * h * bytes_per_pixel(format) as usize
}

fn texture_handle(raw: u64) -> TextureHandle {
    TextureHandle(NonZeroU64::new(raw).expect("RenderDevice impl returned a zero TextureHandle"))
}

fn render_target_handle(raw: u64) -> RenderTargetHandle {
    RenderTargetHandle(
        NonZeroU64::new(raw).expect("RenderDevice impl returned a zero RenderTargetHandle"),
    )
}

/// SAFETY: the caller must guarantee `userdata` was produced by `register()`
/// and is still alive (i.e. the [`Registered`] guard hasn't been dropped).
unsafe fn device<'a>(userdata: *mut c_void) -> &'a mut Box<dyn RenderDevice> {
    &mut *userdata.cast::<Box<dyn RenderDevice>>()
}

// ────────────────────────────────────────────────────────────────────────────
// Trampolines
// ────────────────────────────────────────────────────────────────────────────

unsafe extern "C" fn t_get_caps(userdata: *mut c_void, out: *mut DeviceCaps) {
    crate::panic_guard::guard(|| {
        let caps = device(userdata).caps();
        out.write(caps);
    })
}

unsafe extern "C" fn t_create_texture(
    userdata: *mut c_void,
    label: *const c_char,
    width: u32,
    height: u32,
    num_levels: u32,
    format_raw: u32,
    data: *const *const c_void,
    out: *mut TextureBindingFfi,
) {
    crate::panic_guard::guard(|| {
        let dev = device(userdata);
        let label = cstr_to_str(label);
        let format = texture_format_from_raw(format_raw);

        // Build the per-level slice array; lifetime ends with this fn body.
        let slices: Vec<&[u8]> = if data.is_null() {
            Vec::new()
        } else {
            let ptrs = slice::from_raw_parts(data, num_levels as usize);
            ptrs.iter()
                .enumerate()
                .map(|(lvl, &p)| {
                    let len = level_byte_count(format, width, height, lvl as u32);
                    slice::from_raw_parts(p.cast::<u8>(), len)
                })
                .collect()
        };

        let desc = TextureDesc {
            label: &label,
            width,
            height,
            num_levels,
            format,
            data: if data.is_null() {
                None
            } else {
                Some(slices.as_slice())
            },
        };
        let binding = dev.create_texture(desc);
        out.write(TextureBindingFfi {
            handle: binding.handle.0.get(),
            width: binding.width,
            height: binding.height,
            has_mipmaps: binding.has_mipmaps,
            inverted: binding.inverted,
            has_alpha: binding.has_alpha,
            pad: 0,
        });
    })
}

unsafe extern "C" fn t_update_texture(
    userdata: *mut c_void,
    handle: u64,
    level: u32,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    format_raw: u32,
    data: *const c_void,
) {
    crate::panic_guard::guard(|| {
        let dev = device(userdata);
        let format = texture_format_from_raw(format_raw);
        let len = (width as usize) * (height as usize) * bytes_per_pixel(format) as usize;
        let bytes = slice::from_raw_parts(data.cast::<u8>(), len);
        dev.update_texture(
            texture_handle(handle),
            level,
            TextureRect {
                x,
                y,
                width,
                height,
            },
            bytes,
        );
    })
}

unsafe extern "C" fn t_end_updating_textures(
    userdata: *mut c_void,
    handles: *const u64,
    count: u32,
) {
    crate::panic_guard::guard(|| {
        let dev = device(userdata);
        let raws = slice::from_raw_parts(handles, count as usize);
        let typed: Vec<TextureHandle> = raws.iter().copied().map(texture_handle).collect();
        dev.end_updating_textures(&typed);
    })
}

unsafe extern "C" fn t_drop_texture(userdata: *mut c_void, handle: u64) {
    crate::panic_guard::guard(|| {
        device(userdata).drop_texture(texture_handle(handle));
    })
}

unsafe extern "C" fn t_create_render_target(
    userdata: *mut c_void,
    label: *const c_char,
    width: u32,
    height: u32,
    sample_count: u32,
    needs_stencil: bool,
    out: *mut RenderTargetBindingFfi,
) {
    crate::panic_guard::guard(|| {
        let dev = device(userdata);
        let label = cstr_to_str(label);
        let desc = RenderTargetDesc {
            label: &label,
            width,
            height,
            sample_count,
            needs_stencil,
        };
        let binding = dev.create_render_target(desc);
        out.write(RenderTargetBindingFfi {
            handle: binding.handle.0.get(),
            resolve_texture: TextureBindingFfi {
                handle: binding.resolve_texture.handle.0.get(),
                width: binding.resolve_texture.width,
                height: binding.resolve_texture.height,
                has_mipmaps: binding.resolve_texture.has_mipmaps,
                inverted: binding.resolve_texture.inverted,
                has_alpha: binding.resolve_texture.has_alpha,
                pad: 0,
            },
        });
    })
}

unsafe extern "C" fn t_clone_render_target(
    userdata: *mut c_void,
    label: *const c_char,
    src_handle: u64,
    out: *mut RenderTargetBindingFfi,
) {
    crate::panic_guard::guard(|| {
        let dev = device(userdata);
        let label = cstr_to_str(label);
        let binding = dev.clone_render_target(&label, render_target_handle(src_handle));
        out.write(RenderTargetBindingFfi {
            handle: binding.handle.0.get(),
            resolve_texture: TextureBindingFfi {
                handle: binding.resolve_texture.handle.0.get(),
                width: binding.resolve_texture.width,
                height: binding.resolve_texture.height,
                has_mipmaps: binding.resolve_texture.has_mipmaps,
                inverted: binding.resolve_texture.inverted,
                has_alpha: binding.resolve_texture.has_alpha,
                pad: 0,
            },
        });
    })
}

unsafe extern "C" fn t_drop_render_target(userdata: *mut c_void, handle: u64) {
    crate::panic_guard::guard(|| {
        device(userdata).drop_render_target(render_target_handle(handle));
    })
}

unsafe extern "C" fn t_begin_offscreen_render(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        device(userdata).begin_offscreen_render();
    })
}
unsafe extern "C" fn t_end_offscreen_render(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        device(userdata).end_offscreen_render();
    })
}
unsafe extern "C" fn t_begin_onscreen_render(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        device(userdata).begin_onscreen_render();
    })
}
unsafe extern "C" fn t_end_onscreen_render(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        device(userdata).end_onscreen_render();
    })
}

unsafe extern "C" fn t_set_render_target(userdata: *mut c_void, handle: u64) {
    crate::panic_guard::guard(|| {
        device(userdata).set_render_target(render_target_handle(handle));
    })
}

unsafe extern "C" fn t_begin_tile(userdata: *mut c_void, handle: u64, tile: *const Tile) {
    crate::panic_guard::guard(|| {
        device(userdata).begin_tile(render_target_handle(handle), *tile);
    })
}

unsafe extern "C" fn t_end_tile(userdata: *mut c_void, handle: u64) {
    crate::panic_guard::guard(|| {
        device(userdata).end_tile(render_target_handle(handle));
    })
}

unsafe extern "C" fn t_resolve_render_target(
    userdata: *mut c_void,
    handle: u64,
    tiles: *const Tile,
    count: u32,
) {
    crate::panic_guard::guard(|| {
        let dev = device(userdata);
        let tiles_slice = slice::from_raw_parts(tiles, count as usize);
        dev.resolve_render_target(render_target_handle(handle), tiles_slice);
    })
}

unsafe extern "C" fn t_map_vertices(userdata: *mut c_void, bytes: u32) -> *mut c_void {
    // A panic here yields null; Noesis tolerates a failed map far better than an
    // unwind across the render-thread C ABI.
    crate::panic_guard::guard_or(core::ptr::null_mut(), || {
        device(userdata).map_vertices(bytes).as_mut_ptr().cast()
    })
}
unsafe extern "C" fn t_unmap_vertices(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        device(userdata).unmap_vertices();
    })
}
unsafe extern "C" fn t_map_indices(userdata: *mut c_void, bytes: u32) -> *mut c_void {
    crate::panic_guard::guard_or(core::ptr::null_mut(), || {
        device(userdata).map_indices(bytes).as_mut_ptr().cast()
    })
}
unsafe extern "C" fn t_unmap_indices(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        device(userdata).unmap_indices();
    })
}

unsafe extern "C" fn t_draw_batch(userdata: *mut c_void, batch: *const Batch) {
    crate::panic_guard::guard(|| {
        device(userdata).draw_batch(&*batch);
    })
}

// ────────────────────────────────────────────────────────────────────────────
// Static vtable — populated once with the trampoline addresses.
// ────────────────────────────────────────────────────────────────────────────

static VTABLE: RenderDeviceVTable = RenderDeviceVTable {
    get_caps: t_get_caps,
    create_texture: t_create_texture,
    update_texture: t_update_texture,
    end_updating_textures: t_end_updating_textures,
    drop_texture: t_drop_texture,
    create_render_target: t_create_render_target,
    clone_render_target: t_clone_render_target,
    drop_render_target: t_drop_render_target,
    begin_offscreen_render: t_begin_offscreen_render,
    end_offscreen_render: t_end_offscreen_render,
    begin_onscreen_render: t_begin_onscreen_render,
    end_onscreen_render: t_end_onscreen_render,
    set_render_target: t_set_render_target,
    begin_tile: t_begin_tile,
    end_tile: t_end_tile,
    resolve_render_target: t_resolve_render_target,
    map_vertices: t_map_vertices,
    unmap_vertices: t_unmap_vertices,
    map_indices: t_map_indices,
    unmap_indices: t_unmap_indices,
    draw_batch: t_draw_batch,
};

// ────────────────────────────────────────────────────────────────────────────
// register() and Registered
// ────────────────────────────────────────────────────────────────────────────

/// Owns a Rust [`RenderDevice`] impl together with its C++ `RustRenderDevice`
/// instance. Drop order is C++ first (so any transitively-held textures /
/// render targets fire their `drop_*` callbacks against a still-alive trait
/// object), then the boxed impl.
pub struct Registered {
    handle: NonNull<c_void>,
    userdata: NonNull<Box<dyn RenderDevice>>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Registered {}

impl Registered {
    /// Raw `Noesis::RenderDevice*` for handing to other Noesis APIs that take
    /// a render device (e.g. `IView::SetRenderer`). Borrowed for the lifetime
    /// of this `Registered`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    /// Mutable access to the concrete [`RenderDevice`] impl behind the
    /// registration. Use when a system needs to mutate driver state between
    /// Noesis calls (e.g. swapping the onscreen target view each frame
    /// before driving the renderer).
    ///
    /// The type parameter `D` must match the concrete type passed to
    /// [`register`]; enforced at runtime via `dyn Any` downcast.
    ///
    /// # Panics
    ///
    /// Panics if `D` is not the concrete type `register` was called with.
    pub fn device_mut<D: RenderDevice>(&mut self) -> &mut D {
        // SAFETY: userdata points at the live Box<dyn RenderDevice> produced
        // by register(); the borrow lives only as long as &mut self.
        let boxed: &mut Box<dyn RenderDevice> = unsafe { self.userdata.as_mut() };
        (**boxed)
            .as_any_mut()
            .downcast_mut::<D>()
            .expect("Registered::device_mut: type does not match the one given to register")
    }

    /// Width of offscreen render-target textures, in pixels (`RenderDevice::
    /// SetOffscreenWidth`). `0` (the default) selects automatic sizing. Set
    /// this — and the sibling offscreen / glyph-cache knobs — before the
    /// renderer draws its first frame.
    pub fn set_offscreen_width(&mut self, width: u32) {
        // SAFETY: handle is a live Noesis::RenderDevice* until this guard drops.
        unsafe { dm_noesis_render_device_set_offscreen_width(self.handle.as_ptr(), width) }
    }

    /// Height of offscreen render-target textures, in pixels (`RenderDevice::
    /// SetOffscreenHeight`). `0` (the default) selects automatic sizing.
    pub fn set_offscreen_height(&mut self, height: u32) {
        // SAFETY: handle is a live Noesis::RenderDevice* until this guard drops.
        unsafe { dm_noesis_render_device_set_offscreen_height(self.handle.as_ptr(), height) }
    }

    /// Multisample count for offscreen textures (`RenderDevice::
    /// SetOffscreenSampleCount`). Default is `1` (no MSAA).
    pub fn set_offscreen_sample_count(&mut self, count: u32) {
        // SAFETY: handle is a live Noesis::RenderDevice* until this guard drops.
        unsafe { dm_noesis_render_device_set_offscreen_sample_count(self.handle.as_ptr(), count) }
    }

    /// Number of offscreen textures created up-front at startup
    /// (`RenderDevice::SetOffscreenDefaultNumSurfaces`). Default is `0`.
    pub fn set_offscreen_default_num_surfaces(&mut self, num: u32) {
        // SAFETY: handle is a live Noesis::RenderDevice* until this guard drops.
        unsafe {
            dm_noesis_render_device_set_offscreen_default_num_surfaces(self.handle.as_ptr(), num)
        }
    }

    /// Maximum number of offscreen textures (`RenderDevice::
    /// SetOffscreenMaxNumSurfaces`). `0` (the default) means unlimited.
    pub fn set_offscreen_max_num_surfaces(&mut self, num: u32) {
        // SAFETY: handle is a live Noesis::RenderDevice* until this guard drops.
        unsafe { dm_noesis_render_device_set_offscreen_max_num_surfaces(self.handle.as_ptr(), num) }
    }

    /// Width of the glyph-cache texture, in pixels (`RenderDevice::
    /// SetGlyphCacheWidth`). The default is build-dependent — read it back with
    /// [`Self::glyph_cache_width`] rather than assuming a value.
    pub fn set_glyph_cache_width(&mut self, width: u32) {
        // SAFETY: handle is a live Noesis::RenderDevice* until this guard drops.
        unsafe { dm_noesis_render_device_set_glyph_cache_width(self.handle.as_ptr(), width) }
    }

    /// Height of the glyph-cache texture, in pixels (`RenderDevice::
    /// SetGlyphCacheHeight`). The default is build-dependent — read it back with
    /// [`Self::glyph_cache_height`] rather than assuming a value.
    pub fn set_glyph_cache_height(&mut self, height: u32) {
        // SAFETY: handle is a live Noesis::RenderDevice* until this guard drops.
        unsafe { dm_noesis_render_device_set_glyph_cache_height(self.handle.as_ptr(), height) }
    }

    /// Configured offscreen texture width (`RenderDevice::GetOffscreenWidth`).
    /// `0` means automatic. Companion to [`Self::set_offscreen_width`].
    #[must_use]
    pub fn offscreen_width(&self) -> u32 {
        // SAFETY: handle is a live Noesis::RenderDevice*; const accessor.
        unsafe { dm_noesis_render_device_get_offscreen_width(self.handle.as_ptr()) }
    }

    /// Configured offscreen texture height (`RenderDevice::GetOffscreenHeight`).
    /// `0` means automatic. Companion to [`Self::set_offscreen_height`].
    #[must_use]
    pub fn offscreen_height(&self) -> u32 {
        // SAFETY: handle is a live Noesis::RenderDevice*; const accessor.
        unsafe { dm_noesis_render_device_get_offscreen_height(self.handle.as_ptr()) }
    }

    /// Configured offscreen multisample count
    /// (`RenderDevice::GetOffscreenSampleCount`). Companion to
    /// [`Self::set_offscreen_sample_count`].
    #[must_use]
    pub fn offscreen_sample_count(&self) -> u32 {
        // SAFETY: handle is a live Noesis::RenderDevice*; const accessor.
        unsafe { dm_noesis_render_device_get_offscreen_sample_count(self.handle.as_ptr()) }
    }

    /// Configured startup offscreen-surface count
    /// (`RenderDevice::GetOffscreenDefaultNumSurfaces`). Companion to
    /// [`Self::set_offscreen_default_num_surfaces`].
    #[must_use]
    pub fn offscreen_default_num_surfaces(&self) -> u32 {
        // SAFETY: handle is a live Noesis::RenderDevice*; const accessor.
        unsafe { dm_noesis_render_device_get_offscreen_default_num_surfaces(self.handle.as_ptr()) }
    }

    /// Configured maximum offscreen-surface count
    /// (`RenderDevice::GetOffscreenMaxNumSurfaces`). `0` means unlimited.
    /// Companion to [`Self::set_offscreen_max_num_surfaces`].
    #[must_use]
    pub fn offscreen_max_num_surfaces(&self) -> u32 {
        // SAFETY: handle is a live Noesis::RenderDevice*; const accessor.
        unsafe { dm_noesis_render_device_get_offscreen_max_num_surfaces(self.handle.as_ptr()) }
    }

    /// Configured glyph-cache texture width
    /// (`RenderDevice::GetGlyphCacheWidth`). Build-dependent default. Companion to
    /// [`Self::set_glyph_cache_width`].
    #[must_use]
    pub fn glyph_cache_width(&self) -> u32 {
        // SAFETY: handle is a live Noesis::RenderDevice*; const accessor.
        unsafe { dm_noesis_render_device_get_glyph_cache_width(self.handle.as_ptr()) }
    }

    /// Configured glyph-cache texture height
    /// (`RenderDevice::GetGlyphCacheHeight`). Build-dependent default. Companion to
    /// [`Self::set_glyph_cache_height`].
    #[must_use]
    pub fn glyph_cache_height(&self) -> u32 {
        // SAFETY: handle is a live Noesis::RenderDevice*; const accessor.
        unsafe { dm_noesis_render_device_get_glyph_cache_height(self.handle.as_ptr()) }
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        // SAFETY: handle and userdata were produced together by `register`.
        // dm_noesis_render_device_destroy releases the +1 ref from `_create`;
        // any Noesis-internal Ptr<>s also drop here, transitively destroying
        // RustTexture / RustRenderTarget instances and firing drop_* callbacks
        // back into the still-alive boxed impl.
        unsafe {
            dm_noesis_render_device_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Construct a C++ `RustRenderDevice` backed by the given Rust impl. Returns
/// a [`Registered`] guard that owns both halves; drop it to tear everything
/// down.
///
/// # Panics
///
/// Panics if the C++ factory returns null (only possible on internal logic
/// errors).
pub fn register<D: RenderDevice + 'static>(device: D) -> Registered {
    // Box<dyn …> is a fat pointer; wrap in another Box to get a stable thin
    // pointer we can pass through the C ABI as userdata.
    let outer: Box<Box<dyn RenderDevice>> = Box::new(Box::new(device));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is a 'static and userdata is a freshly leaked Box.
    let handle = unsafe { dm_noesis_render_device_create(&raw const VTABLE, userdata.cast()) };

    Registered {
        handle: NonNull::new(handle).expect("dm_noesis_render_device_create returned null"),
        userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
    }
}
