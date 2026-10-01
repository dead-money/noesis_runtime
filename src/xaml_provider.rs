//! Tell Noesis where to find your XAML.
//!
//! Implement [`XamlProvider`], then install it with [`set_xaml_provider`] or
//! one of the scoped variants ([`set_scheme_xaml_provider`],
//! [`set_assembly_xaml_provider`], [`set_scheme_assembly_xaml_provider`]).
//! Noesis then calls your provider whenever it loads XAML by URI, for example
//! from [`FrameworkElement::load`](crate::view::FrameworkElement::load).
//!
//! # Lifetime
//!
//! Keep the returned [`Registered`] guard alive for as long as Noesis should
//! use your provider. Dropping it unregisters the provider and frees it. Each
//! scope holds one provider: installing another into the same scope replaces
//! the first, and dropping the older guard then leaves the newer one in place.
//! You don't need to call [`crate::shutdown`] first.

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

/// Noesis provider slot a [`Registered`] guard installed into; also the key
/// into [`ACTIVE`].
#[derive(Clone, PartialEq, Eq)]
enum Scope {
    Global,
    Scheme(CString),
    Assembly(CString),
    SchemeAssembly(CString, CString),
}

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Id of the active registration per scope. A guard's `Drop` clears the Noesis
/// slot only if its id still matches, so a stale guard can't tear down a newer
/// registration for the same scope.
static ACTIVE: Mutex<Vec<(Scope, u64)>> = Mutex::new(Vec::new());

/// Installs `handle` into the Noesis slot named by `scope`; null clears it.
///
/// # Safety
///
/// `handle` must be a live `RustXamlProvider*` or null.
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

/// Resolves XAML URIs to bytes. Implement it to serve XAML from memory, an
/// archive, or your asset pipeline, then install it with
/// [`set_xaml_provider`].
///
/// Noesis reads the bytes returned from [`load_xaml`](Self::load_xaml) in
/// place, without copying, while it parses. The parse finishes before the load
/// call that asked for the URI returns, so returning a borrow of data the
/// provider owns (for example a `HashMap<String, Vec<u8>>`) is enough.
///
/// The `Send + Sync` bounds let the [`Registered`] guard move to the thread
/// that drives your view. The guard is `Send` but not `Sync`; see the
/// crate-level "Thread affinity" docs.
pub trait XamlProvider: Send + Sync + 'static {
    /// Downcast hook for [`Registered::provider_mut`]. Every impl is the same:
    ///
    /// ```ignore
    /// fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    /// ```
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Returns the XAML bytes for `uri`, or `None` if the URI is unknown.
    ///
    /// A non-UTF-8 URI arrives lossily decoded. Returning more than 4 GiB is
    /// treated as `None`.
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]>;
}

// SAFETY: `userdata` must come from `register_with` and its `Registered`
// guard must not have been dropped.
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
        // lossy so a non-UTF-8 URI can't panic across the C ABI
        let uri_str = if uri.is_null() {
            std::borrow::Cow::Borrowed("")
        } else {
            CStr::from_ptr(uri).to_string_lossy()
        };
        let Some(bytes) = provider(userdata).load_xaml(&uri_str) else {
            return false;
        };
        // the C ABI length is u32; fail rather than panic in the trampoline
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

/// Guard for an installed [`XamlProvider`]; owns the provider.
///
/// Dropping it unregisters the provider, unless a newer registration has since
/// replaced it in the same scope, and frees it. You don't need to call
/// [`crate::shutdown`] first.
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
    /// Raw `Noesis::XamlProvider*`, valid while `self` is alive. No reference
    /// is added.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    /// Mutable access to the installed provider, for example to add or
    /// replace XAML after installation.
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
        // lock held across check + uninstall: atomic against a concurrent
        // registration for the same scope
        {
            let mut active = ACTIVE.lock().expect("xaml provider registry poisoned");
            if let Some(pos) = active
                .iter()
                .position(|(s, i)| *s == self.scope && *i == self.id)
            {
                active.swap_remove(pos);
                // SAFETY: null clears our slot. Noesis drops its ref here, so
                // nothing it holds points at the userdata freed below.
                unsafe { install(&self.scope, core::ptr::null_mut()) };
            }
        }
        // SAFETY: handle and userdata come from register_with() and are freed
        // exactly once, here. destroy drops our +1 on the C++ wrapper.
        unsafe {
            noesis_xaml_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Installs `provider` as the global XAML provider. Drop the returned
/// [`Registered`] guard to uninstall it.
///
/// # Panics
///
/// Panics if the native provider can't be created, which indicates a bug in
/// this crate.
pub fn set_xaml_provider<P: XamlProvider + 'static>(provider: P) -> Registered {
    register_with(provider, Scope::Global)
}

fn register_with<P: XamlProvider + 'static>(provider: P, scope: Scope) -> Registered {
    // Double-Box gives a stable thin pointer for the C ABI userdata.
    let outer: Box<Box<dyn XamlProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { noesis_xaml_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("noesis_xaml_provider_create returned null");
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    {
        // lock held across install + record so a concurrent Drop for this
        // scope can't uninstall the registration that just replaced it.
        // Noesis takes its own ref; ours lives until the guard drops.
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

/// Installs `provider` for URIs with the given `scheme`, such as `"ui"` for
/// `ui:///menus/main.xaml`. Drop the returned [`Registered`] guard to
/// uninstall it.
///
/// # Panics
///
/// Panics if `scheme` contains an interior NUL byte, or if the native provider
/// can't be created.
pub fn set_scheme_xaml_provider<P: XamlProvider + 'static>(
    scheme: &str,
    provider: P,
) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    register_with(provider, Scope::Scheme(scheme))
}

/// Installs `provider` for pack URIs naming `assembly`, such as `"MyApp"` in
/// `pack://application:,,,/MyApp;component/main.xaml`. Drop the returned
/// [`Registered`] guard to uninstall it.
///
/// # Panics
///
/// Panics if `assembly` contains an interior NUL byte, or if the native
/// provider can't be created.
pub fn set_assembly_xaml_provider<P: XamlProvider + 'static>(
    assembly: &str,
    provider: P,
) -> Registered {
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::Assembly(assembly))
}

/// Installs `provider` for URIs that match both `scheme` and `assembly`. Drop
/// the returned [`Registered`] guard to uninstall it.
///
/// # Panics
///
/// Panics if `scheme` or `assembly` contains an interior NUL byte, or if the
/// native provider can't be created.
pub fn set_scheme_assembly_xaml_provider<P: XamlProvider + 'static>(
    scheme: &str,
    assembly: &str,
    provider: P,
) -> Registered {
    let scheme = CString::new(scheme).expect("scheme contained interior NUL");
    let assembly = CString::new(assembly).expect("assembly contained interior NUL");
    register_with(provider, Scope::SchemeAssembly(scheme, assembly))
}
