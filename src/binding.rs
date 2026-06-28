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

use crate::converters::Converter;
use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_binding_create, dm_noesis_binding_destroy,
    dm_noesis_binding_set_converter, dm_noesis_binding_set_converter_parameter,
    dm_noesis_binding_set_element_name, dm_noesis_binding_set_fallback_value,
    dm_noesis_binding_set_mode, dm_noesis_binding_set_relative_source_self,
    dm_noesis_binding_set_source, dm_noesis_binding_set_string_format,
    dm_noesis_binding_set_update_source_trigger, dm_noesis_box_bool, dm_noesis_box_double,
    dm_noesis_box_int32, dm_noesis_box_string, dm_noesis_framework_element_add_resource,
    dm_noesis_observable_collection_add, dm_noesis_observable_collection_clear,
    dm_noesis_observable_collection_count, dm_noesis_observable_collection_create,
    dm_noesis_observable_collection_get, dm_noesis_observable_collection_insert,
    dm_noesis_observable_collection_remove_at, dm_noesis_observable_collection_set,
    dm_noesis_set_binding,
};
use crate::view::FrameworkElement;

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

/// Box a `bool` as a `Noesis::BoxedValue<bool>`. Like [`box_string`], returns an
/// owning [`Boxed`] holding a `+1` reference.
#[must_use]
pub fn box_bool(value: bool) -> Boxed {
    let ptr = unsafe { dm_noesis_box_bool(value) };
    Boxed {
        ptr: NonNull::new(ptr).expect("dm_noesis_box_bool returned null"),
    }
}

/// Box an `i32` as a `Noesis::BoxedValue<int>`.
#[must_use]
pub fn box_i32(value: i32) -> Boxed {
    let ptr = unsafe { dm_noesis_box_int32(value) };
    Boxed {
        ptr: NonNull::new(ptr).expect("dm_noesis_box_int32 returned null"),
    }
}

/// Box an `f64` as a `Noesis::BoxedValue<double>`.
#[must_use]
pub fn box_f64(value: f64) -> Boxed {
    let ptr = unsafe { dm_noesis_box_double(value) };
    Boxed {
        ptr: NonNull::new(ptr).expect("dm_noesis_box_double returned null"),
    }
}

/// How a [`Binding`] propagates values between source and target. Mirrors
/// `Noesis::BindingMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BindingMode {
    /// Use the target property's default mode.
    Default = 0,
    /// Source ⇄ target.
    TwoWay = 1,
    /// Source → target.
    OneWay = 2,
    /// Source → target once, then disconnect.
    OneTime = 3,
    /// Target → source.
    OneWayToSource = 4,
}

/// When a `TwoWay` / `OneWayToSource` [`Binding`] pushes target changes back to
/// the source. Mirrors `Noesis::UpdateSourceTrigger`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UpdateSourceTrigger {
    /// Use the target property's default trigger (`PropertyChanged` for most
    /// properties; `LostFocus` for `TextBox.Text`).
    Default = 0,
    /// Update the source immediately on every target change.
    PropertyChanged = 1,
    /// Update the source when the target element loses focus.
    LostFocus = 2,
    /// Update the source only on an explicit `UpdateSource` call.
    Explicit = 3,
}

/// A code-built `Noesis::Binding` — the programmatic equivalent of authoring
/// `{Binding ...}` in XAML. Build it with [`Binding::new`] (a property path) or
/// [`Binding::whole`] (bind the whole `DataContext`), chain the knob setters,
/// then wire it onto a target DP with [`set_binding`].
///
/// Owns a `+1` reference released on drop. [`set_binding`] makes Noesis take its
/// own reference, so a [`Binding`] may be dropped right after wiring.
pub struct Binding {
    ptr: NonNull<c_void>,
}

// SAFETY: a Noesis BaseComponent handle; same threading rationale as the other
// owning wrappers in this crate.
unsafe impl Send for Binding {}
unsafe impl Sync for Binding {}

impl Binding {
    /// Create a binding with the given source property path (e.g. `"Title"`,
    /// `"Item.Name"`).
    ///
    /// # Panics
    ///
    /// Panics if `path` contains an interior NUL byte, or if the Noesis
    /// allocation fails.
    #[must_use]
    pub fn new(path: &str) -> Self {
        let c = CString::new(path).expect("binding path contained interior NUL");
        let ptr = unsafe { dm_noesis_binding_create(c.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_binding_create returned null"),
        }
    }

    /// Create a binding with an empty path — binds to the whole `DataContext`
    /// (or `Source`) object, like `{Binding}` in XAML.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis allocation fails.
    #[must_use]
    pub fn whole() -> Self {
        let ptr = unsafe { dm_noesis_binding_create(core::ptr::null()) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_binding_create returned null"),
        }
    }

    /// Raw `Noesis::Binding*` (a `BaseComponent*`). Borrowed for the lifetime of
    /// `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Set the binding [`mode`](BindingMode). Chainable.
    #[must_use]
    pub fn mode(self, mode: BindingMode) -> Self {
        unsafe { dm_noesis_binding_set_mode(self.ptr.as_ptr(), mode as i32) };
        self
    }

    /// Set the [`UpdateSourceTrigger`]. Chainable.
    #[must_use]
    pub fn update_source_trigger(self, trigger: UpdateSourceTrigger) -> Self {
        unsafe { dm_noesis_binding_set_update_source_trigger(self.ptr.as_ptr(), trigger as i32) };
        self
    }

    /// Attach a Rust [`Converter`]. Chainable. The binding takes its own
    /// reference, so the [`Converter`] handle may be dropped afterwards (the
    /// converter stays alive while the binding references it).
    #[must_use]
    pub fn converter(self, converter: &Converter) -> Self {
        unsafe { dm_noesis_binding_set_converter(self.ptr.as_ptr(), converter.raw()) };
        self
    }

    /// Set the converter parameter (a boxed value passed to the converter on
    /// every call). The binding stores its own reference. Chainable.
    #[must_use]
    pub fn converter_parameter(self, parameter: &Boxed) -> Self {
        unsafe { dm_noesis_binding_set_converter_parameter(self.ptr.as_ptr(), parameter.raw()) };
        self
    }

    /// Set a .NET-style composite `StringFormat` (e.g. `"F2"`, `"Value is
    /// {0:F2}"`). Chainable.
    ///
    /// # Panics
    ///
    /// Panics if `format` contains an interior NUL byte.
    #[must_use]
    pub fn string_format(self, format: &str) -> Self {
        let c = CString::new(format).expect("string format contained interior NUL");
        unsafe { dm_noesis_binding_set_string_format(self.ptr.as_ptr(), c.as_ptr()) };
        self
    }

    /// Set the fallback value used when the binding can't produce one. The
    /// binding stores its own reference. Chainable.
    #[must_use]
    pub fn fallback_value(self, value: &Boxed) -> Self {
        unsafe { dm_noesis_binding_set_fallback_value(self.ptr.as_ptr(), value.raw()) };
        self
    }

    /// Bind against another element resolved by its `x:Name`. Chainable.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn element_name(self, name: &str) -> Self {
        let c = CString::new(name).expect("element name contained interior NUL");
        unsafe { dm_noesis_binding_set_element_name(self.ptr.as_ptr(), c.as_ptr()) };
        self
    }

    /// Bind relative to the target element itself (`RelativeSource Self`).
    /// Chainable.
    #[must_use]
    pub fn relative_source_self(self) -> Self {
        unsafe { dm_noesis_binding_set_relative_source_self(self.ptr.as_ptr()) };
        self
    }

    /// Set an explicit binding source object (overrides the inherited
    /// `DataContext`). Chainable.
    ///
    /// # Safety
    ///
    /// `source` must be null or a live `Noesis::BaseComponent*` (e.g. a view
    /// model from [`crate::classes::ClassInstance::raw`]). The binding stores
    /// its own reference; the caller keeps ownership.
    #[must_use]
    pub unsafe fn source(self, source: *mut c_void) -> Self {
        unsafe { dm_noesis_binding_set_source(self.ptr.as_ptr(), source) };
        self
    }
}

impl Drop for Binding {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_binding_create with +1 ref.
        unsafe { dm_noesis_binding_destroy(self.ptr.as_ptr()) }
    }
}

/// Wire `binding` onto `element`'s dependency property named `dp_name`, via
/// `Noesis::BindingOperations::SetBinding` — the code-built equivalent of
/// authoring `dp_name="{Binding ...}"` in XAML. Returns `false` if `element` is
/// not a `DependencyObject` or `dp_name` doesn't resolve to one of its
/// dependency properties.
#[must_use]
pub fn set_binding(element: &FrameworkElement, dp_name: &str, binding: &Binding) -> bool {
    let c = CString::new(dp_name).expect("dp name contained interior NUL");
    // SAFETY: element.raw() is a live FrameworkElement*; binding.raw() a live
    // Binding*; both outlive the call. Noesis takes its own reference.
    unsafe { dm_noesis_set_binding(element.raw(), c.as_ptr(), binding.raw()) }
}

/// Insert `object` (e.g. a [`Converter`] via [`Converter::raw`], or a [`Boxed`]
/// value) into `element`'s `ResourceDictionary` under `key`, creating the
/// dictionary if the element has none. Makes the object reachable from XAML via
/// `{StaticResource key}` — e.g. `{Binding Path, Converter={StaticResource
/// key}}`. The dictionary stores its own reference. Returns `false` if `element`
/// is not a `FrameworkElement`.
///
/// # Safety
///
/// `object` must be a live `Noesis::BaseComponent*` (e.g. [`Converter::raw`] /
/// [`Boxed::raw`] / [`crate::classes::ClassInstance::raw`]).
#[must_use]
pub unsafe fn add_resource(element: &FrameworkElement, key: &str, object: *mut c_void) -> bool {
    let c = CString::new(key).expect("resource key contained interior NUL");
    unsafe { dm_noesis_framework_element_add_resource(element.raw(), c.as_ptr(), object) }
}
