//! Rust-side [`TextureProvider`] trait + [`set_texture_provider`]
//! registration. Mirrors [`crate::xaml_provider`] and
//! [`crate::font_provider`]: a boxed trait object is handed to the C++
//! `RustTextureProvider` subclass via a vtable of trampolines; the
//! returned [`Registered`] guard owns both the boxed impl and the C++
//! provider handle.
//!
//! # How it works
//!
//! Noesis's [`TextureProvider`](https://www.noesisengine.com/docs/) base
//! class has two virtuals we override:
//!
//! - `GetTextureInfo(uri)`: return width / height (and optional atlas
//!   rect + dpi scale) for the image at `uri`. Lets Noesis size an
//!   `Image` element before the pixels are decoded. Return `None` to
//!   signal "not found" — Noesis then falls back to the load path.
//!
//! - `LoadTexture(uri, device)`: return the image as tightly-packed
//!   RGBA8 bytes. The C++ shim immediately hands the bytes to
//!   `device->CreateTexture(...)` on the same RenderDevice Noesis passed
//!   in (our `RustRenderDevice`), so the resulting `Noesis::Texture` is
//!   backed by a real wgpu texture and plugs into `Batch.pattern` /
//!   `Batch.image` through the existing `*_handle()` path. The byte
//!   buffer only needs to live for the duration of the `load` call.
//!
//! # Lifetime
//!
//! The [`Registered`] guard must outlive every Noesis-internal reference
//! that might call back into the provider. Keep it alive until after
//! [`crate::shutdown`] returns.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CStr, c_void};
use std::os::raw::c_char;

use crate::ffi::{
    TextureInfoFfi, TextureProviderVTable, dm_noesis_set_texture_provider,
    dm_noesis_texture_provider_create, dm_noesis_texture_provider_destroy,
};

/// Metadata a [`TextureProvider`] can report for a URI without decoding
/// pixels. `x` / `y` default to `0`; set them only for atlas sub-rects.
/// `dpi_scale` defaults to `1.0` (96dpi).
#[derive(Copy, Clone, Debug, Default)]
pub struct TextureInfo {
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
    pub dpi_scale: f32,
}

impl TextureInfo {
    #[must_use]
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            x: 0,
            y: 0,
            dpi_scale: 1.0,
        }
    }
}

/// Decoded RGBA8 image payload returned by [`TextureProvider::load`].
/// `bytes` must be exactly `width * height * 4` tightly-packed RGBA8.
pub struct ImageData<'a> {
    pub width: u32,
    pub height: u32,
    pub bytes: &'a [u8],
}

/// Rust-side texture provider. `info` returns metadata, `load` returns
/// decoded pixels. Both are keyed by URI strings — Noesis passes the
/// `ImageBrush.ImageSource` / `Image.Source` values through verbatim.
///
/// `Send + Sync` supertraits mirror [`crate::xaml_provider::XamlProvider`]
/// so [`Registered`] can live in a Bevy `Resource`.
pub trait TextureProvider: Send + Sync + 'static {
    /// Downcast escape hatch used by [`Registered::provider_mut`].
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Return metadata for `uri` without decoding pixels. Return `None`
    /// when the URI is unknown. Called by Noesis during layout so it can
    /// size an `Image` before deciding whether to render it.
    fn info(&mut self, uri: &str) -> Option<TextureInfo>;

    /// Return the decoded RGBA8 bytes for `uri`. The returned slice must
    /// stay valid for the duration of the call (the C++ shim copies into
    /// the GPU texture synchronously, so returning a borrow into an
    /// internally-owned `Vec<u8>` is fine).
    ///
    /// Return `None` to signal "not found".
    fn load(&mut self, uri: &str) -> Option<ImageData<'_>>;
}

// ────────────────────────────────────────────────────────────────────────────
// Trampolines
// ────────────────────────────────────────────────────────────────────────────

/// SAFETY: `userdata` must be a pointer produced by [`set_texture_provider`]
/// and still alive.
unsafe fn provider<'a>(userdata: *mut c_void) -> &'a mut Box<dyn TextureProvider> {
    &mut *userdata.cast::<Box<dyn TextureProvider>>()
}

fn cstr_to_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(p) }
            .to_str()
            .expect("noesis passed non-UTF-8 string to TextureProvider")
    }
}

unsafe extern "C" fn t_get_info(
    userdata: *mut c_void,
    uri: *const c_char,
    out: *mut TextureInfoFfi,
) -> bool {
    let uri = cstr_to_str(uri);
    let Some(info) = provider(userdata).info(uri) else {
        return false;
    };
    out.write(TextureInfoFfi {
        width: info.width,
        height: info.height,
        x: info.x,
        y: info.y,
        dpi_scale: info.dpi_scale,
    });
    true
}

unsafe extern "C" fn t_load_texture(
    userdata: *mut c_void,
    uri: *const c_char,
    out_width: *mut u32,
    out_height: *mut u32,
    out_data: *mut *const u8,
    out_len: *mut u32,
) -> bool {
    let uri = cstr_to_str(uri);
    let Some(img) = provider(userdata).load(uri) else {
        return false;
    };
    // Sanity: keep the expected-bytes contract enforced here so the C++
    // shim can trust `len == w * h * 4` when it calls CreateTexture.
    let expected = img.width.saturating_mul(img.height).saturating_mul(4) as usize;
    if img.bytes.len() != expected {
        return false;
    }
    out_width.write(img.width);
    out_height.write(img.height);
    out_data.write(img.bytes.as_ptr());
    out_len.write(u32::try_from(img.bytes.len()).expect("image > 4 GiB"));
    true
}

static VTABLE: TextureProviderVTable = TextureProviderVTable {
    get_info: t_get_info,
    load_texture: t_load_texture,
};

// ────────────────────────────────────────────────────────────────────────────
// Registered — RAII wrapper holding the boxed impl and the C++ provider
// ────────────────────────────────────────────────────────────────────────────

/// Owns a Rust [`TextureProvider`] impl together with its C++
/// `RustTextureProvider` instance. Parallel to
/// [`crate::xaml_provider::Registered`].
pub struct Registered {
    handle: NonNull<c_void>,
    userdata: NonNull<Box<dyn TextureProvider>>,
}

// SAFETY: matches the XAML / Font provider `Registered` wrappers —
// supertrait bound makes the boxed impl Send + Sync, Noesis serialises
// per-object calls.
unsafe impl Send for Registered {}
unsafe impl Sync for Registered {}

impl Registered {
    /// Raw `Noesis::TextureProvider*`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    /// Mutable access to the concrete [`TextureProvider`] impl behind
    /// this guard, via `TypeId`-checked downcast.
    ///
    /// # Panics
    ///
    /// Panics if `P` is not the concrete type passed to
    /// [`set_texture_provider`].
    pub fn provider_mut<P: TextureProvider>(&mut self) -> &mut P {
        let boxed: &mut Box<dyn TextureProvider> = unsafe { self.userdata.as_mut() };
        (**boxed)
            .as_any_mut()
            .downcast_mut::<P>()
            .expect("Registered::provider_mut: type does not match set_texture_provider")
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        // SAFETY: handle + userdata produced together by set_texture_provider;
        // both freed exactly once here.
        unsafe {
            dm_noesis_texture_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Install `provider` as the global Noesis texture provider. Returns a
/// [`Registered`] guard that owns the boxed trait object and the C++
/// wrapper; drop it to tear everything down (after [`crate::shutdown`]).
///
/// # Panics
///
/// Panics if the C++ factory returns null.
pub fn set_texture_provider<P: TextureProvider>(provider: P) -> Registered {
    let outer: Box<Box<dyn TextureProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { dm_noesis_texture_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("dm_noesis_texture_provider_create returned null");
    unsafe { dm_noesis_set_texture_provider(handle.as_ptr()) };

    Registered {
        handle,
        userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
    }
}
