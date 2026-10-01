//! Build [`Rectangle`], [`Ellipse`] and [`Line`] elements in code.
//!
//! Each handle owns a new Noesis shape and releases it on drop, like the
//! handles in [`crate::brushes`] and [`crate::transforms`]. The drawing
//! properties (fill, stroke, dashes, trim, stretch, size) live on the
//! [`Shape`] trait, which all three implement.
//!
//! A shape is a [`FrameworkElement`](crate::view::FrameworkElement): put it in
//! the element tree with [`Shape::as_element`] or its [`raw`](Rectangle::raw)
//! pointer. The tree takes its own reference, so you can drop the handle
//! afterwards. `Fill` and `Stroke` take any [`Brush`] from [`crate::brushes`],
//! and Noesis likewise keeps its own reference to the brush.
//!
//! Getters read from the live Noesis object, so they reflect changes made by
//! styles, bindings or XAML as well as your own setters.
//!
//! Noesis has no `Polygon` or `Polyline` element. Draw those as a
//! `PathGeometry` or `StreamGeometry` in a `Path`.

use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::brushes::Brush;
use crate::ffi::{
    noesis_base_component_add_reference, noesis_base_component_release, noesis_ellipse_create,
    noesis_line_create, noesis_line_get, noesis_line_set, noesis_rectangle_create,
    noesis_rectangle_get_radius_x, noesis_rectangle_get_radius_y, noesis_rectangle_set_radius_x,
    noesis_rectangle_set_radius_y, noesis_shape_get_fill, noesis_shape_get_height,
    noesis_shape_get_stretch, noesis_shape_get_stroke, noesis_shape_get_stroke_dash_array,
    noesis_shape_get_stroke_dash_cap, noesis_shape_get_stroke_dash_offset,
    noesis_shape_get_stroke_end_line_cap, noesis_shape_get_stroke_line_join,
    noesis_shape_get_stroke_miter_limit, noesis_shape_get_stroke_start_line_cap,
    noesis_shape_get_stroke_thickness, noesis_shape_get_trim_end, noesis_shape_get_trim_offset,
    noesis_shape_get_trim_start, noesis_shape_get_width, noesis_shape_set_fill,
    noesis_shape_set_height, noesis_shape_set_stretch, noesis_shape_set_stroke,
    noesis_shape_set_stroke_dash_array, noesis_shape_set_stroke_dash_cap,
    noesis_shape_set_stroke_dash_offset, noesis_shape_set_stroke_end_line_cap,
    noesis_shape_set_stroke_line_join, noesis_shape_set_stroke_miter_limit,
    noesis_shape_set_stroke_start_line_cap, noesis_shape_set_stroke_thickness,
    noesis_shape_set_trim_end, noesis_shape_set_trim_offset, noesis_shape_set_trim_start,
    noesis_shape_set_width,
};

/// How the ends of a line or dash are drawn.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
#[non_exhaustive]
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

/// How the stroke is drawn where two segments meet.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(i32)]
#[non_exhaustive]
pub enum PenLineJoin {
    /// Regular angular (mitered) vertices.
    Miter = 0,
    /// Beveled vertices.
    Bevel = 1,
    /// Rounded vertices.
    Round = 2,
}

/// How a shape fills its allocated space. The same type as
/// [`crate::brushes::Stretch`].
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

/// The properties shared by [`Rectangle`], [`Ellipse`] and [`Line`]: every
/// property of Noesis's `Shape` plus the element's `Width` and `Height`.
///
/// Getters read from the live Noesis object. The enum getters return `None`
/// if Noesis reports a value this crate doesn't know.
pub trait Shape {
    /// Raw `Noesis::Shape*` (also a `FrameworkElement*` and `BaseComponent*`),
    /// borrowed for the lifetime of `self`.
    fn shape_raw(&self) -> *mut c_void;

    /// A new [`FrameworkElement`](crate::view::FrameworkElement) handle to this
    /// shape, for element-tree APIs such as
    /// [`FrameworkElement::set_content`](crate::view::FrameworkElement::set_content)
    /// or
    /// [`FrameworkElement::set_decorator_child`](crate::view::FrameworkElement::set_decorator_child).
    /// It holds its own reference; dropping it doesn't affect `self`.
    #[must_use]
    fn as_element(&self) -> crate::view::FrameworkElement {
        // SAFETY: shape_raw() is a live FrameworkElement*/BaseComponent*; AddRef
        // hands back an independent +1 the returned handle owns and releases on
        // drop.
        let p = unsafe { noesis_base_component_add_reference(self.shape_raw()) };
        let ptr = NonNull::new(p).expect("add_reference returned null on a live shape");
        // SAFETY: `ptr` carries the +1 we just took ownership of.
        unsafe { crate::view::FrameworkElement::from_owned(ptr) }
    }

    /// Set the explicit `Width`. `f32::NAN` means auto.
    fn set_width(&mut self, width: f32) {
        // SAFETY: shape_raw() is a live Shape*/FrameworkElement*.
        unsafe { noesis_shape_set_width(self.shape_raw(), width) };
    }

    /// The explicit `Width`; `NaN` when auto.
    #[must_use]
    fn width(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is a valid f32 slot.
        unsafe { noesis_shape_get_width(self.shape_raw(), &mut out) };
        out
    }

    /// Set the explicit `Height`. `f32::NAN` means auto.
    fn set_height(&mut self, height: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_height(self.shape_raw(), height) };
    }

    /// The explicit `Height`; `NaN` when auto.
    #[must_use]
    fn height(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is a valid f32 slot.
        unsafe { noesis_shape_get_height(self.shape_raw(), &mut out) };
        out
    }

    /// Paint the interior with `brush`. Noesis keeps its own reference, so you
    /// can drop the brush handle afterwards.
    fn set_fill<B: Brush>(&mut self, brush: &B) {
        // SAFETY: shape_raw() is live; brush_raw() is a live Brush* borrowed for
        // the call; Noesis stores its own reference.
        unsafe { noesis_shape_set_fill(self.shape_raw(), brush.brush_raw()) };
    }

    /// Remove the `Fill`.
    fn clear_fill(&mut self) {
        // SAFETY: shape_raw() is live; a null brush clears the property.
        unsafe { noesis_shape_set_fill(self.shape_raw(), core::ptr::null_mut()) };
    }

    /// The current `Fill` brush, or null if unset. Borrowed; use it only to
    /// compare against a brush handle's
    /// [`raw`](crate::brushes::SolidColorBrush::raw) pointer.
    #[must_use]
    fn fill_raw(&self) -> *mut c_void {
        // SAFETY: shape_raw() is live; the returned pointer is borrowed.
        unsafe { noesis_shape_get_fill(self.shape_raw()) }
    }

    /// Paint the outline with `brush`. Noesis keeps its own reference.
    fn set_stroke<B: Brush>(&mut self, brush: &B) {
        // SAFETY: shape_raw() is live; brush_raw() is a live Brush* for the call.
        unsafe { noesis_shape_set_stroke(self.shape_raw(), brush.brush_raw()) };
    }

    /// Remove the `Stroke`.
    fn clear_stroke(&mut self) {
        // SAFETY: shape_raw() is live; a null brush clears the property.
        unsafe { noesis_shape_set_stroke(self.shape_raw(), core::ptr::null_mut()) };
    }

    /// The current `Stroke` brush, or null if unset. Borrowed, like
    /// [`fill_raw`](Self::fill_raw).
    #[must_use]
    fn stroke_raw(&self) -> *mut c_void {
        // SAFETY: shape_raw() is live; the returned pointer is borrowed.
        unsafe { noesis_shape_get_stroke(self.shape_raw()) }
    }

    /// Set the outline width, in pixels.
    fn set_stroke_thickness(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_stroke_thickness(self.shape_raw(), value) };
    }

    /// The outline width, in pixels.
    #[must_use]
    fn stroke_thickness(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { noesis_shape_get_stroke_thickness(self.shape_raw(), &mut out) };
        out
    }

    /// Set the limit on miter length, as a ratio to half the stroke thickness.
    fn set_stroke_miter_limit(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_stroke_miter_limit(self.shape_raw(), value) };
    }

    /// The miter limit.
    #[must_use]
    fn stroke_miter_limit(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { noesis_shape_get_stroke_miter_limit(self.shape_raw(), &mut out) };
        out
    }

    /// Set how far into the dash pattern the stroke starts, in multiples of
    /// the stroke thickness.
    fn set_stroke_dash_offset(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_stroke_dash_offset(self.shape_raw(), value) };
    }

    /// The dash offset.
    #[must_use]
    fn stroke_dash_offset(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { noesis_shape_get_stroke_dash_offset(self.shape_raw(), &mut out) };
        out
    }

    /// Set the fraction of the path to trim from its start, `0.0..=1.0`.
    fn set_trim_start(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_trim_start(self.shape_raw(), value) };
    }

    /// The start trim fraction.
    #[must_use]
    fn trim_start(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { noesis_shape_get_trim_start(self.shape_raw(), &mut out) };
        out
    }

    /// Set the fraction of the path to trim from its end, `0.0..=1.0`.
    fn set_trim_end(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_trim_end(self.shape_raw(), value) };
    }

    /// The end trim fraction.
    #[must_use]
    fn trim_end(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { noesis_shape_get_trim_end(self.shape_raw(), &mut out) };
        out
    }

    /// Shift the trimmed range along the path.
    fn set_trim_offset(&mut self, value: f32) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_trim_offset(self.shape_raw(), value) };
    }

    /// The trim offset.
    #[must_use]
    fn trim_offset(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: shape_raw() is live; `out` is valid.
        unsafe { noesis_shape_get_trim_offset(self.shape_raw(), &mut out) };
        out
    }

    /// Set the cap drawn at the ends of each dash.
    fn set_stroke_dash_cap(&mut self, cap: PenLineCap) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_stroke_dash_cap(self.shape_raw(), cap as i32) };
    }

    /// The dash cap.
    #[must_use]
    fn stroke_dash_cap(&self) -> Option<PenLineCap> {
        // SAFETY: shape_raw() is live.
        PenLineCap::from_ordinal(unsafe { noesis_shape_get_stroke_dash_cap(self.shape_raw()) })
    }

    /// Set the cap drawn at the start of the stroke.
    fn set_stroke_start_line_cap(&mut self, cap: PenLineCap) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_stroke_start_line_cap(self.shape_raw(), cap as i32) };
    }

    /// The start line cap.
    #[must_use]
    fn stroke_start_line_cap(&self) -> Option<PenLineCap> {
        // SAFETY: shape_raw() is live.
        PenLineCap::from_ordinal(unsafe {
            noesis_shape_get_stroke_start_line_cap(self.shape_raw())
        })
    }

    /// Set the cap drawn at the end of the stroke.
    fn set_stroke_end_line_cap(&mut self, cap: PenLineCap) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_stroke_end_line_cap(self.shape_raw(), cap as i32) };
    }

    /// The end line cap.
    #[must_use]
    fn stroke_end_line_cap(&self) -> Option<PenLineCap> {
        // SAFETY: shape_raw() is live.
        PenLineCap::from_ordinal(unsafe { noesis_shape_get_stroke_end_line_cap(self.shape_raw()) })
    }

    /// Set the join drawn where stroke segments meet.
    fn set_stroke_line_join(&mut self, join: PenLineJoin) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_stroke_line_join(self.shape_raw(), join as i32) };
    }

    /// The line join.
    #[must_use]
    fn stroke_line_join(&self) -> Option<PenLineJoin> {
        // SAFETY: shape_raw() is live.
        PenLineJoin::from_ordinal(unsafe { noesis_shape_get_stroke_line_join(self.shape_raw()) })
    }

    /// Set how the shape stretches to fill its allocated space.
    fn set_stretch(&mut self, stretch: Stretch) {
        // SAFETY: shape_raw() is live.
        unsafe { noesis_shape_set_stretch(self.shape_raw(), stretch as i32) };
    }

    /// The stretch mode.
    #[must_use]
    fn stretch(&self) -> Option<Stretch> {
        // SAFETY: shape_raw() is live.
        Stretch::from_ordinal(unsafe { noesis_shape_get_stretch(self.shape_raw()) })
    }

    /// Set the dash pattern as a space-separated list of alternating dash and
    /// gap lengths, in multiples of the stroke thickness (for example
    /// `"2 1"`).
    ///
    /// # Panics
    ///
    /// Panics if `dashes` contains an interior NUL byte.
    fn set_stroke_dash_array(&mut self, dashes: &str) {
        let c = CString::new(dashes).expect("dash array contained NUL");
        // SAFETY: shape_raw() is live; `c` outlives the call and the C side
        // copies it into the Noesis object.
        unsafe { noesis_shape_set_stroke_dash_array(self.shape_raw(), c.as_ptr()) };
    }

    /// The dash pattern string; empty if unset.
    #[must_use]
    fn stroke_dash_array(&self) -> String {
        // SAFETY: shape_raw() is live; the returned pointer is owned by the
        // Noesis object and valid until the next mutation, so we copy immediately.
        let p = unsafe { noesis_shape_get_stroke_dash_array(self.shape_raw()) };
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
            /// Panics if Noesis returns null, which is not expected after
            /// [`crate::init`].
            #[must_use]
            pub fn new() -> Self {
                // SAFETY: a plain component-create call; returns a +1 ref we own.
                let ptr = unsafe { $create() };
                Self {
                    ptr: NonNull::new(ptr).expect(concat!(stringify!($create), " returned null")),
                }
            }

            /// Raw `Noesis::Shape*` (also a `FrameworkElement*` and
            /// `BaseComponent*`), borrowed for the lifetime of `self`.
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
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

shape_handle!(
    Rectangle,
    noesis_rectangle_create,
    "A rectangle, optionally with rounded corners. Size it with\n\
     [`Shape::set_width`] and [`Shape::set_height`]."
);
shape_handle!(
    Ellipse,
    noesis_ellipse_create,
    "An ellipse. Size it with\n\
     [`Shape::set_width`] and [`Shape::set_height`]."
);
shape_handle!(
    Line,
    noesis_line_create,
    "A straight line between two points; set them with\n\
     [`Line::set_points`]."
);

impl Rectangle {
    /// Set the corner radius along the x axis.
    pub fn set_radius_x(&mut self, value: f32) {
        // SAFETY: self.ptr is a live Rectangle*.
        unsafe { noesis_rectangle_set_radius_x(self.ptr.as_ptr(), value) };
    }

    /// The corner radius along the x axis.
    #[must_use]
    pub fn radius_x(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is live; `out` is valid.
        unsafe { noesis_rectangle_get_radius_x(self.ptr.as_ptr(), &mut out) };
        out
    }

    /// Set the corner radius along the y axis.
    pub fn set_radius_y(&mut self, value: f32) {
        // SAFETY: self.ptr is a live Rectangle*.
        unsafe { noesis_rectangle_set_radius_y(self.ptr.as_ptr(), value) };
    }

    /// The corner radius along the y axis.
    #[must_use]
    pub fn radius_y(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is live; `out` is valid.
        unsafe { noesis_rectangle_get_radius_y(self.ptr.as_ptr(), &mut out) };
        out
    }
}

impl Line {
    /// Set the start point `(x1, y1)` and end point `(x2, y2)`.
    pub fn set_points(&mut self, x1: f32, y1: f32, x2: f32, y2: f32) {
        // SAFETY: self.ptr is a live Line*.
        unsafe { noesis_line_set(self.ptr.as_ptr(), x1, y1, x2, y2) };
    }

    /// The endpoints as `[x1, y1, x2, y2]`.
    #[must_use]
    pub fn points(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is live; `out` is a 4-float buffer.
        unsafe { noesis_line_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}
