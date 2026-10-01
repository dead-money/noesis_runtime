//! Build resource dictionaries in code and install application resources.
//!
//! In XAML, brushes, colors, styles and templates live in a
//! `<ResourceDictionary>`: on an element's `Resources`, merged in from other
//! files, or installed process-wide as the application resources. This module
//! is the code-side equivalent:
//!
//! - [`ResourceDictionary`] owns a dictionary. Create one empty or
//!   [`parse`](ResourceDictionary::parse) it from XAML, add entries
//!   ([`add_brush`](ResourceDictionary::add_brush),
//!   [`add_string`](ResourceDictionary::add_string),
//!   [`add_boxed`](ResourceDictionary::add_boxed)), look them up
//!   ([`find`](ResourceDictionary::find),
//!   [`contains`](ResourceDictionary::contains)) and merge other dictionaries
//!   in ([`add_merged`](ResourceDictionary::add_merged)).
//! - [`set_application_resources`] installs a dictionary that every
//!   [`View`](crate::view::View) created afterwards inherits;
//!   [`application_resources_present`] and [`application_resources_contains`]
//!   inspect it.
//! - [`register_default_styles`] adds a dictionary to the default theme.
//!
//! Per-element resources are on
//! [`FrameworkElement`](crate::view::FrameworkElement): `resources`,
//! `set_resources` and `find_resource`.

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::binding::Boxed;
use crate::ffi::{
    noesis_gui_get_application_resources, noesis_gui_register_default_styles,
    noesis_gui_set_application_resources, noesis_resource_dictionary_add,
    noesis_resource_dictionary_add_merged, noesis_resource_dictionary_contains,
    noesis_resource_dictionary_count, noesis_resource_dictionary_create,
    noesis_resource_dictionary_destroy, noesis_resource_dictionary_find,
    noesis_resource_dictionary_parse, noesis_resource_dictionary_set_source,
};

/// An owned `Noesis::ResourceDictionary`, released on drop.
///
/// Installing it ([`set_application_resources`]) or assigning it to an element
/// ([`FrameworkElement::set_resources`](crate::view::FrameworkElement::set_resources))
/// gives Noesis its own reference, so you can drop the handle afterwards, or
/// keep it to go on editing the live dictionary.
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
    /// Panics if Noesis returns null, which is not expected once
    /// [`crate::init`] has run.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no preconditions beyond a live Noesis runtime.
        let ptr = unsafe { noesis_resource_dictionary_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_resource_dictionary_create returned null"),
        }
    }

    /// Parse a `<ResourceDictionary>` from a XAML string. Returns `None` when
    /// the XAML is malformed or its root is not a `ResourceDictionary`.
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

    /// Wrap a `Noesis::ResourceDictionary*` that carries a reference this
    /// handle takes over.
    ///
    /// # Safety
    ///
    /// `ptr` must be a live `Noesis::ResourceDictionary*` carrying a reference
    /// this wrapper takes ownership of (released on drop).
    #[must_use]
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }

    /// Raw `Noesis::ResourceDictionary*` (a `BaseComponent*`), borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Number of entries in this dictionary, not counting merged dictionaries.
    #[must_use]
    pub fn len(&self) -> usize {
        // SAFETY: self.ptr is a live ResourceDictionary*.
        unsafe { noesis_resource_dictionary_count(self.ptr.as_ptr()) as usize }
    }

    /// Whether this dictionary has no entries of its own; merged dictionaries
    /// are ignored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Add any Noesis component under `key`. The dictionary takes its own
    /// reference; you keep yours. Returns `false` if `value` is null. Prefer
    /// the typed [`add_brush`](Self::add_brush),
    /// [`add_boxed`](Self::add_boxed) or [`add_string`](Self::add_string).
    ///
    /// # Safety
    ///
    /// `value` must be a live `Noesis::BaseComponent*` (e.g. [`Boxed::raw`],
    /// [`crate::styles::Style::raw`], a brush, ...).
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

    /// Add a string value under `key`.
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

    /// Add a [`Boxed`] value under `key`. The dictionary takes its own
    /// reference.
    ///
    /// # Panics
    ///
    /// Panics if `key` contains an interior NUL byte.
    pub fn add_boxed(&mut self, key: &str, value: &Boxed) -> bool {
        // SAFETY: value.raw() is a live BaseComponent* for the call.
        unsafe { self.add(key, value.raw()) }
    }

    /// Add a [`Brush`](crate::brushes::Brush) under `key`, for example a
    /// [`SolidColorBrush`](crate::brushes::SolidColorBrush) that XAML then
    /// references as `{StaticResource key}`. The dictionary takes its own
    /// reference; you keep yours.
    ///
    /// # Panics
    ///
    /// Panics if `key` contains an interior NUL byte.
    pub fn add_brush(&mut self, key: &str, brush: &impl crate::brushes::Brush) -> bool {
        // SAFETY: brush.brush_raw() is a live Noesis::Brush* (a BaseComponent*)
        // for the duration of the call; the dictionary AddRefs it.
        unsafe { self.add(key, brush.brush_raw()) }
    }

    /// Whether this dictionary or one of its merged dictionaries contains
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

    /// The value stored under `key`, or `None` if absent. The pointer is
    /// borrowed from the dictionary and may dangle after the entry is replaced
    /// or removed; take your own reference to keep it longer.
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

    /// Add `other` to this dictionary's `MergedDictionaries`, so its entries
    /// resolve through this dictionary. Noesis takes its own reference to
    /// `other`. Returns `false` if the merged-dictionaries collection is
    /// unavailable.
    pub fn add_merged(&mut self, other: &ResourceDictionary) -> bool {
        // SAFETY: both pointers are live ResourceDictionary*; the collection
        // AddRefs `other`.
        unsafe { noesis_resource_dictionary_add_merged(self.ptr.as_ptr(), other.raw()) }
    }

    /// Set this dictionary's `Source` URI, loading that XAML into it through
    /// the registered XAML provider.
    ///
    /// `{StaticResource}` references in the loaded XAML resolve against every
    /// scope already reachable from this dictionary. Merge it into an installed
    /// parent ([`Self::add_merged`]) before calling `set_source`, and it can
    /// reference keys from the parent's earlier merged dictionaries. This is
    /// the building block behind [`crate::gui::install_app_resources_chain`],
    /// for when you also want code-built entries in the same parent.
    ///
    /// Load and parse errors go to the Noesis error handler, not the return
    /// value; `false` means the call was rejected outright.
    ///
    /// # Panics
    ///
    /// Panics if `uri` contains an interior NUL byte.
    pub fn set_source(&mut self, uri: &str) -> bool {
        let c = CString::new(uri).expect("resource dictionary URI contained interior NUL");
        // SAFETY: self.ptr live; c lives for the call.
        unsafe { noesis_resource_dictionary_set_source(self.ptr.as_ptr(), c.as_ptr()) }
    }
}

impl Drop for ResourceDictionary {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (create / parse / from_owned).
        unsafe { noesis_resource_dictionary_destroy(self.ptr.as_ptr()) }
    }
}

/// Install `dict` as the process-wide application resources, replacing any
/// previous ones. Every [`View`](crate::view::View) created afterwards inherits
/// its styles, brushes and templates. Noesis takes its own reference, so you
/// can drop `dict` afterwards.
///
/// To load the dictionary from a XAML file instead, use
/// [`crate::gui::load_application_resources`].
pub fn set_application_resources(dict: &ResourceDictionary) {
    // SAFETY: dict.raw() is a live ResourceDictionary*; Noesis AddRefs it.
    unsafe { noesis_gui_set_application_resources(dict.raw()) }
}

/// Whether application resources are currently installed.
#[must_use]
pub fn application_resources_present() -> bool {
    // SAFETY: borrowed getter; the pointer is only compared against null.
    !unsafe { noesis_gui_get_application_resources() }.is_null()
}

/// Whether the installed application resources, or their merged
/// dictionaries, contain `key`. `false` if none are installed.
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

/// Add the `ResourceDictionary` at `uri` to Noesis's default theme, the
/// styles controls fall back to when no implicit (`x:Key`-less) `Style`
/// applies. The registered XAML provider must be able to serve `uri`. Returns
/// `false` if `uri` is empty.
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
