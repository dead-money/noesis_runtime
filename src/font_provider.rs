//! Rust-side [`FontProvider`] trait + [`set_font_provider`] registration.
//! Mirrors [`crate::xaml_provider`]: a boxed trait object is handed to the
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
//!   bytes of the requested font file. The bytes only need to stay valid
//!   for the duration of the call: the C++ shim copies them into an
//!   owning stream, since Noesis retains the stream inside the resulting
//!   `FontSource` and reads it lazily at glyph-raster time.
//!
//! # Lifetime
//!
//! Keep the [`Registered`] guard alive as long as Noesis should serve fonts
//! through your provider. Dropping it unregisters the provider from Noesis
//! (clearing the slot this guard installed into, unless a newer registration
//! for the same scope has replaced it), releases the C++ wrapper, and frees the
//! boxed impl. There is no need to call [`crate::shutdown`] first.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ptr::NonNull;
use std::borrow::Cow;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ffi::{
    FontProviderVTable, RegisterFontFn, noesis_font_provider_create, noesis_font_provider_destroy,
    noesis_set_font_provider, noesis_set_font_provider_assembly, noesis_set_font_provider_scheme,
    noesis_set_font_provider_scheme_assembly,
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
/// `handle` must be a live `RustFontProvider*` or null; the `Scope`'s `CStrings`
/// outlive the call.
unsafe fn install(scope: &Scope, handle: *mut c_void) {
    match scope {
        Scope::Global => noesis_set_font_provider(handle),
        Scope::Scheme(s) => noesis_set_font_provider_scheme(s.as_ptr(), handle),
        Scope::Assembly(a) => noesis_set_font_provider_assembly(a.as_ptr(), handle),
        Scope::SchemeAssembly(s, a) => {
            noesis_set_font_provider_scheme_assembly(s.as_ptr(), a.as_ptr(), handle)
        }
    }
}

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

/// SAFETY: `userdata` must be a pointer produced by [`set_font_provider`]
/// and still alive.
unsafe fn provider<'a>(userdata: *mut c_void) -> &'a mut Box<dyn FontProvider> {
    &mut *userdata.cast::<Box<dyn FontProvider>>()
}

/// Decode a Noesis-supplied string lossily. Odd/non-UTF-8 engine input must not
/// panic across the C ABI, so invalid bytes become U+FFFD rather than aborting.
fn cstr_to_str<'a>(p: *const c_char) -> Cow<'a, str> {
    if p.is_null() {
        Cow::Borrowed("")
    } else {
        unsafe { CStr::from_ptr(p) }.to_string_lossy()
    }
}

unsafe extern "C" fn t_scan_folder(
    userdata: *mut c_void,
    folder_uri: *const c_char,
    register_fn: RegisterFontFn,
    register_cx: *mut c_void,
) {
    crate::panic_guard::guard(|| {
        let folder = cstr_to_str(folder_uri);
        // `provider(userdata)`'s `&mut` is live for the whole `scan_folder`
        // call. The shim's `register_fn` only buffers the filenames and defers
        // the actual `RegisterFont` (which re-enters `t_open_font` and needs
        // its own `&mut` to this provider) until after `scan_folder` returns,
        // so no second `&mut` is minted while this one is live.
        provider(userdata).scan_folder(&folder, &mut |filename: &str| {
            // Allocate a NUL-terminated copy for the C ABI. The shim copies the
            // name into its own buffer during this call, so the CString can drop
            // right after. A filename with an interior NUL can't cross the C ABI;
            // skip it rather than panic.
            if let Ok(c) = std::ffi::CString::new(filename) {
                register_fn(register_cx, c.as_ptr());
            }
        });
    })
}

unsafe extern "C" fn t_open_font(
    userdata: *mut c_void,
    folder_uri: *const c_char,
    filename: *const c_char,
    out_data: *mut *const u8,
    out_len: *mut u32,
) -> bool {
    crate::panic_guard::guard(|| {
        let folder = cstr_to_str(folder_uri);
        let name = cstr_to_str(filename);
        let Some(bytes) = provider(userdata).open_font(&folder, &name) else {
            return false;
        };
        // A >4 GiB font file can't be represented to the shim, so treat as failure
        // rather than panicking inside the trampoline.
        let Ok(len) = u32::try_from(bytes.len()) else {
            return false;
        };
        out_data.write(bytes.as_ptr());
        out_len.write(len);
        true
    })
}

static VTABLE: FontProviderVTable = FontProviderVTable {
    scan_folder: t_scan_folder,
    open_font: t_open_font,
};

/// Owns a Rust [`FontProvider`] impl together with its C++
/// `RustFontProvider` instance. Parallel to
/// [`crate::xaml_provider::Registered`]: dropping unregisters the provider from
/// Noesis (clearing this guard's slot unless a newer registration for the same
/// scope has replaced it), releases the C++ wrapper, and frees the boxed impl.
#[must_use = "dropping the guard unregisters the provider and frees it"]
pub struct Registered {
    handle: NonNull<c_void>,
    userdata: NonNull<Box<dyn FontProvider>>,
    scope: Scope,
    id: u64,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Registered {}

impl Registered {
    /// Raw `Noesis::FontProvider*`. Useful for other Noesis APIs that
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
    /// [`FontProvider::open_font`] callback, even if the cache has
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
        // `noesis_font_provider_create` and points at a `RustFontProvider`
        // that's live for the lifetime of `self`. The two CStrings outlive
        // the synchronous FFI call.
        unsafe {
            crate::ffi::noesis_font_provider_register_font(
                self.handle.as_ptr(),
                folder.as_ptr(),
                name.as_ptr(),
            );
        }
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        // Clear the Noesis slot only while we're still its active registration;
        // a newer set_*_provider for the same scope must keep firing. Hold the
        // lock across the check + uninstall so it stays atomic against a
        // concurrent registration.
        {
            let mut active = ACTIVE.lock().expect("font provider registry poisoned");
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
            noesis_font_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Install `provider` as the global Noesis font provider. Returns a
/// [`Registered`] guard that owns the boxed trait object and the C++
/// wrapper; drop it to unregister the provider and tear everything down.
///
/// # Panics
///
/// Panics if the C++ factory returns null.
pub fn set_font_provider<P: FontProvider>(provider: P) -> Registered {
    register_with(provider, Scope::Global)
}

/// Build the C++ `RustFontProvider` wrapping `provider`, install it into the
/// slot named by `scope` (the only thing that differs between the global /
/// scheme / assembly variants), record it as that scope's active registration,
/// and return the owning [`Registered`] guard. Keeps the four public setters DRY.
fn register_with<P: FontProvider>(provider: P, scope: Scope) -> Registered {
    let outer: Box<Box<dyn FontProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { noesis_font_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("noesis_font_provider_create returned null");
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        // Hold the registry lock across install + record so a concurrent Drop
        // for the same scope can't observe a half-updated slot and uninstall a
        // registration that just replaced it. Noesis retains its own +1; we
        // keep ours until the Registered is dropped.
        let mut active = ACTIVE.lock().expect("font provider registry poisoned");
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

/// Install `provider` as the font provider for the URI `scheme` (the part
/// before `://`). Noesis consults the scheme-scoped provider for matching
/// font URIs in preference to the global one.
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `scheme` contains an interior
/// NUL byte.
pub fn set_scheme_font_provider<P: FontProvider>(scheme: &str, provider: P) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    register_with(provider, Scope::Scheme(scheme))
}

/// Install `provider` as the font provider for `assembly` (the assembly name in
/// a pack URI).
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `assembly` contains an interior
/// NUL byte.
pub fn set_assembly_font_provider<P: FontProvider>(assembly: &str, provider: P) -> Registered {
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::Assembly(assembly))
}

/// Install `provider` as the font provider scoped to both a `scheme` and an
/// `assembly`.
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `scheme` / `assembly` contain an
/// interior NUL byte.
pub fn set_scheme_assembly_font_provider<P: FontProvider>(
    scheme: &str,
    assembly: &str,
    provider: P,
) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::SchemeAssembly(scheme, assembly))
}

/// Register the global font fallback chain. Each entry is a family name
/// Noesis will search when an element's explicit `FontFamily` lacks a
/// requested glyph. Fallbacks can be bare family names (`"Arial"`) or
/// path-rooted references to a font already known to the font provider
/// (`"Fonts/#Bitter"`). Also acts as the de-facto *default* font for
/// elements that don't specify any `FontFamily` at all; Noesis walks the
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
        unsafe { crate::ffi::noesis_set_font_fallbacks(core::ptr::null(), 0) };
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
        crate::ffi::noesis_set_font_fallbacks(ptrs.as_ptr(), ptrs.len() as u32);
    }
}

/// Default font properties applied to elements that don't set them.
/// `weight`, `stretch`, and `style` are Noesis font-enum codes
/// (`FontWeight`, `FontStretch`, `FontStyle`); the WPF-normal default is
/// `(15.0, 400, 5, 0)`.
pub fn set_font_default_properties(size: f32, weight: i32, stretch: i32, style: i32) {
    unsafe {
        crate::ffi::noesis_set_font_default_properties(size, weight, stretch, style);
    }
}
