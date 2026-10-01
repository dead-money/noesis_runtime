//! Data binding: drive XAML from Rust-owned data.
//!
//! Bindings are usually authored in XAML (`{Binding Path}` on a property,
//! `ItemsSource="{Binding}"` on a list control). This module supplies the data
//! they resolve against, and a way to build bindings in code:
//!
//! - [`ObservableCollection`] backs a list. Bind it with
//!   [`FrameworkElement::set_items_source`](crate::view::FrameworkElement::set_items_source);
//!   every mutation from Rust ([`push_string`](ObservableCollection::push_string),
//!   [`remove_at`](ObservableCollection::remove_at),
//!   [`move_item`](ObservableCollection::move_item), ...) raises
//!   `CollectionChanged`, and the control updates its item containers on the
//!   next [`View::update`](crate::view::View::update).
//! - [`box_string`], [`box_bool`], [`box_i32`], [`box_f64`] and [`box_f32`]
//!   wrap plain values as Noesis objects ([`Boxed`]) for use as list items,
//!   converter parameters, fallback values or resources.
//! - [`Binding`] and [`set_binding`] are the code equivalent of
//!   `Prop="{Binding ...}"`; [`clear_binding`] removes one.
//! - [`add_resource`] puts a converter or value in an element's resources so
//!   XAML can reach it with `{StaticResource key}`.
//!
//! For property binding, the source is usually a
//! [`ClassInstance`](crate::classes::ClassInstance): a Rust-backed
//! `DependencyObject` view model. Set it as a `DataContext` and bind to its
//! dependency properties; see
//! [`ClassRegistration::create_instance`](crate::classes::ClassRegistration::create_instance).
//!
//! ```no_run
//! use noesis_runtime::binding::ObservableCollection;
//! use noesis_runtime::view::FrameworkElement;
//!
//! noesis_runtime::init();
//! let mut list = FrameworkElement::parse(
//!     r#"<ListBox xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"/>"#,
//! )
//! .unwrap();
//!
//! let mut items = ObservableCollection::new();
//! items.push_string("Sword");
//! items.push_string("Shield");
//! assert!(list.set_items_source(&items));
//!
//! // The ListBox picks this up on the next View::update.
//! items.remove_at(0);
//! ```

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::converters::Converter;
use crate::ffi::{
    noesis_base_component_release, noesis_binding_create, noesis_binding_destroy,
    noesis_binding_set_converter, noesis_binding_set_converter_parameter,
    noesis_binding_set_element_name, noesis_binding_set_fallback_value, noesis_binding_set_mode,
    noesis_binding_set_relative_source_find_ancestor,
    noesis_binding_set_relative_source_previous_data, noesis_binding_set_relative_source_self,
    noesis_binding_set_relative_source_templated_parent, noesis_binding_set_source,
    noesis_binding_set_string_format, noesis_binding_set_update_source_trigger, noesis_box_bool,
    noesis_box_double, noesis_box_float, noesis_box_int32, noesis_box_string, noesis_clear_binding,
    noesis_framework_element_add_resource, noesis_observable_collection_add,
    noesis_observable_collection_clear, noesis_observable_collection_count,
    noesis_observable_collection_create, noesis_observable_collection_get,
    noesis_observable_collection_insert, noesis_observable_collection_move,
    noesis_observable_collection_remove_at, noesis_observable_collection_set, noesis_set_binding,
};
use crate::view::FrameworkElement;

/// Boxes a string as a `Noesis::BoxedValue<String>`. Noesis copies the bytes.
///
/// # Panics
///
/// Panics if `value` contains an interior NUL byte.
#[must_use]
pub fn box_string(value: &str) -> Boxed {
    let c = CString::new(value).expect("boxed string contained interior NUL");
    // SAFETY: c lives for the call; the C side copies into a Noesis::String.
    let ptr = unsafe { noesis_box_string(c.as_ptr()) };
    Boxed {
        ptr: NonNull::new(ptr).expect("noesis_box_string returned null"),
    }
}

/// An owned `Noesis::BoxedValue`, made by [`box_string`], [`box_bool`],
/// [`box_i32`], [`box_f64`] or [`box_f32`]. Releases its reference on drop.
///
/// Anything you hand it to (a collection, a [`Binding`], a resource
/// dictionary) takes its own reference, so you can drop the `Boxed` right
/// after.
pub struct Boxed {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Boxed {}

impl Boxed {
    /// Raw `Noesis::BaseComponent*`, valid while `self` lives. No reference is
    /// added.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for Boxed {
    fn drop(&mut self) {
        // SAFETY: every noesis_box_* constructor hands out a +1 ref we own.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A `Noesis::ObservableCollection<BaseComponent>` owned from Rust. Bind it to
/// an `ItemsControl` with
/// [`FrameworkElement::set_items_source`](crate::view::FrameworkElement::set_items_source),
/// then mutate it to drive the list. Releases its reference on drop; a bound
/// control keeps its own.
///
/// Indices are positions in the current collection. Out-of-range indices
/// return `false` or `None`; they never panic.
pub struct ObservableCollection {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ObservableCollection {}

impl Default for ObservableCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl ObservableCollection {
    /// Creates an empty collection.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null, which is not expected after
    /// [`crate::init`].
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { noesis_observable_collection_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_observable_collection_create returned null"),
        }
    }

    /// Raw `Noesis::BaseComponent*` for the collection, valid while `self`
    /// lives. No reference is added. Most code passes `&self` to
    /// [`FrameworkElement::set_items_source`](crate::view::FrameworkElement::set_items_source)
    /// instead.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Number of items in the collection.
    #[must_use]
    pub fn len(&self) -> usize {
        // SAFETY: self.ptr is a live ObservableCollection*.
        let n = unsafe { noesis_observable_collection_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }

    /// Whether the collection has no items.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Appends `value` as a boxed string and returns its index, or `None` if
    /// Noesis rejected the add.
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

    /// Appends `value` as a boxed `bool` and returns its index, or `None` if
    /// Noesis rejected the add.
    pub fn push_bool(&mut self, value: bool) -> Option<usize> {
        let boxed = box_bool(value);
        // SAFETY: `boxed` is a live BaseComponent* for the duration of the call;
        // it drops afterwards, releasing our ref while the collection keeps its own.
        unsafe { self.push_component(boxed.raw()) }
    }

    /// Appends `value` as a boxed `i32` and returns its index, or `None` if
    /// Noesis rejected the add.
    pub fn push_i32(&mut self, value: i32) -> Option<usize> {
        let boxed = box_i32(value);
        // SAFETY: `boxed` is a live BaseComponent* for the duration of the call;
        // it drops afterwards, releasing our ref while the collection keeps its own.
        unsafe { self.push_component(boxed.raw()) }
    }

    /// Appends `value` as a boxed `f64` and returns its index, or `None` if
    /// Noesis rejected the add.
    pub fn push_f64(&mut self, value: f64) -> Option<usize> {
        let boxed = box_f64(value);
        // SAFETY: `boxed` is a live BaseComponent* for the duration of the call;
        // it drops afterwards, releasing our ref while the collection keeps its own.
        unsafe { self.push_component(boxed.raw()) }
    }

    /// Appends a [`ClassInstance`](crate::classes::ClassInstance) view model
    /// and returns its index, or `None` if Noesis rejected the add. The
    /// collection takes its own reference. Render it with a `<DataTemplate>`
    /// that binds to the instance's dependency properties.
    pub fn push_object(&mut self, instance: &crate::classes::ClassInstance) -> Option<usize> {
        // SAFETY: `instance.raw()` is a live BaseComponent* for the lifetime of
        // `instance`, which outlives this call; the collection takes its own ref.
        unsafe { self.push_component(instance.raw()) }
    }

    /// Inserts a [`ClassInstance`](crate::classes::ClassInstance) view model
    /// at `index`. `index == len` appends. Returns `false` if `index > len`.
    /// The collection takes its own reference. This is the safe form of
    /// [`insert_component`](Self::insert_component).
    pub fn insert_object(
        &mut self,
        index: usize,
        instance: &crate::classes::ClassInstance,
    ) -> bool {
        // SAFETY: `instance.raw()` is a live BaseComponent* for the lifetime of
        // `instance`, which outlives this call; the collection takes its own ref.
        unsafe { self.insert_component(index, instance.raw()) }
    }

    /// Appends any Noesis object and returns its index, or `None` if Noesis
    /// rejected the add. The collection takes its own reference; you keep
    /// yours.
    ///
    /// # Safety
    ///
    /// `item` must be a live `Noesis::BaseComponent*`, e.g. from
    /// [`Boxed::raw`] or [`ClassInstance::raw`](crate::classes::ClassInstance::raw).
    pub unsafe fn push_component(&mut self, item: *mut c_void) -> Option<usize> {
        let idx = unsafe { noesis_observable_collection_add(self.ptr.as_ptr(), item) };
        (idx >= 0).then_some(idx as usize)
    }

    /// Inserts any Noesis object at `index`. `index == len` appends. Returns
    /// `false` if `index > len`. The collection takes its own reference.
    ///
    /// # Safety
    ///
    /// `item` must be a live `Noesis::BaseComponent*`.
    pub unsafe fn insert_component(&mut self, index: usize, item: *mut c_void) -> bool {
        unsafe { noesis_observable_collection_insert(self.ptr.as_ptr(), index as u32, item) }
    }

    /// Replaces the item at `index`. Returns `false` if `index >= len`. The
    /// collection takes its own reference to `item`.
    ///
    /// # Safety
    ///
    /// `item` must be a live `Noesis::BaseComponent*`.
    pub unsafe fn set_component(&mut self, index: usize, item: *mut c_void) -> bool {
        unsafe { noesis_observable_collection_set(self.ptr.as_ptr(), index as u32, item) }
    }

    /// Removes the item at `index`. Returns `false` if `index >= len`.
    pub fn remove_at(&mut self, index: usize) -> bool {
        // SAFETY: self.ptr is a live ObservableCollection*.
        unsafe { noesis_observable_collection_remove_at(self.ptr.as_ptr(), index as u32) }
    }

    /// Moves the item at `old_index` to `new_index`. Returns `false` if either
    /// index is `>= len`.
    ///
    /// This raises a single `Move` change rather than a remove and an add, so
    /// a bound `ItemsControl` relocates the existing container and keeps its
    /// selection and scroll position. Use it to reorder rows, e.g. after a
    /// sort.
    pub fn move_item(&mut self, old_index: usize, new_index: usize) -> bool {
        // SAFETY: self.ptr is a live ObservableCollection*.
        unsafe {
            noesis_observable_collection_move(self.ptr.as_ptr(), old_index as u32, new_index as u32)
        }
    }

    /// Removes every item.
    pub fn clear(&mut self) {
        // SAFETY: self.ptr is a live ObservableCollection*.
        unsafe { noesis_observable_collection_clear(self.ptr.as_ptr()) }
    }

    /// The item at `index` as a borrowed `BaseComponent*`, or `None` if
    /// `index >= len`. No reference is added, so the pointer can dangle once a
    /// later mutation removes or replaces the item.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live ObservableCollection*.
        let p = unsafe { noesis_observable_collection_get(self.ptr.as_ptr(), index as u32) };
        NonNull::new(p)
    }
}

impl Drop for ObservableCollection {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_observable_collection_create with +1 ref.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// Boxes a `bool` as a `Noesis::BoxedValue<bool>`.
#[must_use]
pub fn box_bool(value: bool) -> Boxed {
    let ptr = unsafe { noesis_box_bool(value) };
    Boxed {
        ptr: NonNull::new(ptr).expect("noesis_box_bool returned null"),
    }
}

/// Boxes an `i32` as a `Noesis::BoxedValue<int>`.
#[must_use]
pub fn box_i32(value: i32) -> Boxed {
    let ptr = unsafe { noesis_box_int32(value) };
    Boxed {
        ptr: NonNull::new(ptr).expect("noesis_box_int32 returned null"),
    }
}

/// Boxes an `f64` as a `Noesis::BoxedValue<double>`. For `float`-typed
/// dependency properties use [`box_f32`] instead.
#[must_use]
pub fn box_f64(value: f64) -> Boxed {
    let ptr = unsafe { noesis_box_double(value) };
    Boxed {
        ptr: NonNull::new(ptr).expect("noesis_box_double returned null"),
    }
}

/// Boxes an `f32` as a `Noesis::BoxedValue<float>`.
///
/// Use this, not [`box_f64`], for `float`-typed dependency properties
/// (`FontSize`, `Opacity`, ...). Noesis does not coerce a boxed `double` to
/// `float` through a [`Style`](crate::styles::Style) setter or a resource
/// entry, so a `box_f64` value there is silently ignored.
#[must_use]
pub fn box_f32(value: f32) -> Boxed {
    // SAFETY: no preconditions; returns a +1-owned BoxedValue<float>.
    let ptr = unsafe { noesis_box_float(value) };
    Boxed {
        ptr: NonNull::new(ptr).expect("noesis_box_float returned null"),
    }
}

/// How a [`Binding`] moves values between source and target.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
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

/// When a `TwoWay` or `OneWayToSource` [`Binding`] writes target changes back
/// to the source.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum UpdateSourceTrigger {
    /// Use the target property's default trigger (`PropertyChanged` for most
    /// properties; `LostFocus` for `TextBox.Text`).
    Default = 0,
    /// On every target change.
    PropertyChanged = 1,
    /// When the target element loses focus.
    LostFocus = 2,
    /// Only on an explicit `UpdateSource` call.
    Explicit = 3,
}

/// A binding built in code, the equivalent of `{Binding ...}` in XAML.
///
/// Start with [`Binding::new`] (a property path) or [`Binding::whole`] (the
/// whole `DataContext`), chain the setters, then attach it to a dependency
/// property with [`set_binding`]. Noesis keeps its own reference, so the
/// `Binding` can be dropped right after.
///
/// ```no_run
/// use noesis_runtime::binding::{set_binding, Binding, BindingMode};
/// # fn demo(element: &noesis_runtime::view::FrameworkElement) {
/// let binding = Binding::new("Health")
///     .mode(BindingMode::OneWay)
///     .string_format("HP {0}");
/// assert!(set_binding(element, "Text", &binding));
/// # }
/// ```
pub struct Binding {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Binding {}

impl Binding {
    /// Creates a binding to the source property path `path`, e.g. `"Title"`
    /// or `"Item.Name"`.
    ///
    /// # Panics
    ///
    /// Panics if `path` contains an interior NUL byte, or if the Noesis
    /// allocation fails.
    #[must_use]
    pub fn new(path: &str) -> Self {
        let c = CString::new(path).expect("binding path contained interior NUL");
        let ptr = unsafe { noesis_binding_create(c.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_binding_create returned null"),
        }
    }

    /// Creates a binding with no path, so it binds to the whole source object
    /// (the `DataContext` or [`source`](Self::source)), like `{Binding}`.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis allocation fails.
    #[must_use]
    pub fn whole() -> Self {
        let ptr = unsafe { noesis_binding_create(core::ptr::null()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_binding_create returned null"),
        }
    }

    /// Raw `Noesis::Binding*`, valid while `self` lives. No reference is
    /// added.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Sets the [`BindingMode`].
    #[must_use]
    pub fn mode(self, mode: BindingMode) -> Self {
        unsafe { noesis_binding_set_mode(self.ptr.as_ptr(), mode as i32) };
        self
    }

    /// Sets the [`UpdateSourceTrigger`].
    #[must_use]
    pub fn update_source_trigger(self, trigger: UpdateSourceTrigger) -> Self {
        unsafe { noesis_binding_set_update_source_trigger(self.ptr.as_ptr(), trigger as i32) };
        self
    }

    /// Attaches a [`Converter`]. The binding keeps its own reference, so the
    /// `Converter` handle can be dropped afterwards.
    #[must_use]
    pub fn converter(self, converter: &Converter) -> Self {
        unsafe { noesis_binding_set_converter(self.ptr.as_ptr(), converter.raw()) };
        self
    }

    /// Sets the value passed to the converter as its parameter on every call.
    /// The binding keeps its own reference.
    #[must_use]
    pub fn converter_parameter(self, parameter: &Boxed) -> Self {
        unsafe { noesis_binding_set_converter_parameter(self.ptr.as_ptr(), parameter.raw()) };
        self
    }

    /// Sets a .NET-style `StringFormat`, e.g. `"F2"` or `"Value is {0:F2}"`.
    ///
    /// # Panics
    ///
    /// Panics if `format` contains an interior NUL byte.
    #[must_use]
    pub fn string_format(self, format: &str) -> Self {
        let c = CString::new(format).expect("string format contained interior NUL");
        unsafe { noesis_binding_set_string_format(self.ptr.as_ptr(), c.as_ptr()) };
        self
    }

    /// Sets the value used when the binding can't produce one. The binding
    /// keeps its own reference.
    #[must_use]
    pub fn fallback_value(self, value: &Boxed) -> Self {
        unsafe { noesis_binding_set_fallback_value(self.ptr.as_ptr(), value.raw()) };
        self
    }

    /// Uses the element named `name` (its `x:Name`) as the source.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn element_name(self, name: &str) -> Self {
        let c = CString::new(name).expect("element name contained interior NUL");
        unsafe { noesis_binding_set_element_name(self.ptr.as_ptr(), c.as_ptr()) };
        self
    }

    /// Uses the target element itself as the source (`RelativeSource Self`).
    #[must_use]
    pub fn relative_source_self(self) -> Self {
        unsafe { noesis_binding_set_relative_source_self(self.ptr.as_ptr()) };
        self
    }

    /// Uses an ancestor of the target as the source, like
    /// `{RelativeSource FindAncestor, AncestorType=type_name, AncestorLevel=level}`.
    ///
    /// `type_name` is a registered Noesis class name such as `"StackPanel"`.
    /// Built-in types are registered once any loaded XAML has referenced them.
    /// `level` is 1-based: `1` is the nearest ancestor of that type, and `0`
    /// is treated as `1`.
    ///
    /// An unknown `type_name` (or one with an interior NUL) leaves the
    /// relative source unset instead of panicking. Use
    /// [`try_relative_source_find_ancestor`](Self::try_relative_source_find_ancestor)
    /// to detect that.
    #[must_use]
    pub fn relative_source_find_ancestor(self, type_name: &str, level: u32) -> Self {
        let _ = self.set_relative_source_find_ancestor(type_name, level);
        self
    }

    /// Like [`relative_source_find_ancestor`](Self::relative_source_find_ancestor),
    /// but returns `false` when `type_name` is unknown or contains an interior
    /// NUL, leaving the binding unchanged.
    pub fn try_relative_source_find_ancestor(&self, type_name: &str, level: u32) -> bool {
        self.set_relative_source_find_ancestor(type_name, level)
    }

    fn set_relative_source_find_ancestor(&self, type_name: &str, level: u32) -> bool {
        let Ok(c) = CString::new(type_name) else {
            return false;
        };
        // SAFETY: self.ptr is a live Binding*; c lives for the call. The C side
        // resolves the type by name and returns false (no-op) if it is unknown.
        unsafe {
            noesis_binding_set_relative_source_find_ancestor(self.ptr.as_ptr(), c.as_ptr(), level)
        }
    }

    /// Uses the previous item of the data-bound collection as the source
    /// (`RelativeSource PreviousData`).
    #[must_use]
    pub fn relative_source_previous_data(self) -> Self {
        // SAFETY: self.ptr is a live Binding*.
        unsafe { noesis_binding_set_relative_source_previous_data(self.ptr.as_ptr()) };
        self
    }

    /// Uses the control a `ControlTemplate` is applied to as the source
    /// (`RelativeSource TemplatedParent`).
    #[must_use]
    pub fn relative_source_templated_parent(self) -> Self {
        // SAFETY: self.ptr is a live Binding*.
        unsafe { noesis_binding_set_relative_source_templated_parent(self.ptr.as_ptr()) };
        self
    }

    /// Sets an explicit source object, overriding the inherited `DataContext`.
    /// Null clears it. The binding keeps its own reference.
    ///
    /// # Safety
    ///
    /// `source` must be null or a live `Noesis::BaseComponent*`, e.g. from
    /// [`ClassInstance::raw`](crate::classes::ClassInstance::raw).
    #[must_use]
    pub unsafe fn source(self, source: *mut c_void) -> Self {
        unsafe { noesis_binding_set_source(self.ptr.as_ptr(), source) };
        self
    }
}

impl Drop for Binding {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_binding_create with +1 ref.
        unsafe { noesis_binding_destroy(self.ptr.as_ptr()) }
    }
}

/// Binds `element`'s dependency property `dp_name` with `binding`, like
/// writing `dp_name="{Binding ...}"` in XAML. Replaces any existing binding.
/// Returns `false` if `dp_name` is not a dependency property of `element`.
///
/// # Panics
///
/// Panics if `dp_name` contains an interior NUL byte.
#[must_use]
pub fn set_binding(element: &FrameworkElement, dp_name: &str, binding: &Binding) -> bool {
    let c = CString::new(dp_name).expect("dp name contained interior NUL");
    // SAFETY: element.raw() is a live FrameworkElement*; binding.raw() a live
    // Binding*; both outlive the call. Noesis takes its own reference.
    unsafe { noesis_set_binding(element.raw(), c.as_ptr(), binding.raw()) }
}

/// Removes the binding on `element`'s dependency property `dp_name`, undoing
/// [`set_binding`]. The property falls back to its local or default value,
/// discarding the last value the binding wrote.
///
/// Returns `true` even if no binding was set, so you don't need to track which
/// properties are bound. Returns `false` only if `dp_name` is not a dependency
/// property of `element`.
///
/// # Panics
///
/// Panics if `dp_name` contains an interior NUL byte.
#[must_use]
pub fn clear_binding(element: &FrameworkElement, dp_name: &str) -> bool {
    let c = CString::new(dp_name).expect("dp name contained interior NUL");
    // SAFETY: element.raw() is a live FrameworkElement*; c lives for the call.
    unsafe { noesis_clear_binding(element.raw(), c.as_ptr()) }
}

/// Adds `object` to `element`'s resources under `key`, creating the resource
/// dictionary if needed. XAML under `element` can then reach it with
/// `{StaticResource key}`, e.g. `{Binding Path, Converter={StaticResource key}}`.
/// The dictionary keeps its own reference. Returns `false` if `object` is null.
///
/// # Panics
///
/// Panics if `key` contains an interior NUL byte.
///
/// # Safety
///
/// `object` must be null or a live `Noesis::BaseComponent*`, e.g. from
/// [`Converter::raw`], [`Boxed::raw`] or
/// [`ClassInstance::raw`](crate::classes::ClassInstance::raw).
#[must_use]
pub unsafe fn add_resource(element: &FrameworkElement, key: &str, object: *mut c_void) -> bool {
    let c = CString::new(key).expect("resource key contained interior NUL");
    unsafe { noesis_framework_element_add_resource(element.raw(), c.as_ptr(), object) }
}
