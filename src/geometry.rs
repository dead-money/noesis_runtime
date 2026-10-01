//! Build geometries from code: [`StreamGeometry`], [`PathGeometry`] (made of
//! [`PathFigure`]s and segments), [`EllipseGeometry`], [`RectangleGeometry`],
//! [`LineGeometry`], [`CombinedGeometry`] and [`GeometryGroup`]. All of them
//! implement the [`Geometry`] trait.
//!
//! Each type owns one Noesis reference and releases it on drop. Assigning a
//! geometry to a `Path`'s `Data`, or any other `Geometry`-typed property, makes
//! Noesis take its own reference, so you can drop the Rust handle right after:
//!
//! ```no_run
//! # use noesis_runtime::geometry::{EllipseGeometry, Geometry};
//! # use noesis_runtime::view::FrameworkElement;
//! # let mut path: FrameworkElement = unimplemented!();
//! let ellipse = EllipseGeometry::new(50.0, 50.0, 40.0, 30.0);
//! // SAFETY: `path` is a live Path element; the geometry pointer is borrowed.
//! unsafe { path.set_component("Data", ellipse.geometry_raw()) };
//! ```
//!
//! Getters read from the live Noesis object, so they reflect the geometry's
//! current state. A new, unpopulated geometry reports empty bounds.

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::ffi::{
    noesis_arc_segment_create, noesis_arc_segment_get, noesis_base_component_release,
    noesis_bezier_segment_create, noesis_bezier_segment_get, noesis_combined_geometry_create,
    noesis_combined_geometry_get_geometry1, noesis_combined_geometry_get_geometry2,
    noesis_combined_geometry_get_mode, noesis_combined_geometry_set_geometry1,
    noesis_combined_geometry_set_geometry2, noesis_combined_geometry_set_mode,
    noesis_ellipse_geometry_create, noesis_ellipse_geometry_get, noesis_geometry_get_bounds,
    noesis_geometry_get_render_bounds, noesis_geometry_get_transform,
    noesis_geometry_group_add_child, noesis_geometry_group_child_count,
    noesis_geometry_group_create, noesis_geometry_group_get_fill_rule,
    noesis_geometry_group_set_fill_rule, noesis_geometry_is_empty, noesis_geometry_set_transform,
    noesis_line_geometry_create, noesis_line_geometry_get, noesis_line_segment_create,
    noesis_line_segment_get_point, noesis_path_figure_add_segment, noesis_path_figure_create,
    noesis_path_figure_get_is_closed, noesis_path_figure_get_is_filled,
    noesis_path_figure_get_start_point, noesis_path_figure_segment_count,
    noesis_path_figure_set_is_closed, noesis_path_figure_set_is_filled,
    noesis_path_figure_set_start_point, noesis_path_geometry_add_figure,
    noesis_path_geometry_create, noesis_path_geometry_figure_count,
    noesis_path_geometry_get_fill_rule, noesis_path_geometry_set_fill_rule,
    noesis_poly_bezier_segment_create, noesis_poly_line_segment_create,
    noesis_poly_quadratic_bezier_segment_create, noesis_poly_segment_get_point,
    noesis_poly_segment_point_count, noesis_quadratic_bezier_segment_create,
    noesis_quadratic_bezier_segment_get, noesis_rectangle_geometry_create,
    noesis_rectangle_geometry_get, noesis_stream_geometry_context_arc_to,
    noesis_stream_geometry_context_begin_figure, noesis_stream_geometry_context_close,
    noesis_stream_geometry_context_cubic_to, noesis_stream_geometry_context_destroy,
    noesis_stream_geometry_context_line_to, noesis_stream_geometry_context_quadratic_to,
    noesis_stream_geometry_context_set_is_closed, noesis_stream_geometry_create,
    noesis_stream_geometry_create_from_data, noesis_stream_geometry_get_fill_rule,
    noesis_stream_geometry_open, noesis_stream_geometry_set_data,
    noesis_stream_geometry_set_fill_rule,
};
use crate::transforms::Transform;

/// An axis-aligned rectangle, as returned by [`Geometry::bounds`] and
/// [`Geometry::render_bounds`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width (`0` for an empty geometry).
    pub width: f32,
    /// Height (`0` for an empty geometry).
    pub height: f32,
}

impl Rect {
    fn from_array(a: [f32; 4]) -> Self {
        Self {
            x: a[0],
            y: a[1],
            width: a[2],
            height: a[3],
        }
    }
}

/// How overlapping areas inside a geometry decide what is filled.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FillRule {
    /// Odd-crossing rule (the XAML default).
    EvenOdd,
    /// Non-zero winding rule.
    Nonzero,
}

impl FillRule {
    fn to_ordinal(self) -> i32 {
        match self {
            FillRule::EvenOdd => 0,
            FillRule::Nonzero => 1,
        }
    }

    fn from_ordinal(v: i32) -> Self {
        match v {
            1 => FillRule::Nonzero,
            _ => FillRule::EvenOdd,
        }
    }
}

/// How the two operands of a [`CombinedGeometry`] are combined.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum GeometryCombineMode {
    /// `A ∪ B`.
    Union,
    /// `A ∩ B`.
    Intersect,
    /// `(A − B) ∪ (B − A)`.
    Xor,
    /// `A − B`.
    Exclude,
}

impl GeometryCombineMode {
    fn to_ordinal(self) -> i32 {
        match self {
            GeometryCombineMode::Union => 0,
            GeometryCombineMode::Intersect => 1,
            GeometryCombineMode::Xor => 2,
            GeometryCombineMode::Exclude => 3,
        }
    }

    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(GeometryCombineMode::Union),
            1 => Some(GeometryCombineMode::Intersect),
            2 => Some(GeometryCombineMode::Xor),
            3 => Some(GeometryCombineMode::Exclude),
            _ => None,
        }
    }
}

/// Direction an arc sweeps, for [`ArcSegment`] and
/// [`StreamGeometryContext::arc_to`].
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SweepDirection {
    /// Counter-clockwise (negative-angle) direction.
    Counterclockwise,
    /// Clockwise (positive-angle) direction.
    Clockwise,
}

impl SweepDirection {
    fn to_ordinal(self) -> i32 {
        match self {
            SweepDirection::Counterclockwise => 0,
            SweepDirection::Clockwise => 1,
        }
    }

    fn from_ordinal(v: i32) -> Self {
        match v {
            1 => SweepDirection::Clockwise,
            _ => SweepDirection::Counterclockwise,
        }
    }
}

/// Any Noesis `Geometry`. Implemented by every geometry type in this module,
/// and accepted by [`CombinedGeometry`], [`GeometryGroup`] and, as
/// `&dyn Geometry`, by [`DrawingContext`](crate::drawing::DrawingContext).
pub trait Geometry {
    /// Borrowed `Noesis::Geometry*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime. Pass it to a raw property setter such as
    /// [`FrameworkElement::set_component`](crate::view::FrameworkElement::set_component).
    fn geometry_raw(&self) -> *mut c_void;

    /// The bounds of the filled area.
    #[must_use]
    fn bounds(&self) -> Rect {
        let mut out = [0.0f32; 4];
        // SAFETY: geometry_raw() is a live Geometry*; `out` is 4 floats.
        unsafe { noesis_geometry_get_bounds(self.geometry_raw(), out.as_mut_ptr()) };
        Rect::from_array(out)
    }

    /// The render bounds with no pen, so stroke width is not included.
    #[must_use]
    fn render_bounds(&self) -> Rect {
        let mut out = [0.0f32; 4];
        // SAFETY: geometry_raw() is a live Geometry*; `out` is 4 floats.
        unsafe { noesis_geometry_get_render_bounds(self.geometry_raw(), out.as_mut_ptr()) };
        Rect::from_array(out)
    }

    /// Whether the geometry describes no area or path.
    #[must_use]
    fn is_empty(&self) -> bool {
        // SAFETY: geometry_raw() is a live Geometry*.
        unsafe { noesis_geometry_is_empty(self.geometry_raw()) == 1 }
    }

    /// Applies a [`Transform`] to the geometry. Noesis takes its own reference,
    /// so `transform` may be dropped afterwards. Returns `false` only if the
    /// handle is not a Noesis `Geometry`, which can't happen for the types in
    /// this module.
    fn set_transform(&mut self, transform: &dyn Transform) -> bool {
        // SAFETY: geometry_raw() is a live Geometry*; transform_raw() is a live
        // Transform* borrowed for the duration of the call.
        unsafe { noesis_geometry_set_transform(self.geometry_raw(), transform.transform_raw()) }
    }

    /// Borrowed `Noesis::Transform*` currently applied, or null. No reference
    /// is added; don't release it.
    #[must_use]
    fn transform_raw(&self) -> *mut c_void {
        // SAFETY: geometry_raw() is a live Geometry*.
        unsafe { noesis_geometry_get_transform(self.geometry_raw()) }
    }
}

/// Any Noesis `PathSegment`. Implemented by every segment type so
/// [`PathFigure::add_segment`] accepts any of them.
pub trait PathSegment {
    /// Borrowed `Noesis::PathSegment*`, valid for `self`'s lifetime.
    fn segment_raw(&self) -> *mut c_void;
}

macro_rules! base_component_handle {
    ($name:ident) => {
        // SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
        unsafe impl Send for $name {}

        impl $name {
            /// Raw `Noesis::BaseComponent*`. Borrowed for the lifetime of `self`.
            #[must_use]
            pub fn raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }

        impl Drop for $name {
            fn drop(&mut self) {
                // SAFETY: produced by a `*_create` entrypoint with a +1 ref that
                // we own; released exactly once here.
                unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

macro_rules! geometry_handle {
    ($name:ident) => {
        base_component_handle!($name);

        impl Geometry for $name {
            fn geometry_raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }
    };
}

macro_rules! segment_handle {
    ($name:ident) => {
        base_component_handle!($name);

        impl PathSegment for $name {
            fn segment_raw(&self) -> *mut c_void {
                self.ptr.as_ptr()
            }
        }
    };
}

/// A lightweight geometry described by drawing commands (via
/// [`StreamGeometry::open`]) or a path-markup string. Cheaper than
/// [`PathGeometry`], but its figures can't be read back.
pub struct StreamGeometry {
    ptr: NonNull<c_void>,
}

geometry_handle!(StreamGeometry);

impl Default for StreamGeometry {
    fn default() -> Self {
        Self::new()
    }
}

impl StreamGeometry {
    /// Create an empty stream geometry.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { noesis_stream_geometry_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_stream_geometry_create returned null"),
        }
    }

    /// Creates a stream geometry from a path-markup string such as
    /// `"M 0,0 L 10,0 10,10 Z"`.
    ///
    /// Unparseable data is not an error: Noesis logs a warning and the geometry
    /// is empty.
    ///
    /// # Panics
    ///
    /// Panics if `data` contains an interior NUL byte, or if allocating the
    /// geometry fails and Noesis returns null.
    #[must_use]
    pub fn from_data(data: &str) -> Self {
        let c = CString::new(data).expect("data contains NUL");
        // SAFETY: `c` outlives the call; the C side copies the string.
        let ptr = unsafe { noesis_stream_geometry_create_from_data(c.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_stream_geometry_create_from_data returned null"),
        }
    }

    /// Replaces the geometry with the figures in a path-markup string.
    ///
    /// # Panics
    ///
    /// Panics if `data` contains an interior NUL byte.
    pub fn set_data(&mut self, data: &str) {
        let c = CString::new(data).expect("data contains NUL");
        // SAFETY: self.ptr is a live StreamGeometry*; `c` outlives the call.
        unsafe { noesis_stream_geometry_set_data(self.ptr.as_ptr(), c.as_ptr()) };
    }

    /// Set the fill rule.
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        // SAFETY: self.ptr is a live StreamGeometry*.
        unsafe { noesis_stream_geometry_set_fill_rule(self.ptr.as_ptr(), rule.to_ordinal()) };
    }

    /// Read the fill rule back from the live object.
    #[must_use]
    pub fn fill_rule(&self) -> FillRule {
        // SAFETY: self.ptr is a live StreamGeometry*.
        FillRule::from_ordinal(unsafe { noesis_stream_geometry_get_fill_rule(self.ptr.as_ptr()) })
    }

    /// Opens a [`StreamGeometryContext`] for describing the geometry with
    /// drawing commands. Call [`StreamGeometryContext::close`] to write the
    /// figures into this geometry; dropping the context without closing leaves
    /// the geometry unchanged.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to open the context.
    #[must_use]
    pub fn open(&self) -> StreamGeometryContext {
        // SAFETY: self.ptr is a live StreamGeometry*. The returned context keeps
        // its own reference to the geometry alive.
        let ptr = unsafe { noesis_stream_geometry_open(self.ptr.as_ptr()) };
        StreamGeometryContext {
            ctx: NonNull::new(ptr).expect("noesis_stream_geometry_open returned null"),
        }
    }
}

/// Records figures for a [`StreamGeometry`]. Start each figure with
/// [`begin_figure`](Self::begin_figure), add the `*_to` commands, then call
/// [`close`](Self::close) to write everything into the geometry. Dropping
/// without closing discards the commands.
///
/// ```no_run
/// use noesis_runtime::geometry::StreamGeometry;
///
/// let geometry = StreamGeometry::new();
/// let ctx = geometry.open();
/// ctx.begin_figure(0.0, 0.0, true);
/// ctx.line_to(100.0, 0.0);
/// ctx.line_to(50.0, 80.0);
/// ctx.close();
/// ```
pub struct StreamGeometryContext {
    ctx: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for StreamGeometryContext {}

impl StreamGeometryContext {
    /// Starts a new figure at `(x, y)`. `is_closed` joins its last point back
    /// to the start.
    pub fn begin_figure(&self, x: f32, y: f32, is_closed: bool) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe { noesis_stream_geometry_context_begin_figure(self.ctx.as_ptr(), x, y, is_closed) };
    }

    /// Draw a straight line to `(x, y)`.
    pub fn line_to(&self, x: f32, y: f32) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe { noesis_stream_geometry_context_line_to(self.ctx.as_ptr(), x, y) };
    }

    /// Draw a cubic Bézier curve through control points `p1`, `p2` to `p3`.
    pub fn cubic_to(&self, p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe {
            noesis_stream_geometry_context_cubic_to(
                self.ctx.as_ptr(),
                p1.0,
                p1.1,
                p2.0,
                p2.1,
                p3.0,
                p3.1,
            )
        };
    }

    /// Draw a quadratic Bézier curve through control point `p1` to `p2`.
    pub fn quadratic_to(&self, p1: (f32, f32), p2: (f32, f32)) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe {
            noesis_stream_geometry_context_quadratic_to(self.ctx.as_ptr(), p1.0, p1.1, p2.0, p2.1)
        };
    }

    /// Draws an elliptical arc to `(x, y)` with radii `(width, height)`, the
    /// ellipse rotated by `rotation_deg` degrees. `is_large_arc` picks the arc
    /// longer than 180 degrees.
    pub fn arc_to(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rotation_deg: f32,
        is_large_arc: bool,
        sweep: SweepDirection,
    ) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe {
            noesis_stream_geometry_context_arc_to(
                self.ctx.as_ptr(),
                x,
                y,
                width,
                height,
                rotation_deg,
                is_large_arc,
                sweep.to_ordinal(),
            )
        };
    }

    /// Overrides the `is_closed` flag of the current figure.
    pub fn set_is_closed(&self, is_closed: bool) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe { noesis_stream_geometry_context_set_is_closed(self.ctx.as_ptr(), is_closed) };
    }

    /// Writes the recorded figures into the geometry.
    pub fn close(self) {
        // SAFETY: self.ctx is a live StreamGeometryContext*; freed by close().
        unsafe { noesis_stream_geometry_context_close(self.ctx.as_ptr()) };
        // close() already freed the context.
        core::mem::forget(self);
    }
}

impl Drop for StreamGeometryContext {
    fn drop(&mut self) {
        // SAFETY: only reached when not closed; close() forgets self.
        unsafe { noesis_stream_geometry_context_destroy(self.ctx.as_ptr()) };
    }
}

/// A geometry made of [`PathFigure`]s, each a start point plus segments.
/// Unlike [`StreamGeometry`], its figures and segments stay inspectable.
pub struct PathGeometry {
    ptr: NonNull<c_void>,
}

geometry_handle!(PathGeometry);

impl Default for PathGeometry {
    fn default() -> Self {
        Self::new()
    }
}

impl PathGeometry {
    /// Create an empty path geometry (with an empty figure collection).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { noesis_path_geometry_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_path_geometry_create returned null"),
        }
    }

    /// Appends a figure and returns its index. The geometry takes its own
    /// reference, so `figure` may be dropped afterwards.
    pub fn add_figure(&mut self, figure: &PathFigure) -> i32 {
        // SAFETY: self.ptr is a live PathGeometry*; figure.raw() is a live
        // PathFigure* borrowed for the call.
        unsafe { noesis_path_geometry_add_figure(self.ptr.as_ptr(), figure.raw()) }
    }

    /// Number of figures in the geometry.
    #[must_use]
    pub fn figure_count(&self) -> usize {
        // SAFETY: self.ptr is a live PathGeometry*.
        unsafe { noesis_path_geometry_figure_count(self.ptr.as_ptr()) }.max(0) as usize
    }

    /// Set the fill rule.
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        // SAFETY: self.ptr is a live PathGeometry*.
        unsafe { noesis_path_geometry_set_fill_rule(self.ptr.as_ptr(), rule.to_ordinal()) };
    }

    /// Read the fill rule back from the live object.
    #[must_use]
    pub fn fill_rule(&self) -> FillRule {
        // SAFETY: self.ptr is a live PathGeometry*.
        FillRule::from_ordinal(unsafe { noesis_path_geometry_get_fill_rule(self.ptr.as_ptr()) })
    }
}

/// A start point followed by connected [`PathSegment`]s. Add it to a
/// [`PathGeometry`] with [`PathGeometry::add_figure`].
pub struct PathFigure {
    ptr: NonNull<c_void>,
}

base_component_handle!(PathFigure);

impl Default for PathFigure {
    fn default() -> Self {
        Self::new()
    }
}

impl PathFigure {
    /// Create an empty figure (with an empty segment collection).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the figure.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { noesis_path_figure_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_path_figure_create returned null"),
        }
    }

    /// Set the figure's start point.
    pub fn set_start_point(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { noesis_path_figure_set_start_point(self.ptr.as_ptr(), x, y) };
    }

    /// Read the start point `(x, y)` back from the live object.
    #[must_use]
    pub fn start_point(&self) -> (f32, f32) {
        let mut out = [0.0f32; 2];
        // SAFETY: self.ptr is a live PathFigure*; `out` is 2 floats.
        unsafe { noesis_path_figure_get_start_point(self.ptr.as_ptr(), out.as_mut_ptr()) };
        (out[0], out[1])
    }

    /// Set whether the figure is closed (first and last segments joined).
    pub fn set_is_closed(&mut self, is_closed: bool) {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { noesis_path_figure_set_is_closed(self.ptr.as_ptr(), is_closed) };
    }

    /// Set whether the figure's contained area is filled.
    pub fn set_is_filled(&mut self, is_filled: bool) {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { noesis_path_figure_set_is_filled(self.ptr.as_ptr(), is_filled) };
    }

    /// Read whether the figure is closed, from the live object.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { noesis_path_figure_get_is_closed(self.ptr.as_ptr()) == 1 }
    }

    /// Read whether the figure is filled, from the live object.
    #[must_use]
    pub fn is_filled(&self) -> bool {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { noesis_path_figure_get_is_filled(self.ptr.as_ptr()) == 1 }
    }

    /// Appends a segment and returns its index. The figure takes its own
    /// reference, so `segment` may be dropped afterwards.
    pub fn add_segment<S: PathSegment>(&mut self, segment: &S) -> i32 {
        // SAFETY: self.ptr is a live PathFigure*; segment_raw() is a live
        // PathSegment* borrowed for the call.
        unsafe { noesis_path_figure_add_segment(self.ptr.as_ptr(), segment.segment_raw()) }
    }

    /// Number of segments in the figure.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { noesis_path_figure_segment_count(self.ptr.as_ptr()) }.max(0) as usize
    }
}

/// A straight line from the previous point to an end point.
pub struct LineSegment {
    ptr: NonNull<c_void>,
}

segment_handle!(LineSegment);

impl LineSegment {
    /// Create a line segment ending at `(x, y)`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the segment.
    #[must_use]
    pub fn new(x: f32, y: f32) -> Self {
        let ptr = unsafe { noesis_line_segment_create(x, y) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_line_segment_create returned null"),
        }
    }

    /// Read the end point `(x, y)` back from the live object.
    #[must_use]
    pub fn point(&self) -> (f32, f32) {
        let mut out = [0.0f32; 2];
        // SAFETY: self.ptr is a live LineSegment*; `out` is 2 floats.
        unsafe { noesis_line_segment_get_point(self.ptr.as_ptr(), out.as_mut_ptr()) };
        (out[0], out[1])
    }
}

/// A cubic Bézier curve with two control points and an end point.
pub struct BezierSegment {
    ptr: NonNull<c_void>,
}

segment_handle!(BezierSegment);

impl BezierSegment {
    /// Create a cubic Bézier through control points `p1`, `p2` to `p3`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the segment.
    #[must_use]
    pub fn new(p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) -> Self {
        let ptr = unsafe { noesis_bezier_segment_create(p1.0, p1.1, p2.0, p2.1, p3.0, p3.1) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_bezier_segment_create returned null"),
        }
    }

    /// Read `[p1, p2, p3]` back from the live object.
    #[must_use]
    pub fn points(&self) -> [(f32, f32); 3] {
        let mut out = [0.0f32; 6];
        // SAFETY: self.ptr is a live BezierSegment*; `out` is 6 floats.
        unsafe { noesis_bezier_segment_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        [(out[0], out[1]), (out[2], out[3]), (out[4], out[5])]
    }
}

/// A quadratic Bézier curve with one control point and an end point.
pub struct QuadraticBezierSegment {
    ptr: NonNull<c_void>,
}

segment_handle!(QuadraticBezierSegment);

impl QuadraticBezierSegment {
    /// Create a quadratic Bézier through control point `p1` to `p2`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the segment.
    #[must_use]
    pub fn new(p1: (f32, f32), p2: (f32, f32)) -> Self {
        let ptr = unsafe { noesis_quadratic_bezier_segment_create(p1.0, p1.1, p2.0, p2.1) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_quadratic_bezier_segment_create returned null"),
        }
    }

    /// Read `[p1, p2]` back from the live object.
    #[must_use]
    pub fn points(&self) -> [(f32, f32); 2] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live QuadraticBezierSegment*; `out` is 4 floats.
        unsafe { noesis_quadratic_bezier_segment_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        [(out[0], out[1]), (out[2], out[3])]
    }
}

/// An elliptical arc from the previous point to an end point.
pub struct ArcSegment {
    ptr: NonNull<c_void>,
}

segment_handle!(ArcSegment);

/// The parameters of an [`ArcSegment`], for [`ArcSegment::from_fields`] and
/// [`ArcSegment::get`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ArcFields {
    /// End point `(x, y)`.
    pub point: (f32, f32),
    /// Radii `(width, height)`.
    pub size: (f32, f32),
    /// Rotation of the ellipse relative to the x-axis, in degrees.
    pub rotation_deg: f32,
    /// Whether to take the arc longer than 180 degrees.
    pub is_large_arc: bool,
    pub sweep: SweepDirection,
}

impl ArcSegment {
    /// Creates an elliptical arc to `(x, y)` with radii `(width, height)`. The
    /// other arguments match the [`ArcFields`] fields.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the segment.
    #[must_use]
    pub fn new(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rotation_deg: f32,
        is_large_arc: bool,
        sweep: SweepDirection,
    ) -> Self {
        let ptr = unsafe {
            noesis_arc_segment_create(
                x,
                y,
                width,
                height,
                rotation_deg,
                is_large_arc,
                sweep.to_ordinal(),
            )
        };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_arc_segment_create returned null"),
        }
    }

    /// Creates an elliptical arc from named fields. Same as [`new`](Self::new).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the segment.
    #[must_use]
    pub fn from_fields(fields: ArcFields) -> Self {
        Self::new(
            fields.point.0,
            fields.point.1,
            fields.size.0,
            fields.size.1,
            fields.rotation_deg,
            fields.is_large_arc,
            fields.sweep,
        )
    }

    /// Read all arc fields back from the live object.
    #[must_use]
    pub fn get(&self) -> ArcFields {
        let mut point = [0.0f32; 2];
        let mut size = [0.0f32; 2];
        let mut rotation_deg = 0.0f32;
        let mut is_large_arc = false;
        let mut sweep = 0i32;
        // SAFETY: self.ptr is a live ArcSegment*; all out params are valid.
        unsafe {
            noesis_arc_segment_get(
                self.ptr.as_ptr(),
                point.as_mut_ptr(),
                size.as_mut_ptr(),
                &mut rotation_deg,
                &mut is_large_arc,
                &mut sweep,
            )
        };
        ArcFields {
            point: (point[0], point[1]),
            size: (size[0], size[1]),
            rotation_deg,
            is_large_arc,
            sweep: SweepDirection::from_ordinal(sweep),
        }
    }
}

macro_rules! poly_segment {
    ($name:ident, $create:ident, $doc:literal) => {
        #[doc = $doc]
        pub struct $name {
            ptr: NonNull<c_void>,
        }

        segment_handle!($name);

        impl $name {
            /// Create the segment from a slice of `(x, y)` points.
            ///
            /// # Panics
            ///
            /// Panics if Noesis fails to allocate the segment.
            #[must_use]
            pub fn new(points: &[(f32, f32)]) -> Self {
                let flat: Vec<f32> = points.iter().flat_map(|p| [p.0, p.1]).collect();
                // SAFETY: `flat` outlives the call; the C side copies the points.
                let ptr = unsafe { $create(flat.as_ptr(), points.len() as u32) };
                Self {
                    ptr: NonNull::new(ptr).expect(concat!(stringify!($create), " returned null")),
                }
            }

            /// Number of points read back from the live object.
            #[must_use]
            pub fn point_count(&self) -> usize {
                // SAFETY: self.ptr is a live poly segment*.
                unsafe { noesis_poly_segment_point_count(self.ptr.as_ptr()) }.max(0) as usize
            }

            /// The point at `index`, or `None` if out of range.
            #[must_use]
            pub fn point(&self, index: usize) -> Option<(f32, f32)> {
                let mut out = [0.0f32; 2];
                // SAFETY: self.ptr is a live poly segment*; `out` is 2 floats.
                let ok = unsafe {
                    noesis_poly_segment_get_point(self.ptr.as_ptr(), index as u32, out.as_mut_ptr())
                };
                ok.then_some((out[0], out[1]))
            }
        }
    };
}

poly_segment!(
    PolyLineSegment,
    noesis_poly_line_segment_create,
    "A run of straight lines through a list of points."
);
poly_segment!(
    PolyBezierSegment,
    noesis_poly_bezier_segment_create,
    "A run of cubic Bézier curves. Points come in groups of three: two control points, then an end point."
);
poly_segment!(
    PolyQuadraticBezierSegment,
    noesis_poly_quadratic_bezier_segment_create,
    "A run of quadratic Bézier curves. Points come in pairs: a control point, then an end point."
);

/// An ellipse defined by a center and radii.
pub struct EllipseGeometry {
    ptr: NonNull<c_void>,
}

geometry_handle!(EllipseGeometry);

impl EllipseGeometry {
    /// Create an ellipse centered at `(cx, cy)` with radii `(rx, ry)`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn new(cx: f32, cy: f32, rx: f32, ry: f32) -> Self {
        let ptr = unsafe { noesis_ellipse_geometry_create(cx, cy, rx, ry) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_ellipse_geometry_create returned null"),
        }
    }

    /// Read `[centerX, centerY, radiusX, radiusY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live EllipseGeometry*; `out` is 4 floats.
        unsafe { noesis_ellipse_geometry_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

/// A rectangle, optionally with rounded corners.
pub struct RectangleGeometry {
    ptr: NonNull<c_void>,
}

geometry_handle!(RectangleGeometry);

impl RectangleGeometry {
    /// Creates a rectangle `(x, y, width, height)` with corner radii
    /// `(rx, ry)`. Pass zero radii for square corners.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32, rx: f32, ry: f32) -> Self {
        let ptr = unsafe { noesis_rectangle_geometry_create(x, y, width, height, rx, ry) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_rectangle_geometry_create returned null"),
        }
    }

    /// Creates a rectangle from a [`Rect`] and corner radii `(rx, ry)`. Same
    /// as [`new`](Self::new).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn from_rect(rect: Rect, radii: (f32, f32)) -> Self {
        Self::new(rect.x, rect.y, rect.width, rect.height, radii.0, radii.1)
    }

    /// Read the rectangle `[x, y, width, height]` back from the live object.
    #[must_use]
    pub fn rect(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live RectangleGeometry*; `out` is 4 floats.
        unsafe {
            noesis_rectangle_geometry_get(
                self.ptr.as_ptr(),
                out.as_mut_ptr(),
                core::ptr::null_mut(),
            )
        };
        out
    }

    /// Read the corner radii `(rx, ry)` back from the live object.
    #[must_use]
    pub fn radii(&self) -> (f32, f32) {
        let mut out = [0.0f32; 2];
        // SAFETY: self.ptr is a live RectangleGeometry*; `out` is 2 floats.
        unsafe {
            noesis_rectangle_geometry_get(
                self.ptr.as_ptr(),
                core::ptr::null_mut(),
                out.as_mut_ptr(),
            )
        };
        (out[0], out[1])
    }
}

/// A straight line between two points.
pub struct LineGeometry {
    ptr: NonNull<c_void>,
}

geometry_handle!(LineGeometry);

impl LineGeometry {
    /// Create a line from `(x1, y1)` to `(x2, y2)`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn new(x1: f32, y1: f32, x2: f32, y2: f32) -> Self {
        let ptr = unsafe { noesis_line_geometry_create(x1, y1, x2, y2) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_line_geometry_create returned null"),
        }
    }

    /// Read `[startX, startY, endX, endY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live LineGeometry*; `out` is 4 floats.
        unsafe { noesis_line_geometry_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

/// Two geometries combined by a [`GeometryCombineMode`] (union,
/// intersection, xor or exclusion).
pub struct CombinedGeometry {
    ptr: NonNull<c_void>,
}

geometry_handle!(CombinedGeometry);

impl CombinedGeometry {
    /// Combine `geometry1` and `geometry2` with `mode`. Noesis takes its own
    /// references to the operands, so they may be dropped afterwards.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn new<A: Geometry, B: Geometry>(
        mode: GeometryCombineMode,
        geometry1: &A,
        geometry2: &B,
    ) -> Self {
        // SAFETY: the operand pointers are live Geometry*; Noesis AddRefs them.
        let ptr = unsafe {
            noesis_combined_geometry_create(
                mode.to_ordinal(),
                geometry1.geometry_raw(),
                geometry2.geometry_raw(),
            )
        };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_combined_geometry_create returned null"),
        }
    }

    /// Replace the first operand.
    pub fn set_geometry1<A: Geometry>(&mut self, geometry: &A) {
        // SAFETY: self.ptr is live; geometry_raw() is a live Geometry*.
        unsafe {
            noesis_combined_geometry_set_geometry1(self.ptr.as_ptr(), geometry.geometry_raw())
        };
    }

    /// Replace the second operand.
    pub fn set_geometry2<B: Geometry>(&mut self, geometry: &B) {
        // SAFETY: self.ptr is live; geometry_raw() is a live Geometry*.
        unsafe {
            noesis_combined_geometry_set_geometry2(self.ptr.as_ptr(), geometry.geometry_raw())
        };
    }

    /// Borrowed `Noesis::Geometry*` of the first operand, or null. No
    /// reference is added.
    #[must_use]
    pub fn geometry1_raw(&self) -> *mut c_void {
        // SAFETY: self.ptr is a live CombinedGeometry*.
        unsafe { noesis_combined_geometry_get_geometry1(self.ptr.as_ptr()) }
    }

    /// Borrowed `Noesis::Geometry*` of the second operand, or null. No
    /// reference is added.
    #[must_use]
    pub fn geometry2_raw(&self) -> *mut c_void {
        // SAFETY: self.ptr is a live CombinedGeometry*.
        unsafe { noesis_combined_geometry_get_geometry2(self.ptr.as_ptr()) }
    }

    /// Set the combine mode.
    pub fn set_mode(&mut self, mode: GeometryCombineMode) {
        // SAFETY: self.ptr is a live CombinedGeometry*.
        unsafe { noesis_combined_geometry_set_mode(self.ptr.as_ptr(), mode.to_ordinal()) };
    }

    /// The combine mode, or `None` if Noesis reports one this crate doesn't
    /// know.
    #[must_use]
    pub fn mode(&self) -> Option<GeometryCombineMode> {
        // SAFETY: self.ptr is a live CombinedGeometry*.
        GeometryCombineMode::from_ordinal(unsafe {
            noesis_combined_geometry_get_mode(self.ptr.as_ptr())
        })
    }
}

/// Several child geometries drawn as one, with overlaps resolved by a
/// [`FillRule`].
pub struct GeometryGroup {
    ptr: NonNull<c_void>,
}

geometry_handle!(GeometryGroup);

impl Default for GeometryGroup {
    fn default() -> Self {
        Self::new()
    }
}

impl GeometryGroup {
    /// Create an empty geometry group (with an empty child collection).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the group.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { noesis_geometry_group_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_geometry_group_create returned null"),
        }
    }

    /// Appends a child geometry and returns its index. The group takes its own
    /// reference, so `child` may be dropped afterwards.
    pub fn add_child<G: Geometry>(&mut self, child: &G) -> i32 {
        // SAFETY: self.ptr is a live GeometryGroup*; geometry_raw() is a live
        // Geometry* borrowed for the call.
        unsafe { noesis_geometry_group_add_child(self.ptr.as_ptr(), child.geometry_raw()) }
    }

    /// Number of child geometries in the group.
    #[must_use]
    pub fn child_count(&self) -> usize {
        // SAFETY: self.ptr is a live GeometryGroup*.
        unsafe { noesis_geometry_group_child_count(self.ptr.as_ptr()) }.max(0) as usize
    }

    /// Set the fill rule.
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        // SAFETY: self.ptr is a live GeometryGroup*.
        unsafe { noesis_geometry_group_set_fill_rule(self.ptr.as_ptr(), rule.to_ordinal()) };
    }

    /// Read the fill rule back from the live object.
    #[must_use]
    pub fn fill_rule(&self) -> FillRule {
        // SAFETY: self.ptr is a live GeometryGroup*.
        FillRule::from_ordinal(unsafe { noesis_geometry_group_get_fill_rule(self.ptr.as_ptr()) })
    }
}
