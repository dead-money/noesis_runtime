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
//! `ItemTemplate`. Full code-built `VisualTree` factories and
//! `DataTemplateSelector`-from-Rust are deferred (see TODO §7).

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::binding::Boxed;
use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_control_template_parse,
    dm_noesis_data_template_parse, dm_noesis_framework_template_find_name,
    dm_noesis_style_add_setter, dm_noesis_style_create, dm_noesis_style_destroy,
    dm_noesis_style_set_based_on, dm_noesis_style_set_target_type,
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

// SAFETY: a Noesis BaseComponent handle; same threading rationale as the other
// owning wrappers in this crate.
unsafe impl Send for Style {}
unsafe impl Sync for Style {}

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

// SAFETY: a Noesis BaseComponent handle; same rationale as the other wrappers.
unsafe impl Send for ControlTemplate {}
unsafe impl Sync for ControlTemplate {}

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

// SAFETY: a Noesis BaseComponent handle; same rationale as the other wrappers.
unsafe impl Send for DataTemplate {}
unsafe impl Sync for DataTemplate {}

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
