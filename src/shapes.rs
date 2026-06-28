//! Code-built `Shape` elements (TODO §10): construct `Rectangle`, `Ellipse`,
//! and `Line` from Rust and set their drawing properties without authoring XAML.
//!
//! Each handle owns a freshly-created Noesis shape holding a single `+1`
//! reference released on [`Drop`] — the same ownership idiom as
//! [`crate::brushes`] / [`crate::transforms`]. A shape *is* a
//! [`FrameworkElement`](crate::view::FrameworkElement), so once built you can
//! hand its [`raw`](Rectangle::raw) pointer to the element tree (e.g. as a
//! `Panel` child) and let Noesis take its own reference, after which the Rust
//! handle may be dropped.
//!
//! `Fill` and `Stroke` reuse the existing brush wrappers in
//! [`crate::brushes`]: the setters accept any [`Brush`] handle and Noesis takes
//! its own reference to the brush.
//!
//! Every setter has a read-back getter that re-reads from the live Noesis
//! object (`GetRadiusX`, `GetStrokeThickness`, `GetX1`, …), so a test proves a
//! value actually crossed the FFI rather than echoing a Rust-side cache: a
//! stubbed setter fails the round-trip.
//!
//! ## SDK scope
//!
//! Noesis 3.2.13 ships only `Rectangle`, `Ellipse`, `Line`, and `Path` as shape
//! elements — there is **no** `Polygon`/`Polyline`. Build a polygon or polyline
//! as a `PathGeometry`/`StreamGeometry` hosted in a `Path` (the §10 geometry
//! path). See `## Known SDK limitations` in `TODO.md`.

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::brushes::Brush;
use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_ellipse_create, dm_noesis_line_create,
    dm_noesis_line_get, dm_noesis_line_set, dm_noesis_rectangle_create,
    dm_noesis_rectangle_get_radius_x, dm_noesis_rectangle_get_radius_y,
    dm_noesis_rectangle_set_radius_x, dm_noesis_rectangle_set_radius_y, dm_noesis_shape_get_fill,
    dm_noesis_shape_get_height, dm_noesis_shape_get_stretch, dm_noesis_shape_get_stroke,
    dm_noesis_shape_get_stroke_dash_array, dm_noesis_shape_get_stroke_dash_cap,
    dm_noesis_shape_get_stroke_dash_offset, dm_noesis_shape_get_stroke_end_line_cap,
    dm_noesis_shape_get_stroke_line_join, dm_noesis_shape_get_stroke_miter_limit,
    dm_noesis_shape_get_stroke_start_line_cap, dm_noesis_shape_get_stroke_thickness,
    dm_noesis_shape_get_trim_end, dm_noesis_shape_get_trim_offset, dm_noesis_shape_get_trim_start,
    dm_noesis_shape_get_width, dm_noesis_shape_set_fill, dm_noesis_shape_set_height,
    dm_noesis_shape_set_stretch, dm_noesis_shape_set_stroke, dm_noesis_shape_set_stroke_dash_array,
    dm_noesis_shape_set_stroke_dash_cap, dm_noesis_shape_set_stroke_dash_offset,
    dm_noesis_shape_set_stroke_end_line_cap, dm_noesis_shape_set_stroke_line_join,
    dm_noesis_shape_set_stroke_miter_limit, dm_noesis_shape_set_stroke_start_line_cap,
    dm_noesis_shape_set_stroke_thickness, dm_noesis_shape_set_trim_end,
    dm_noesis_shape_set_trim_offset, dm_noesis_shape_set_trim_start, dm_noesis_shape_set_width,
};

/// How the ends of a dash (or a line) are drawn. Ordinals mirror
/// `Noesis::PenLineCap`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum PenLineCap {
    /// A cap that does not extend past the last point of the line.
    Flat = 0,
    /// A rectangle half the line thickness long.
    Square = 1,
    /// A semicircle with diameter equal to the line thickness.
    Round = 2,
    /// An isosceles right triangle.
    Triangle = 3,
}

/// How the vertices of a shape are joined. Ordinals mirror
/// `Noesis::PenLineJoin`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
pub enum PenLineJoin {
    /// Regular angular (mitered) vertices.
    Miter = 0,
    /// Beveled vertices.
    Bevel = 1,
    /// Rounded vertices.
    Round = 2,
}

/// How a shape fills its allocated space. Re-exported from [`crate::brushes`]
/// so the crate has a single `Stretch` type (ordinals mirror `Noesis::Stretch`).
pub use crate::brushes::Stretch;

impl PenLineCap {
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Flat),
            1 => Some(Self::Square),
            2 => Some(Self::Round),
            3 => Some(Self::Triangle),
            _ => None,
        }
    }
}

impl PenLineJoin {
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Miter),
            1 => Some(Self::Bevel),
            2 => Some(Self::Round),
            _ => None,
        }
    }
}

/// A handle to a Noesis `Shape` (the base class of [`Rectangle`], [`Ellipse`],
/// and [`Line`]). The trait carries every property defined on `Shape` plus the
/// inherited `FrameworkElement` `Width`/`Height`, so all three concrete shapes
/// share one implementation.
///
/// Getters re-read from the live Noesis object; a stubbed setter therefore fails
/// the round-trip in `tests/shapes.rs`.
pub trait Shape {
    /// Borrowed `Noesis::Shape*` (also a `FrameworkElement*` / `BaseComponent*`),
    /// valid for `self`'s lifetime. Used by the shared property methods and by
    /// callers handing the shape to other Noesis APIs (e.g. tree insertion).
    fn shape_raw(&self) -> *mut c_void;

    /// Set the element's explicit `Width` (a `FrameworkElement` DP; `NaN` ==
    /// "auto").
    fn set_width(&mut self, width: f32) {
        // SAFETY: shape_raw() is a live Shape*/FrameworkElement*.
        unsafe { dm_noesis_shape_set_width(self.shape_raw(), width) };
    }

    /// Read the element's explicit `Width` back from the live object.
    #[must_use]
    fn width(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is a valid f32 slot.
        unsafe { dm_noesis_shape_get_width(self.shape_raw(), &mut out) };
        out
    }

    /// Set the element's explicit `Height` (`NaN` == "auto").
    fn set_height(&mut self, height: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_height(self.shape_raw(), height) };
    }

    /// Read the element's explicit `Height` back from the live object.
    #[must_use]
    fn height(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is a valid f32 slot.
        unsafe { dm_noesis_shape_get_height(self.shape_raw(), &mut out) };
        out
    }

    /// Paint the shape's interior with `brush` (any [`Brush`] handle). Noesis
    /// takes its own reference, so the brush handle may be dropped afterwards.
    fn set_fill<B: Brush>(&mut self, brush: &B) {
        // SAFETY: shape_raw() is live; brush_raw() is a live Brush* borrowed for
        // the call; Noesis stores its own reference.
        unsafe { dm_noesis_shape_set_fill(self.shape_raw(), brush.brush_raw()) };
    }

    /// Clear the shape's `Fill`.
    fn clear_fill(&mut self) {
        // SAFETY: shape_raw() is live; a null brush clears the property.
        unsafe { dm_noesis_shape_set_fill(self.shape_raw(), core::ptr::null_mut()) };
    }

    /// Borrowed `Brush*` currently set as `Fill`, or null if unset. Returned
    /// without a `+1`; use it only for identity checks against a brush handle's
    /// [`raw`](crate::brushes::SolidColorBrush::raw).
    #[must_use]
    fn fill_raw(&self) -> *mut c_void {
        // SAFETY: shape_raw() is live; the returned pointer is borrowed.
        unsafe { dm_noesis_shape_get_fill(self.shape_raw()) }
    }

    /// Paint the shape's outline with `brush`.
    fn set_stroke<B: Brush>(&mut self, brush: &B) {
        // SAFETY: shape_raw() is live; brush_raw() is a live Brush* for the call.
        unsafe { dm_noesis_shape_set_stroke(self.shape_raw(), brush.brush_raw()) };
    }

    /// Clear the shape's `Stroke`.
    fn clear_stroke(&mut self) {
        // SAFETY: shape_raw() is live; a null brush clears the property.
        unsafe { dm_noesis_shape_set_stroke(self.shape_raw(), core::ptr::null_mut()) };
    }

    /// Borrowed `Brush*` currently set as `Stroke`, or null if unset.
    #[must_use]
    fn stroke_raw(&self) -> *mut c_void {
        // SAFETY: shape_raw() is live; the returned pointer is borrowed.
        unsafe { dm_noesis_shape_get_stroke(self.shape_raw()) }
    }

    /// Set the outline width.
    fn set_stroke_thickness(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_stroke_thickness(self.shape_raw(), value) };
    }

    /// Read the outline width back from the live object.
    #[must_use]
    fn stroke_thickness(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { dm_noesis_shape_get_stroke_thickness(self.shape_raw(), &mut out) };
        out
    }

    /// Set the miter-length limit (ratio to half the `StrokeThickness`).
    fn set_stroke_miter_limit(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_stroke_miter_limit(self.shape_raw(), value) };
    }

    /// Read the miter limit back from the live object.
    #[must_use]
    fn stroke_miter_limit(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { dm_noesis_shape_get_stroke_miter_limit(self.shape_raw(), &mut out) };
        out
    }

    /// Set the distance into the dash pattern at which a dash begins.
    fn set_stroke_dash_offset(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_stroke_dash_offset(self.shape_raw(), value) };
    }

    /// Read the dash offset back from the live object.
    #[must_use]
    fn stroke_dash_offset(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { dm_noesis_shape_get_stroke_dash_offset(self.shape_raw(), &mut out) };
        out
    }

    /// Set the amount to trim from the start of the geometry path (`0..=1`).
    fn set_trim_start(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_trim_start(self.shape_raw(), value) };
    }

    /// Read the trim-start back from the live object.
    #[must_use]
    fn trim_start(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { dm_noesis_shape_get_trim_start(self.shape_raw(), &mut out) };
        out
    }

    /// Set the amount to trim from the end of the geometry path (`0..=1`).
    fn set_trim_end(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_trim_end(self.shape_raw(), value) };
    }

    /// Read the trim-end back from the live object.
    #[must_use]
    fn trim_end(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { dm_noesis_shape_get_trim_end(self.shape_raw(), &mut out) };
        out
    }

    /// Set the amount to offset trimming the geometry path.
    fn set_trim_offset(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_trim_offset(self.shape_raw(), value) };
    }

    /// Read the trim-offset back from the live object.
    #[must_use]
    fn trim_offset(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { dm_noesis_shape_get_trim_offset(self.shape_raw(), &mut out) };
        out
    }

    /// Set the cap drawn at the ends of each dash.
    fn set_stroke_dash_cap(&mut self, cap: PenLineCap) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_stroke_dash_cap(self.shape_raw(), cap as i32) };
    }

    /// Read the dash cap back from the live object (`None` if not a shape).
    #[must_use]
    fn stroke_dash_cap(&self) -> Option<PenLineCap> {
        // SAFETY: shape_raw() is live.
        PenLineCap::from_ordinal(unsafe { dm_noesis_shape_get_stroke_dash_cap(self.shape_raw()) })
    }

    /// Set the cap drawn at the start of the stroke.
    fn set_stroke_start_line_cap(&mut self, cap: PenLineCap) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_stroke_start_line_cap(self.shape_raw(), cap as i32) };
    }

    /// Read the start line cap back from the live object.
    #[must_use]
    fn stroke_start_line_cap(&self) -> Option<PenLineCap> {
        // SAFETY: shape_raw() is live.
        PenLineCap::from_ordinal(unsafe {
            dm_noesis_shape_get_stroke_start_line_cap(self.shape_raw())
        })
    }

    /// Set the cap drawn at the end of the stroke.
    fn set_stroke_end_line_cap(&mut self, cap: PenLineCap) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_stroke_end_line_cap(self.shape_raw(), cap as i32) };
    }

    /// Read the end line cap back from the live object.
    #[must_use]
    fn stroke_end_line_cap(&self) -> Option<PenLineCap> {
        // SAFETY: shape_raw() is live.
        PenLineCap::from_ordinal(unsafe {
            dm_noesis_shape_get_stroke_end_line_cap(self.shape_raw())
        })
    }

    /// Set the join used at the vertices of the stroke.
    fn set_stroke_line_join(&mut self, join: PenLineJoin) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_stroke_line_join(self.shape_raw(), join as i32) };
    }

    /// Read the line join back from the live object.
    #[must_use]
    fn stroke_line_join(&self) -> Option<PenLineJoin> {
        // SAFETY: shape_raw() is live.
        PenLineJoin::from_ordinal(unsafe { dm_noesis_shape_get_stroke_line_join(self.shape_raw()) })
    }

    /// Set how the shape stretches to fill its allocated space.
    fn set_stretch(&mut self, stretch: Stretch) {
        // SAFETY: shape_raw() is live.
        unsafe { dm_noesis_shape_set_stretch(self.shape_raw(), stretch as i32) };
    }

    /// Read the stretch mode back from the live object.
    #[must_use]
    fn stretch(&self) -> Option<Stretch> {
        // SAFETY: shape_raw() is live.
        Stretch::from_ordinal(unsafe { dm_noesis_shape_get_stretch(self.shape_raw()) })
    }

    /// Set the dash pattern. Noesis exposes this as a space-separated string of
    /// dash/gap lengths (e.g. `"2 1 3"`).
    ///
    /// # Panics
    ///
    /// Panics if `dashes` contains an interior NUL byte.
    fn set_stroke_dash_array(&mut self, dashes: &str) {
        let c = CString::new(dashes).expect("dash array contained NUL");
        // SAFETY: shape_raw() is live; `c` outlives the call and the C side
        // copies it into the Noesis object.
        unsafe { dm_noesis_shape_set_stroke_dash_array(self.shape_raw(), c.as_ptr()) };
    }

    /// Read the dash pattern back from the live object as an owned `String`
    /// (empty if unset).
    #[must_use]
    fn stroke_dash_array(&self) -> String {
        // SAFETY: shape_raw() is live; the returned pointer is owned by the
        // Noesis object and valid until the next mutation — we copy immediately.
        let p = unsafe { dm_noesis_shape_get_stroke_dash_array(self.shape_raw()) };
        if p.is_null() {
            String::new()
        } else {
            // SAFETY: `p` is a NUL-terminated C string from Noesis.
            unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned()
        }
    }
}

macro_rules! shape_handle {
    ($name:ident, $create:ident, $doc:literal) => {
        #[doc = $doc]
        pub struct $name {
            ptr: NonNull<c_void>,
        }

        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl $name {
            /// Create the shape with default property values.
            ///
            /// # Panics
            ///
            /// Panics if Noesis fails to allocate the shape (not expected after
            /// [`crate::init`]).
            #[must_use]
            pub fn new() -> Self {
                // SAFETY: a plain component-create call; returns a +1 ref we own.
                let ptr = unsafe { $create() };
                Self {
                    ptr: NonNull::new(ptr).expect(concat!(stringify!($create), " returned null")),
                }
            }

            /// Raw `Noesis::Shape*` (also a `FrameworkElement*` /
            /// `BaseComponent*`). Borrowed for the lifetime of `self`; hand it to
            /// other Noesis APIs (e.g. inserting the shape into an element tree).
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl Shape for $name {
            fn shape_raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a `*_create` entrypoint with a +1 ref we
                // own; released exactly once here.
                unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

shape_handle!(
    Rectangle,
    dm_noesis_rectangle_create,
    "A `Rectangle` shape. Adds rounded-corner radii on top of the shared\n\
     [`Shape`] surface; its size comes from the inherited\n\
     [`Shape::set_width`]/[`Shape::set_height`]."
);
shape_handle!(
    Ellipse,
    dm_noesis_ellipse_create,
    "An `Ellipse` shape. Carries only the shared [`Shape`] surface; its size\n\
     comes from the inherited [`Shape::set_width`]/[`Shape::set_height`]."
);
shape_handle!(
    Line,
    dm_noesis_line_create,
    "A `Line` shape defined by its two endpoints `(X1, Y1)`–`(X2, Y2)` in\n\
     addition to the shared [`Shape`] surface."
);

impl Rectangle {
    /// Set the x-axis corner radius.
    pub fn set_radius_x(&mut self, value: f32) {
        // SAFETY: self.ptr is a live Rectangle*.
        unsafe { dm_noesis_rectangle_set_radius_x(self.ptr.as_ptr(), value) };
    }

    /// Read the x-axis corner radius back from the live object.
    #[must_use]
    pub fn radius_x(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is live; `out` is valid.
        unsafe { dm_noesis_rectangle_get_radius_x(self.ptr.as_ptr(), &mut out) };
        out
    }

    /// Set the y-axis corner radius.
    pub fn set_radius_y(&mut self, value: f32) {
        // SAFETY: self.ptr is a live Rectangle*.
        unsafe { dm_noesis_rectangle_set_radius_y(self.ptr.as_ptr(), value) };
    }

    /// Read the y-axis corner radius back from the live object.
    #[must_use]
    pub fn radius_y(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is live; `out` is valid.
        unsafe { dm_noesis_rectangle_get_radius_y(self.ptr.as_ptr(), &mut out) };
        out
    }
}

impl Line {
    /// Set both endpoints at once: `(x1, y1)` start and `(x2, y2)` end.
    pub fn set_points(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        // SAFETY: self.ptr is a live Line*.
        unsafe { dm_noesis_line_set(self.ptr.as_ptr(), x1, y1, x2, y2) };
    }

    /// Read both endpoints back from the live object as `[x1, y1, x2, y2]`.
    #[must_use]
    pub fn points(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is live; `out` is a 4-float buffer.
        unsafe { dm_noesis_line_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}
