//! Supply image pixels to Noesis from Rust. Implement [`TextureProvider`] to
//! resolve `Image.Source` / `ImageBrush.ImageSource` URIs into RGBA8 images,
//! then register it with [`set_texture_provider`] or one of the scoped
//! variants ([`set_scheme_texture_provider`], [`set_assembly_texture_provider`],
//! [`set_scheme_assembly_texture_provider`]). It works like
//! [`crate::xaml_provider`] and [`crate::font_provider`].
//!
//! # How it works
//!
//! - [`TextureProvider::info`] reports an image's size (and optional atlas
//!   offset and DPI scale) so Noesis can lay out an `Image` before any pixels
//!   are decoded. Returning `None` reports the image as not found.
//! - [`TextureProvider::load`] returns the image as tightly packed RGBA8 bytes.
//!   They are passed straight to
//!   [`RenderDevice::create_texture`](crate::render_device::RenderDevice::create_texture)
//!   on the device rendering the view, which copies them right away, so you
//!   can return a borrow into a buffer your provider owns.
//!
//! # Lifetime
//!
//! Keep the [`Registered`] guard alive as long as Noesis should resolve
//! textures through your provider. Dropping it unregisters the provider
//! (unless a newer registration for the same scope has replaced it) and frees
//! your impl. You don't need to call [`crate::shutdown`] first.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ptr::NonNull;
use std::borrow::Cow;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ffi::{
    TextureInfoFfi, TextureProviderVTable, noesis_set_texture_provider,
    noesis_set_texture_provider_assembly, noesis_set_texture_provider_scheme,
    noesis_set_texture_provider_scheme_assembly, noesis_texture_provider_create,
    noesis_texture_provider_destroy,
};

/// Which Noesis provider slot a [`Registered`] guard installed into. `Drop`
/// uses it both to clear exactly that slot and as the key into [`ACTIVE`].
#[derive(Clone, PartialEq, Eq)]
enum Scope {
    Global,
    Scheme(CString),
    Assembly(CString),
    SchemeAssembly(CString, CString),
}

/// Monotonic registration ids; `0` is reserved as "no active registration".
static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Id of the currently-active registration per scope (last-registration-wins).
/// A guard's `Drop` clears the Noesis slot only if its id still matches the
/// entry here, so a stale guard can't tear down a newer registration for the
/// same scope. See [`crate::xaml_provider`] for the full rationale.
static ACTIVE: Mutex<Vec<(Scope, u64)>> = Mutex::new(Vec::new());

/// Install `handle` (or null, to clear) into the Noesis slot named by `scope`.
///
/// # Safety
///
/// `handle` must be a live `RustTextureProvider*` or null; the `Scope`'s
/// `CStrings` outlive the call.
unsafe fn install(scope: &Scope, handle: *mut c_void) {
    match scope {
        Scope::Global => noesis_set_texture_provider(handle),
        Scope::Scheme(s) => noesis_set_texture_provider_scheme(s.as_ptr(), handle),
        Scope::Assembly(a) => noesis_set_texture_provider_assembly(a.as_ptr(), handle),
        Scope::SchemeAssembly(s, a) => {
            noesis_set_texture_provider_scheme_assembly(s.as_ptr(), a.as_ptr(), handle)
        }
    }
}

/// Image metadata returned by [`TextureProvider::info`]. Start from
/// [`TextureInfo::new`] and set `x` / `y` only when the image is a sub-rect of
/// an atlas.
#[derive(Copy, Clone, Debug)]
pub struct TextureInfo {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Left edge of the image inside an atlas texture, in pixels.
    pub x: u32,
    /// Top edge of the image inside an atlas texture, in pixels.
    pub y: u32,
    /// Pixel density relative to 96 DPI. Must not be zero; Noesis divides by
    /// it.
    pub dpi_scale: f32,
}

impl Default for TextureInfo {
    /// A zero-sized whole image with `dpi_scale` 1.0, so a
    /// `..Default::default()` splat never leaves a zero divisor.
    fn default() -> Self {
        Self {
            width: 0,
            height: 0,
            x: 0,
            y: 0,
            dpi_scale: 1.0,
        }
    }
}

impl TextureInfo {
    /// Metadata for a whole image of the given size in pixels, at 96 DPI
    /// (`dpi_scale` 1.0) with no atlas offset.
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

/// A decoded image returned by [`TextureProvider::load`].
pub struct ImageData<'a> {
    /// Width in pixels. Must be non-zero.
    pub width: u32,
    /// Height in pixels. Must be non-zero.
    pub height: u32,
    /// Tightly packed RGBA8 rows, exactly `width * height * 4` bytes. Any other
    /// length makes the load fail.
    pub bytes: &'a [u8],
}

/// Resolves image URIs to pixels for Noesis. Both methods get the URI string
/// exactly as written in `Image.Source` / `ImageBrush.ImageSource`.
///
/// The trait requires `Send + Sync` so the boxed impl can move between
/// threads. The [`Registered`] guard itself is `Send` but not `Sync`; in Bevy,
/// store it as a `NonSend` resource.
pub trait TextureProvider: Send + Sync + 'static {
    /// Downcast hook behind [`Registered::provider_mut`]. Every impl is the
    /// same one-liner:
    ///
    /// ```ignore
    /// fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    /// ```
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Metadata for `uri`, without decoding pixels. Noesis calls this during
    /// layout to size an `Image`. Return `None` if you don't know the URI;
    /// Noesis receives an empty size, its "not found" signal.
    fn info(&mut self, uri: &str) -> Option<TextureInfo>;

    /// The decoded image for `uri`, or `None` if not found. The bytes are
    /// copied into a texture before this call's result is dropped, so
    /// borrowing from a buffer you own is fine. An [`ImageData`] with a zero
    /// dimension or the wrong byte length is treated as not found.
    fn load(&mut self, uri: &str) -> Option<ImageData<'_>>;
}

/// SAFETY: `userdata` must be a pointer produced by [`set_texture_provider`]
/// and still alive.
unsafe fn provider<'a>(userdata: *mut c_void) -> &'a mut Box<dyn TextureProvider> {
    &mut *userdata.cast::<Box<dyn TextureProvider>>()
}

/// Decode a Noesis-supplied URI lossily. Odd/non-UTF-8 engine input must not
/// panic across the C ABI, so invalid bytes become U+FFFD rather than aborting.
fn cstr_to_str<'a>(p: *const c_char) -> Cow<'a, str> {
    if p.is_null() {
        Cow::Borrowed("")
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy()
    }
}

unsafe extern "C" fn t_get_info(
    userdata: *mut c_void,
    uri: *const c_char,
    out: *mut TextureInfoFfi,
) -> bool {
    crate::panic_guard::guard(|| {
        let uri = cstr_to_str(uri);
        let Some(info) = provider(userdata).info(&uri) else {
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
    })
}

unsafe extern "C" fn t_load_texture(
    userdata: *mut c_void,
    uri: *const c_char,
    out_width: *mut u32,
    out_height: *mut u32,
    out_data: *mut *const u8,
    out_len: *mut u32,
) -> bool {
    crate::panic_guard::guard(|| {
        let uri = cstr_to_str(uri);
        let Some(img) = provider(userdata).load(&uri) else {
            return false;
        };
        let expected = img.width.saturating_mul(img.height).saturating_mul(4) as usize;
        if img.bytes.len() != expected {
            return false;
        }
        // The ABI length is u32; a >4 GiB buffer fails instead of panicking
        let Ok(len) = u32::try_from(img.bytes.len()) else {
            return false;
        };
        out_width.write(img.width);
        out_height.write(img.height);
        out_data.write(img.bytes.as_ptr());
        out_len.write(len);
        true
    })
}

static VTABLE: TextureProviderVTable = TextureProviderVTable {
    get_info: t_get_info,
    load_texture: t_load_texture,
};

/// Keeps a registered [`TextureProvider`] installed. Dropping it unregisters
/// the provider (unless a newer registration for the same scope has replaced
/// it) and frees your impl.
#[must_use = "dropping the guard unregisters the provider and frees it"]
pub struct Registered {
    handle: NonNull<c_void>,
    userdata: NonNull<Box<dyn TextureProvider>>,
    scope: Scope,
    id: u64,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Registered {}

impl Registered {
    /// Raw `Noesis::TextureProvider*`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    /// Mutable access to the provider you registered, for example to add
    /// images to its cache.
    ///
    /// # Panics
    ///
    /// Panics if `P` is not the type that was registered.
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
        // Only clear the slot if no newer registration replaced us. The lock
        // spans check + uninstall to stay atomic against a concurrent register.
        {
            let mut active = ACTIVE.lock().expect("texture provider registry poisoned");
            if let Some(pos) = active
                .iter()
                .position(|(s, i)| *s == self.scope && *i == self.id)
            {
                active.swap_remove(pos);
                // SAFETY: null clears our slot; the scope's CStrings outlive the
                // call. Releasing Noesis's own Ptr here means no wrapper points
                // at the userdata we free below.
                unsafe { install(&self.scope, core::ptr::null_mut()) };
            }
        }
        // SAFETY: handle + userdata produced together by register_with(); both
        // freed exactly once here. destroy drops our +1 and fires the C++
        // destructor; the boxed impl is then freed.
        unsafe {
            noesis_texture_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Install `provider` as the global texture provider, replacing any earlier
/// global one. Drop the returned guard to unregister it.
///
/// # Panics
///
/// Panics if the C++ factory returns null.
pub fn set_texture_provider<P: TextureProvider>(provider: P) -> Registered {
    register_with(provider, Scope::Global)
}

/// Build the C++ `RustTextureProvider` wrapping `provider`, install it into the
/// slot named by `scope` (the only thing that differs between the global /
/// scheme / assembly variants), record it as that scope's active registration,
/// and return the owning [`Registered`] guard.
fn register_with<P: TextureProvider>(provider: P, scope: Scope) -> Registered {
    let outer: Box<Box<dyn TextureProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { noesis_texture_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("noesis_texture_provider_create returned null");
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        // Lock spans install + record so a concurrent Drop for the same scope
        // can't uninstall the registration that just replaced it.
        let mut active = ACTIVE.lock().expect("texture provider registry poisoned");
        // SAFETY: handle is freshly created and live.
        unsafe { install(&scope, handle.as_ptr()) };
        if let Some(slot) = active.iter_mut().find(|(s, _)| *s == scope) {
            slot.1 = id;
        } else {
            active.push((scope.clone(), id));
        }
    }

    Registered {
        handle,
        userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
        scope,
        id,
    }
}

/// Install `provider` for URIs with the given `scheme` (the part before
/// `://`). Noesis prefers it over the global provider for matching URIs.
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `scheme` contains an interior
/// NUL byte.
pub fn set_scheme_texture_provider<P: TextureProvider>(scheme: &str, provider: P) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    register_with(provider, Scope::Scheme(scheme))
}

/// Install `provider` for URIs that name `assembly` (the assembly in a pack
/// URI).
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `assembly` contains an interior
/// NUL byte.
pub fn set_assembly_texture_provider<P: TextureProvider>(
    assembly: &str,
    provider: P,
) -> Registered {
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::Assembly(assembly))
}

/// Install `provider` for URIs that match both `scheme` and `assembly`.
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `scheme` / `assembly` contain an
/// interior NUL byte.
pub fn set_scheme_assembly_texture_provider<P: TextureProvider>(
    scheme: &str,
    assembly: &str,
    provider: P,
) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::SchemeAssembly(scheme, assembly))
}
