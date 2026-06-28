//! Code-side element-tree construction (Phase 1): build and mutate panel trees
//! and `Grid` row/column definitions from Rust.
//!
//! The built-in element types are created via XAML parse and driven by name, but
//! the collections that hold a tree's structure — `Panel::Children`,
//! `Grid::RowDefinitions` / `ColumnDefinitions` — and the `Decorator::Child`
//! slot are **not** `DependencyProperty`s, so the by-name DP setters cannot
//! reach them. This module wraps the typed C++ accessors instead:
//!
//! * [`panel_children`] hands out a [`PanelChildren`] view over a parsed
//!   `Panel`'s (`StackPanel` / `Grid` / `Canvas` / …) live
//!   `UIElementCollection`, with add / insert / remove / clear / count / get.
//! * [`row_definitions`] / [`column_definitions`] hand out a
//!   [`DefinitionCollection`] over a `Grid`'s definitions; build
//!   [`RowDefinition`] / [`ColumnDefinition`] from code, set their
//!   [`GridLength`], add them, and read the lengths back.
//! * The `Decorator` / `Border` `Child` slot is on
//!   [`FrameworkElement`](crate::view::FrameworkElement) itself
//!   ([`set_decorator_child`](crate::view::FrameworkElement::set_decorator_child)
//!   / [`decorator_child`](crate::view::FrameworkElement::decorator_child)).
//!
//! Each collection handle holds a `+1` reference to the live Noesis collection
//! (which is also owned by its host element) and releases it on [`Drop`], the
//! same ownership idiom as [`crate::text_inlines::InlineCollection`]. The
//! definition builders are owning handles over freshly-created Noesis objects;
//! adding one to a collection makes the collection take its own reference, so
//! the builder handle may be dropped afterwards.
//!
//! Read-back getters re-read from the live Noesis object, so they prove a value
//! crossed the FFI rather than echoing a Rust-side cache.

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::ffi::{
    dm_noesis_base_component_add_reference, dm_noesis_base_component_release,
    dm_noesis_definition_collection_add, dm_noesis_definition_collection_clear,
    dm_noesis_definition_collection_count, dm_noesis_definition_collection_get,
    dm_noesis_definition_collection_insert, dm_noesis_definition_collection_remove_at,
    dm_noesis_grid_column_definition_create, dm_noesis_grid_column_definition_get_width,
    dm_noesis_grid_column_definition_set_width, dm_noesis_grid_get_column_definitions,
    dm_noesis_grid_get_row_definitions, dm_noesis_grid_row_definition_create,
    dm_noesis_grid_row_definition_get_height, dm_noesis_grid_row_definition_set_height,
    dm_noesis_panel_children_add, dm_noesis_panel_children_clear, dm_noesis_panel_children_count,
    dm_noesis_panel_children_get, dm_noesis_panel_children_get_at, dm_noesis_panel_children_insert,
    dm_noesis_panel_children_remove_at,
};
use crate::view::FrameworkElement;

/// The kind of value a [`GridLength`] holds, mirroring `Noesis::GridUnitType`.
///
/// Note the WPF-unusual ordinal order (`Auto` precedes `Pixel`), matched to the
/// SDK's `NsGui/GridLength.h` so the value round-trips by ordinal.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum GridUnitType {
    /// Size determined by the content (the `value` is ignored).
    Auto = 0,
    /// Size expressed as an absolute number of device-independent pixels.
    Pixel = 1,
    /// Size expressed as a weighted proportion of the remaining space (`*`).
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

/// A marshalled `Noesis::GridLength`: a `value` paired with its
/// [`GridUnitType`]. Used to size a [`RowDefinition`] / [`ColumnDefinition`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct GridLength {
    /// The magnitude. Ignored for [`GridUnitType::Auto`]; a pixel count for
    /// [`GridUnitType::Pixel`]; a star weight for [`GridUnitType::Star`].
    pub value: f32,
    /// How `value` is interpreted.
    pub unit: GridUnitType,
}

impl GridLength {
    /// An absolute pixel length.
    #[must_use]
    pub const fn pixels(value: f32) -> Self {
        Self {
            value,
            unit: GridUnitType::Pixel,
        }
    }

    /// An auto-sized length (sizes to content).
    #[must_use]
    pub const fn auto() -> Self {
        Self {
            value: 0.0,
            unit: GridUnitType::Auto,
        }
    }

    /// A star (proportional) length with the given weight.
    #[must_use]
    pub const fn star(weight: f32) -> Self {
        Self {
            value: weight,
            unit: GridUnitType::Star,
        }
    }
}

/// A `Noesis::BaseDefinition` builder ([`RowDefinition`] / [`ColumnDefinition`]).
/// Implemented by both so [`DefinitionCollection::add`] / `insert` accept either
/// while keeping non-definition objects out.
pub trait GridDefinition {
    /// Borrowed `Noesis::BaseDefinition*` (a `BaseComponent*`), valid for
    /// `self`'s lifetime. Used by the collection sugar; not normally called
    /// directly.
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
            /// Construct a definition with a default `1*` length.
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
            /// Returns `false` only if the underlying object is somehow not the
            /// expected definition type (not expected for a live handle).
            #[must_use = "a false return means the length was not set"]
            pub fn set_length(&mut self, length: GridLength) -> bool {
                // SAFETY: self.ptr is a live definition*.
                unsafe { $set(self.ptr.as_ptr(), length.value, length.unit as i32) }
            }

            /// Read the length back from the live Noesis object. `None` if the
            /// unit ordinal is unknown (not expected for a live definition).
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

            /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
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
                unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

definition_handle!(
    /// A `Grid` `RowDefinition`. Its [`GridLength`] sizes the row's height.
    RowDefinition,
    dm_noesis_grid_row_definition_create,
    dm_noesis_grid_row_definition_set_height,
    dm_noesis_grid_row_definition_get_height,
    "Set the row's `Height`."
);
definition_handle!(
    /// A `Grid` `ColumnDefinition`. Its [`GridLength`] sizes the column's width.
    ColumnDefinition,
    dm_noesis_grid_column_definition_create,
    dm_noesis_grid_column_definition_set_width,
    dm_noesis_grid_column_definition_get_width,
    "Set the column's `Width`."
);

/// An owning handle over a live `Noesis::UIElementCollection` — a parsed
/// `Panel`'s `Children`. Holds a `+1` reference released on [`Drop`]; the
/// collection is also owned by the host `Panel`, so this is a non-exclusive view
/// that keeps it alive while held.
pub struct PanelChildren {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for PanelChildren {}

impl PanelChildren {
    /// Append `child` to the panel (the collection takes its own reference, so
    /// `child` may be dropped afterwards). Returns the insertion index, or
    /// `None` if `child` is not a `UIElement`.
    pub fn add(&mut self, child: &FrameworkElement) -> Option<usize> {
        // SAFETY: self.ptr is a live UIElementCollection*; child.raw() is a live
        // UIElement* for the call.
        let idx = unsafe { dm_noesis_panel_children_add(self.ptr.as_ptr(), child.raw()) };
        (idx >= 0).then_some(idx as usize)
    }

    /// Insert `child` at `index` (allows `index == count`). Returns `false` if
    /// `child` is not a `UIElement` or `index` is out of range.
    #[must_use = "a false return means the child was not inserted"]
    pub fn insert(&mut self, index: usize, child: &FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live UIElementCollection*; child.raw() is live.
        unsafe { dm_noesis_panel_children_insert(self.ptr.as_ptr(), index as u32, child.raw()) }
    }

    /// Remove the child at `index`. Returns `false` if `index` is out of range.
    #[must_use = "a false return means nothing was removed"]
    pub fn remove_at(&mut self, index: usize) -> bool {
        // SAFETY: self.ptr is a live UIElementCollection*.
        unsafe { dm_noesis_panel_children_remove_at(self.ptr.as_ptr(), index as u32) }
    }

    /// Remove every child.
    #[must_use = "a false return means this is not a panel children collection"]
    pub fn clear(&mut self) -> bool {
        // SAFETY: self.ptr is a live UIElementCollection*.
        unsafe { dm_noesis_panel_children_clear(self.ptr.as_ptr()) }
    }

    /// Number of children currently in the collection.
    #[must_use]
    pub fn count(&self) -> usize {
        // SAFETY: self.ptr is a live UIElementCollection*.
        let n = unsafe { dm_noesis_panel_children_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }

    /// The child at `index` as an owning [`FrameworkElement`] (an independent
    /// `+1`, so dropping it does not affect the panel), or `None` if out of
    /// range.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<FrameworkElement> {
        let borrowed = NonNull::new(self.get_raw(index))?;
        // AddRef so the returned handle owns its reference, released on drop.
        // SAFETY: `borrowed` is a live UIElement* (BaseComponent*).
        let owned = unsafe { dm_noesis_base_component_add_reference(borrowed.as_ptr()) };
        NonNull::new(owned).map(|ptr| unsafe { FrameworkElement::from_owned(ptr) })
    }

    /// Borrowed raw `UIElement*` at `index`, or null if out of range. Useful for
    /// proving structure via pointer identity against a child that was added
    /// (compare against [`FrameworkElement::raw`]).
    #[must_use]
    pub fn get_raw(&self, index: usize) -> *mut c_void {
        // SAFETY: self.ptr is a live UIElementCollection*; bounds checked C-side.
        unsafe { dm_noesis_panel_children_get_at(self.ptr.as_ptr(), index as u32) }
    }
}

impl Drop for PanelChildren {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_panel_children_get with a +1 ref we own;
        // released exactly once here.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// An owning handle over a live `Grid` `RowDefinitionCollection` or
/// `ColumnDefinitionCollection`. Holds a `+1` reference released on [`Drop`];
/// the collection is also owned by the host `Grid`.
pub struct DefinitionCollection {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for DefinitionCollection {}

impl DefinitionCollection {
    /// Append `definition` (the collection takes its own reference, so the
    /// builder handle may be dropped afterwards). Returns the insertion index,
    /// or `None` if `definition` is the wrong definition type for this
    /// collection.
    pub fn add<D: GridDefinition>(&mut self, definition: &D) -> Option<usize> {
        // SAFETY: self.ptr is a live definition collection*; definition_raw() is
        // a live BaseDefinition* for the call.
        let idx = unsafe {
            dm_noesis_definition_collection_add(self.ptr.as_ptr(), definition.definition_raw())
        };
        (idx >= 0).then_some(idx as usize)
    }

    /// Insert `definition` at `index` (allows `index == count`). Returns `false`
    /// on a type mismatch or out-of-range `index`.
    #[must_use = "a false return means the definition was not inserted"]
    pub fn insert<D: GridDefinition>(&mut self, index: usize, definition: &D) -> bool {
        // SAFETY: self.ptr is a live definition collection*; definition_raw() is
        // a live BaseDefinition* for the call.
        unsafe {
            dm_noesis_definition_collection_insert(
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
        unsafe { dm_noesis_definition_collection_remove_at(self.ptr.as_ptr(), index as u32) }
    }

    /// Remove every definition.
    #[must_use = "a false return means this is not a definition collection"]
    pub fn clear(&mut self) -> bool {
        // SAFETY: self.ptr is a live definition collection*.
        unsafe { dm_noesis_definition_collection_clear(self.ptr.as_ptr()) }
    }

    /// Number of definitions currently in the collection.
    #[must_use]
    pub fn count(&self) -> usize {
        // SAFETY: self.ptr is a live definition collection*.
        let n = unsafe { dm_noesis_definition_collection_count(self.ptr.as_ptr()) };
        n.max(0) as usize
    }

    /// Borrowed raw `BaseDefinition*` at `index`, or null if out of range.
    /// Useful for proving structure via pointer identity against a definition
    /// that was added (compare against [`RowDefinition::raw`] /
    /// [`ColumnDefinition::raw`]).
    #[must_use]
    pub fn get_raw(&self, index: usize) -> *mut c_void {
        // SAFETY: self.ptr is a live definition collection*; bounds checked
        // C-side.
        unsafe { dm_noesis_definition_collection_get(self.ptr.as_ptr(), index as u32) }
    }
}

impl Drop for DefinitionCollection {
    fn drop(&mut self) {
        // SAFETY: produced by a grid-get-definitions entrypoint with a +1 ref we
        // own; released exactly once here.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// The live [`PanelChildren`] of a `Panel` (`StackPanel` / `Grid` / `Canvas` /
/// …). `None` if `element` is not a `Panel`.
#[must_use]
pub fn panel_children(element: &FrameworkElement) -> Option<PanelChildren> {
    // SAFETY: element.raw() is a live FrameworkElement*; the C side DynamicCasts
    // to Panel and hands out a +1 collection (or null).
    let ptr = unsafe { dm_noesis_panel_children_get(element.raw()) };
    NonNull::new(ptr).map(|ptr| PanelChildren { ptr })
}

/// The live [`DefinitionCollection`] of a `Grid`'s `RowDefinitions`. `None` if
/// `element` is not a `Grid`.
#[must_use]
pub fn row_definitions(element: &FrameworkElement) -> Option<DefinitionCollection> {
    // SAFETY: element.raw() is a live FrameworkElement*; +1 collection or null.
    let ptr = unsafe { dm_noesis_grid_get_row_definitions(element.raw()) };
    NonNull::new(ptr).map(|ptr| DefinitionCollection { ptr })
}

/// The live [`DefinitionCollection`] of a `Grid`'s `ColumnDefinitions`. `None`
/// if `element` is not a `Grid`.
#[must_use]
pub fn column_definitions(element: &FrameworkElement) -> Option<DefinitionCollection> {
    // SAFETY: element.raw() is a live FrameworkElement*; +1 collection or null.
    let ptr = unsafe { dm_noesis_grid_get_column_definitions(element.raw()) };
    NonNull::new(ptr).map(|ptr| DefinitionCollection { ptr })
}
