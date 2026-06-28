//! Code-built `ImageSource` / `BitmapSource` family: construct
//! [`CroppedBitmap`], [`TextureSource`], [`BitmapImage`], and
//! [`DynamicTextureSource`] objects from Rust without authoring XAML.
//!
//! Each type here is an owning handle over a freshly-created Noesis object
//! holding a single `+1` reference, released on [`Drop`] — the same pattern as
//! [`crate::brushes::SolidColorBrush`]. Assigning the object to an element
//! (e.g. as an `Image.Source`, or via
//! [`ImageBrush::with_source`](crate::brushes::ImageBrush::with_source) using
//! [`raw`](CroppedBitmap::raw)) makes Noesis take its own reference, so the Rust
//! handle may be dropped afterwards.
//!
//! Read-back getters re-read from the live Noesis object so they prove a value
//! crossed the FFI rather than echoing a Rust-side cache.
//!
//! # GPU dependencies
//!
//! [`CroppedBitmap`] (source pointer + crop rect) round-trips fully headless and
//! is the centerpiece. The remaining surface has values that resolve only on a
//! `RenderDevice` render pass and read back null / `0` headless:
//!
//! - [`TextureSource::texture`] is `None` until a host
//!   `RenderDevice`-created `Texture` is bound (a `Noesis::Texture*` is only
//!   minted by a live render device — see "Known SDK limitations" in
//!   `LIMITATIONS.md`).
//! - [`BitmapSource`] pixel dims / dpi ([`BitmapSource::pixel_size`],
//!   [`BitmapSource::dpi`]) stay `0` until a texture provider resolves the image.
//! - [`DynamicTextureSource`]'s callback fires from the render thread, so it is
//!   only invoked under a live render pass; construction +
//!   [`resize`](DynamicTextureSource::resize) + the pixel-size getter are
//!   exercised here.

use core::ptr::NonNull;
use std::ffi::{CStr, CString};
use std::os::raw::c_void;

use crate::ffi::{
    TextureRenderCallback, noesis_base_component_release, noesis_bitmap_image_create,
    noesis_bitmap_image_get_uri_source, noesis_bitmap_image_set_uri_source,
    noesis_bitmap_source_get_dpi, noesis_bitmap_source_get_pixel_size,
    noesis_cropped_bitmap_create, noesis_cropped_bitmap_get_source,
    noesis_cropped_bitmap_get_source_rect, noesis_cropped_bitmap_set_source,
    noesis_cropped_bitmap_set_source_rect, noesis_dynamic_texture_source_create,
    noesis_dynamic_texture_source_get_pixel_size, noesis_dynamic_texture_source_resize,
    noesis_texture_source_create, noesis_texture_source_get_texture,
    noesis_texture_source_set_texture,
};

/// An integer rectangle (`Noesis::Int32Rect`): top-left `(x, y)` and unsigned
/// `width` / `height`. An all-zero rect is the "Empty" sentinel that renders the
/// entire source image.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct Int32Rect {
    /// Left edge, in pixels.
    pub x: i32,
    /// Top edge, in pixels.
    pub y: i32,
    /// Width, in pixels.
    pub width: u32,
    /// Height, in pixels.
    pub height: u32,
}

impl Int32Rect {
    /// Construct a rect from its four fields.
    #[must_use]
    pub fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// A handle to a Noesis `BitmapSource` (the constructible imaging types here all
/// derive from it). Lets the source-wiring sugar accept any of them while
/// keeping non-bitmap objects out.
pub trait BitmapSource {
    /// Borrowed `Noesis::BitmapSource*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime.
    fn bitmap_source_raw(&self) -> *mut c_void;

    /// Pixel dimensions `(width, height)` read back from the live object.
    ///
    /// `(0, 0)` headless until a texture provider resolves the bitmap on a
    /// `RenderDevice` render pass.
    #[must_use]
    fn pixel_size(&self) -> (i32, i32) {
        let mut w = 0i32;
        let mut h = 0i32;
        // SAFETY: the raw pointer is a live BitmapSource*; out params are valid.
        unsafe {
            noesis_bitmap_source_get_pixel_size(
                self.bitmap_source_raw(),
                &mut w as *mut i32,
                &mut h as *mut i32,
            );
        }
        (w, h)
    }

    /// Horizontal / vertical DPI `(dpiX, dpiY)` read back from the live object.
    ///
    /// Defaults headless until resolved on a render pass.
    #[must_use]
    fn dpi(&self) -> (f32, f32) {
        let mut x = 0f32;
        let mut y = 0f32;
        // SAFETY: the raw pointer is a live BitmapSource*; out params are valid.
        unsafe {
            noesis_bitmap_source_get_dpi(
                self.bitmap_source_raw(),
                &mut x as *mut f32,
                &mut y as *mut f32,
            );
        }
        (x, y)
    }
}

macro_rules! base_component_handle {
    ($name:ident) => {
        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl $name {
            /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a `*_create` entrypoint with a +1 ref that
                // we own; released exactly once here.
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

/// A `CroppedBitmap`: an image source that crops another [`BitmapSource`] to a
/// rectangular [`Int32Rect`]. Fully round-trippable headless — no GPU needed.
pub struct CroppedBitmap {
    ptr: NonNull<c_void>,
}

base_component_handle!(CroppedBitmap);

impl Default for CroppedBitmap {
    fn default() -> Self {
        Self::new()
    }
}

impl CroppedBitmap {
    /// Create an empty cropped bitmap (no source, Empty source rect).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the object.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no arguments; the C side default-constructs.
        let ptr = unsafe { noesis_cropped_bitmap_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_cropped_bitmap_create returned null"),
        }
    }

    /// Point the crop at a `source` bitmap. Noesis takes its own reference, so
    /// the `source` handle may be dropped afterwards.
    pub fn set_source<S: BitmapSource>(&mut self, source: &S) -> bool {
        // SAFETY: self.ptr is a live CroppedBitmap*; source raw is a live
        // BitmapSource* (Noesis AddRefs it).
        unsafe { noesis_cropped_bitmap_set_source(self.ptr.as_ptr(), source.bitmap_source_raw()) }
    }

    /// Borrowed `BitmapSource*` currently set as the source, or `None`. The
    /// pointer has no `+1`; do not release it. It equals the
    /// [`raw`](BitmapSource::bitmap_source_raw) of the handle passed to
    /// [`set_source`](Self::set_source).
    #[must_use]
    pub fn source(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live CroppedBitmap*; returned pointer is borrowed.
        let p = unsafe { noesis_cropped_bitmap_get_source(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set the crop rectangle. An all-zero rect renders the entire source image.
    pub fn set_source_rect(&mut self, rect: Int32Rect) {
        // SAFETY: self.ptr is a live CroppedBitmap*.
        unsafe {
            noesis_cropped_bitmap_set_source_rect(
                self.ptr.as_ptr(),
                rect.x,
                rect.y,
                rect.width,
                rect.height,
            );
        }
    }

    /// Read the crop rectangle back from the live object.
    #[must_use]
    pub fn source_rect(&self) -> Int32Rect {
        let mut r = Int32Rect::default();
        // SAFETY: self.ptr is a live CroppedBitmap*; out params are valid.
        unsafe {
            noesis_cropped_bitmap_get_source_rect(
                self.ptr.as_ptr(),
                &mut r.x as *mut i32,
                &mut r.y as *mut i32,
                &mut r.width as *mut u32,
                &mut r.height as *mut u32,
            );
        }
        r
    }
}

impl BitmapSource for CroppedBitmap {
    fn bitmap_source_raw(&self) -> *mut c_void {
        self.raw()
    }
}

/// A `TextureSource`: a [`BitmapSource`] backed by a `Noesis::Texture`.
///
/// A real `Texture` is only minted by a host `RenderDevice`, so the default-
/// constructed form here has no texture ([`texture`](Self::texture) is `None`)
/// until one is bound via [`set_texture`](Self::set_texture) with a borrowed
/// `Texture*` from such a device.
pub struct TextureSource {
    ptr: NonNull<c_void>,
}

base_component_handle!(TextureSource);

impl Default for TextureSource {
    fn default() -> Self {
        Self::new()
    }
}

impl TextureSource {
    /// Default-construct a texture source with no texture bound.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the object.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: null texture => default ctor.
        let ptr = unsafe { noesis_texture_source_create(core::ptr::null_mut()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_texture_source_create returned null"),
        }
    }

    /// Construct a texture source bound to a borrowed `Noesis::Texture*`. Noesis
    /// stores it in an owning `Ptr<Texture>`. Returns `None` only if allocation
    /// fails.
    ///
    /// # Safety
    ///
    /// `texture` must be a valid live `Noesis::Texture*` (a `BaseComponent*`
    /// from a host `RenderDevice`) or null.
    #[must_use]
    pub unsafe fn with_texture(texture: *mut c_void) -> Option<Self> {
        // SAFETY: per contract, `texture` is a live Texture* or null.
        let ptr = unsafe { noesis_texture_source_create(texture) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Bind a borrowed `Noesis::Texture*` (or null to clear).
    ///
    /// # Safety
    ///
    /// `texture` must be a valid live `Noesis::Texture*` or null.
    pub unsafe fn set_texture(&mut self, texture: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live TextureSource*; `texture` per contract.
        unsafe { noesis_texture_source_set_texture(self.ptr.as_ptr(), texture) }
    }

    /// Borrowed `Texture*` currently bound, or `None`. The pointer has no `+1`;
    /// do not release it. `None` until a host `RenderDevice`-created `Texture` is
    /// bound.
    #[must_use]
    pub fn texture(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live TextureSource*; returned pointer is borrowed.
        let p = unsafe { noesis_texture_source_get_texture(self.ptr.as_ptr()) };
        NonNull::new(p)
    }
}

impl BitmapSource for TextureSource {
    fn bitmap_source_raw(&self) -> *mut c_void {
        self.raw()
    }
}

/// A `BitmapImage`: a [`BitmapSource`] created from an image file at a URI.
///
/// The [`uri_source`](Self::uri_source) round-trips headless; pixel dims / dpi
/// stay `0` until a texture provider resolves the image on a render pass.
pub struct BitmapImage {
    ptr: NonNull<c_void>,
}

base_component_handle!(BitmapImage);

impl Default for BitmapImage {
    fn default() -> Self {
        Self::new()
    }
}

impl BitmapImage {
    /// Default-construct a bitmap image with an empty URI source.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the object.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: null uri => default ctor.
        let ptr = unsafe { noesis_bitmap_image_create(core::ptr::null()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_bitmap_image_create returned null"),
        }
    }

    /// Construct a bitmap image with its `uri` source set.
    ///
    /// # Panics
    ///
    /// Panics if `uri` contains an interior NUL, or if Noesis fails to allocate.
    #[must_use]
    pub fn from_uri(uri: &str) -> Self {
        let c = CString::new(uri).expect("BitmapImage uri contains interior NUL");
        // SAFETY: `c` outlives the call; the C side copies it into a Uri.
        let ptr = unsafe { noesis_bitmap_image_create(c.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_bitmap_image_create returned null"),
        }
    }

    /// Replace the URI source.
    ///
    /// # Panics
    ///
    /// Panics if `uri` contains an interior NUL.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_uri_source(&mut self, uri: &str) -> bool {
        let c = CString::new(uri).expect("BitmapImage uri contains interior NUL");
        // SAFETY: self.ptr is a live BitmapImage*; `c` outlives the call.
        unsafe { noesis_bitmap_image_set_uri_source(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Read the canonicalized URI source back from the live object.
    ///
    /// Returns an empty string for a default-constructed image.
    #[must_use]
    pub fn uri_source(&self) -> String {
        // SAFETY: self.ptr is a live BitmapImage*; the returned pointer is
        // borrowed and valid until the UriSource changes or the image drops, so
        // we copy it into an owned String immediately.
        let p = unsafe { noesis_bitmap_image_get_uri_source(self.ptr.as_ptr()) };
        if p.is_null() {
            return String::new();
        }
        // SAFETY: `p` is a valid NUL-terminated C string from Noesis.
        unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
    }
}

impl BitmapSource for BitmapImage {
    fn bitmap_source_raw(&self) -> *mut c_void {
        self.raw()
    }
}

/// A `DynamicTextureSource`: an `ImageSource` that regenerates its texture per
/// frame via a render-thread callback (appropriate for video-like content).
///
/// The callback is only invoked under a live `RenderDevice` render pass.
/// Construction, [`resize`](Self::resize), and [`pixel_size`](Self::pixel_size)
/// are exercisable headless.
pub struct DynamicTextureSource {
    ptr: NonNull<c_void>,
}

base_component_handle!(DynamicTextureSource);

impl DynamicTextureSource {
    /// Create a dynamic texture source of `width` x `height` pixels driven by
    /// `callback`. `user` is passed back to the callback verbatim.
    ///
    /// The callback receives a borrowed `Noesis::RenderDevice*` and must return
    /// a borrowed `Noesis::Texture*` (or null). It is invoked from the render
    /// thread, so it only fires while a view containing this source is rendered.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the object.
    ///
    /// # Safety
    ///
    /// `callback` must uphold the render-thread `TextureRenderCallback` contract:
    /// returning a `Texture*` valid for the device it is handed, and `user` must
    /// remain valid for as long as this source can be rendered.
    #[must_use]
    pub unsafe fn new(
        width: u32,
        height: u32,
        callback: TextureRenderCallback,
        user: *mut c_void,
    ) -> Self {
        // SAFETY: per contract; the C side reinterprets the fn pointer to the
        // Noesis TextureRenderCallback and stores `user`.
        let ptr = unsafe { noesis_dynamic_texture_source_create(width, height, callback, user) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_dynamic_texture_source_create returned null"),
        }
    }

    /// Resize the dynamic texture.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        // SAFETY: self.ptr is a live DynamicTextureSource*.
        unsafe { noesis_dynamic_texture_source_resize(self.ptr.as_ptr(), width, height) }
    }

    /// Read the texture pixel dimensions `(width, height)` back from the live
    /// object (the values passed to [`new`](Self::new) / [`resize`](Self::resize)).
    #[must_use]
    pub fn pixel_size(&self) -> (u32, u32) {
        let mut w = 0u32;
        let mut h = 0u32;
        // SAFETY: self.ptr is a live DynamicTextureSource*; out params valid.
        unsafe {
            noesis_dynamic_texture_source_get_pixel_size(
                self.ptr.as_ptr(),
                &mut w as *mut u32,
                &mut h as *mut u32,
            );
        }
        (w, h)
    }
}
