//! Teach Noesis where to find your XAML. Implement the [`XamlProvider`] trait,
//! then call [`set_xaml_provider`] (or one of the scheme/assembly-scoped
//! variants) to install it. Your boxed impl is handed to a C++
//! `RustXamlProvider` subclass through a vtable of trampolines, and the returned
//! [`Registered`] guard owns both the boxed impl and the C++ provider handle.
//!
//! # Lifetime
//!
//! Keep the [`Registered`] guard alive as long as Noesis should serve XAML
//! through your provider. Dropping it unregisters the provider from Noesis
//! (clearing the slot this guard installed into — unless a newer registration
//! for the same scope has replaced it), releases the C++ wrapper, and frees the
//! boxed Rust impl. There is no need to call [`crate::shutdown`] first.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::ffi::{
    XamlProviderVTable, noesis_set_xaml_provider, noesis_set_xaml_provider_assembly,
    noesis_set_xaml_provider_scheme, noesis_set_xaml_provider_scheme_assembly,
    noesis_xaml_provider_create, noesis_xaml_provider_destroy,
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
/// same scope. Mirrors `integration.rs`'s per-hook `*_ACTIVE` scheme, extended
/// to a small per-scope table because scheme/assembly slots are independent.
static ACTIVE: Mutex<Vec<(Scope, u64)>> = Mutex::new(Vec::new());

/// Install `handle` (or null, to clear) into the Noesis slot named by `scope`.
///
/// # Safety
///
/// `handle` must be a live `RustXamlProvider*` or null; the `Scope`'s `CStrings`
/// outlive the call.
unsafe fn install(scope: &Scope, handle: *mut c_void) {
    match scope {
        Scope::Global => noesis_set_xaml_provider(handle),
        Scope::Scheme(s) => noesis_set_xaml_provider_scheme(s.as_ptr(), handle),
        Scope::Assembly(a) => noesis_set_xaml_provider_assembly(a.as_ptr(), handle),
        Scope::SchemeAssembly(s, a) => {
            noesis_set_xaml_provider_scheme_assembly(s.as_ptr(), a.as_ptr(), handle)
        }
    }
}

/// Resolves XAML URIs to bytes on demand. Implement this to serve XAML from
/// memory, an archive, an asset pipeline, or anywhere else, then register it
/// with [`set_xaml_provider`].
///
/// The bytes returned from [`load_xaml`] are wrapped in a Noesis `MemoryStream`
/// *without copying*, so they must stay valid until the XAML parse that
/// triggered the lookup returns. Noesis parses synchronously inside
/// `GUI::LoadXaml`, so storing the bytes in `&self` (e.g. a
/// `HashMap<String, Vec<u8>>`) and returning a borrow is enough.
///
/// [`load_xaml`]: Self::load_xaml
///
/// The `Send + Sync` supertraits make the boxed impl `Send`, so the
/// [`Registered`] guard can be *moved* across threads; the guard is `Send` but
/// **not** `Sync` (it exposes `&self` Noesis accessors), so store it in a
/// `NonSend` resource. Same threading rationale as
/// [`crate::render_device::RenderDevice`].
///
/// [`Registered`]: Registered
pub trait XamlProvider: Send + Sync + 'static {
    /// Downcast hook that powers [`Registered::provider_mut`]. Every impl is the
    /// same one-liner:
    ///
    /// ```ignore
    /// fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    /// ```
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Return the XAML bytes for `uri`, or `None` if the URI is unknown.
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]>;
}

// SAFETY: `userdata` must be a pointer produced by `register_with` and still
// alive (the `Registered` guard hasn't been dropped).
unsafe fn provider<'a>(userdata: *mut c_void) -> &'a mut Box<dyn XamlProvider> {
    &mut *userdata.cast::<Box<dyn XamlProvider>>()
}

unsafe extern "C" fn t_load_xaml(
    userdata: *mut c_void,
    uri: *const c_char,
    out_data: *mut *const u8,
    out_len: *mut u32,
) -> bool {
    crate::panic_guard::guard(|| {
        // Noesis URIs are normally ASCII/UTF-8; decode lossily so a stray
        // non-UTF-8 URI can't panic across the C ABI (it just won't match).
        let uri_str = if uri.is_null() {
            std::borrow::Cow::Borrowed("")
        } else {
            CStr::from_ptr(uri).to_string_lossy()
        };
        let Some(bytes) = provider(userdata).load_xaml(&uri_str) else {
            return false;
        };
        // A >4 GiB document can't be represented to the shim; treat as failure
        // rather than panicking inside the trampoline.
        let Ok(len) = u32::try_from(bytes.len()) else {
            return false;
        };
        out_data.write(bytes.as_ptr());
        out_len.write(len);
        true
    })
}

static VTABLE: XamlProviderVTable = XamlProviderVTable {
    load_xaml: t_load_xaml,
};

/// Owns a Rust [`XamlProvider`] impl together with its C++ `RustXamlProvider`
/// instance. Dropping unregisters the provider from Noesis (clearing the slot
/// this guard installed into, unless a newer registration for the same scope
/// has since replaced it), releases the +1 ref we hold on the C++ side, and
/// frees the boxed impl. There is no requirement to call [`crate::shutdown`]
/// first.
#[must_use = "dropping the guard unregisters the provider and frees it"]
pub struct Registered {
    handle: NonNull<c_void>,
    userdata: NonNull<Box<dyn XamlProvider>>,
    scope: Scope,
    id: u64,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Registered {}

impl Registered {
    /// Raw `Noesis::XamlProvider*`, for passing to other Noesis APIs that
    /// take a provider. Borrowed for the lifetime of this `Registered`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    /// Mutable access to the concrete [`XamlProvider`] impl behind the
    /// registration. `P` must be the concrete type you registered (via
    /// [`set_xaml_provider`] or a scoped variant); enforced at runtime via a
    /// `dyn Any` downcast.
    ///
    /// # Panics
    ///
    /// Panics if `P` is not the concrete type that was registered.
    pub fn provider_mut<P: XamlProvider>(&mut self) -> &mut P {
        // SAFETY: userdata points at the live Box<dyn XamlProvider> produced
        // by set_xaml_provider(); borrow scoped to &mut self.
        let boxed: &mut Box<dyn XamlProvider> = unsafe { self.userdata.as_mut() };
        (**boxed)
            .as_any_mut()
            .downcast_mut::<P>()
            .expect("Registered::provider_mut: type does not match set_xaml_provider")
    }
}

impl Drop for Registered {
    fn drop(&mut self) {
        // Clear the Noesis slot only while we're still its active registration;
        // a newer set_*_provider for the same scope must keep firing. Hold the
        // lock across the check + uninstall so it stays atomic against a
        // concurrent registration.
        {
            let mut active = ACTIVE.lock().expect("xaml provider registry poisoned");
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
            noesis_xaml_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Install `provider` as the global Noesis XAML provider. Holds both the
/// boxed trait object and the C++ wrapper; drop the returned [`Registered`]
/// guard to unregister the provider and tear everything down.
///
/// # Panics
///
/// Panics if the C++ factory returns null (only possible on internal logic
/// errors).
pub fn set_xaml_provider<P: XamlProvider + 'static>(provider: P) -> Registered {
    register_with(provider, Scope::Global)
}

/// Shared construction for all four setters: build the C++ wrapper, install it
/// into the slot named by `scope` (the only step that varies), record it as
/// that scope's active registration, and return the owning guard.
fn register_with<P: XamlProvider + 'static>(provider: P, scope: Scope) -> Registered {
    // Double-Box gives a stable thin pointer for the C ABI userdata.
    let outer: Box<Box<dyn XamlProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { noesis_xaml_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("noesis_xaml_provider_create returned null");
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        // Hold the registry lock across install + record so a concurrent Drop
        // for the same scope can't observe a half-updated slot and uninstall a
        // registration that just replaced it. Noesis retains its own +1; we
        // keep ours until the Registered is dropped.
        let mut active = ACTIVE.lock().expect("xaml provider registry poisoned");
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

/// Install `provider` as the XAML provider for the URI `scheme` (the part
/// before `://`, e.g. `"pack"` for `pack://...`). Noesis consults the
/// scheme-scoped provider for matching URIs in preference to the global one.
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `scheme` contains an interior
/// NUL byte.
pub fn set_scheme_xaml_provider<P: XamlProvider + 'static>(
    scheme: &str,
    provider: P,
) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    register_with(provider, Scope::Scheme(scheme))
}

/// Install `provider` as the XAML provider for `assembly` (the assembly name in
/// a pack URI, e.g. `MyApp` in `pack://application:,,,/MyApp;component/...`).
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `assembly` contains an interior
/// NUL byte.
pub fn set_assembly_xaml_provider<P: XamlProvider + 'static>(
    assembly: &str,
    provider: P,
) -> Registered {
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::Assembly(assembly))
}

/// Install `provider` as the XAML provider scoped to both a `scheme` and an
/// `assembly`.
///
/// # Panics
///
/// Panics if the C++ factory returns null, or `scheme` / `assembly` contain an
/// interior NUL byte.
pub fn set_scheme_assembly_xaml_provider<P: XamlProvider + 'static>(
    scheme: &str,
    assembly: &str,
    provider: P,
) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::SchemeAssembly(scheme, assembly))
}
