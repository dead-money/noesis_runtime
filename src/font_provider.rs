//! Rust-side [`FontProvider`] trait + [`set_font_provider`] registration.
//! Mirrors [`crate::xaml_provider`] — a boxed trait object is handed to the
//! C++ `RustFontProvider` subclass via a vtable of trampolines; the
//! returned [`Registered`] guard owns both the boxed impl and the C++
//! provider handle.
//!
//! # How it works
//!
//! Noesis's `CachedFontProvider` base class handles font-matching
//! internally (weight/stretch/style lookup, face caching). We only need
//! to supply two things:
//!
//! - `scan_folder(folder_uri, register)`: the first time a font is
//!   requested from `folder_uri`, invoked once; `register(filename)`
//!   should be called for each font file in that folder. Noesis then
//!   opens each registered filename via `open_font` below to scan its
//!   face metadata.
//! - `open_font(folder_uri, filename) -> Option<&[u8]>`: returns the raw
//!   bytes of the requested font file. The bytes must stay valid until
//!   Noesis finishes reading the stream (same contract as
//!   `XamlProvider::load_xaml`).
//!
//! # Lifetime
//!
//! The [`Registered`] guard must outlive every Noesis-internal reference
//! that might call back into the provider. In practice that means keeping
//! it alive until after [`crate::shutdown`] returns.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CStr, c_void};
use std::os::raw::c_char;

use crate::ffi::{
    FontProviderVTable, RegisterFontFn, dm_noesis_font_provider_create,
    dm_noesis_font_provider_destroy, dm_noesis_set_font_provider,
};

/// Rust-side font provider. `scan_folder` registers every font Noesis
/// might need in the given folder; `open_font` returns the bytes for a
/// registered filename on demand.
///
/// `Send + Sync` supertraits mirror [`crate::xaml_provider::XamlProvider`]
/// so [`Registered`] can live in a Bevy `Resource`.
pub trait FontProvider: Send + Sync + 'static {
    /// Downcast escape hatch used by [`Registered::provider_mut`].
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Register every font available in `folder_uri`. Call
    /// `register(filename)` once per font filename (e.g.
    /// `"Bitter-Regular.ttf"`). Noesis opens each via [`Self::open_font`]
    /// immediately after to scan face metadata, so the filenames must
    /// resolve in that step.
    fn scan_folder(&mut self, folder_uri: &str, register: &mut dyn FnMut(&str));

    /// Return the raw bytes of `filename` within `folder_uri`. Returns
    /// `None` when the filename is unknown.
    fn open_font(&mut self, folder_uri: &str, filename: &str) -> Option<&[u8]>;
}

// ────────────────────────────────────────────────────────────────────────────
// Trampolines
// ────────────────────────────────────────────────────────────────────────────

/// SAFETY: `userdata` must be a pointer produced by [`set_font_provider`]
/// and still alive.
unsafe fn provider<'a>(userdata: *mut c_void) -> &'a mut Box<dyn FontProvider> {
    &mut *userdata.cast::<Box<dyn FontProvider>>()
}

fn cstr_to_str<'a>(p: *const c_char) -> &'a str {
    if p.is_null() {
        ""
    } else {
        unsafe { CStr::from_ptr(p) }
            .to_str()
            .expect("noesis passed non-UTF-8 string to FontProvider")
    }
}

unsafe extern "C" fn t_scan_folder(
    userdata: *mut c_void,
    folder_uri: *const c_char,
    register_fn: RegisterFontFn,
    register_cx: *mut c_void,
) {
    let folder = cstr_to_str(folder_uri);
    let prov = provider(userdata);
    prov.scan_folder(folder, &mut |filename: &str| {
        // Allocate a NUL-terminated copy for the C ABI. The callback is
        // synchronous and copies internally (via `RegisterFont`), so the
        // CString can drop right after.
        let c = std::ffi::CString::new(filename).expect("font filename contains interior NUL");
        register_fn(register_cx, c.as_ptr());
    });
}

unsafe extern "C" fn t_open_font(
    userdata: *mut c_void,
    folder_uri: *const c_char,
    filename: *const c_char,
    out_data: *mut *const u8,
    out_len: *mut u32,
) -> bool {
    let folder = cstr_to_str(folder_uri);
    let name = cstr_to_str(filename);
    let Some(bytes) = provider(userdata).open_font(folder, name) else {
        return false;
    };
    out_data.write(bytes.as_ptr());
    out_len.write(u32::try_from(bytes.len()).expect("font file > 4 GiB"));
    true
}

static VTABLE: FontProviderVTable = FontProviderVTable {
    scan_folder: t_scan_folder,
    open_font: t_open_font,
};

// ────────────────────────────────────────────────────────────────────────────
// Registered — RAII wrapper holding the boxed impl and the C++ provider
// ────────────────────────────────────────────────────────────────────────────

/// Owns a Rust [`FontProvider`] impl together with its C++
/// `RustFontProvider` instance. Parallel to
/// [`crate::xaml_provider::Registered`].
pub struct Registered {
    handle: NonNull<c_void>,
    userdata: NonNull<Box<dyn FontProvider>>,
}

// SAFETY: matches [`crate::render_device::Registered`] — the boxed impl
// is `Send + Sync` (trait supertrait bound), Noesis serialises per-object
// calls across threads, and `Registered` has no `&self` methods that race
// on Noesis state.
unsafe impl Send for Registered {}
unsafe impl Sync for Registered {}

impl Registered {
    /// Raw `Noesis::FontProvider*` — useful for other Noesis APIs that
    /// take a font provider.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    /// Mutable access to the concrete [`FontProvider`] impl behind this
    /// guard, via `TypeId`-checked downcast.
    ///
    /// # Panics
    ///
    /// Panics if `F` is not the concrete type passed to
    /// [`set_font_provider`].
    pub fn provider_mut<F: FontProvider>(&mut self) -> &mut F {
        let boxed: &mut Box<dyn FontProvider> = unsafe { self.userdata.as_mut() };
        (**boxed)
            .as_any_mut()
            .downcast_mut::<F>()
            .expect("Registered::provider_mut: type does not match set_font_provider")
    }

    /// Eagerly register a `(folder_uri, filename)` face with the
    /// underlying `CachedFontProvider` cache, bypassing Noesis's lazy
    /// `ScanFolder` model. Once registered, any later
    /// `FontFamily="folder_uri/#Family"` lookup whose face metadata
    /// matches will resolve through this provider's
    /// [`FontProvider::open_font`] callback — even if the cache has
    /// already been scanned.
    ///
    /// Calling this for a `(folder_uri, filename)` already registered is
    /// safe: Noesis re-opens the stream and re-scans face metadata; the
    /// duplicate face is ignored. Callers are responsible for any
    /// deduplication if the open + scan cost matters.
    ///
    /// The bytes returned by `open_font` for `(folder_uri, filename)`
    /// must remain valid for the duration of this call (Noesis reads
    /// face metadata synchronously inside the FFI).
    ///
    /// # Panics
    ///
    /// Panics if `folder_uri` or `filename` contain interior NUL bytes.
    pub fn register_font(&self, folder_uri: &str, filename: &str) {
        use std::ffi::CString;
        let folder = CString::new(folder_uri).expect("folder_uri contained interior NUL");
        let name = CString::new(filename).expect("filename contained interior NUL");
        // SAFETY: `self.handle` was returned by
        // `dm_noesis_font_provider_create` and points at a `RustFontProvider`
        // that's live for the lifetime of `self`. The two CStrings outlive
        // the synchronous FFI call.
        unsafe {
            crate::ffi::dm_noesis_font_provider_register_font(
                self.handle.as_ptr(),
                folder.as_ptr(),
                name.as_ptr(),
            );
        }
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        // SAFETY: handle + userdata produced together by set_font_provider;
        // both freed exactly once here.
        unsafe {
            dm_noesis_font_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Install `provider` as the global Noesis font provider. Returns a
/// [`Registered`] guard that owns the boxed trait object and the C++
/// wrapper; drop it to tear everything down (after [`crate::shutdown`]).
///
/// # Panics
///
/// Panics if the C++ factory returns null.
pub fn set_font_provider<P: FontProvider>(provider: P) -> Registered {
    let outer: Box<Box<dyn FontProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { dm_noesis_font_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("dm_noesis_font_provider_create returned null");
    unsafe { dm_noesis_set_font_provider(handle.as_ptr()) };

    Registered {
        handle,
        userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
    }
}

/// Register the global font fallback chain — each entry is a family name
/// Noesis will search when an element's explicit `FontFamily` lacks a
/// requested glyph. Fallbacks can be bare family names (`"Arial"`) or
/// path-rooted references to a font already known to the font provider
/// (`"Fonts/#Bitter"`). Also acts as the de-facto *default* font for
/// elements that don't specify any `FontFamily` at all — Noesis walks the
/// fallback chain in order.
///
/// This is a process-global Noesis setting; call once per run (typically
/// right after registering the font provider). Passing an empty slice
/// clears the fallback chain.
///
/// # Panics
///
/// Panics if any entry contains an interior NUL byte.
pub fn set_font_fallbacks<S: AsRef<str>>(families: &[S]) {
    use std::ffi::CString;
    use std::os::raw::c_char;

    if families.is_empty() {
        unsafe { crate::ffi::dm_noesis_set_font_fallbacks(core::ptr::null(), 0) };
        return;
    }

    let cstrings: Vec<CString> = families
        .iter()
        .map(|f| CString::new(f.as_ref()).expect("fallback family contained interior NUL"))
        .collect();
    let ptrs: Vec<*const c_char> = cstrings.iter().map(|c| c.as_ptr()).collect();
    // SAFETY: Noesis copies the names into its own storage; `ptrs` only
    // needs to be valid for the call's duration.
    unsafe {
        crate::ffi::dm_noesis_set_font_fallbacks(ptrs.as_ptr(), ptrs.len() as u32);
    }
}

/// Default font properties applied when elements don't set them.
/// `weight`, `stretch`, `style` mirror the enums in `NsGui/InputEnums.h`;
/// a WPF-normal default is `(15.0, 400, 5, 0)`.
pub fn set_font_default_properties(size: f32, weight: i32, stretch: i32, style: i32) {
    unsafe {
        crate::ffi::dm_noesis_set_font_default_properties(size, weight, stretch, style);
    }
}
