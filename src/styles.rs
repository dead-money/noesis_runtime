//! `Style` from code + template assignment (TODO §7).
//!
//! The XAML you'd write as:
//!
//! ```xml
//! <Style TargetType="TextBlock">
//!   <Setter Property="FontSize" Value="24"/>
//! </Style>
//! ```
//!
//! built programmatically:
//!
//! ```no_run
//! # use dm_noesis_runtime::styles::Style;
//! # use dm_noesis_runtime::binding::box_f64;
//! let mut style = Style::new();
//! style.set_target_type("TextBlock");
//! style.add_setter("FontSize", &box_f64(24.0));
//! ```
//!
//! then assigned with
//! [`FrameworkElement::set_style`](crate::view::FrameworkElement::set_style).
//!
//! Templates ([`ControlTemplate`], [`DataTemplate`]) are the high-value,
//! tractable path: parse a `<ControlTemplate>` / `<DataTemplate>` from an
//! in-memory XAML string and assign it — a `ControlTemplate` via
//! [`FrameworkElement::set_control_template`](crate::view::FrameworkElement::set_control_template),
//! a `DataTemplate` via the existing `set_component` path on `ContentTemplate` /
//! `ItemTemplate`.
//!
//! Style triggers are also built here: [`Trigger`], [`DataTrigger`],
//! [`MultiTrigger`] and [`EventTrigger`] construct from code, attach to a
//! [`Style`]'s `Triggers` collection ([`Style::add_trigger`]) and read back
//! ([`Style::get_trigger`]). A [`TemplateSelector`] wraps a
//! `Noesis::DataTemplateSelector` whose `SelectTemplate` dispatches into Rust.
//!
//! Noesis 3.2.13 has no `FrameworkElementFactory`, so the WPF-style code-built
//! template *factory tree* does not exist here; the XAML-parse + assign path is
//! the supported way to author a template's visual tree.

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::binding::{Binding, Boxed};
use crate::ffi::{
    TemplateSelectorVTable, dm_noesis_base_component_release, dm_noesis_control_template_parse,
    dm_noesis_data_template_parse, dm_noesis_framework_template_find_name,
    dm_noesis_style_add_setter, dm_noesis_style_create, dm_noesis_style_destroy,
    dm_noesis_style_set_based_on, dm_noesis_style_set_target_type,
    dm_noesis_templates_data_trigger_add_setter, dm_noesis_templates_data_trigger_create,
    dm_noesis_templates_data_trigger_get_binding, dm_noesis_templates_data_trigger_get_value,
    dm_noesis_templates_data_trigger_set_binding, dm_noesis_templates_data_trigger_set_value,
    dm_noesis_templates_data_trigger_setter_count, dm_noesis_templates_event_trigger_action_count,
    dm_noesis_templates_event_trigger_create,
    dm_noesis_templates_event_trigger_get_routed_event_name,
    dm_noesis_templates_event_trigger_get_source_name,
    dm_noesis_templates_event_trigger_set_routed_event,
    dm_noesis_templates_event_trigger_set_source_name,
    dm_noesis_templates_multi_trigger_add_condition, dm_noesis_templates_multi_trigger_add_setter,
    dm_noesis_templates_multi_trigger_condition_count, dm_noesis_templates_multi_trigger_create,
    dm_noesis_templates_multi_trigger_get_condition_property_name,
    dm_noesis_templates_multi_trigger_get_condition_value,
    dm_noesis_templates_multi_trigger_setter_count, dm_noesis_templates_selector_create,
    dm_noesis_templates_selector_destroy, dm_noesis_templates_selector_select,
    dm_noesis_templates_style_add_trigger, dm_noesis_templates_style_get_trigger,
    dm_noesis_templates_style_trigger_count, dm_noesis_templates_trigger_add_setter,
    dm_noesis_templates_trigger_create, dm_noesis_templates_trigger_get_property_name,
    dm_noesis_templates_trigger_get_value, dm_noesis_templates_trigger_set_property,
    dm_noesis_templates_trigger_set_value, dm_noesis_templates_trigger_setter_count,
    dm_noesis_unbox_bool, dm_noesis_unbox_int32, dm_noesis_unbox_string,
};
use crate::view::FrameworkElement;

/// A code-built `Noesis::Style` — the programmatic equivalent of a XAML
/// `<Style>`. Owns a `+1` reference released on drop.
///
/// Assigning it to an element
/// ([`FrameworkElement::set_style`](crate::view::FrameworkElement::set_style))
/// or adding it to a [`ResourceDictionary`](crate::resources::ResourceDictionary)
/// makes Noesis take its own reference, so the handle may be dropped afterwards.
///
/// A `Style` is *sealed* the first time it's applied; mutate it (target type,
/// setters, based-on) **before** assigning it.
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
    /// Panics if the Noesis allocation fails (returns null).
    #[must_use]
    pub fn new() -> Self {
        // SAFETY: no preconditions beyond a live Noesis runtime.
        let ptr = unsafe { dm_noesis_style_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_style_create returned null"),
        }
    }

    /// Wrap an already-owned (`+1`) `Noesis::Style*` — e.g. the `AddRef`'d result
    /// of `get_style`.
    ///
    /// # Safety
    ///
    /// `ptr` must be a live `Noesis::Style*` carrying a reference this wrapper
    /// takes ownership of (released on drop).
    #[must_use]
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }

    /// Raw `Noesis::Style*` (a `BaseComponent*`). Borrowed for the lifetime of
    /// `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Set the style's `TargetType` by registered type name (e.g. `"TextBlock"`,
    /// `"Button"`). The type is resolved through Noesis's reflection registry —
    /// it must already be registered, which the built-in controls do on first
    /// use (and any type referenced from loaded XAML does). Returns `false`
    /// (leaving the target type unset) if the name is unknown or contains an
    /// interior NUL byte. Setting the target type is a prerequisite for
    /// [`add_setter`](Self::add_setter), which resolves DPs on it.
    pub fn set_target_type(&mut self, type_name: &str) -> bool {
        let Ok(c) = CString::new(type_name) else {
            return false;
        };
        // SAFETY: self.ptr live; c lives for the call. The C side returns false
        // (no-op) on an unknown type name.
        unsafe { dm_noesis_style_set_target_type(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Append a `Setter` for the dependency property named `dp_name` (resolved
    /// on the style's `TargetType`) with the boxed `value`. The setter stores
    /// its own reference to `value`. Returns `false` if no target type is set,
    /// `dp_name` is not a DP on that type, or `dp_name` contains an interior NUL
    /// byte. Call [`set_target_type`](Self::set_target_type) first.
    pub fn add_setter(&mut self, dp_name: &str, value: &Boxed) -> bool {
        // SAFETY: value.raw() is a live BaseComponent* for the call.
        unsafe { self.add_setter_raw(dp_name, value.raw()) }
    }

    /// Append a `Setter` taking a raw `BaseComponent*` value (e.g. a brush, a
    /// nested object). See [`add_setter`](Self::add_setter) for the resolution
    /// rules and return contract.
    ///
    /// # Safety
    ///
    /// `value` must be a live `Noesis::BaseComponent*` that outlives the call;
    /// the setter takes its own reference.
    pub unsafe fn add_setter_raw(&mut self, dp_name: &str, value: *mut c_void) -> bool {
        let Ok(c) = CString::new(dp_name) else {
            return false;
        };
        // SAFETY: self.ptr live; c lives for the call; value per # Safety.
        unsafe { dm_noesis_style_add_setter(self.ptr.as_ptr(), c.as_ptr(), value) }
    }

    /// Set the `BasedOn` style this style inherits setters/triggers from
    /// (`Style.BasedOn`). Noesis takes its own reference to `base`.
    pub fn set_based_on(&mut self, base: &Style) {
        // SAFETY: both pointers are live Style*; Noesis AddRefs `base`.
        unsafe { dm_noesis_style_set_based_on(self.ptr.as_ptr(), base.raw()) }
    }

    /// Append `trigger` to this style's `Triggers` collection
    /// (`Style::GetTriggers`). The collection takes its own reference, so the
    /// trigger handle may be dropped afterwards. Like setters, triggers must be
    /// added **before** the style is sealed (first applied). Returns `false`
    /// only if the underlying handles are invalid.
    pub fn add_trigger<T: TriggerHandle>(&mut self, trigger: &T) -> bool {
        // SAFETY: both pointers are live; Noesis AddRefs the trigger.
        unsafe { dm_noesis_templates_style_add_trigger(self.ptr.as_ptr(), trigger.trigger_ptr()) }
    }

    /// Number of triggers in this style's `Triggers` collection.
    #[must_use]
    pub fn trigger_count(&self) -> u32 {
        // SAFETY: self.ptr is a live Style*.
        let n = unsafe { dm_noesis_templates_style_trigger_count(self.ptr.as_ptr()) };
        u32::try_from(n.max(0)).unwrap_or(0)
    }

    /// Borrow the trigger at `index` back out of the live `Triggers` collection,
    /// `AddRef`'d so Rust owns it. Use the [`TriggerReadback`] accessors to
    /// re-read its property / value / setter surface straight from Noesis (a
    /// genuine FFI round-trip). `None` if `index` is out of range.
    #[must_use]
    pub fn get_trigger(&self, index: u32) -> Option<TriggerReadback> {
        // SAFETY: self.ptr is a live Style*; the result is a +1-owned trigger.
        let p = unsafe { dm_noesis_templates_style_get_trigger(self.ptr.as_ptr(), index) };
        NonNull::new(p).map(|ptr| TriggerReadback { ptr })
    }
}

impl Drop for Style {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (create / from_owned).
        unsafe { dm_noesis_style_destroy(self.ptr.as_ptr()) }
    }
}

/// A parsed `Noesis::ControlTemplate`. Owns a `+1` reference released on drop.
///
/// Assign it with
/// [`FrameworkElement::set_control_template`](crate::view::FrameworkElement::set_control_template)
/// (which targets `Control::SetTemplate`); Noesis takes its own reference, so
/// the handle may be dropped afterwards. After the templated control has been
/// laid out, name its parts with [`find_name`](Self::find_name) or via
/// [`FrameworkElement::template_child`](crate::view::FrameworkElement::template_child).
pub struct ControlTemplate {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ControlTemplate {}

impl ControlTemplate {
    /// Parse a bare `<ControlTemplate>` from an in-memory XAML string. Returns
    /// `None` when the XAML is malformed or its root is not a `ControlTemplate`.
    ///
    /// # Panics
    ///
    /// Panics if `xaml` contains an interior NUL byte.
    #[must_use]
    pub fn parse(xaml: &str) -> Option<Self> {
        let c = CString::new(xaml).expect("xaml contained interior NUL");
        // SAFETY: c lives for the call; the result is a +1-owned ControlTemplate.
        let ptr = unsafe { dm_noesis_control_template_parse(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Wrap an already-owned (`+1`) `Noesis::ControlTemplate*`.
    ///
    /// # Safety
    ///
    /// `ptr` must be a live `Noesis::ControlTemplate*` carrying a reference this
    /// wrapper takes ownership of.
    #[must_use]
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }

    /// Raw `Noesis::ControlTemplate*` (a `BaseComponent*`). Borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Find a named element inside this template as applied to
    /// `templated_parent` (`FrameworkTemplate::FindName`). Borrowed (no `+1`);
    /// valid only while the template stays applied to that parent. Returns
    /// `None` if the name isn't found (e.g. the template hasn't been applied /
    /// laid out yet).
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
            dm_noesis_framework_template_find_name(
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
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A parsed `Noesis::DataTemplate`. Owns a `+1` reference released on drop.
///
/// Assign it through the existing component-DP path — e.g.
/// `element.set_component("ContentTemplate", template.raw())` on a
/// `ContentControl`, or `"ItemTemplate"` on an `ItemsControl` — which makes
/// Noesis take its own reference.
pub struct DataTemplate {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for DataTemplate {}

impl DataTemplate {
    /// Parse a bare `<DataTemplate>` from an in-memory XAML string. Returns
    /// `None` when the XAML is malformed or its root is not a `DataTemplate`.
    ///
    /// # Panics
    ///
    /// Panics if `xaml` contains an interior NUL byte.
    #[must_use]
    pub fn parse(xaml: &str) -> Option<Self> {
        let c = CString::new(xaml).expect("xaml contained interior NUL");
        // SAFETY: c lives for the call; the result is a +1-owned DataTemplate.
        let ptr = unsafe { dm_noesis_data_template_parse(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Raw `Noesis::DataTemplate*` (a `BaseComponent*`), for handing to
    /// [`FrameworkElement::set_component`](crate::view::FrameworkElement::set_component)
    /// on a template DP (`ContentTemplate` / `ItemTemplate`). Borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Find a named element inside this template as applied to
    /// `templated_parent` (`FrameworkTemplate::FindName`). Borrowed (no `+1`).
    /// Returns `None` if not found.
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
            dm_noesis_framework_template_find_name(
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
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

// ── Triggers (TODO §7) ───────────────────────────────────────────────────────

/// An owned (`+1`) boxed value handed back from a trigger / condition getter.
/// Released on drop. The `as_*` accessors unbox the common primitive payloads so
/// a test can prove the value survived the round-trip through Noesis.
pub struct OwnedValue {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for OwnedValue {}

impl OwnedValue {
    /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Unbox a `bool` payload, or `None` if the value is not a boxed `bool`.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live boxed BaseComponent*.
        let ok = unsafe { dm_noesis_unbox_bool(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Unbox an `i32` payload, or `None` if the value is not a boxed `i32`.
    #[must_use]
    pub fn as_i32(&self) -> Option<i32> {
        let mut out = 0;
        // SAFETY: self.ptr is a live boxed BaseComponent*.
        let ok = unsafe { dm_noesis_unbox_int32(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Borrow the bytes of a boxed `String` payload, or `None` if the value is
    /// not a boxed string.
    #[must_use]
    pub fn as_string(&self) -> Option<String> {
        // SAFETY: self.ptr is a live boxed BaseComponent*; the returned pointer
        // (if non-null) is valid while self is alive.
        let p = unsafe { dm_noesis_unbox_string(self.ptr.as_ptr()) };
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
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// Sealed marker for the four trigger wrappers, so [`Style::add_trigger`] can
/// take any of them by reference.
pub trait TriggerHandle: private::Sealed {
    /// Raw `Noesis::BaseTrigger*`. Borrowed for the lifetime of `self`.
    fn trigger_ptr(&self) -> *mut c_void;
}

mod private {
    pub trait Sealed {}
}

macro_rules! owned_trigger {
    ($name:ident, $create:ident, $doc:literal) => {
        #[doc = $doc]
        ///
        /// Owns a `+1` reference released on drop. Add it to a [`Style`] with
        /// [`Style::add_trigger`] (the `Triggers` collection takes its own
        /// reference) before the style is sealed.
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
            /// Panics if the Noesis allocation fails (returns null).
            #[must_use]
            pub fn new() -> Self {
                // SAFETY: no preconditions beyond a live Noesis runtime.
                let ptr = unsafe { $create() };
                Self {
                    ptr: NonNull::new(ptr).expect(concat!(stringify!($create), " returned null")),
                }
            }

            /// Raw `Noesis::BaseTrigger*`. Borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced with a +1 ref (create).
                unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
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
    dm_noesis_templates_trigger_create,
    "A property `Trigger` — applies its setters while a dependency property on the\ntargeted element equals the trigger `Value` (`Noesis::Trigger`)."
);
owned_trigger!(
    DataTrigger,
    dm_noesis_templates_data_trigger_create,
    "A `DataTrigger` — applies its setters while a bound value equals the trigger\n`Value` (`Noesis::DataTrigger`)."
);
owned_trigger!(
    MultiTrigger,
    dm_noesis_templates_multi_trigger_create,
    "A `MultiTrigger` — applies its setters while **all** of its property\nconditions are met (`Noesis::MultiTrigger`)."
);
owned_trigger!(
    EventTrigger,
    dm_noesis_templates_event_trigger_create,
    "An `EventTrigger` — runs its actions in response to a routed event\n(`Noesis::EventTrigger`)."
);

impl Trigger {
    /// Set the `Property` this trigger watches, resolved by `dp_name` on the
    /// reflection-registered `type_name` (e.g. `("ToggleButton", "IsChecked")`).
    /// Returns `false` if the type or DP name is unknown, or either string has
    /// an interior NUL.
    pub fn set_property(&mut self, type_name: &str, dp_name: &str) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; the CStrings live for the call.
        unsafe {
            dm_noesis_templates_trigger_set_property(self.ptr.as_ptr(), t.as_ptr(), d.as_ptr())
        }
    }

    /// Name of the trigger's `Property`, read back from the live object, or
    /// `None` if unset.
    #[must_use]
    pub fn property_name(&self) -> Option<String> {
        read_name(unsafe { dm_noesis_templates_trigger_get_property_name(self.ptr.as_ptr()) })
    }

    /// Set the `Value` the property is compared against (Noesis stores its own
    /// reference to the boxed value).
    pub fn set_value(&mut self, value: &Boxed) -> bool {
        // SAFETY: self.ptr live; value.raw() is a live boxed BaseComponent*.
        unsafe { dm_noesis_templates_trigger_set_value(self.ptr.as_ptr(), value.raw()) }
    }

    /// The trigger's `Value`, `AddRef`'d back out of the live object.
    #[must_use]
    pub fn value(&self) -> Option<OwnedValue> {
        owned_value(unsafe { dm_noesis_templates_trigger_get_value(self.ptr.as_ptr()) })
    }

    /// Append a setter (`dp_name` resolved on `type_name`) applied while the
    /// trigger is active. Returns `false` on an unresolvable DP or null value.
    pub fn add_setter(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            dm_noesis_templates_trigger_add_setter(
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
        count(unsafe { dm_noesis_templates_trigger_setter_count(self.ptr.as_ptr()) })
    }
}

impl DataTrigger {
    /// Set the `Binding` whose produced value is compared against the trigger
    /// `Value`. Noesis stores its own reference. Returns `false` only on invalid
    /// handles.
    pub fn set_binding(&mut self, binding: &Binding) -> bool {
        // SAFETY: self.ptr live; binding.raw() is a live BaseBinding*.
        unsafe { dm_noesis_templates_data_trigger_set_binding(self.ptr.as_ptr(), binding.raw()) }
    }

    /// Whether a `Binding` is set, observed by reading it back from the live
    /// object (`AddRef`'d and immediately released).
    #[must_use]
    pub fn has_binding(&self) -> bool {
        // SAFETY: self.ptr live; the +1 result is released here if present.
        let p = unsafe { dm_noesis_templates_data_trigger_get_binding(self.ptr.as_ptr()) };
        if p.is_null() {
            false
        } else {
            // SAFETY: p is a +1-owned handout we own and must release.
            unsafe { dm_noesis_base_component_release(p) };
            true
        }
    }

    /// Set the `Value` the bound value is compared against.
    pub fn set_value(&mut self, value: &Boxed) -> bool {
        // SAFETY: self.ptr live; value.raw() is a live boxed BaseComponent*.
        unsafe { dm_noesis_templates_data_trigger_set_value(self.ptr.as_ptr(), value.raw()) }
    }

    /// The trigger's `Value`, `AddRef`'d back out of the live object.
    #[must_use]
    pub fn value(&self) -> Option<OwnedValue> {
        owned_value(unsafe { dm_noesis_templates_data_trigger_get_value(self.ptr.as_ptr()) })
    }

    /// Append a setter (`dp_name` resolved on `type_name`).
    pub fn add_setter(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            dm_noesis_templates_data_trigger_add_setter(
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
        count(unsafe { dm_noesis_templates_data_trigger_setter_count(self.ptr.as_ptr()) })
    }
}

impl MultiTrigger {
    /// Append a `Condition{ Property=dp_name@type_name, Value }`. The trigger
    /// activates only when every condition is met. Returns `false` on an
    /// unresolvable DP or null value.
    pub fn add_condition(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            dm_noesis_templates_multi_trigger_add_condition(
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
        count(unsafe { dm_noesis_templates_multi_trigger_condition_count(self.ptr.as_ptr()) })
    }

    /// Property name of the condition at `index`, read back from the live
    /// object.
    #[must_use]
    pub fn condition_property_name(&self, index: u32) -> Option<String> {
        read_name(unsafe {
            dm_noesis_templates_multi_trigger_get_condition_property_name(self.ptr.as_ptr(), index)
        })
    }

    /// `Value` of the condition at `index`, `AddRef`'d back out of the live
    /// object.
    #[must_use]
    pub fn condition_value(&self, index: u32) -> Option<OwnedValue> {
        owned_value(unsafe {
            dm_noesis_templates_multi_trigger_get_condition_value(self.ptr.as_ptr(), index)
        })
    }

    /// Append a setter (`dp_name` resolved on `type_name`).
    pub fn add_setter(&mut self, type_name: &str, dp_name: &str, value: &Boxed) -> bool {
        let (Ok(t), Ok(d)) = (CString::new(type_name), CString::new(dp_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings + value live for the call.
        unsafe {
            dm_noesis_templates_multi_trigger_add_setter(
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
        count(unsafe { dm_noesis_templates_multi_trigger_setter_count(self.ptr.as_ptr()) })
    }
}

impl EventTrigger {
    /// Set the `RoutedEvent` that fires this trigger, resolved by `event_name`
    /// registered on `owner_type` (e.g. `("Button", "Click")`). Returns `false`
    /// if the type or event name is unknown.
    pub fn set_routed_event(&mut self, owner_type: &str, event_name: &str) -> bool {
        let (Ok(o), Ok(e)) = (CString::new(owner_type), CString::new(event_name)) else {
            return false;
        };
        // SAFETY: self.ptr live; CStrings live for the call.
        unsafe {
            dm_noesis_templates_event_trigger_set_routed_event(
                self.ptr.as_ptr(),
                o.as_ptr(),
                e.as_ptr(),
            )
        }
    }

    /// Name of the trigger's `RoutedEvent`, read back from the live object.
    #[must_use]
    pub fn routed_event_name(&self) -> Option<String> {
        read_name(unsafe {
            dm_noesis_templates_event_trigger_get_routed_event_name(self.ptr.as_ptr())
        })
    }

    /// Set the `SourceName` (the named element whose event activates this
    /// trigger).
    pub fn set_source_name(&mut self, name: &str) -> bool {
        let Ok(n) = CString::new(name) else {
            return false;
        };
        // SAFETY: self.ptr live; n lives for the call.
        unsafe { dm_noesis_templates_event_trigger_set_source_name(self.ptr.as_ptr(), n.as_ptr()) }
    }

    /// The trigger's `SourceName`, read back from the live object (empty string
    /// if unset).
    #[must_use]
    pub fn source_name(&self) -> Option<String> {
        read_name(unsafe { dm_noesis_templates_event_trigger_get_source_name(self.ptr.as_ptr()) })
    }

    /// Number of `TriggerAction` objects in the trigger's `Actions` collection.
    #[must_use]
    pub fn action_count(&self) -> u32 {
        count(unsafe { dm_noesis_templates_event_trigger_action_count(self.ptr.as_ptr()) })
    }
}

/// A trigger `AddRef`'d back out of a [`Style`]'s `Triggers` collection by
/// [`Style::get_trigger`]. Its accessors re-read the *live* Noesis object, so a
/// reading test proves the construction actually crossed the FFI. Each accessor
/// targets one concrete trigger kind (via a `DynamicCast` on the C side) and
/// returns `None` / `0` if this handle is a different kind.
pub struct TriggerReadback {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for TriggerReadback {}

impl TriggerReadback {
    /// Raw `Noesis::BaseTrigger*`. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Property name (`Trigger` only).
    #[must_use]
    pub fn property_name(&self) -> Option<String> {
        read_name(unsafe { dm_noesis_templates_trigger_get_property_name(self.ptr.as_ptr()) })
    }

    /// Compared `Value` (`Trigger` only).
    #[must_use]
    pub fn value(&self) -> Option<OwnedValue> {
        owned_value(unsafe { dm_noesis_templates_trigger_get_value(self.ptr.as_ptr()) })
    }

    /// Setter count (`Trigger` only — use the kind-specific reads for others).
    #[must_use]
    pub fn setter_count(&self) -> u32 {
        count(unsafe { dm_noesis_templates_trigger_setter_count(self.ptr.as_ptr()) })
    }

    /// Routed-event name (`EventTrigger` only).
    #[must_use]
    pub fn routed_event_name(&self) -> Option<String> {
        read_name(unsafe {
            dm_noesis_templates_event_trigger_get_routed_event_name(self.ptr.as_ptr())
        })
    }

    /// Condition count (`MultiTrigger` only).
    #[must_use]
    pub fn condition_count(&self) -> u32 {
        count(unsafe { dm_noesis_templates_multi_trigger_condition_count(self.ptr.as_ptr()) })
    }
}

impl Drop for TriggerReadback {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref by get_trigger.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

// ── shared read-back helpers ─────────────────────────────────────────────────

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

// ── DataTemplateSelector from Rust (TODO §7) ─────────────────────────────────

/// User logic for a [`TemplateSelector`]: choose a [`DataTemplate`] for a data
/// item. Return `None` to select no template.
pub trait SelectTemplate: Send + 'static {
    /// `item` is the borrowed boxed data object (`BaseComponent*`, may be null);
    /// `container` is the borrowed item container (`DependencyObject*`, may be
    /// null). Return the chosen template (its reference is **borrowed** — the
    /// selector keeps its candidate templates alive).
    fn select(&mut self, item: *mut c_void, container: *mut c_void) -> Option<*mut c_void>;
}

impl<F: FnMut(*mut c_void, *mut c_void) -> Option<*mut c_void> + Send + 'static> SelectTemplate
    for F
{
    fn select(&mut self, item: *mut c_void, container: *mut c_void) -> Option<*mut c_void> {
        self(item, container)
    }
}

/// A `Noesis::DataTemplateSelector` whose `SelectTemplate` dispatches into Rust.
/// Owns a `+1` reference released on drop; assign its [`raw`](Self::raw) pointer
/// to an `ItemsControl::ItemTemplateSelector` / `ContentControl` selector DP via
/// the component path, or drive it directly with [`select`](Self::select).
pub struct TemplateSelector {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for TemplateSelector {}

impl TemplateSelector {
    /// Build a selector backed by `handler`. The handler box is owned by the
    /// native object and dropped when its last reference is released.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis allocation fails (returns null).
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
            dm_noesis_templates_selector_create(&VTABLE, userdata, selector_free_trampoline)
        };
        match NonNull::new(ptr) {
            Some(ptr) => Self { ptr },
            None => {
                // Reclaim the leaked box so a (single) failed create doesn't leak.
                // SAFETY: userdata is the box we just leaked and ownership wasn't taken.
                drop(unsafe { Box::from_raw(userdata.cast::<Box<dyn SelectTemplate>>()) });
                panic!("dm_noesis_templates_selector_create returned null");
            }
        }
    }

    /// Raw `Noesis::DataTemplateSelector*` (a `BaseComponent*`). Borrowed for the
    /// lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Drive `SelectTemplate(item, container)` through the C++ virtual, returning
    /// the borrowed `DataTemplate*` the handler chose (or `None`).
    ///
    /// # Safety
    ///
    /// `item` and `container` are passed straight to Noesis: each must be either
    /// null or a live `BaseComponent*` / `DependencyObject*` that outlives the
    /// call.
    #[must_use]
    pub unsafe fn select(
        &self,
        item: *mut c_void,
        container: *mut c_void,
    ) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live selector; item/container per # Safety.
        let p = unsafe { dm_noesis_templates_selector_select(self.ptr.as_ptr(), item, container) };
        NonNull::new(p)
    }
}

impl Drop for TemplateSelector {
    fn drop(&mut self) {
        // SAFETY: produced with a +1 ref (create); destroy releases it and, on
        // the final release, runs selector_free_trampoline to drop the handler.
        unsafe { dm_noesis_templates_selector_destroy(self.ptr.as_ptr()) }
    }
}

unsafe extern "C" fn selector_select_trampoline(
    userdata: *mut c_void,
    item: *mut c_void,
    container: *mut c_void,
) -> *mut c_void {
    if userdata.is_null() {
        return core::ptr::null_mut();
    }
    // SAFETY: userdata is the Box<Box<dyn SelectTemplate>> leaked in `new`, alive
    // until selector_free_trampoline runs.
    let handler = unsafe { &mut *userdata.cast::<Box<dyn SelectTemplate>>() };
    handler
        .select(item, container)
        .unwrap_or(core::ptr::null_mut())
}

unsafe extern "C" fn selector_free_trampoline(userdata: *mut c_void) {
    if userdata.is_null() {
        return;
    }
    // SAFETY: called exactly once when the native object is destroyed; reclaims
    // the leaked handler box.
    drop(unsafe { Box::from_raw(userdata.cast::<Box<dyn SelectTemplate>>()) });
}
