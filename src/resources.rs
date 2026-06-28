//! `ResourceDictionary` access + application resources (TODO §7).
//!
//! XAML authors put brushes, colours, styles and templates in a
//! `<ResourceDictionary>` — either inline on an element's `Resources`, merged
//! from sibling files, or installed process-wide as the application resources.
//! This module supplies the code-built equivalents:
//!
//! * [`ResourceDictionary`] — a Rust-owned `Noesis::ResourceDictionary`. Build
//!   one, add `key → BaseComponent*` entries ([`add`](ResourceDictionary::add)
//!   / [`add_string`](ResourceDictionary::add_string)), look entries up
//!   ([`find`](ResourceDictionary::find) / [`contains`](ResourceDictionary::contains)),
//!   wire merged dictionaries ([`add_merged`](ResourceDictionary::add_merged)),
//!   or [`parse`](ResourceDictionary::parse) a `<ResourceDictionary>` from an
//!   in-memory XAML string.
//!
//! * [`set_application_resources`] / [`application_resources_present`] /
//!   [`application_resources_contains`] — install and inspect the process-global
//!   `GUI::SetApplicationResources` dictionary every [`crate::view::View`]
//!   created afterwards inherits.
//!
//! * [`register_default_styles`] — register a dictionary into the internal
//!   theme (`GUI::RegisterDefaultStyles`).
//!
//! Per-element resources and `FindResource` live on
//! [`FrameworkElement`](crate::view::FrameworkElement) (the
//! `resources` / `set_resources` / `find_resource` methods).

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::binding::Boxed;
use crate::ffi::{
    noesis_gui_get_application_resources, noesis_gui_register_default_styles,
    noesis_gui_set_application_resources, noesis_resource_dictionary_add,
    noesis_resource_dictionary_add_merged, noesis_resource_dictionary_contains,
    noesis_resource_dictionary_count, noesis_resource_dictionary_create,
    noesis_resource_dictionary_destroy, noesis_resource_dictionary_find,
    noesis_resource_dictionary_parse,
};

/// A Rust handle to a `Noesis::ResourceDictionary`. Owns a `+1` reference
/// released on drop.
///
/// Installing it ([`set_application_resources`]) or assigning it to an element
/// ([`FrameworkElement::set_resources`](crate::view::FrameworkElement::set_resources))
/// makes Noesis take its own reference, so the handle may be dropped afterwards
/// — but keeping it lets you keep mutating the live dictionary.
pub struct ResourceDictionary {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ResourceDictionary {}

impl Default for ResourceDictionary {
    fn default() -> Self {
        Self::new()
    }
}

impl ResourceDictionary {
    /// Create an empty resource dictionary.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis allocation fails (returns null) — not expected once
    /// [`crate::init`] has run.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no preconditions beyond a live Noesis runtime.
        let ptr = unsafe { noesis_resource_dictionary_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_resource_dictionary_create returned null"),
        }
    }

    /// Parse a bare `<ResourceDictionary>` from an in-memory XAML string (via
    /// `GUI::ParseXaml`). Returns `None` when the XAML is malformed or its root
    /// is not a `ResourceDictionary`.
    ///
    /// # Panics
    ///
    /// Panics if `xaml` contains an interior NUL byte.
    #[must_use]
    pub fn parse(xaml: &str) -> Option<Self> {
        let c = CString::new(xaml).expect("xaml contained interior NUL");
        // SAFETY: c.as_ptr() lives for the call; the C side only reads it while
        // parsing. The result is a freshly-created +1-owned dictionary.
        let ptr = unsafe { noesis_resource_dictionary_parse(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Wrap an already-owned (`+1`) `Noesis::ResourceDictionary*` — e.g. the
    /// result of a `get_*` accessor that `AddRef`'d before handing out.
    ///
    /// # Safety
    ///
    /// `ptr` must be a live `Noesis::ResourceDictionary*` carrying a reference
    /// this wrapper takes ownership of (released on drop).
    #[must_use]
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }

    /// Raw `Noesis::ResourceDictionary*` (a `BaseComponent*`). Borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Number of entries in the base dictionary (excludes merged dictionaries).
    #[must_use]
    pub fn len(&self) -> usize {
        // SAFETY: self.ptr is a live ResourceDictionary*.
        unsafe { noesis_resource_dictionary_count(self.ptr.as_ptr()) as usize }
    }

    /// Whether the base dictionary is empty (ignores merged dictionaries).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Add a `BaseComponent*` under `key`; the dictionary takes its own
    /// reference, so the caller retains ownership of `value`. Returns `false`
    /// on a NULL `value` (or a key with no effect).
    ///
    /// # Safety
    ///
    /// `value` must be a live `Noesis::BaseComponent*` (e.g. [`Boxed::raw`],
    /// [`crate::styles::Style::raw`], a brush, …).
    ///
    /// # Panics
    ///
    /// Panics if `key` contains an interior NUL byte.
    pub unsafe fn add(&mut self, key: &str, value: *mut c_void) -> bool {
        let c = CString::new(key).expect("resource key contained interior NUL");
        // SAFETY: self.ptr live; c lives for the call; value is the caller's
        // responsibility per the # Safety contract. The dictionary AddRefs.
        unsafe { noesis_resource_dictionary_add(self.ptr.as_ptr(), c.as_ptr(), value) }
    }

    /// Convenience: box `value` as a `BoxedValue<String>` and add it under
    /// `key`. The dictionary takes its own reference to the boxed value.
    ///
    /// # Panics
    ///
    /// Panics if `key` or `value` contain an interior NUL byte.
    pub fn add_string(&mut self, key: &str, value: &str) -> bool {
        let boxed = crate::binding::box_string(value);
        // SAFETY: `boxed` is a live BaseComponent* for the duration of the call;
        // it drops afterwards, releasing our ref while the dictionary keeps its own.
        unsafe { self.add(key, boxed.raw()) }
    }

    /// Add a boxed value under `key`. Thin sugar over [`add`](Self::add) that
    /// keeps the [`Boxed`] borrow contract explicit.
    ///
    /// # Panics
    ///
    /// Panics if `key` contains an interior NUL byte.
    pub fn add_boxed(&mut self, key: &str, value: &Boxed) -> bool {
        // SAFETY: value.raw() is a live BaseComponent* for the call.
        unsafe { self.add(key, value.raw()) }
    }

    /// Whether the dictionary (or one of its merged dictionaries) contains
    /// `key`.
    ///
    /// # Panics
    ///
    /// Panics if `key` contains an interior NUL byte.
    #[must_use]
    pub fn contains(&self, key: &str) -> bool {
        let c = CString::new(key).expect("resource key contained interior NUL");
        // SAFETY: self.ptr live; c lives for the call.
        unsafe { noesis_resource_dictionary_contains(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Borrowed (no `+1`) pointer to the value stored under `key`, or `None` if
    /// absent. The dictionary owns the reference; copy / re-root it if you need
    /// it past the next mutation. Uses the non-throwing `Find`, so a miss is a
    /// clean `None`.
    ///
    /// # Panics
    ///
    /// Panics if `key` contains an interior NUL byte.
    #[must_use]
    pub fn find(&self, key: &str) -> Option<NonNull<c_void>> {
        let c = CString::new(key).expect("resource key contained interior NUL");
        // SAFETY: self.ptr live; c lives for the call. The returned pointer is
        // borrowed (owned by the dictionary).
        let p = unsafe { noesis_resource_dictionary_find(self.ptr.as_ptr(), c.as_ptr()) };
        NonNull::new(p)
    }

    /// Add `other` to this dictionary's `MergedDictionaries` collection — its
    /// entries become resolvable through this dictionary (WPF/Noesis merge
    /// semantics). The collection takes its own reference to `other`. Returns
    /// `false` if the merged-dictionaries collection is unavailable.
    pub fn add_merged(&mut self, other: &ResourceDictionary) -> bool {
        // SAFETY: both pointers are live ResourceDictionary*; the collection
        // AddRefs `other`.
        unsafe { noesis_resource_dictionary_add_merged(self.ptr.as_ptr(), other.raw()) }
    }
}

impl Drop for ResourceDictionary {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (create / parse / from_owned).
        unsafe { noesis_resource_dictionary_destroy(self.ptr.as_ptr()) }
    }
}

/// Install `dict` as the process-global application resources
/// (`GUI::SetApplicationResources`). Every [`crate::view::View`] created
/// afterwards inherits these styles, brushes and templates. Noesis takes its
/// own reference, so `dict` may be dropped afterwards (the resources stay
/// installed). Replaces any previously-installed dictionary.
///
/// This is the code-built counterpart to
/// [`crate::gui::load_application_resources`] (which loads a dictionary by URI
/// through the XAML provider).
pub fn set_application_resources(dict: &ResourceDictionary) {
    // SAFETY: dict.raw() is a live ResourceDictionary*; Noesis AddRefs it.
    unsafe { noesis_gui_set_application_resources(dict.raw()) }
}

/// Whether any application resources dictionary is currently installed
/// (`GUI::GetApplicationResources() != null`).
#[must_use]
pub fn application_resources_present() -> bool {
    // SAFETY: borrowed getter; the pointer is only compared against null.
    !unsafe { noesis_gui_get_application_resources() }.is_null()
}

/// Whether the installed application resources contain `key` (including its
/// merged dictionaries). `false` if no application resources are installed.
///
/// # Panics
///
/// Panics if `key` contains an interior NUL byte.
#[must_use]
pub fn application_resources_contains(key: &str) -> bool {
    // SAFETY: borrowed app-resources pointer, valid for this call; we forward
    // it (without releasing) to the dictionary `contains` query.
    let app = unsafe { noesis_gui_get_application_resources() };
    if app.is_null() {
        return false;
    }
    let c = CString::new(key).expect("resource key contained interior NUL");
    // SAFETY: `app` is a live (borrowed) ResourceDictionary*; c lives for the call.
    unsafe { noesis_resource_dictionary_contains(app, c.as_ptr()) }
}

/// Register `uri`'s `ResourceDictionary` into the internal theme
/// (`GUI::RegisterDefaultStyles`) — useful for setting default styles that
/// implicit-keyed `Style`s (those without an `x:Key`) resolve against. Requires
/// a XAML provider that can serve `uri`. Returns `false` on an empty `uri`.
///
/// # Panics
///
/// Panics if `uri` contains an interior NUL byte.
#[must_use]
pub fn register_default_styles(uri: &str) -> bool {
    let c = CString::new(uri).expect("uri contained interior NUL");
    // SAFETY: c lives for the call; the C side copies into Noesis::Uri.
    unsafe { noesis_gui_register_default_styles(c.as_ptr()) }
}
