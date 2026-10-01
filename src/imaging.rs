//! Image sources built from code: [`BitmapImage`] (an image file by URI),
//! [`CroppedBitmap`], [`TextureSource`] and [`DynamicTextureSource`].
//!
//! Each type owns one Noesis reference and releases it on drop. Assigning one
//! to an element, as an `Image.Source` or through
//! [`ImageBrush::with_source`](crate::brushes::ImageBrush::with_source) with
//! its `raw()` pointer, makes Noesis take its own reference, so the Rust handle
//! may be dropped afterwards.
//!
//! # Values that need a render device
//!
//! Some values only resolve once a render device is drawing:
//!
//! - [`BitmapSource::pixel_size`] and [`BitmapSource::dpi`] stay at their
//!   defaults until a texture provider loads the image.
//! - [`TextureSource::texture`] is `None` until you bind a `Noesis::Texture*`,
//!   which only a render device can create.
//! - The [`DynamicTextureSource`] callback runs on the render thread, only
//!   while a view showing the source is rendered.

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

/// An integer pixel rectangle. An all-zero rect means "empty", which a
/// [`CroppedBitmap`] treats as the whole source image.
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

/// Any Noesis `BitmapSource`. Implemented by [`CroppedBitmap`],
/// [`TextureSource`] and [`BitmapImage`], so any of them can be a
/// [`CroppedBitmap`] source.
pub trait BitmapSource {
    /// Borrowed `Noesis::BitmapSource*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime.
    fn bitmap_source_raw(&self) -> *mut c_void;

    /// Pixel dimensions `(width, height)`. `(0, 0)` until a texture provider
    /// has loaded the bitmap during rendering.
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

    /// Horizontal and vertical DPI. Holds Noesis's default until the bitmap
    /// has loaded during rendering.
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

/// An image source that crops another [`BitmapSource`] to an [`Int32Rect`].
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
    /// Creates a cropped bitmap with no source and an empty (whole-image) crop
    /// rect.
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

    /// Sets the bitmap to crop. Noesis takes its own reference, so `source` may
    /// be dropped afterwards. Returns `false` only if this handle is not a
    /// `CroppedBitmap`, which can't happen.
    pub fn set_source<S: BitmapSource>(&mut self, source: &S) -> bool {
        // SAFETY: self.ptr is a live CroppedBitmap*; source raw is a live
        // BitmapSource* (Noesis AddRefs it).
        unsafe { noesis_cropped_bitmap_set_source(self.ptr.as_ptr(), source.bitmap_source_raw()) }
    }

    /// Borrowed `BitmapSource*` currently set as the source, or `None`. No
    /// reference is added; don't release it. It equals the
    /// [`bitmap_source_raw`](BitmapSource::bitmap_source_raw) of the handle
    /// passed to [`set_source`](Self::set_source).
    #[must_use]
    pub fn source(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live CroppedBitmap*; returned pointer is borrowed.
        let p = unsafe { noesis_cropped_bitmap_get_source(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Sets the crop rectangle in source pixels. An all-zero rect shows the
    /// whole source image.
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

    /// The crop rectangle.
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

/// A [`BitmapSource`] backed by a `Noesis::Texture`, for showing a texture your
/// renderer already owns.
///
/// Only a render device can create a `Texture`, so [`TextureSource::new`] has
/// none until you bind one with [`set_texture`](Self::set_texture).
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
    /// Creates a texture source with no texture bound.
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

    /// Creates a texture source bound to `texture`. Noesis takes its own
    /// reference to the texture. A null `texture` gives the same result as
    /// [`new`](Self::new). Returns `None` only if allocation fails.
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

    /// Binds `texture`, or clears the binding when it is null. Noesis takes its
    /// own reference. Returns `false` only if this handle is not a
    /// `TextureSource`, which can't happen.
    ///
    /// # Safety
    ///
    /// `texture` must be a valid live `Noesis::Texture*` or null.
    pub unsafe fn set_texture(&mut self, texture: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live TextureSource*; `texture` per contract.
        unsafe { noesis_texture_source_set_texture(self.ptr.as_ptr(), texture) }
    }

    /// Borrowed `Texture*` currently bound, or `None`. No reference is added;
    /// don't release it.
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

/// A [`BitmapSource`] that loads an image file by URI through the texture
/// provider. Pixel size and DPI are unknown until the image loads during
/// rendering.
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
    /// Creates a bitmap image with an empty URI.
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

    /// Creates a bitmap image that loads from `uri`.
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

    /// Replaces the URI. Returns `false` only if this handle is not a
    /// `BitmapImage`, which can't happen.
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

    /// The URI as Noesis canonicalized it; empty for an image created with
    /// [`new`](Self::new).
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

/// An image source whose texture a callback supplies each frame on the render
/// thread, for video and other content that changes every frame.
pub struct DynamicTextureSource {
    ptr: NonNull<c_void>,
}

base_component_handle!(DynamicTextureSource);

impl DynamicTextureSource {
    /// Creates a `width` x `height` pixel source driven by `callback`. `user`
    /// is passed back to the callback unchanged.
    ///
    /// The callback receives a borrowed `Noesis::RenderDevice*` and returns a
    /// borrowed `Noesis::Texture*`, or null. It runs on the render thread, only
    /// while a view showing this source is rendered.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the object.
    ///
    /// # Safety
    ///
    /// `callback` must return a `Texture*` valid for the device it is given, and
    /// `user` must stay valid, and safe to use from the render thread, for as
    /// long as this source can be rendered.
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

    /// Resizes the dynamic texture. Returns `false` only if this
    /// handle is not a `DynamicTextureSource`, which can't happen.
    pub fn resize(&mut self, width: u32, height: u32) -> bool {
        // SAFETY: self.ptr is a live DynamicTextureSource*.
        unsafe { noesis_dynamic_texture_source_resize(self.ptr.as_ptr(), width, height) }
    }

    /// Pixel dimensions `(width, height)` from [`new`](Self::new) or the last
    /// [`resize`](Self::resize).
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
