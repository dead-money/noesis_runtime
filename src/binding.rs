//! Data-binding bridge (TODO §3): drive XAML from Rust-owned data.
//!
//! Bindings are authored in XAML — `{Binding Path}` on a property,
//! `ItemsSource="{Binding}"` on a list control. This module supplies the
//! runtime data those bindings resolve against:
//!
//! * [`ObservableCollection`] — a Rust handle to Noesis's
//!   `ObservableCollection<BaseComponent>`. It implements
//!   `INotifyCollectionChanged`, so once you bind it to an
//!   `ItemsControl::ItemsSource`
//!   ([`FrameworkElement::set_items_source`](crate::view::FrameworkElement::set_items_source)),
//!   every [`push_string`](ObservableCollection::push_string) /
//!   [`remove_at`](ObservableCollection::remove_at) / … from Rust raises
//!   `CollectionChanged` and the control regenerates its item containers on the
//!   next `View::update`.
//!
//! * [`box_string`] — wrap a `&str` as a `BoxedValue<String>` so it can be a
//!   collection item rendered by a `<DataTemplate>` with `{Binding}` (the whole
//!   item).
//!
//! For *property* binding (as opposed to list binding), the source is a
//! [`ClassInstance`](crate::classes::ClassInstance) — a Rust-backed
//! `DependencyObject` view model. Set it as a `DataContext` and bind to its
//! DPs; see [`ClassRegistration::create_instance`](crate::classes::ClassRegistration::create_instance).

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_box_string, dm_noesis_observable_collection_add,
    dm_noesis_observable_collection_clear, dm_noesis_observable_collection_count,
    dm_noesis_observable_collection_create, dm_noesis_observable_collection_get,
    dm_noesis_observable_collection_insert, dm_noesis_observable_collection_remove_at,
    dm_noesis_observable_collection_set,
};

/// Box a UTF-8 string as a `Noesis::BoxedValue<String>`, returned as a borrowed
/// opaque pointer wrapped in an owning [`Boxed`]. Noesis copies the bytes, so
/// the input may go away after this call.
///
/// # Panics
///
/// Panics if `value` contains an interior NUL byte.
#[must_use]
pub fn box_string(value: &str) -> Boxed {
    let c = CString::new(value).expect("boxed string contained interior NUL");
    // SAFETY: c lives for the call; the C side copies into a Noesis::String.
    let ptr = unsafe { dm_noesis_box_string(c.as_ptr()) };
    Boxed {
        ptr: NonNull::new(ptr).expect("dm_noesis_box_string returned null"),
    }
}

/// Owned handle to a boxed value (a `Noesis::BoxedValue*`). Holds a `+1`
/// reference released on drop. Adding it to an [`ObservableCollection`] makes
/// the collection take its own reference, so a [`Boxed`] may be dropped right
/// after the `push` if you don't need it again.
pub struct Boxed {
    ptr: NonNull<c_void>,
}

// SAFETY: a Noesis BaseComponent handle; same threading rationale as the other
// owning wrappers in this crate.
unsafe impl Send for Boxed {}
unsafe impl Sync for Boxed {}

impl Boxed {
    /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for Boxed {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_box_string with +1 ref.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A Rust handle to a `Noesis::ObservableCollection<BaseComponent>`. Owns a
/// `+1` reference released on drop. Bind it to an `ItemsControl::ItemsSource`
/// and mutate it from Rust to drive a data-bound list.
pub struct ObservableCollection {
    ptr: NonNull<c_void>,
}

// SAFETY: a Noesis BaseComponent handle; same threading rationale as the other
// owning wrappers in this crate (per-object calls serialised by the caller).
unsafe impl Send for ObservableCollection {}
unsafe impl Sync for ObservableCollection {}

impl Default for ObservableCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservableCollection {
    /// Create an empty observable collection.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis allocation fails (returns null) — not expected once
    /// [`crate::init`] has run.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { dm_noesis_observable_collection_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_observable_collection_create returned null"),
        }
    }

    /// Raw `Noesis::BaseComponent*` (the collection), for handing to
    /// [`FrameworkElement::set_items_source`](crate::view::FrameworkElement::set_items_source).
    /// Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Number of items currently in the collection.
    #[must_use]
    pub fn len(&self) -> usize {
        // SAFETY: self.ptr is a live ObservableCollection*.
        let n = unsafe { dm_noesis_observable_collection_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }

    /// Whether the collection is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Append a boxed string item, returning its index (or `None` on failure).
    /// The collection takes its own reference to the boxed value, so nothing
    /// needs to be kept alive on the Rust side.
    ///
    /// # Panics
    ///
    /// Panics if `value` contains an interior NUL byte.
    pub fn push_string(&mut self, value: &str) -> Option<usize> {
        let boxed = box_string(value);
        // SAFETY: `boxed` is a live BaseComponent* for the duration of the call;
        // it drops afterwards, releasing our ref while the collection keeps its own.
        unsafe { self.push_component(boxed.raw()) }
    }

    /// Append an arbitrary `BaseComponent*` item, returning its index (or `None`
    /// if the underlying handle is not a collection). The collection takes its
    /// own reference; the caller retains ownership of `item`.
    ///
    /// # Safety
    ///
    /// `item` must be a valid live `Noesis::BaseComponent*` (e.g. from
    /// [`Boxed::raw`], [`crate::classes::ClassInstance::raw`], or another Noesis
    /// accessor).
    pub unsafe fn push_component(&mut self, item: *mut c_void) -> Option<usize> {
        let idx = unsafe { dm_noesis_observable_collection_add(self.ptr.as_ptr(), item) };
        (idx >= 0).then_some(idx as usize)
    }

    /// Insert a `BaseComponent*` at `index` (allows `index == len`). Returns
    /// `false` on an out-of-range index.
    ///
    /// # Safety
    ///
    /// `item` must be a valid live `Noesis::BaseComponent*`.
    pub unsafe fn insert_component(&mut self, index: usize, item: *mut c_void) -> bool {
        unsafe { dm_noesis_observable_collection_insert(self.ptr.as_ptr(), index as u32, item) }
    }

    /// Replace the item at `index`. Returns `false` if `index >= len`.
    ///
    /// # Safety
    ///
    /// `item` must be a valid live `Noesis::BaseComponent*`.
    pub unsafe fn set_component(&mut self, index: usize, item: *mut c_void) -> bool {
        unsafe { dm_noesis_observable_collection_set(self.ptr.as_ptr(), index as u32, item) }
    }

    /// Remove the item at `index`. Returns `false` if `index >= len`.
    pub fn remove_at(&mut self, index: usize) -> bool {
        // SAFETY: self.ptr is a live ObservableCollection*.
        unsafe { dm_noesis_observable_collection_remove_at(self.ptr.as_ptr(), index as u32) }
    }

    /// Remove every item.
    pub fn clear(&mut self) {
        // SAFETY: self.ptr is a live ObservableCollection*.
        unsafe { dm_noesis_observable_collection_clear(self.ptr.as_ptr()) }
    }

    /// Borrowed (no `+1`) pointer to the item at `index`, or `None` if out of
    /// range. The collection owns the reference; copy / re-root if you need it
    /// past the next mutation.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live ObservableCollection*.
        let p = unsafe { dm_noesis_observable_collection_get(self.ptr.as_ptr(), index as u32) };
        NonNull::new(p)
    }
}

impl Drop for ObservableCollection {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_observable_collection_create with +1 ref.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}
