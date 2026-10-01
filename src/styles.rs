//! Build styles, triggers and templates in code.
//!
//! A [`Style`] is the code form of a XAML `<Style>`. This XAML:
//!
//! ```xml
//! <Style TargetType="TextBlock">
//!   <Setter Property="FontSize" Value="24"/>
//! </Style>
//! ```
//!
//! is built like this:
//!
//! ```no_run
//! # use noesis_runtime::styles::Style;
//! # use noesis_runtime::binding::box_f64;
//! let mut style = Style::new();
//! style.set_target_type("TextBlock");
//! style.add_setter("FontSize", &box_f64(24.0));
//! ```
//!
//! Apply it with
//! [`FrameworkElement::set_style`](crate::view::FrameworkElement::set_style),
//! or [`Style::builder`] for the chained form.
//!
//! Triggers ([`Trigger`], [`DataTrigger`], [`MultiTrigger`],
//! [`MultiDataTrigger`], [`EventTrigger`]) are built here too and added with
//! [`Style::add_trigger`]; [`Style::get_trigger`] reads them back.
//!
//! Templates come from XAML, because Noesis has no `FrameworkElementFactory`
//! for building a template's visual tree in code. Parse a [`ControlTemplate`]
//! and apply it with
//! [`FrameworkElement::set_control_template`](crate::view::FrameworkElement::set_control_template),
//! or parse a [`DataTemplate`] and set it on `ContentTemplate` or
//! `ItemTemplate` with
//! [`FrameworkElement::set_component`](crate::view::FrameworkElement::set_component).
//! A [`TemplateSelector`] picks a `DataTemplate` per item with Rust code.

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::animation::BeginStoryboard;
use crate::binding::{Binding, Boxed};
use crate::ffi::{
    TemplateSelectorVTable, noesis_base_component_release, noesis_control_template_parse,
    noesis_data_template_parse, noesis_framework_template_find_name, noesis_style_add_setter,
    noesis_style_create, noesis_style_destroy, noesis_style_set_based_on,
    noesis_style_set_target_type, noesis_templates_data_trigger_add_setter,
    noesis_templates_data_trigger_create, noesis_templates_data_trigger_get_binding,
    noesis_templates_data_trigger_get_value, noesis_templates_data_trigger_set_binding,
    noesis_templates_data_trigger_set_value, noesis_templates_data_trigger_setter_count,
    noesis_templates_event_trigger_action_count, noesis_templates_event_trigger_add_action,
    noesis_templates_event_trigger_create, noesis_templates_event_trigger_get_routed_event_name,
    noesis_templates_event_trigger_get_source_name,
    noesis_templates_event_trigger_set_routed_event,
    noesis_templates_event_trigger_set_source_name,
    noesis_templates_multi_data_trigger_add_condition,
    noesis_templates_multi_data_trigger_add_setter,
    noesis_templates_multi_data_trigger_condition_count,
    noesis_templates_multi_data_trigger_condition_has_binding,
    noesis_templates_multi_data_trigger_create,
    noesis_templates_multi_data_trigger_get_condition_value,
    noesis_templates_multi_data_trigger_setter_count, noesis_templates_multi_trigger_add_condition,
    noesis_templates_multi_trigger_add_setter, noesis_templates_multi_trigger_condition_count,
    noesis_templates_multi_trigger_create,
    noesis_templates_multi_trigger_get_condition_property_name,
    noesis_templates_multi_trigger_get_condition_value,
    noesis_templates_multi_trigger_setter_count, noesis_templates_selector_create,
    noesis_templates_selector_destroy, noesis_templates_selector_select,
    noesis_templates_style_add_trigger, noesis_templates_style_get_trigger,
    noesis_templates_style_trigger_count, noesis_templates_trigger_add_setter,
    noesis_templates_trigger_create, noesis_templates_trigger_get_property_name,
    noesis_templates_trigger_get_value, noesis_templates_trigger_set_property,
    noesis_templates_trigger_set_value, noesis_templates_trigger_setter_count, noesis_unbox_bool,
    noesis_unbox_int32, noesis_unbox_string,
};
use crate::view::FrameworkElement;

/// An owned `Noesis::Style`, the code form of a XAML `<Style>`. Released on
/// drop.
///
/// Applying it to an element
/// ([`FrameworkElement::set_style`](crate::view::FrameworkElement::set_style))
/// or adding it to a [`ResourceDictionary`](crate::resources::ResourceDictionary)
/// gives Noesis its own reference, so you can drop the handle afterwards.
///
/// A style is sealed the first time it is applied. Set its target type,
/// setters, triggers and `BasedOn` before that.
pub struct Style {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Style {}

impl Default for Style {
    fn default() -> Self {
        Self::new()
    }
}

impl Style {
    /// Create an empty style (no target type, no setters).
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null.
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no preconditions beyond a live Noesis runtime.
        let ptr = unsafe { noesis_style_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_style_create returned null"),
        }
    }

    /// Wrap a `Noesis::Style*` that carries a reference this handle takes over.
    ///
    /// # Safety
    ///
    /// `ptr` must be a live `Noesis::Style*` carrying a reference this wrapper
    /// takes ownership of (released on drop).
    #[must_use]
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }

    /// Raw `Noesis::Style*` (a `BaseComponent*`), borrowed for the lifetime of
    /// `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Set the `TargetType` by type name, such as `"TextBlock"` or `"Button"`.
    /// Call this before [`add_setter`](Self::add_setter), which looks up
    /// properties on the target type.
    ///
    /// The type must already be registered with Noesis's reflection; built-in
    /// controls register on first use, and so does any type referenced from
    /// loaded XAML. Returns `false`, leaving the target type unchanged, if the
    /// name is unknown or contains a NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_target_type(&mut self, type_name: &str) -> bool {
        let Ok(c) = CString::new(type_name) else {
            return false;
        };
        // SAFETY: self.ptr live; c lives for the call. The C side returns false
        // (no-op) on an unknown type name.
        unsafe { noesis_style_set_target_type(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Add a `Setter` that sets the dependency property `dp_name` on the
    /// target type to `value`. The setter keeps its own reference to `value`.
    ///
    /// Returns `false` if no target type is set, `dp_name` is not a dependency
    /// property of it, or `dp_name` contains a NUL byte.
    pub fn add_setter(&mut self, dp_name: &str, value: &Boxed) -> bool {
        // SAFETY: value.raw() is a live BaseComponent* for the call.
        unsafe { self.add_setter_raw(dp_name, value.raw()) }
    }

    /// [`add_setter`](Self::add_setter) with a raw `BaseComponent*` value, such
    /// as a brush. Also returns `false` if `value` is null.
    ///
    /// # Safety
    ///
    /// `value` must be null or a live `Noesis::BaseComponent*`.
    pub unsafe fn add_setter_raw(&mut self, dp_name: &str, value: *mut c_void) -> bool {
        let Ok(c) = CString::new(dp_name) else {
            return false;
        };
        // SAFETY: self.ptr live; c lives for the call; value per # Safety.
        unsafe { noesis_style_add_setter(self.ptr.as_ptr(), c.as_ptr(), value) }
    }

    /// Set the `BasedOn` style this one inherits setters and triggers from.
    /// Noesis takes its own reference to `base`.
    pub fn set_based_on(&mut self, base: &Style) {
        // SAFETY: both pointers are live Style*; Noesis AddRefs `base`.
        unsafe { noesis_style_set_based_on(self.ptr.as_ptr(), base.raw()) }
    }

    /// Add `trigger` to this style's `Triggers`. The collection takes its own
    /// reference, so you can drop the trigger handle afterwards. Add triggers
    /// before the style is first applied. Returns `false` only on an invalid
    /// handle.
    pub fn add_trigger<T: TriggerHandle>(&mut self, trigger: &T) -> bool {
        // SAFETY: both pointers are live; Noesis AddRefs the trigger.
        unsafe { noesis_templates_style_add_trigger(self.ptr.as_ptr(), trigger.trigger_ptr()) }
    }

    /// Number of triggers in this style's `Triggers`.
    #[must_use]
    pub fn trigger_count(&self) -> u32 {
        // SAFETY: self.ptr is a live Style*.
        let n = unsafe { noesis_templates_style_trigger_count(self.ptr.as_ptr()) };
        u32::try_from(n.max(0)).unwrap_or(0)
    }

    /// A handle to the trigger at `index` in this style's `Triggers`, or `None`
    /// if `index` is out of range.
    #[must_use]
    pub fn get_trigger(&self, index: u32) -> Option<TriggerReadback> {
        // SAFETY: self.ptr is a live Style*; the result is a +1-owned trigger.
        let p = unsafe { noesis_templates_style_get_trigger(self.ptr.as_ptr(), index) };
        NonNull::new(p).map(|ptr| TriggerReadback { ptr })
    }

    /// Start a [`StyleBuilder`] for `target_type` (such as `"TextBlock"`),
    /// resolved like [`set_target_type`](Self::set_target_type).
    ///
    /// # Panics
    ///
    /// Panics if `target_type` is not a registered type or contains a NUL
    /// byte. Use [`Style::new`] and [`set_target_type`](Self::set_target_type)
    /// to handle that case instead.
    pub fn builder(target_type: &str) -> StyleBuilder {
        let mut style = Style::new();
        assert!(
            style.set_target_type(target_type),
            "Style::builder: unknown target type {target_type:?}"
        );
        StyleBuilder { style }
    }
}

/// Chained construction of a [`Style`]. Start with [`Style::builder`], add
/// setters, triggers and a `BasedOn` style, then call [`build`](Self::build).
///
/// ```no_run
/// # use noesis_runtime::styles::Style;
/// # use noesis_runtime::binding::box_f64;
/// let style = Style::builder("TextBlock")
///     .setter("FontSize", &box_f64(24.0))
///     .build();
/// ```
#[must_use]
pub struct StyleBuilder {
    style: Style,
}

impl StyleBuilder {
    /// Add a setter, as [`Style::add_setter`]. A setter whose property can't be
    /// resolved is skipped without an error.
    pub fn setter(mut self, dp_name: &str, value: &Boxed) -> Self {
        let _ = self.style.add_setter(dp_name, value);
        self
    }

    /// Set the `BasedOn` style this style inherits from.
    pub fn based_on(mut self, base: &Style) -> Self {
        self.style.set_based_on(base);
        self
    }

    /// Add a trigger, as [`Style::add_trigger`].
    pub fn trigger<T: TriggerHandle>(mut self, trigger: &T) -> Self {
        let _ = self.style.add_trigger(trigger);
        self
    }

    /// Finish and return the built [`Style`].
    #[must_use]
    pub fn build(self) -> Style {
        self.style
    }
}

impl Drop for Style {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (create / from_owned).
        unsafe { noesis_style_destroy(self.ptr.as_ptr()) }
    }
}

/// A control template parsed from XAML. Released on drop.
///
/// Apply it to a control with
/// [`FrameworkElement::set_control_template`](crate::view::FrameworkElement::set_control_template);
/// Noesis takes its own reference, so you can drop the handle afterwards.
/// Once the control has been laid out, find the template's named parts with
/// [`find_name`](Self::find_name) or
/// [`FrameworkElement::template_child`](crate::view::FrameworkElement::template_child).
pub struct ControlTemplate {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ControlTemplate {}

impl ControlTemplate {
    /// Parse a `<ControlTemplate>` from a XAML string. Returns `None` when the
    /// XAML is malformed or its root is not a `ControlTemplate`.
    ///
    /// # Panics
    ///
    /// Panics if `xaml` contains an interior NUL byte.
    #[must_use]
    pub fn parse(xaml: &str) -> Option<Self> {
        let c = CString::new(xaml).expect("xaml contained interior NUL");
        // SAFETY: c lives for the call; the result is a +1-owned ControlTemplate.
        let ptr = unsafe { noesis_control_template_parse(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Wrap a `Noesis::ControlTemplate*` that carries a reference this handle
    /// takes over.
    ///
    /// # Safety
    ///
    /// `ptr` must be a live `Noesis::ControlTemplate*` carrying a reference this
    /// wrapper takes ownership of.
    #[must_use]
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }

    /// Raw `Noesis::ControlTemplate*` (a `BaseComponent*`), borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The element named `name` in this template's instance on
    /// `templated_parent`, or `None` if not found, which includes before the
    /// template has been applied and laid out. The pointer is borrowed and
    /// valid only while the template stays applied to that parent.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn find_name(
        &self,
        name: &str,
        templated_parent: &FrameworkElement,
    ) -> Option<NonNull<c_void>> {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr live; c lives for the call; templated_parent.raw() is
        // a live FrameworkElement*. The returned pointer is borrowed.
        let p = unsafe {
            noesis_framework_template_find_name(
                self.ptr.as_ptr(),
                c.as_ptr(),
                templated_parent.raw(),
            )
        };
        NonNull::new(p)
    }
}

impl Drop for ControlTemplate {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (parse / from_owned).
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A data template parsed from XAML. Released on drop.
///
/// Set it on a `ContentControl`'s `ContentTemplate` or an `ItemsControl`'s
/// `ItemTemplate` with
/// [`FrameworkElement::set_component`](crate::view::FrameworkElement::set_component),
/// passing [`raw`](Self::raw). Noesis takes its own reference.
pub struct DataTemplate {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for DataTemplate {}

impl DataTemplate {
    /// Parse a `<DataTemplate>` from a XAML string. Returns `None` when the
    /// XAML is malformed or its root is not a `DataTemplate`.
    ///
    /// # Panics
    ///
    /// Panics if `xaml` contains an interior NUL byte.
    #[must_use]
    pub fn parse(xaml: &str) -> Option<Self> {
        let c = CString::new(xaml).expect("xaml contained interior NUL");
        // SAFETY: c lives for the call; the result is a +1-owned DataTemplate.
        let ptr = unsafe { noesis_data_template_parse(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Raw `Noesis::DataTemplate*` (a `BaseComponent*`), borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The element named `name` in this template's instance on
    /// `templated_parent`, or `None` if not found. Borrowed, as in
    /// [`ControlTemplate::find_name`].
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn find_name(
        &self,
        name: &str,
        templated_parent: &FrameworkElement,
    ) -> Option<NonNull<c_void>> {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr live; c lives for the call; templated_parent.raw() is
        // a live FrameworkElement*. The returned pointer is borrowed.
        let p = unsafe {
            noesis_framework_template_find_name(
                self.ptr.as_ptr(),
                c.as_ptr(),
                templated_parent.raw(),
            )
        };
        NonNull::new(p)
    }
}

impl Drop for DataTemplate {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (parse).
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A boxed value read back from a trigger or condition. Released on drop.
/// The `as_*` methods unbox `bool`, `i32` and string payloads.
pub struct OwnedValue {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for OwnedValue {}

impl OwnedValue {
    /// Raw `Noesis::BaseComponent*`, borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Unbox a `bool` payload, or `None` if the value is not a boxed `bool`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live boxed BaseComponent*.
        let ok = unsafe { noesis_unbox_bool(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `i32` payload, or `None` if the value is not a boxed `i32`.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let mut out = 0;
        // SAFETY: self.ptr is a live boxed BaseComponent*.
        let ok = unsafe { noesis_unbox_int32(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Copy out a string payload, or `None` if the value is not a boxed
    /// string.
    #[must_use]
    pub fn as_string(&self) -> Option<String> {
        // SAFETY: self.ptr is a live boxed BaseComponent*; the returned pointer
        // (if non-null) is valid while self is alive.
        let p = unsafe { noesis_unbox_string(self.ptr.as_ptr()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: p is a NUL-terminated C string owned by the boxed value.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }
}

impl Drop for OwnedValue {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref by a *_get_value getter.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// Implemented by every trigger type in this module, so
/// [`Style::add_trigger`] accepts any of them. Sealed.
pub trait TriggerHandle: private::Sealed {
    /// Raw `Noesis::BaseTrigger*`, borrowed for the lifetime of `self`.
    fn trigger_ptr(&self) -> *mut c_void;
}

mod private {
    pub trait Sealed {}
}

macro_rules! owned_trigger {
    ($name:ident, $create:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// Released on drop. Add it to a [`Style`] with [`Style::add_trigger`]
        /// before the style is first applied.
        pub struct $name {
            ptr: NonNull<c_void>,
        }

        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl $name {
            /// Construct an empty trigger.
            ///
            /// # Panics
            ///
            /// Panics if Noesis returns null.
            #[must_use]
            pub fn new() -> Self {
                // SAFETY: no preconditions beyond a live Noesis runtime.
                let ptr = unsafe { $create() };
                Self {
                    ptr: NonNull::new(ptr).expect(concat!(stringify!($create), " returned null")),
                }
            }

            /// Raw `Noesis::BaseTrigger*`, borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced with a +1 ref (create).
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }

        impl private::Sealed for $name {}
        impl TriggerHandle for $name {
            fn trigger_ptr(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }
    };
}

owned_trigger!(
    Trigger,
    noesis_templates_trigger_create,
    "A property `Trigger`: applies its setters while a dependency property on the\ntargeted element equals the trigger `Value` (`Noesis::Trigger`)."
);
owned_trigger!(
    DataTrigger,
    noesis_templates_data_trigger_create,
    "A `DataTrigger`: applies its setters while a bound value equals the trigger\n`Value` (`Noesis::DataTrigger`)."
);
owned_trigger!(
    MultiTrigger,
    noesis_templates_multi_trigger_create,
    "A `MultiTrigger`: applies its setters while all of its property\nconditions are met (`Noesis::MultiTrigger`)."
);
owned_trigger!(
    MultiDataTrigger,
    noesis_templates_multi_data_trigger_create,
    "A `MultiDataTrigger`: applies its setters while all of its\nbinding conditions are met (`Noesis::MultiDataTrigger`)."
);
owned_trigger!(
    EventTrigger,
    noesis_templates_event_trigger_create,
    "An `EventTrigger`: runs its actions in response to a routed event\n(`Noesis::EventTrigger`)."
);

impl Trigger {
    /// Set the dependency property this trigger watches, named by type and
    /// property, such as `("ToggleButton", "IsChecked")`. Returns `false` if
    /// either name is unknown or contains a NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_property(&mut self, type_name: &str, dp_name: &str) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; the CStrings live for the call.
        unsafe { noesis_templates_trigger_set_property(self.ptr.as_ptr(), t.as_ptr(), d.as_ptr()) }
    }

    /// Name of the watched property, or `None` if unset.
    #[must_use]
    pub fn property_name(&self) -> Option<String> {
        read_name(unsafe { noesis_templates_trigger_get_property_name(self.ptr.as_ptr()) })
    }

    /// Set the `Value` the property is compared against. Noesis keeps its own
    /// reference.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_value(&mut self, value: &Boxed) -> bool {
        // SAFETY: self.ptr live; value.raw() is a live boxed BaseComponent*.
        unsafe { noesis_templates_trigger_set_value(self.ptr.as_ptr(), value.raw()) }
    }

    /// The trigger's `Value`, or `None` if unset.
    #[must_use]
    pub fn value(&self) -> Option<OwnedValue> {
        owned_value(unsafe { noesis_templates_trigger_get_value(self.ptr.as_ptr()) })
    }

    /// Add a setter for property `dp_name` of `type_name`, applied while the
    /// trigger is active. Returns `false` if either name is unknown or
    /// contains a NUL byte.
    pub fn add_setter(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            noesis_templates_trigger_add_setter(
                self.ptr.as_ptr(),
                t.as_ptr(),
                d.as_ptr(),
                value.raw(),
            )
        }
    }

    /// Number of setters attached to this trigger.
    #[must_use]
    pub fn setter_count(&self) -> u32 {
        count(unsafe { noesis_templates_trigger_setter_count(self.ptr.as_ptr()) })
    }
}

impl DataTrigger {
    /// Set the `Binding` whose value is compared against the trigger `Value`.
    /// Noesis keeps its own reference. Returns `false` only on an invalid
    /// handle.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_binding(&mut self, binding: &Binding) -> bool {
        // SAFETY: self.ptr live; binding.raw() is a live BaseBinding*.
        unsafe { noesis_templates_data_trigger_set_binding(self.ptr.as_ptr(), binding.raw()) }
    }

    /// Whether a `Binding` is set.
    #[must_use]
    pub fn has_binding(&self) -> bool {
        // SAFETY: self.ptr live; the +1 result is released here if present.
        let p = unsafe { noesis_templates_data_trigger_get_binding(self.ptr.as_ptr()) };
        if p.is_null() {
            false
        } else {
            // SAFETY: p is a +1-owned handout we own and must release.
            unsafe { noesis_base_component_release(p) };
            true
        }
    }

    /// Set the `Value` the bound value is compared against.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_value(&mut self, value: &Boxed) -> bool {
        // SAFETY: self.ptr live; value.raw() is a live boxed BaseComponent*.
        unsafe { noesis_templates_data_trigger_set_value(self.ptr.as_ptr(), value.raw()) }
    }

    /// The trigger's `Value`, or `None` if unset.
    #[must_use]
    pub fn value(&self) -> Option<OwnedValue> {
        owned_value(unsafe { noesis_templates_data_trigger_get_value(self.ptr.as_ptr()) })
    }

    /// Add a setter for property `dp_name` of `type_name`, applied while the
    /// trigger is active. Returns `false` if either name is unknown or
    /// contains a NUL byte.
    pub fn add_setter(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            noesis_templates_data_trigger_add_setter(
                self.ptr.as_ptr(),
                t.as_ptr(),
                d.as_ptr(),
                value.raw(),
            )
        }
    }

    /// Number of setters attached to this trigger.
    #[must_use]
    pub fn setter_count(&self) -> u32 {
        count(unsafe { noesis_templates_data_trigger_setter_count(self.ptr.as_ptr()) })
    }
}

impl MultiTrigger {
    /// Add a condition that property `dp_name` of `type_name` equals `value`.
    /// Returns `false` if either name is unknown or contains a NUL byte.
    pub fn add_condition(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            noesis_templates_multi_trigger_add_condition(
                self.ptr.as_ptr(),
                t.as_ptr(),
                d.as_ptr(),
                value.raw(),
            )
        }
    }

    /// Number of conditions.
    #[must_use]
    pub fn condition_count(&self) -> u32 {
        count(unsafe { noesis_templates_multi_trigger_condition_count(self.ptr.as_ptr()) })
    }

    /// Property name of the condition at `index`, or `None` if out of range.
    #[must_use]
    pub fn condition_property_name(&self, index: u32) -> Option<String> {
        read_name(unsafe {
            noesis_templates_multi_trigger_get_condition_property_name(self.ptr.as_ptr(), index)
        })
    }

    /// `Value` of the condition at `index`, or `None` if out of range.
    #[must_use]
    pub fn condition_value(&self, index: u32) -> Option<OwnedValue> {
        owned_value(unsafe {
            noesis_templates_multi_trigger_get_condition_value(self.ptr.as_ptr(), index)
        })
    }

    /// Add a setter for property `dp_name` of `type_name`, applied while all
    /// conditions are met. Returns `false` if either name is unknown or
    /// contains a NUL byte.
    pub fn add_setter(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            noesis_templates_multi_trigger_add_setter(
                self.ptr.as_ptr(),
                t.as_ptr(),
                d.as_ptr(),
                value.raw(),
            )
        }
    }

    /// Number of setters.
    #[must_use]
    pub fn setter_count(&self) -> u32 {
        count(unsafe { noesis_templates_multi_trigger_setter_count(self.ptr.as_ptr()) })
    }
}

impl MultiDataTrigger {
    /// Add a condition that the value produced by `binding` (typically from the
    /// `DataContext`) equals `value`. Noesis keeps its own references to both.
    /// Returns `false` only on an invalid handle.
    pub fn add_condition(&mut self, binding: &Binding, value: &Boxed) -> bool {
        // SAFETY: self.ptr live; binding.raw() is a live BaseBinding*; value.raw()
        // a live boxed BaseComponent*, both for the call.
        unsafe {
            noesis_templates_multi_data_trigger_add_condition(
                self.ptr.as_ptr(),
                binding.raw(),
                value.raw(),
            )
        }
    }

    /// Number of conditions.
    #[must_use]
    pub fn condition_count(&self) -> u32 {
        count(unsafe { noesis_templates_multi_data_trigger_condition_count(self.ptr.as_ptr()) })
    }

    /// Whether the condition at `index` has a `Binding`. `false` if `index` is
    /// out of range.
    #[must_use]
    pub fn condition_has_binding(&self, index: u32) -> bool {
        // SAFETY: self.ptr live; returns -1 / 0 / 1.
        let has = unsafe {
            noesis_templates_multi_data_trigger_condition_has_binding(self.ptr.as_ptr(), index)
        };
        has == 1
    }

    /// `Value` of the condition at `index`, or `None` if out of range.
    #[must_use]
    pub fn condition_value(&self, index: u32) -> Option<OwnedValue> {
        owned_value(unsafe {
            noesis_templates_multi_data_trigger_get_condition_value(self.ptr.as_ptr(), index)
        })
    }

    /// Add a setter for property `dp_name` of `type_name`, applied while all
    /// conditions are met. Returns `false` if either name is unknown or
    /// contains a NUL byte.
    pub fn add_setter(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            noesis_templates_multi_data_trigger_add_setter(
                self.ptr.as_ptr(),
                t.as_ptr(),
                d.as_ptr(),
                value.raw(),
            )
        }
    }

    /// Number of setters.
    #[must_use]
    pub fn setter_count(&self) -> u32 {
        count(unsafe { noesis_templates_multi_data_trigger_setter_count(self.ptr.as_ptr()) })
    }
}

impl EventTrigger {
    /// Set the routed event that fires this trigger, named by owner type and
    /// event, such as `("Button", "Click")`. Returns `false` if either name is
    /// unknown or contains a NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_routed_event(&mut self, owner_type: &str, event_name: &str) -> bool {
        let (Ok(o), Ok(e)) = (CString::new(owner_type), CString::new(event_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings live for the call.
        unsafe {
            noesis_templates_event_trigger_set_routed_event(
                self.ptr.as_ptr(),
                o.as_ptr(),
                e.as_ptr(),
            )
        }
    }

    /// Name of the trigger's routed event, or `None` if unset.
    #[must_use]
    pub fn routed_event_name(&self) -> Option<String> {
        read_name(unsafe {
            noesis_templates_event_trigger_get_routed_event_name(self.ptr.as_ptr())
        })
    }

    /// Set `SourceName`, the name of the element whose event fires this
    /// trigger. Returns `false` if `name` contains a NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_source_name(&mut self, name: &str) -> bool {
        let Ok(n) = CString::new(name) else {
            return false;
        };
        // SAFETY: self.ptr live; n lives for the call.
        unsafe { noesis_templates_event_trigger_set_source_name(self.ptr.as_ptr(), n.as_ptr()) }
    }

    /// The trigger's `SourceName`, or `None` if unset or empty.
    #[must_use]
    pub fn source_name(&self) -> Option<String> {
        read_name(unsafe { noesis_templates_event_trigger_get_source_name(self.ptr.as_ptr()) })
            .filter(|s| !s.is_empty())
    }

    /// Number of actions in the trigger's `Actions`.
    #[must_use]
    pub fn action_count(&self) -> u32 {
        count(unsafe { noesis_templates_event_trigger_action_count(self.ptr.as_ptr()) })
    }

    /// Add a [`BeginStoryboard`] action, which starts its [`Storyboard`] when
    /// the routed event fires. The `Actions` collection takes its own
    /// reference, so you can drop the action handle afterwards. Returns `false`
    /// only on an invalid handle.
    ///
    /// [`Storyboard`]: crate::animation::Storyboard
    pub fn add_action(&mut self, action: &BeginStoryboard) -> bool {
        // SAFETY: self.ptr live; action.raw() is a live TriggerAction*
        // (BeginStoryboard*); Noesis AddRefs it into the Actions collection.
        unsafe { noesis_templates_event_trigger_add_action(self.ptr.as_ptr(), action.raw()) }
    }
}

/// A trigger read back from a [`Style`] by [`Style::get_trigger`]. Released
/// on drop.
///
/// Each accessor applies to one trigger kind and returns `None` or `0` when
/// the trigger is a different kind.
pub struct TriggerReadback {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for TriggerReadback {}

impl TriggerReadback {
    /// Raw `Noesis::BaseTrigger*`, borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Watched property name ([`Trigger`] only).
    #[must_use]
    pub fn property_name(&self) -> Option<String> {
        read_name(unsafe { noesis_templates_trigger_get_property_name(self.ptr.as_ptr()) })
    }

    /// Compared `Value` ([`Trigger`] only).
    #[must_use]
    pub fn value(&self) -> Option<OwnedValue> {
        owned_value(unsafe { noesis_templates_trigger_get_value(self.ptr.as_ptr()) })
    }

    /// Setter count ([`Trigger`] only).
    #[must_use]
    pub fn setter_count(&self) -> u32 {
        count(unsafe { noesis_templates_trigger_setter_count(self.ptr.as_ptr()) })
    }

    /// Routed-event name ([`EventTrigger`] only).
    #[must_use]
    pub fn routed_event_name(&self) -> Option<String> {
        read_name(unsafe {
            noesis_templates_event_trigger_get_routed_event_name(self.ptr.as_ptr())
        })
    }

    /// Condition count ([`MultiTrigger`] only).
    #[must_use]
    pub fn condition_count(&self) -> u32 {
        count(unsafe { noesis_templates_multi_trigger_condition_count(self.ptr.as_ptr()) })
    }
}

impl Drop for TriggerReadback {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref by get_trigger.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

fn read_name(p: *const std::os::raw::c_char) -> Option<String> {
    if p.is_null() {
        return None;
    }
    // SAFETY: p is a NUL-terminated C string owned by Noesis, valid for the call.
    Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
}

fn owned_value(p: *mut c_void) -> Option<OwnedValue> {
    NonNull::new(p).map(|ptr| OwnedValue { ptr })
}

fn count(n: i32) -> u32 {
    u32::try_from(n.max(0)).unwrap_or(0)
}

/// Your logic for a [`TemplateSelector`]: pick a [`DataTemplate`] for a data
/// item. Implemented for closures with the same signature as
/// [`select`](Self::select).
pub trait SelectTemplate: Send + 'static {
    /// Return the raw `DataTemplate*` to use for `item`, or `None` for no
    /// template. `item` is the data object (`BaseComponent*`) and `container`
    /// the item's container (`DependencyObject*`); both are borrowed and may be
    /// null.
    ///
    /// The returned pointer is borrowed, so keep the candidate templates alive
    /// yourself, for example by owning the [`DataTemplate`]s in the selector.
    /// Calls can re-enter when a selected template hosts items that use the
    /// same selector, so keep mutable state behind interior mutability.
    fn select(&self, item: *mut c_void, container: *mut c_void) -> Option<*mut c_void>;
}

impl<F: Fn(*mut c_void, *mut c_void) -> Option<*mut c_void> + Send + 'static> SelectTemplate for F {
    fn select(&self, item: *mut c_void, container: *mut c_void) -> Option<*mut c_void> {
        self(item, container)
    }
}

/// A `DataTemplateSelector` that chooses templates with your
/// [`SelectTemplate`] logic. Released on drop.
///
/// Set it on `ItemTemplateSelector` or `ContentTemplateSelector` with
/// [`FrameworkElement::set_component`](crate::view::FrameworkElement::set_component),
/// passing [`raw`](Self::raw), or call [`select`](Self::select) directly.
pub struct TemplateSelector {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for TemplateSelector {}

impl TemplateSelector {
    /// Create a selector backed by `handler`. `handler` is dropped when Noesis
    /// releases the selector's last reference, which may be after this handle
    /// drops.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns null.
    #[must_use]
    pub fn new<H: SelectTemplate>(handler: H) -> Self {
        let boxed: Box<Box<dyn SelectTemplate>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(boxed).cast::<c_void>();
        const VTABLE: TemplateSelectorVTable = TemplateSelectorVTable {
            select: selector_select_trampoline,
        };
        // SAFETY: VTABLE is 'static; userdata is the leaked handler box, freed by
        // selector_free_trampoline when the native object is destroyed.
        let ptr = unsafe {
            noesis_templates_selector_create(&VTABLE, userdata, selector_free_trampoline)
        };
        match NonNull::new(ptr) {
            Some(ptr) => Self { ptr },
            None => {
                // Reclaim the leaked box so a (single) failed create doesn't leak.
                // SAFETY: userdata is the box we just leaked and ownership wasn't taken.
                drop(unsafe { Box::from_raw(userdata.cast::<Box<dyn SelectTemplate>>()) });
                panic!("noesis_templates_selector_create returned null");
            }
        }
    }

    /// Raw `Noesis::DataTemplateSelector*` (a `BaseComponent*`), borrowed for
    /// the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Run the selector for `item` and `container`, returning the borrowed
    /// `DataTemplate*` it chose, or `None`.
    ///
    /// # Safety
    ///
    /// `item` must be null or a live `BaseComponent*`, and `container` null or
    /// a live `DependencyObject*`.
    #[must_use]
    pub unsafe fn select(
        &self,
        item: *mut c_void,
        container: *mut c_void,
    ) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live selector; item/container per # Safety.
        let p = unsafe { noesis_templates_selector_select(self.ptr.as_ptr(), item, container) };
        NonNull::new(p)
    }
}

impl Drop for TemplateSelector {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (create); destroy releases it and, on
        // the final release, runs selector_free_trampoline to drop the handler.
        unsafe { noesis_templates_selector_destroy(self.ptr.as_ptr()) }
    }
}

unsafe extern "C" fn selector_select_trampoline(
    userdata: *mut c_void,
    item: *mut c_void,
    container: *mut c_void,
) -> *mut c_void {
    crate::panic_guard::guard_or(core::ptr::null_mut(), || {
        if userdata.is_null() {
            return core::ptr::null_mut();
        }
        // SAFETY: userdata is the Box<Box<dyn SelectTemplate>> leaked in `new`, alive
        // until selector_free_trampoline runs. Shared `&`: re-entrant handler box
        // (see `SelectTemplate::select`).
        let handler = unsafe { &*userdata.cast::<Box<dyn SelectTemplate>>() };
        handler
            .select(item, container)
            .unwrap_or(core::ptr::null_mut())
    })
}

unsafe extern "C" fn selector_free_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        // SAFETY: called exactly once when the native object is destroyed; reclaims
        // the leaked handler box.
        drop(unsafe { Box::from_raw(userdata.cast::<Box<dyn SelectTemplate>>()) });
    })
}
