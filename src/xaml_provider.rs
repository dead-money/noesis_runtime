//! Rust-side [`XamlProvider`] trait + the [`set_xaml_provider`] registration
//! entrypoint. Mirrors `crate::render_device::vtable::register` — a boxed
//! trait object is handed to the C++ `RustXamlProvider` subclass via a vtable
//! of trampolines; the returned [`Registered`] guard owns both the boxed impl
//! and the C++ provider handle.
//!
//! # Lifetime
//!
//! The `Registered` guard must outlive every Noesis-internal reference that
//! might call back into [`XamlProvider::load_xaml`]. In practice that means
//! keeping it alive until after `noesis_runtime::shutdown()` returns — the latter
//! releases Noesis's internal `Ptr<XamlProvider>`, after which the C++
//! wrapper's refcount drops to 1 (ours). Dropping the guard then releases the
//! final ref, fires the C++ destructor, and frees the boxed Rust impl.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};
use std::os::raw::c_char;

use crate::ffi::{
    XamlProviderVTable, noesis_set_xaml_provider, noesis_set_xaml_provider_assembly,
    noesis_set_xaml_provider_scheme, noesis_set_xaml_provider_scheme_assembly,
    noesis_xaml_provider_create, noesis_xaml_provider_destroy,
};

/// Rust-side XAML provider. The bytes returned from [`load_xaml`] are wrapped
/// in a Noesis `MemoryStream` *without copying* and must stay valid until the
/// XAML parse that triggered the lookup returns. Since Noesis parses
/// synchronously inside `GUI::LoadXaml`, storing the bytes in `&self` (e.g.
/// a `HashMap<String, Vec<u8>>`) and returning a borrow is sufficient.
///
/// [`load_xaml`]: Self::load_xaml
///
/// `Send + Sync` supertraits make the boxed impl `Send` (so the
/// [`Registered`] guard can be *moved* across threads); the guard is `Send`
/// but **not** `Sync` (it has `&self` Noesis accessors). Store it in a
/// `NonSend` resource. Safety rationale identical to
/// [`crate::render_device::RenderDevice`].
///
/// [`Registered`]: Registered
pub trait XamlProvider: Send + Sync + 'static {
    /// Downcast escape hatch used by [`Registered::provider_mut`]. Standard
    /// one-line body for every impl:
    ///
    /// ```ignore
    /// fn as_any_mut(&mut self) -> &mut dyn std::any::Any { self }
    /// ```
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;

    /// Return the XAML bytes for `uri`, or `None` if the URI is unknown.
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]>;
}

// ────────────────────────────────────────────────────────────────────────────
// Trampoline
// ────────────────────────────────────────────────────────────────────────────

/// SAFETY: `userdata` must be a pointer produced by `register_xaml_provider`
/// and still alive (the [`Registered`] guard hasn't been dropped).
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
        // A >4 GiB document can't be represented to the shim — treat as failure
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

// ────────────────────────────────────────────────────────────────────────────
// Registered — RAII wrapper holding the boxed impl and the C++ provider
// ────────────────────────────────────────────────────────────────────────────

/// Owns a Rust [`XamlProvider`] impl together with its C++ `RustXamlProvider`
/// instance. Dropping releases the +1 ref we hold on the C++ side and frees
/// the boxed impl. The caller is responsible for having called
/// [`crate::shutdown`] before this drop so Noesis's own `Ptr<XamlProvider>`
/// is already released; otherwise the final destructor fires later than
/// expected and the boxed impl outlives its C++ wrapper briefly (still
/// safe — no further callbacks are possible after `Shutdown`).
pub struct Registered {
    handle: NonNull<c_void>,
    userdata: NonNull<Box<dyn XamlProvider>>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Registered {}

impl Registered {
    /// Raw `Noesis::XamlProvider*` — useful for passing to other Noesis APIs
    /// that take a provider. Borrowed for the lifetime of this `Registered`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.handle.as_ptr()
    }

    /// Mutable access to the concrete [`XamlProvider`] impl behind the
    /// registration. The type parameter `P` must match what was passed to
    /// [`set_xaml_provider`]; enforced at runtime via `dyn Any` downcast.
    ///
    /// # Panics
    ///
    /// Panics if `P` is not the concrete type passed to `set_xaml_provider`.
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
        // SAFETY: handle + userdata produced together by register(); both
        // freed exactly once here.
        unsafe {
            noesis_xaml_provider_destroy(self.handle.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Install `provider` as the global Noesis XAML provider. Holds both the
/// boxed trait object and the C++ wrapper; drop the returned [`Registered`]
/// guard to tear everything down (after [`crate::shutdown`]).
///
/// # Panics
///
/// Panics if the C++ factory returns null (only possible on internal logic
/// errors).
pub fn set_xaml_provider<P: XamlProvider + 'static>(provider: P) -> Registered {
    // SAFETY: install globally — Noesis retains its own +1.
    register_with(provider, |handle| unsafe {
        noesis_set_xaml_provider(handle)
    })
}

/// Build the C++ `RustXamlProvider` wrapping `provider`, hand its handle to
/// `install` (the only thing that differs between the global / scheme /
/// assembly variants), and return the owning [`Registered`] guard. Keeps the
/// four public setters DRY.
fn register_with<P: XamlProvider + 'static>(
    provider: P,
    install: impl FnOnce(*mut c_void),
) -> Registered {
    // Double-Box gives a stable thin pointer for the C ABI userdata.
    let outer: Box<Box<dyn XamlProvider>> = Box::new(Box::new(provider));
    let userdata = Box::into_raw(outer);
    // SAFETY: VTABLE is 'static; userdata is freshly leaked.
    let handle = unsafe { noesis_xaml_provider_create(&raw const VTABLE, userdata.cast()) };
    let handle = NonNull::new(handle).expect("noesis_xaml_provider_create returned null");
    // Noesis retains its own +1; we keep ours until the Registered is dropped.
    install(handle.as_ptr());

    Registered {
        handle,
        userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
    }
}

/// Install `provider` as the XAML provider for the URI `scheme` (the part
/// before `://`, e.g. `"pack"` for `pack://...`). Noesis consults the
/// scheme-scoped provider for matching URIs in preference to the global one.
/// Reuses the same trampoline + [`Registered`] machinery as
/// [`set_xaml_provider`]; only the install call differs.
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
    register_with(provider, move |handle| {
        // SAFETY: handle is a live RustXamlProvider*; `scheme` outlives the call.
        unsafe { noesis_set_xaml_provider_scheme(scheme.as_ptr(), handle) }
    })
}

/// Install `provider` as the XAML provider for `assembly` (the assembly name in
/// a pack URI, e.g. `MyApp` in `pack://application:,,,/MyApp;component/...`).
/// Reuses [`set_xaml_provider`]'s machinery; only the install call differs.
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
    register_with(provider, move |handle| {
        // SAFETY: handle is a live RustXamlProvider*; `assembly` outlives the call.
        unsafe { noesis_set_xaml_provider_assembly(assembly.as_ptr(), handle) }
    })
}

/// Install `provider` as the XAML provider scoped to both a `scheme` and an
/// `assembly`. Reuses [`set_xaml_provider`]'s machinery; only the install call
/// differs.
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
    register_with(provider, move |handle| {
        // SAFETY: handle is a live RustXamlProvider*; both CStrings outlive the call.
        unsafe {
            noesis_set_xaml_provider_scheme_assembly(scheme.as_ptr(), assembly.as_ptr(), handle)
        }
    })
}
