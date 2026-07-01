//! Supply image pixels to Noesis from Rust. Implement [`TextureProvider`]
//! to resolve `Image.Source` / `ImageBrush.ImageSource` URIs into RGBA8
//! textures, then register it with [`set_texture_provider`] (or one of the
//! scheme/assembly-scoped variants). Your boxed impl is handed to a C++
//! `RustTextureProvider` subclass through a vtable of trampolines, and the
//! returned [`Registered`] guard owns both the boxed impl and the C++ handle.
//!
//! This parallels [`crate::xaml_provider`] and [`crate::font_provider`] if you
//! have already used those.
//!
//! # How it works
//!
//! Noesis's [`TextureProvider`](https://www.noesisengine.com/docs/) base
//! class has two virtuals we override:
//!
//! - `GetTextureInfo(uri)`: return width / height (and optional atlas
//!   rect + dpi scale) for the image at `uri`. Lets Noesis size an
//!   `Image` element before the pixels are decoded. Return `None` to
//!   signal "not found"; Noesis then falls back to the load path.
//!
//! - `LoadTexture(uri, device)`: return the image as tightly-packed
//!   RGBA8 bytes. The C++ shim immediately hands the bytes to
//!   `device->CreateTexture(...)` on the same `RenderDevice` Noesis passed
//!   in (our `RustRenderDevice`), so the resulting `Noesis::Texture` is
//!   backed by a real wgpu texture and plugs into `Batch.pattern` /
//!   `Batch.image` through the existing `*_handle()` path. The byte
//!   buffer only needs to live for the duration of the `load` call.
//!
//! # Lifetime
//!
//! Keep the [`Registered`] guard alive as long as Noesis should resolve
//! textures through your provider. Dropping it unregisters the provider from
//! Noesis (clearing the slot this guard installed into, unless a newer
//! registration for the same scope has replaced it), releases the C++ wrapper,
//! and frees the boxed impl. There is no need to call [`crate::shutdown`]
//! first.

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

/// Metadata a [`TextureProvider`] can report for a URI without decoding
/// pixels. [`new`](Self::new) leaves `x` / `y` at `0` (set them only for atlas
/// sub-rects) and `dpi_scale` at `1.0` (96dpi).
#[derive(Copy, Clone, Debug)]
pub struct TextureInfo {
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
    pub dpi_scale: f32,
}

impl Default for TextureInfo {
    /// `dpi_scale` defaults to `1.0`, not `0.0`: the C++ side divides by it, so
    /// a zero from a `..Default::default()` splat would poison the size math.
    /// Everything else is a zero-sized whole-image texture.
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
    /// Metadata for a whole-image texture of the given size, at 96dpi
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

/// Decoded RGBA8 image payload returned by [`TextureProvider::load`].
/// `bytes` must be exactly `width * height * 4` tightly-packed RGBA8.
pub struct ImageData<'a> {
    pub width: u32,
    pub height: u32,
    pub bytes: &'a [u8],
}

/// Resolves image URIs to pixels for Noesis. Implement [`info`](Self::info)
/// to report a texture's size during layout and [`load`](Self::load) to hand
/// back the decoded RGBA8 bytes. Both are keyed by the URI string Noesis takes
/// verbatim from `ImageBrush.ImageSource` / `Image.Source`.
///
/// The `Send + Sync` supertraits let the resulting [`Registered`] guard live in
/// a Bevy `Resource`.
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
        // Enforce `len == w * h * 4` here so the C++ shim can trust it at CreateTexture.
        let expected = img.width.saturating_mul(img.height).saturating_mul(4) as usize;
        if img.bytes.len() != expected {
            return false;
        }
        // A >4 GiB buffer can't be represented to the shim, so treat it as a
        // failure rather than panicking inside the trampoline.
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

/// Owns a Rust [`TextureProvider`] impl together with its C++
/// `RustTextureProvider` instance. Parallel to
/// [`crate::xaml_provider::Registered`]: dropping unregisters the provider from
/// Noesis (clearing this guard's slot unless a newer registration for the same
/// scope has replaced it), releases the C++ wrapper, and frees the boxed impl.
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
        // Clear the Noesis slot only while we're still its active registration;
        // a newer set_*_provider for the same scope must keep firing. Hold the
        // lock across the check + uninstall so it stays atomic against a
        // concurrent registration.
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

/// Install `provider` as the global Noesis texture provider. Returns a
/// [`Registered`] guard that owns the boxed trait object and the C++
/// wrapper; drop it to unregister the provider and tear everything down.
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
        // Hold the registry lock across install + record so a concurrent Drop
        // for the same scope can't observe a half-updated slot and uninstall a
        // registration that just replaced it. Noesis retains its own +1; we
        // keep ours until the Registered is dropped.
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

/// Install `provider` as the texture provider for the URI `scheme` (the part
/// before `://`). Noesis consults the scheme-scoped provider for matching
/// texture URIs in preference to the global one. Reuses
/// [`set_texture_provider`]'s trampoline + [`Registered`] machinery; only the
/// install call differs.
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `scheme` contains an interior
/// NUL byte.
pub fn set_scheme_texture_provider<P: TextureProvider>(scheme: &str, provider: P) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    register_with(provider, Scope::Scheme(scheme))
}

/// Install `provider` as the texture provider for `assembly` (the assembly name
/// in a pack URI). Reuses [`set_texture_provider`]'s machinery; only the
/// install call differs.
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

/// Install `provider` as the texture provider scoped to both a `scheme` and an
/// `assembly`. Reuses [`set_texture_provider`]'s machinery; only the install
/// call differs.
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
