//! Build and change element trees from code: panel children and `Grid` rows and
//! columns.
//!
//! A panel's `Children` and a grid's `RowDefinitions` / `ColumnDefinitions`
//! are collections, not dependency properties, so the by-name property setters
//! can't reach them. This module wraps them directly:
//!
//! * [`FrameworkElement::add_child`] appends to a panel. For insert, remove,
//!   and lookup, get a [`PanelChildren`] from [`panel_children`].
//! * [`row_definitions`] and [`column_definitions`] return a grid's
//!   [`DefinitionCollection`]. Create [`RowDefinition`]s and
//!   [`ColumnDefinition`]s, size them with a [`GridLength`], and add them.
//! * A `Decorator` or `Border` has a single child, set with
//!   [`FrameworkElement::set_decorator_child`].
//!
//! Collection handles keep the collection alive while held; the host element
//! owns it too. Adding a child or definition gives the collection its own
//! reference, so you can drop your handle afterwards.

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::ffi::{
    noesis_base_component_add_reference, noesis_base_component_release,
    noesis_definition_collection_add, noesis_definition_collection_clear,
    noesis_definition_collection_count, noesis_definition_collection_get,
    noesis_definition_collection_insert, noesis_definition_collection_remove_at,
    noesis_grid_column_definition_create, noesis_grid_column_definition_get_width,
    noesis_grid_column_definition_set_width, noesis_grid_get_column_definitions,
    noesis_grid_get_row_definitions, noesis_grid_row_definition_create,
    noesis_grid_row_definition_get_height, noesis_grid_row_definition_set_height,
    noesis_panel_children_add, noesis_panel_children_clear, noesis_panel_children_count,
    noesis_panel_children_get, noesis_panel_children_get_at, noesis_panel_children_insert,
    noesis_panel_children_remove_at,
};
use crate::view::FrameworkElement;

/// How a [`GridLength`] is measured.
///
/// The ordinals match Noesis's `NsGui/GridLength.h`; values cross the FFI by
/// ordinal.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
#[non_exhaustive]
pub enum GridUnitType {
    /// Sized to content; `value` is ignored.
    Auto = 0,
    /// A fixed size in DIPs.
    Pixel = 1,
    /// A weighted share of the remaining space (`*` in XAML).
    Star = 2,
}

impl GridUnitType {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Auto),
            1 => Some(Self::Pixel),
            2 => Some(Self::Star),
            _ => None,
        }
    }
}

/// The height of a [`RowDefinition`] or width of a [`ColumnDefinition`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridLength {
    /// DIPs for [`GridUnitType::Pixel`], a weight for [`GridUnitType::Star`],
    /// ignored for [`GridUnitType::Auto`].
    pub value: f32,
    pub unit: GridUnitType,
}

impl GridLength {
    /// A fixed length in DIPs.
    #[must_use]
    pub const fn pixels(value: f32) -> Self {
        Self {
            value,
            unit: GridUnitType::Pixel,
        }
    }

    /// Size to content.
    #[must_use]
    pub const fn auto() -> Self {
        Self {
            value: 0.0,
            unit: GridUnitType::Auto,
        }
    }

    /// A share of the remaining space; `star(2.0)` is `2*` in XAML.
    #[must_use]
    pub const fn star(weight: f32) -> Self {
        Self {
            value: weight,
            unit: GridUnitType::Star,
        }
    }
}

/// A [`RowDefinition`] or [`ColumnDefinition`], as accepted by
/// [`DefinitionCollection::add`] and [`DefinitionCollection::insert`].
pub trait GridDefinition {
    /// Borrowed `Noesis::BaseDefinition*`, valid while `self` is alive.
    fn definition_raw(&self) -> *mut c_void;
}

macro_rules! definition_handle {
    ($(#[$meta:meta])* $name:ident, $create:ident, $set:ident, $get:ident, $lendoc:literal) => {
        $(#[$meta])*
        pub struct $name {
            ptr: NonNull<c_void>,
        }

        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl $name {
            /// Create a definition with the default length, `1*`.
            ///
            /// # Panics
            ///
            /// Panics if Noesis fails to allocate the object.
            #[must_use]
            pub fn new() -> Self {
                // SAFETY: hands out a +1 definition object.
                let ptr = unsafe { $create() };
                Self {
                    ptr: NonNull::new(ptr)
                        .unwrap_or_else(|| panic!(concat!(stringify!($create), " returned null"))),
                }
            }

            #[doc = $lendoc]
            ///
            /// Returns `false` only if the handle fails its type check, which
            /// doesn't happen for a live handle.
            #[must_use = "a false return means the length was not set"]
            pub fn set_length(&mut self, length: GridLength) -> bool {
                // SAFETY: self.ptr is a live definition*.
                unsafe { $set(self.ptr.as_ptr(), length.value, length.unit as i32) }
            }

            /// The current length. `None` only if Noesis reports an unknown
            /// unit.
            #[must_use]
            pub fn length(&self) -> Option<GridLength> {
                let mut value = 0.0_f32;
                let mut unit = -1_i32;
                // SAFETY: self.ptr is a live definition*; both out-pointers are
                // valid for the call.
                let ok = unsafe { $get(self.ptr.as_ptr(), &mut value, &mut unit) };
                if !ok {
                    return None;
                }
                GridUnitType::from_raw(unit).map(|unit| GridLength { value, unit })
            }

            /// Raw `Noesis::BaseComponent*`, valid while `self` is alive.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl GridDefinition for $name {
            fn definition_raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a *_create entrypoint with a +1 ref we own;
                // released exactly once here.
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

definition_handle!(
    /// A `Grid` row. Its [`GridLength`] is the row's height. Holds one
    /// reference, released on drop.
    RowDefinition,
    noesis_grid_row_definition_create,
    noesis_grid_row_definition_set_height,
    noesis_grid_row_definition_get_height,
    "Set the row's `Height`."
);
definition_handle!(
    /// A `Grid` column. Its [`GridLength`] is the column's width. Holds one
    /// reference, released on drop.
    ColumnDefinition,
    noesis_grid_column_definition_create,
    noesis_grid_column_definition_set_width,
    noesis_grid_column_definition_get_width,
    "Set the column's `Width`."
);

/// A panel's live `Children` collection, from [`panel_children`]. Changes
/// apply to the panel immediately. Holds one reference, released on drop.
pub struct PanelChildren {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for PanelChildren {}

impl PanelChildren {
    /// Append `child` and return its index. The panel keeps its own reference,
    /// so you can drop `child`. Returns `None` if `child` is not a `UIElement`.
    pub fn add(&mut self, child: &FrameworkElement) -> Option<usize> {
        // SAFETY: self.ptr is a live UIElementCollection*; child.raw() is a live
        // UIElement* for the call.
        let idx = unsafe { noesis_panel_children_add(self.ptr.as_ptr(), child.raw()) };
        (idx >= 0).then_some(idx as usize)
    }

    /// Insert `child` at `index`; `index == count()` appends. Returns `false`
    /// if `child` is not a `UIElement` or `index` is out of range.
    #[must_use = "a false return means the child was not inserted"]
    pub fn insert(&mut self, index: usize, child: &FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live UIElementCollection*; child.raw() is live.
        unsafe { noesis_panel_children_insert(self.ptr.as_ptr(), index as u32, child.raw()) }
    }

    /// Remove the child at `index`. Returns `false` if `index` is out of range.
    #[must_use = "a false return means nothing was removed"]
    pub fn remove_at(&mut self, index: usize) -> bool {
        // SAFETY: self.ptr is a live UIElementCollection*.
        unsafe { noesis_panel_children_remove_at(self.ptr.as_ptr(), index as u32) }
    }

    /// Remove every child.
    #[must_use = "a false return means this is not a panel children collection"]
    pub fn clear(&mut self) -> bool {
        // SAFETY: self.ptr is a live UIElementCollection*.
        unsafe { noesis_panel_children_clear(self.ptr.as_ptr()) }
    }

    /// Number of children.
    #[must_use]
    pub fn count(&self) -> usize {
        // SAFETY: self.ptr is a live UIElementCollection*.
        let n = unsafe { noesis_panel_children_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }

    /// The child at `index`, or `None` if out of range. The returned handle
    /// holds its own reference; dropping it doesn't remove the child.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<FrameworkElement> {
        let borrowed = NonNull::new(self.get_raw(index))?;
        // SAFETY: `borrowed` is a live UIElement* (BaseComponent*).
        let owned = unsafe { noesis_base_component_add_reference(borrowed.as_ptr()) };
        NonNull::new(owned).map(|ptr| unsafe { FrameworkElement::from_owned(ptr) })
    }

    /// The borrowed `UIElement*` at `index`, or null if out of range. Compare
    /// with [`FrameworkElement::raw`] to identify a child without taking a
    /// reference.
    #[must_use]
    pub fn get_raw(&self, index: usize) -> *mut c_void {
        // SAFETY: self.ptr is a live UIElementCollection*; bounds checked C-side.
        unsafe { noesis_panel_children_get_at(self.ptr.as_ptr(), index as u32) }
    }
}

impl Drop for PanelChildren {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_panel_children_get with a +1 ref we own;
        // released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A grid's live `RowDefinitions` or `ColumnDefinitions`, from
/// [`row_definitions`] or [`column_definitions`]. Changes apply to the grid
/// immediately. Holds one reference, released on drop.
pub struct DefinitionCollection {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for DefinitionCollection {}

impl DefinitionCollection {
    /// Append `definition` and return its index. The grid keeps its own
    /// reference, so you can drop `definition`. Returns `None` if it is the
    /// wrong kind, e.g. a [`ColumnDefinition`] added to row definitions.
    pub fn add<D: GridDefinition>(&mut self, definition: &D) -> Option<usize> {
        // SAFETY: self.ptr is a live definition collection*; definition_raw() is
        // a live BaseDefinition* for the call.
        let idx = unsafe {
            noesis_definition_collection_add(self.ptr.as_ptr(), definition.definition_raw())
        };
        (idx >= 0).then_some(idx as usize)
    }

    /// Insert `definition` at `index`; `index == count()` appends. Returns
    /// `false` if `index` is out of range. A definition of the wrong kind is
    /// not detected here, so insert only the kind this collection holds.
    #[must_use = "a false return means the definition was not inserted"]
    pub fn insert<D: GridDefinition>(&mut self, index: usize, definition: &D) -> bool {
        // SAFETY: self.ptr is a live definition collection*; definition_raw() is
        // a live BaseDefinition* for the call.
        unsafe {
            noesis_definition_collection_insert(
                self.ptr.as_ptr(),
                index as u32,
                definition.definition_raw(),
            )
        }
    }

    /// Remove the definition at `index`. Returns `false` if `index` is out of
    /// range.
    #[must_use = "a false return means nothing was removed"]
    pub fn remove_at(&mut self, index: usize) -> bool {
        // SAFETY: self.ptr is a live definition collection*.
        unsafe { noesis_definition_collection_remove_at(self.ptr.as_ptr(), index as u32) }
    }

    /// Remove every definition.
    #[must_use = "a false return means this is not a definition collection"]
    pub fn clear(&mut self) -> bool {
        // SAFETY: self.ptr is a live definition collection*.
        unsafe { noesis_definition_collection_clear(self.ptr.as_ptr()) }
    }

    /// Number of definitions.
    #[must_use]
    pub fn count(&self) -> usize {
        // SAFETY: self.ptr is a live definition collection*.
        let n = unsafe { noesis_definition_collection_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }

    /// The borrowed `BaseDefinition*` at `index`, or null if out of range.
    /// Compare with [`RowDefinition::raw`] or [`ColumnDefinition::raw`] to
    /// identify a definition.
    #[must_use]
    pub fn get_raw(&self, index: usize) -> *mut c_void {
        // SAFETY: self.ptr is a live definition collection*; bounds checked
        // C-side.
        unsafe { noesis_definition_collection_get(self.ptr.as_ptr(), index as u32) }
    }
}

impl Drop for DefinitionCollection {
    fn drop(&mut self) {
        // SAFETY: produced by a grid-get-definitions entrypoint with a +1 ref we
        // own; released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

impl FrameworkElement {
    /// Append `child` to this panel's `Children` (`StackPanel`, `Grid`,
    /// `Canvas`, ...). The panel keeps its own reference, so you can drop
    /// `child`. Returns `false` if this element is not a `Panel` or `child` is
    /// not a `UIElement`.
    ///
    /// For the index, insertion, or removal, use [`panel_children`]. For a
    /// `Decorator` or `Border` use
    /// [`set_decorator_child`](FrameworkElement::set_decorator_child); for a
    /// `ContentControl` use [`set_content`](FrameworkElement::set_content).
    #[must_use = "a false return means the child was not added (not a Panel / not a UIElement)"]
    pub fn add_child(&mut self, child: &FrameworkElement) -> bool {
        panel_children(self).is_some_and(|mut children| children.add(child).is_some())
    }
}

/// The [`PanelChildren`] of a panel (`StackPanel`, `Grid`, `Canvas`, ...).
/// `None` if `element` is not a `Panel`.
#[must_use]
pub fn panel_children(element: &FrameworkElement) -> Option<PanelChildren> {
    // SAFETY: element.raw() is a live FrameworkElement*; the C side DynamicCasts
    // to Panel and hands out a +1 collection (or null).
    let ptr = unsafe { noesis_panel_children_get(element.raw()) };
    NonNull::new(ptr).map(|ptr| PanelChildren { ptr })
}

/// A grid's `RowDefinitions`. `None` if `element` is not a `Grid`.
#[must_use]
pub fn row_definitions(element: &FrameworkElement) -> Option<DefinitionCollection> {
    // SAFETY: element.raw() is a live FrameworkElement*; +1 collection or null.
    let ptr = unsafe { noesis_grid_get_row_definitions(element.raw()) };
    NonNull::new(ptr).map(|ptr| DefinitionCollection { ptr })
}

/// A grid's `ColumnDefinitions`. `None` if `element` is not a `Grid`.
#[must_use]
pub fn column_definitions(element: &FrameworkElement) -> Option<DefinitionCollection> {
    // SAFETY: element.raw() is a live FrameworkElement*; +1 collection or null.
    let ptr = unsafe { noesis_grid_get_column_definitions(element.raw()) };
    NonNull::new(ptr).map(|ptr| DefinitionCollection { ptr })
}
