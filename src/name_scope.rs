//! XAML namescopes: the tables that map `x:Name` values to objects.
//!
//! Use [`NameScope`] to work with a scope object directly: create one and
//! attach it to an element with [`NameScope::set_on`], fetch an element's scope
//! with [`NameScope::of`], then register and look up names. For the common case
//! of finding a named element, [`FrameworkElement::find_name`] is simpler.

use core::ptr::NonNull;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_void};

use crate::ffi::{
    noesis_base_component_release, noesis_name_scope_create, noesis_name_scope_enum,
    noesis_name_scope_find_name, noesis_name_scope_find_object, noesis_name_scope_get,
    noesis_name_scope_register_name, noesis_name_scope_set, noesis_name_scope_unregister_name,
    noesis_name_scope_update_name,
};
use crate::view::FrameworkElement;

/// A `Noesis::NameScope`. The handle holds one reference, released on drop.
pub struct NameScope {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for NameScope {}

impl NameScope {
    /// Creates an empty namescope.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the namescope.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: the C side returns a freshly-created NameScope* at +1.
        let ptr = unsafe { noesis_name_scope_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_name_scope_create returned null"),
        }
    }

    /// The namescope attached directly to `element`, or `None`. Does not search
    /// ancestors.
    #[must_use]
    pub fn of(element: &FrameworkElement) -> Option<Self> {
        // SAFETY: element.raw() is a live BaseComponent*; the C side AddRef's
        // any attached scope before handing it back.
        let ptr = unsafe { noesis_name_scope_get(element.raw()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Attaches `scope` to `element`, or clears it with `None`. Noesis holds its
    /// own reference to the scope. Returns `false` if the scope was not set.
    #[must_use = "a false return means the scope was not set (element is not a DependencyObject)"]
    pub fn set_on(element: &mut FrameworkElement, scope: Option<&NameScope>) -> bool {
        let scope_ptr = scope.map_or(core::ptr::null_mut(), NameScope::raw);
        // SAFETY: both pointers are live (or null to clear); Noesis stores its
        // own reference to the scope.
        unsafe { noesis_name_scope_set(element.raw(), scope_ptr) }
    }

    /// The object registered under `name`, or `None`.
    ///
    /// A scope can hold any object, such as a `Storyboard`, so the result is
    /// not necessarily an element. Element methods called on a non-element
    /// handle do nothing or return `None`.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn find_name(&self, name: &str) -> Option<FrameworkElement> {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr is a live NameScope*; c lives for the call; the C
        // side AddRef's the found object (or returns NULL).
        let ptr = unsafe { noesis_name_scope_find_name(self.ptr.as_ptr(), c.as_ptr()) };
        // SAFETY: ptr is a +1 BaseComponent* we take ownership of.
        NonNull::new(ptr).map(|ptr| unsafe { FrameworkElement::from_owned(ptr) })
    }

    /// Registers `obj` under `name`. The scope holds its own reference. Do not
    /// register a name twice; Noesis leaves that undefined. Use
    /// [`Self::update_name`] to replace an entry.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn register_name(&mut self, name: &str, obj: &FrameworkElement) {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: all pointers are live; c lives for the call.
        unsafe { noesis_name_scope_register_name(self.ptr.as_ptr(), c.as_ptr(), obj.raw()) };
    }

    /// Removes `name` from this scope.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn unregister_name(&mut self, name: &str) {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr is a live NameScope*; c lives for the call.
        unsafe { noesis_name_scope_unregister_name(self.ptr.as_ptr(), c.as_ptr()) };
    }

    /// Replaces the object registered under `name` with `obj`.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn update_name(&mut self, name: &str, obj: &FrameworkElement) {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: all pointers are live; c lives for the call.
        unsafe { noesis_name_scope_update_name(self.ptr.as_ptr(), c.as_ptr(), obj.raw()) };
    }

    /// The name `obj` is registered under in this scope, or `None`.
    #[must_use]
    pub fn find_object(&self, obj: &FrameworkElement) -> Option<String> {
        // SAFETY: both pointers are live; the returned C string is owned by the
        // scope and valid until the scope is mutated. We copy it immediately.
        let p = unsafe { noesis_name_scope_find_object(self.ptr.as_ptr(), obj.raw()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: p is a live, NUL-terminated C string from Noesis.
        let s = unsafe { CStr::from_ptr(p) };
        Some(s.to_string_lossy().into_owned())
    }

    /// Calls `f` with each name and object in this scope. The object is
    /// borrowed for that call only; use
    /// [`clone_ref`](FrameworkElement::clone_ref) to keep it. Entries whose name
    /// is not valid UTF-8 are skipped.
    pub fn for_each<F: FnMut(&str, &FrameworkElement)>(&self, mut f: F) {
        let mut callback: &mut dyn FnMut(&str, &FrameworkElement) = &mut f;

        unsafe extern "C" fn tramp(ud: *mut c_void, name: *const c_char, obj: *mut c_void) {
            crate::panic_guard::guard(|| {
                // SAFETY: ud is the &mut &mut dyn FnMut passed below.
                let f = unsafe { &mut *ud.cast::<&mut dyn FnMut(&str, &FrameworkElement)>() };
                let name = if name.is_null() {
                    ""
                } else {
                    // SAFETY: name is a live, NUL-terminated C string for this call.
                    match unsafe { CStr::from_ptr(name) }.to_str() {
                        Ok(s) => s,
                        Err(_) => return,
                    }
                };
                if let Some(p) = NonNull::new(obj) {
                    // SAFETY: from_owned just stores the ptr; ManuallyDrop keeps the
                    // borrowed object from being Released here.
                    let elem =
                        core::mem::ManuallyDrop::new(unsafe { FrameworkElement::from_owned(p) });
                    f(name, &elem);
                }
            })
        }

        // SAFETY: tramp matches the C ABI; `callback` outlives the synchronous
        // enumeration; self.ptr is a live NameScope*.
        unsafe {
            noesis_name_scope_enum(self.ptr.as_ptr(), tramp, (&raw mut callback).cast());
        }
    }

    /// The underlying `Noesis::NameScope*`, valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Default for NameScope {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NameScope {
    fn drop(&mut self) {
        // SAFETY: self.ptr carries a +1 we own; released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) };
    }
}
