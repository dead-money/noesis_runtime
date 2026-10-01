//! Serve font files to Noesis from your own asset pipeline.
//!
//! Implement [`FontProvider`] and install it with [`set_font_provider`] (or a
//! scheme- or assembly-scoped variant). Noesis's `CachedFontProvider` does the
//! font matching (weight, stretch, style, face caching); your provider only
//! lists and opens files:
//!
//! - [`FontProvider::scan_folder`] runs the first time a font is requested
//!   from a folder. Call `register(filename)` for each font file in it; Noesis
//!   then opens each one through `open_font` to read its face metadata.
//! - [`FontProvider::open_font`] returns a font file's raw bytes. They only
//!   need to stay valid for the call: the shim copies them, because Noesis
//!   keeps the stream and reads it lazily at glyph-raster time.
//!
//! [`set_font_fallbacks`] and [`set_font_default_properties`] configure the
//! process-wide fallback chain and default font.
//!
//! # Lifetime
//!
//! Keep the returned [`Registered`] guard alive as long as Noesis should serve
//! fonts through your provider. Dropping it unregisters the provider (unless a
//! newer registration for the same scope has replaced it), releases the C++
//! wrapper, and frees your impl. You don't need to call [`crate::shutdown`]
//! first.
//!
//! ```no_run
//! use noesis_runtime::font_provider::{FontProvider, set_font_fallbacks, set_font_provider};
//!
//! struct Fonts {
//!     bitter: Vec<u8>,
//! }
//!
//! impl FontProvider for Fonts {
//!     fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
//!         self
//!     }
//!
//!     fn scan_folder(&mut self, folder_uri: &str, register: &mut dyn FnMut(&str)) {
//!         if folder_uri == "Fonts" {
//!             register("Bitter-Regular.ttf");
//!         }
//!     }
//!
//!     fn open_font(&mut self, folder_uri: &str, filename: &str) -> Option<&[u8]> {
//!         (folder_uri == "Fonts" && filename == "Bitter-Regular.ttf").then_some(&self.bitter[..])
//!     }
//! }
//!
//! let _fonts = set_font_provider(Fonts { bitter: std::fs::read("Bitter-Regular.ttf").unwrap() });
//! set_font_fallbacks(&["Fonts/#Bitter"]);
//! ```

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

/// The Noesis provider slot a [`Registered`] guard installed into; also the key
/// into [`ACTIVE`].
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
/// same scope.
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

/// Lists and opens font files for Noesis. See the [module docs](self) for how
/// the two callbacks fit together.
///
/// The `Send + Sync` supertraits let the [`Registered`] guard move between
/// threads. The guard itself is `Send` but not `Sync`, so keep it on the
/// thread that drives Noesis (in Bevy, a `NonSend` resource).
///
/// A panic inside a callback is caught at the C ABI instead of unwinding into
/// Noesis; a panicking `open_font` counts as `None`.
pub trait FontProvider: Send + Sync + 'static {
    /// Downcast hook for [`Registered::provider_mut`]. Implement it as
    /// `fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }`.
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Registers every font in `folder_uri`. Call `register(filename)` once per
    /// font file (e.g. `"Bitter-Regular.ttf"`). Noesis opens each one through
    /// [`Self::open_font`] right after this returns, so every registered name
    /// must resolve there. Filenames containing a NUL byte are skipped.
    fn scan_folder(&mut self, folder_uri: &str, register: &mut dyn FnMut(&str));

    /// Returns the raw bytes of `filename` in `folder_uri`, or `None` if the
    /// file is unknown. The bytes only need to live for the call; Noesis keeps
    /// its own copy. Files of 4 GiB or more are treated as `None`.
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
        // The shim's `register_fn` only buffers names; `RegisterFont` (which
        // re-enters `t_open_font` for its own `&mut`) runs after `scan_folder`
        // returns, so the two `&mut`s to the provider never overlap.
        provider(userdata).scan_folder(&folder, &mut |filename: &str| {
            // The shim copies the name during the call. Interior-NUL names
            // can't cross the C ABI; skip rather than panic.
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
        // The shim takes a u32 length.
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

/// Guard returned by [`set_font_provider`] and its scoped variants. Owns your
/// [`FontProvider`] impl and the C++ provider wrapping it.
///
/// Dropping it unregisters the provider from Noesis (unless a newer
/// registration for the same scope has replaced it), releases the C++ wrapper,
/// and frees your impl.
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
    /// Raw `Noesis::FontProvider*`, borrowed for the guard's lifetime.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    /// Mutable access to your concrete [`FontProvider`] impl.
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

    /// Registers one font file now instead of waiting for Noesis to call
    /// [`FontProvider::scan_folder`]. Use it to add a font to a folder that
    /// has already been scanned. Later `FontFamily="folder_uri/#Family"`
    /// lookups that match the file's faces resolve through
    /// [`FontProvider::open_font`].
    ///
    /// This calls `open_font` synchronously to read face metadata. Registering
    /// the same file twice is harmless but repeats that open and scan, so
    /// deduplicate yourself if the cost matters.
    ///
    /// # Panics
    ///
    /// Panics if `folder_uri` or `filename` contain interior NUL bytes.
    pub fn register_font(&self, folder_uri: &str, filename: &str) {
        use std::ffi::CString;
        let folder = CString::new(folder_uri).expect("folder_uri contained interior NUL");
        let name = CString::new(filename).expect("filename contained interior NUL");
        // SAFETY: `self.handle` is a live `RustFontProvider` for the lifetime of
        // `self`; the CStrings outlive the synchronous call.
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
        // Hold the lock across check + uninstall so a concurrent registration
        // for the same scope can't be cleared by this stale guard.
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
        // SAFETY: handle and userdata were created together by register_with()
        // and are freed exactly once here. Noesis's slot no longer references
        // the handle (or a newer provider replaced it), so dropping our +1
        // destroys the wrapper before its userdata is freed.
        unsafe {
            noesis_font_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Installs `provider` as the global Noesis font provider, replacing any
/// previous one. Drop the returned guard to unregister it.
///
/// # Panics
///
/// Panics if the C++ factory returns null.
pub fn set_font_provider<P: FontProvider>(provider: P) -> Registered {
    register_with(provider, Scope::Global)
}

/// Wraps `provider` in a C++ `RustFontProvider`, installs it into `scope`'s
/// slot, and records it as that scope's active registration.
fn register_with<P: FontProvider>(provider: P, scope: Scope) -> Registered {
    let outer: Box<Box<dyn FontProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { noesis_font_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("noesis_font_provider_create returned null");
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        // Hold the lock across install + record so a concurrent Drop for the
        // same scope can't uninstall the registration that just replaced it.
        // Noesis takes its own +1; ours lives until the guard drops.
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

/// Installs `provider` for font URIs with the given `scheme` (the part before
/// `://`). Noesis prefers it over the global provider for those URIs.
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `scheme` contains an interior
/// NUL byte.
pub fn set_scheme_font_provider<P: FontProvider>(scheme: &str, provider: P) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    register_with(provider, Scope::Scheme(scheme))
}

/// Installs `provider` for font URIs in `assembly` (the assembly name in a pack
/// URI).
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `assembly` contains an interior
/// NUL byte.
pub fn set_assembly_font_provider<P: FontProvider>(assembly: &str, provider: P) -> Registered {
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::Assembly(assembly))
}

/// Installs `provider` for font URIs that match both `scheme` and `assembly`.
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

/// Sets the process-wide font fallback chain, searched in order when an
/// element's `FontFamily` lacks a glyph. It also supplies the font for
/// elements that set no `FontFamily`. Entries are family names (`"Arial"`) or
/// provider paths (`"Fonts/#Bitter"`). An empty slice clears the chain.
///
/// Call it once, typically right after installing the font provider.
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

/// Sets the process-wide default font size and face properties for elements
/// that don't set them. `weight`, `stretch`, and `style` are the Noesis
/// `FontWeight`, `FontStretch`, and `FontStyle` values; `Normal` is `400`, `5`,
/// and `0` respectively.
pub fn set_font_default_properties(size: f32, weight: i32, stretch: i32, style: i32) {
    unsafe {
        crate::ffi::noesis_set_font_default_properties(size, weight, stretch, style);
    }
}
