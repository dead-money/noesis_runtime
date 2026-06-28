//! Code-built geometry object model (TODO §10): construct `Geometry` objects —
//! `StreamGeometry`, `PathGeometry` (figures + segments), the primitive
//! `Ellipse`/`Rectangle`/`Line` geometries, `CombinedGeometry`, and
//! `GeometryGroup` — from Rust without authoring XAML.
//!
//! Every type here is an owning handle over a freshly-created Noesis object
//! holding a single `+1` reference, released on [`Drop`] — the same pattern as
//! [`crate::brushes`] / [`crate::transforms`]. Assigning a finished geometry to
//! a `Path`'s `Data` (or any `Geometry`-typed property) makes Noesis take its
//! own reference, so the Rust handle may be dropped right after assignment. Use
//! the generic component DP path, e.g.:
//!
//! ```no_run
//! # use dm_noesis_runtime::geometry::{EllipseGeometry, Geometry};
//! # use dm_noesis_runtime::view::FrameworkElement;
//! # let mut path: FrameworkElement = unimplemented!();
//! let ellipse = EllipseGeometry::new(50.0, 50.0, 40.0, 30.0);
//! // SAFETY: `path` is a live Path element; the geometry pointer is borrowed.
//! unsafe { path.set_component("Data", ellipse.geometry_raw()) };
//! ```
//!
//! Read-back getters re-read from the live Noesis object — [`Geometry::bounds`]
//! / [`Geometry::render_bounds`] prove a real path was built (a no-op
//! constructor yields empty bounds), figure / segment / child counts prove the
//! collection wiring crossed the FFI, and the `FillRule` / `GeometryCombineMode`
//! accessors prove the enum round-trips. A stubbed implementation fails the
//! tests in `tests/geometry.rs`.

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::ffi::{
    dm_noesis_arc_segment_create, dm_noesis_arc_segment_get, dm_noesis_base_component_release,
    dm_noesis_bezier_segment_create, dm_noesis_bezier_segment_get,
    dm_noesis_combined_geometry_create, dm_noesis_combined_geometry_get_geometry1,
    dm_noesis_combined_geometry_get_geometry2, dm_noesis_combined_geometry_get_mode,
    dm_noesis_combined_geometry_set_geometry1, dm_noesis_combined_geometry_set_geometry2,
    dm_noesis_combined_geometry_set_mode, dm_noesis_ellipse_geometry_create,
    dm_noesis_ellipse_geometry_get, dm_noesis_geometry_get_bounds,
    dm_noesis_geometry_get_render_bounds, dm_noesis_geometry_get_transform,
    dm_noesis_geometry_group_add_child, dm_noesis_geometry_group_child_count,
    dm_noesis_geometry_group_create, dm_noesis_geometry_group_get_fill_rule,
    dm_noesis_geometry_group_set_fill_rule, dm_noesis_geometry_is_empty,
    dm_noesis_geometry_set_transform, dm_noesis_line_geometry_create, dm_noesis_line_geometry_get,
    dm_noesis_line_segment_create, dm_noesis_line_segment_get_point,
    dm_noesis_path_figure_add_segment, dm_noesis_path_figure_create,
    dm_noesis_path_figure_get_is_closed, dm_noesis_path_figure_get_is_filled,
    dm_noesis_path_figure_get_start_point, dm_noesis_path_figure_segment_count,
    dm_noesis_path_figure_set_is_closed, dm_noesis_path_figure_set_is_filled,
    dm_noesis_path_figure_set_start_point, dm_noesis_path_geometry_add_figure,
    dm_noesis_path_geometry_create, dm_noesis_path_geometry_figure_count,
    dm_noesis_path_geometry_get_fill_rule, dm_noesis_path_geometry_set_fill_rule,
    dm_noesis_poly_bezier_segment_create, dm_noesis_poly_line_segment_create,
    dm_noesis_poly_quadratic_bezier_segment_create, dm_noesis_poly_segment_get_point,
    dm_noesis_poly_segment_point_count, dm_noesis_quadratic_bezier_segment_create,
    dm_noesis_quadratic_bezier_segment_get, dm_noesis_rectangle_geometry_create,
    dm_noesis_rectangle_geometry_get, dm_noesis_stream_geometry_context_arc_to,
    dm_noesis_stream_geometry_context_begin_figure, dm_noesis_stream_geometry_context_close,
    dm_noesis_stream_geometry_context_cubic_to, dm_noesis_stream_geometry_context_destroy,
    dm_noesis_stream_geometry_context_line_to, dm_noesis_stream_geometry_context_quadratic_to,
    dm_noesis_stream_geometry_context_set_is_closed, dm_noesis_stream_geometry_create,
    dm_noesis_stream_geometry_create_from_data, dm_noesis_stream_geometry_get_fill_rule,
    dm_noesis_stream_geometry_open, dm_noesis_stream_geometry_set_data,
    dm_noesis_stream_geometry_set_fill_rule,
};
use crate::transforms::Transform;

/// An axis-aligned rectangle (`{x, y, width, height}`), as returned by
/// [`Geometry::bounds`] / [`Geometry::render_bounds`].
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

/// How the intersecting areas inside a geometry are combined (Noesis `FillRule`).
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

/// How the two operands of a [`CombinedGeometry`] are combined (Noesis
/// `GeometryCombineMode`).
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

/// Direction an [`ArcSegment`] / `ArcTo` sweeps (Noesis `SweepDirection`).
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

/// A handle to a Noesis `Geometry`. Implemented by every geometry type in this
/// module so generic code (and the `Data` assignment sugar) can accept any of
/// them while keeping non-geometry objects out. The base getters re-read from
/// the live Noesis object, proving a real path was constructed.
pub trait Geometry {
    /// Borrowed `Noesis::Geometry*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime. Used by assignment / combination; not normally called directly.
    fn geometry_raw(&self) -> *mut c_void;

    /// The geometry's fill bounds (`GetBounds`), read from the live object.
    #[must_use]
    fn bounds(&self) -> Rect {
        let mut out = [0.0f32; 4];
        // SAFETY: geometry_raw() is a live Geometry*; `out` is 4 floats.
        unsafe { dm_noesis_geometry_get_bounds(self.geometry_raw(), out.as_mut_ptr()) };
        Rect::from_array(out)
    }

    /// The geometry's render bounds with a null `Pen` (`GetRenderBounds`).
    #[must_use]
    fn render_bounds(&self) -> Rect {
        let mut out = [0.0f32; 4];
        // SAFETY: geometry_raw() is a live Geometry*; `out` is 4 floats.
        unsafe { dm_noesis_geometry_get_render_bounds(self.geometry_raw(), out.as_mut_ptr()) };
        Rect::from_array(out)
    }

    /// Whether the geometry is empty (`IsEmpty`).
    #[must_use]
    fn is_empty(&self) -> bool {
        // SAFETY: geometry_raw() is a live Geometry*.
        unsafe { dm_noesis_geometry_is_empty(self.geometry_raw()) == 1 }
    }

    /// Apply a `Transform` to the geometry. Noesis takes its own reference, so
    /// `transform` may be dropped afterwards. Returns `false` only on an
    /// internal type mismatch.
    ///
    /// Takes `&dyn Transform` (rather than a generic) so this trait stays
    /// object-safe — `&dyn Geometry` is the shape argument of
    /// [`DrawingContext::draw_geometry`](crate::drawing::DrawingContext::draw_geometry)
    /// / [`push_clip`](crate::drawing::DrawingContext::push_clip).
    fn set_transform(&mut self, transform: &dyn Transform) -> bool {
        // SAFETY: geometry_raw() is a live Geometry*; transform_raw() is a live
        // Transform* borrowed for the duration of the call.
        unsafe { dm_noesis_geometry_set_transform(self.geometry_raw(), transform.transform_raw()) }
    }

    /// Borrowed `Noesis::Transform*` currently applied to the geometry (no `+1`),
    /// or null.
    #[must_use]
    fn transform_raw(&self) -> *mut c_void {
        // SAFETY: geometry_raw() is a live Geometry*.
        unsafe { dm_noesis_geometry_get_transform(self.geometry_raw()) }
    }
}

/// A handle to a Noesis `PathSegment`. Implemented by every segment type so
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
                unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
            }
        }
    };
}

/// Implements [`Geometry`] (and the boilerplate) for an owning geometry handle.
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

/// Implements [`PathSegment`] (and the boilerplate) for an owning segment handle.
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

// ── StreamGeometry ───────────────────────────────────────────────────────────

/// A `StreamGeometry`: a lightweight geometry described by drawing commands
/// (via [`StreamGeometry::open`]) or an SVG path-data string.
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
        let ptr = unsafe { dm_noesis_stream_geometry_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_stream_geometry_create returned null"),
        }
    }

    /// Create a stream geometry from an SVG path-data string
    /// (e.g. `"M 0,0 L 10,0 10,10 Z"`).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry or `data` contains an
    /// interior NUL byte.
    #[must_use]
    pub fn from_data(data: &str) -> Self {
        let c = CString::new(data).expect("data contains NUL");
        // SAFETY: `c` outlives the call; the C side copies the string.
        let ptr = unsafe { dm_noesis_stream_geometry_create_from_data(c.as_ptr()) };
        Self {
            ptr: NonNull::new(ptr)
                .expect("dm_noesis_stream_geometry_create_from_data returned null"),
        }
    }

    /// Rebuild the geometry from an SVG path-data string.
    ///
    /// # Panics
    ///
    /// Panics if `data` contains an interior NUL byte.
    pub fn set_data(&mut self, data: &str) {
        let c = CString::new(data).expect("data contains NUL");
        // SAFETY: self.ptr is a live StreamGeometry*; `c` outlives the call.
        unsafe { dm_noesis_stream_geometry_set_data(self.ptr.as_ptr(), c.as_ptr()) };
    }

    /// Set the fill rule.
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        // SAFETY: self.ptr is a live StreamGeometry*.
        unsafe { dm_noesis_stream_geometry_set_fill_rule(self.ptr.as_ptr(), rule.to_ordinal()) };
    }

    /// Read the fill rule back from the live object.
    #[must_use]
    pub fn fill_rule(&self) -> FillRule {
        // SAFETY: self.ptr is a live StreamGeometry*.
        FillRule::from_ordinal(unsafe {
            dm_noesis_stream_geometry_get_fill_rule(self.ptr.as_ptr())
        })
    }

    /// Open a [`StreamGeometryContext`] for defining the geometry with drawing
    /// commands. Call [`StreamGeometryContext::close`] to flush the figures into
    /// this geometry; dropping the context without closing leaves the geometry
    /// unaltered.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to open the context.
    #[must_use]
    pub fn open(&self) -> StreamGeometryContext {
        // SAFETY: self.ptr is a live StreamGeometry*. The returned context keeps
        // its own reference to the geometry alive.
        let ptr = unsafe { dm_noesis_stream_geometry_open(self.ptr.as_ptr()) };
        StreamGeometryContext {
            ctx: NonNull::new(ptr).expect("dm_noesis_stream_geometry_open returned null"),
        }
    }
}

/// A drawing context for a [`StreamGeometry`]. Build figures with
/// [`begin_figure`](Self::begin_figure) + the `*_to` commands, then
/// [`close`](Self::close) to flush them into the geometry. Dropping without
/// closing frees the context and leaves the geometry unaltered.
pub struct StreamGeometryContext {
    ctx: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for StreamGeometryContext {}

impl StreamGeometryContext {
    /// Start a new figure at `(x, y)`. `is_closed` joins the first and last
    /// segments.
    pub fn begin_figure(&self, x: f32, y: f32, is_closed: bool) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe {
            dm_noesis_stream_geometry_context_begin_figure(self.ctx.as_ptr(), x, y, is_closed)
        };
    }

    /// Draw a straight line to `(x, y)`.
    pub fn line_to(&self, x: f32, y: f32) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe { dm_noesis_stream_geometry_context_line_to(self.ctx.as_ptr(), x, y) };
    }

    /// Draw a cubic Bézier curve through control points `p1`, `p2` to `p3`.
    pub fn cubic_to(&self, p1: (f32, f32), p2: (f32, f32), p3: (f32, f32)) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe {
            dm_noesis_stream_geometry_context_cubic_to(
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
            dm_noesis_stream_geometry_context_quadratic_to(
                self.ctx.as_ptr(),
                p1.0,
                p1.1,
                p2.0,
                p2.1,
            )
        };
    }

    /// Draw an elliptical arc to `(x, y)` with radii `(width, height)`.
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
            dm_noesis_stream_geometry_context_arc_to(
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

    /// Override the `is_closed` flag of the current figure.
    pub fn set_is_closed(&self, is_closed: bool) {
        // SAFETY: self.ctx is a live StreamGeometryContext*.
        unsafe { dm_noesis_stream_geometry_context_set_is_closed(self.ctx.as_ptr(), is_closed) };
    }

    /// Flush the recorded figures into the geometry and free the context.
    pub fn close(self) {
        // SAFETY: self.ctx is a live StreamGeometryContext*; freed by close().
        unsafe { dm_noesis_stream_geometry_context_close(self.ctx.as_ptr()) };
        // Skip Drop so the context is not freed a second time.
        core::mem::forget(self);
    }
}

impl Drop for StreamGeometryContext {
    fn drop(&mut self) {
        // SAFETY: not closed — free the heap context without flushing. close()
        // forgets self, so this never double-frees.
        unsafe { dm_noesis_stream_geometry_context_destroy(self.ctx.as_ptr()) };
    }
}

// ── PathGeometry + PathFigure ────────────────────────────────────────────────

/// A `PathGeometry`: a collection of [`PathFigure`]s describing the path.
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
        let ptr = unsafe { dm_noesis_path_geometry_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_path_geometry_create returned null"),
        }
    }

    /// Append a figure. The geometry takes its own reference, so `figure` may be
    /// dropped afterwards. Returns the new figure index.
    pub fn add_figure(&mut self, figure: &PathFigure) -> i32 {
        // SAFETY: self.ptr is a live PathGeometry*; figure.raw() is a live
        // PathFigure* borrowed for the call.
        unsafe { dm_noesis_path_geometry_add_figure(self.ptr.as_ptr(), figure.raw()) }
    }

    /// Number of figures in the geometry.
    #[must_use]
    pub fn figure_count(&self) -> usize {
        // SAFETY: self.ptr is a live PathGeometry*.
        unsafe { dm_noesis_path_geometry_figure_count(self.ptr.as_ptr()) }.max(0) as usize
    }

    /// Set the fill rule.
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        // SAFETY: self.ptr is a live PathGeometry*.
        unsafe { dm_noesis_path_geometry_set_fill_rule(self.ptr.as_ptr(), rule.to_ordinal()) };
    }

    /// Read the fill rule back from the live object.
    #[must_use]
    pub fn fill_rule(&self) -> FillRule {
        // SAFETY: self.ptr is a live PathGeometry*.
        FillRule::from_ordinal(unsafe { dm_noesis_path_geometry_get_fill_rule(self.ptr.as_ptr()) })
    }
}

/// A `PathFigure`: a connected sequence of [`PathSegment`]s with a start point.
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
        let ptr = unsafe { dm_noesis_path_figure_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_path_figure_create returned null"),
        }
    }

    /// Set the figure's start point.
    pub fn set_start_point(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { dm_noesis_path_figure_set_start_point(self.ptr.as_ptr(), x, y) };
    }

    /// Read the start point `(x, y)` back from the live object.
    #[must_use]
    pub fn start_point(&self) -> (f32, f32) {
        let mut out = [0.0f32; 2];
        // SAFETY: self.ptr is a live PathFigure*; `out` is 2 floats.
        unsafe { dm_noesis_path_figure_get_start_point(self.ptr.as_ptr(), out.as_mut_ptr()) };
        (out[0], out[1])
    }

    /// Set whether the figure is closed (first and last segments joined).
    pub fn set_is_closed(&mut self, is_closed: bool) {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { dm_noesis_path_figure_set_is_closed(self.ptr.as_ptr(), is_closed) };
    }

    /// Set whether the figure's contained area is filled.
    pub fn set_is_filled(&mut self, is_filled: bool) {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { dm_noesis_path_figure_set_is_filled(self.ptr.as_ptr(), is_filled) };
    }

    /// Read whether the figure is closed, from the live object.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { dm_noesis_path_figure_get_is_closed(self.ptr.as_ptr()) == 1 }
    }

    /// Read whether the figure is filled, from the live object.
    #[must_use]
    pub fn is_filled(&self) -> bool {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { dm_noesis_path_figure_get_is_filled(self.ptr.as_ptr()) == 1 }
    }

    /// Append a segment. The figure takes its own reference, so `segment` may be
    /// dropped afterwards. Returns the new segment index.
    pub fn add_segment<S: PathSegment>(&mut self, segment: &S) -> i32 {
        // SAFETY: self.ptr is a live PathFigure*; segment_raw() is a live
        // PathSegment* borrowed for the call.
        unsafe { dm_noesis_path_figure_add_segment(self.ptr.as_ptr(), segment.segment_raw()) }
    }

    /// Number of segments in the figure.
    #[must_use]
    pub fn segment_count(&self) -> usize {
        // SAFETY: self.ptr is a live PathFigure*.
        unsafe { dm_noesis_path_figure_segment_count(self.ptr.as_ptr()) }.max(0) as usize
    }
}

// ── Path segments ────────────────────────────────────────────────────────────

/// A straight `LineSegment` to a point.
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
        let ptr = unsafe { dm_noesis_line_segment_create(x, y) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_line_segment_create returned null"),
        }
    }

    /// Read the end point `(x, y)` back from the live object.
    #[must_use]
    pub fn point(&self) -> (f32, f32) {
        let mut out = [0.0f32; 2];
        // SAFETY: self.ptr is a live LineSegment*; `out` is 2 floats.
        unsafe { dm_noesis_line_segment_get_point(self.ptr.as_ptr(), out.as_mut_ptr()) };
        (out[0], out[1])
    }
}

/// A cubic `BezierSegment` with two control points and an end point.
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
        let ptr = unsafe { dm_noesis_bezier_segment_create(p1.0, p1.1, p2.0, p2.1, p3.0, p3.1) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_bezier_segment_create returned null"),
        }
    }

    /// Read `[p1, p2, p3]` back from the live object.
    #[must_use]
    pub fn points(&self) -> [(f32, f32); 3] {
        let mut out = [0.0f32; 6];
        // SAFETY: self.ptr is a live BezierSegment*; `out` is 6 floats.
        unsafe { dm_noesis_bezier_segment_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        [(out[0], out[1]), (out[2], out[3]), (out[4], out[5])]
    }
}

/// A `QuadraticBezierSegment` with one control point and an end point.
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
        let ptr = unsafe { dm_noesis_quadratic_bezier_segment_create(p1.0, p1.1, p2.0, p2.1) };
        Self {
            ptr: NonNull::new(ptr)
                .expect("dm_noesis_quadratic_bezier_segment_create returned null"),
        }
    }

    /// Read `[p1, p2]` back from the live object.
    #[must_use]
    pub fn points(&self) -> [(f32, f32); 2] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live QuadraticBezierSegment*; `out` is 4 floats.
        unsafe { dm_noesis_quadratic_bezier_segment_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        [(out[0], out[1]), (out[2], out[3])]
    }
}

/// An elliptical `ArcSegment`.
pub struct ArcSegment {
    ptr: NonNull<c_void>,
}

segment_handle!(ArcSegment);

/// The read-back fields of an [`ArcSegment`].
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct ArcFields {
    /// End point `(x, y)`.
    pub point: (f32, f32),
    /// Radii `(width, height)`.
    pub size: (f32, f32),
    /// Rotation of the ellipse about the x-axis (degrees).
    pub rotation_deg: f32,
    /// Whether the arc spans more than 180°.
    pub is_large_arc: bool,
    /// Sweep direction.
    pub sweep: SweepDirection,
}

impl ArcSegment {
    /// Create an elliptical arc to `(x, y)` with radii `(width, height)`.
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
            dm_noesis_arc_segment_create(
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
            ptr: NonNull::new(ptr).expect("dm_noesis_arc_segment_create returned null"),
        }
    }

    /// Create an elliptical arc from an [`ArcFields`] struct (the ergonomic
    /// alternative to the 7-positional-argument [`new`](Self::new); round-trips
    /// with [`get`](Self::get)).
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
            dm_noesis_arc_segment_get(
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
                unsafe { dm_noesis_poly_segment_point_count(self.ptr.as_ptr()) }.max(0) as usize
            }

            /// Read the point at `index` back from the live object, or `None`.
            #[must_use]
            pub fn point(&self, index: usize) -> Option<(f32, f32)> {
                let mut out = [0.0f32; 2];
                // SAFETY: self.ptr is a live poly segment*; `out` is 2 floats.
                let ok = unsafe {
                    dm_noesis_poly_segment_get_point(
                        self.ptr.as_ptr(),
                        index as u32,
                        out.as_mut_ptr(),
                    )
                };
                ok.then_some((out[0], out[1]))
            }
        }
    };
}

poly_segment!(
    PolyLineSegment,
    dm_noesis_poly_line_segment_create,
    "A `PolyLineSegment`: a run of straight lines through a point list."
);
poly_segment!(
    PolyBezierSegment,
    dm_noesis_poly_bezier_segment_create,
    "A `PolyBezierSegment`: a run of cubic Béziers (points in groups of three)."
);
poly_segment!(
    PolyQuadraticBezierSegment,
    dm_noesis_poly_quadratic_bezier_segment_create,
    "A `PolyQuadraticBezierSegment`: a run of quadratic Béziers (points in pairs)."
);

// ── Primitive geometries ─────────────────────────────────────────────────────

/// An `EllipseGeometry` defined by a center and radii.
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
        let ptr = unsafe { dm_noesis_ellipse_geometry_create(cx, cy, rx, ry) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_ellipse_geometry_create returned null"),
        }
    }

    /// Read `[centerX, centerY, radiusX, radiusY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live EllipseGeometry*; `out` is 4 floats.
        unsafe { dm_noesis_ellipse_geometry_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

/// A `RectangleGeometry`, optionally with rounded corners.
pub struct RectangleGeometry {
    ptr: NonNull<c_void>,
}

geometry_handle!(RectangleGeometry);

impl RectangleGeometry {
    /// Create a rectangle `(x, y, width, height)` with corner radii `(rx, ry)`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn new(x: f32, y: f32, width: f32, height: f32, rx: f32, ry: f32) -> Self {
        let ptr = unsafe { dm_noesis_rectangle_geometry_create(x, y, width, height, rx, ry) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_rectangle_geometry_create returned null"),
        }
    }

    /// Create a rectangle from a [`Rect`] and corner radii `(rx, ry)` — the
    /// ergonomic alternative to the 6-positional-argument [`new`](Self::new).
    /// Round-trips with [`rect`](Self::rect) / [`radii`](Self::radii).
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
            dm_noesis_rectangle_geometry_get(
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
            dm_noesis_rectangle_geometry_get(
                self.ptr.as_ptr(),
                core::ptr::null_mut(),
                out.as_mut_ptr(),
            )
        };
        (out[0], out[1])
    }
}

/// A `LineGeometry` between two points.
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
        let ptr = unsafe { dm_noesis_line_geometry_create(x1, y1, x2, y2) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_line_geometry_create returned null"),
        }
    }

    /// Read `[startX, startY, endX, endY]` back from the live object.
    #[must_use]
    pub fn get(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live LineGeometry*; `out` is 4 floats.
        unsafe { dm_noesis_line_geometry_get(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

// ── CombinedGeometry ─────────────────────────────────────────────────────────

/// A `CombinedGeometry` of two operand geometries combined by a
/// [`GeometryCombineMode`].
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
            dm_noesis_combined_geometry_create(
                mode.to_ordinal(),
                geometry1.geometry_raw(),
                geometry2.geometry_raw(),
            )
        };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_combined_geometry_create returned null"),
        }
    }

    /// Replace the first operand.
    pub fn set_geometry1<A: Geometry>(&mut self, geometry: &A) {
        // SAFETY: self.ptr is live; geometry_raw() is a live Geometry*.
        unsafe {
            dm_noesis_combined_geometry_set_geometry1(self.ptr.as_ptr(), geometry.geometry_raw())
        };
    }

    /// Replace the second operand.
    pub fn set_geometry2<B: Geometry>(&mut self, geometry: &B) {
        // SAFETY: self.ptr is live; geometry_raw() is a live Geometry*.
        unsafe {
            dm_noesis_combined_geometry_set_geometry2(self.ptr.as_ptr(), geometry.geometry_raw())
        };
    }

    /// Borrowed `Noesis::Geometry*` of the first operand (no `+1`), or null.
    #[must_use]
    pub fn geometry1_raw(&self) -> *mut c_void {
        // SAFETY: self.ptr is a live CombinedGeometry*.
        unsafe { dm_noesis_combined_geometry_get_geometry1(self.ptr.as_ptr()) }
    }

    /// Borrowed `Noesis::Geometry*` of the second operand (no `+1`), or null.
    #[must_use]
    pub fn geometry2_raw(&self) -> *mut c_void {
        // SAFETY: self.ptr is a live CombinedGeometry*.
        unsafe { dm_noesis_combined_geometry_get_geometry2(self.ptr.as_ptr()) }
    }

    /// Set the combine mode.
    pub fn set_mode(&mut self, mode: GeometryCombineMode) {
        // SAFETY: self.ptr is a live CombinedGeometry*.
        unsafe { dm_noesis_combined_geometry_set_mode(self.ptr.as_ptr(), mode.to_ordinal()) };
    }

    /// Read the combine mode back from the live object.
    #[must_use]
    pub fn mode(&self) -> Option<GeometryCombineMode> {
        // SAFETY: self.ptr is a live CombinedGeometry*.
        GeometryCombineMode::from_ordinal(unsafe {
            dm_noesis_combined_geometry_get_mode(self.ptr.as_ptr())
        })
    }
}

// ── GeometryGroup ────────────────────────────────────────────────────────────

/// A `GeometryGroup`: several child geometries combined by a [`FillRule`].
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
        let ptr = unsafe { dm_noesis_geometry_group_create() };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_geometry_group_create returned null"),
        }
    }

    /// Append a child geometry. The group takes its own reference, so `child`
    /// may be dropped afterwards. Returns the new child index.
    pub fn add_child<G: Geometry>(&mut self, child: &G) -> i32 {
        // SAFETY: self.ptr is a live GeometryGroup*; geometry_raw() is a live
        // Geometry* borrowed for the call.
        unsafe { dm_noesis_geometry_group_add_child(self.ptr.as_ptr(), child.geometry_raw()) }
    }

    /// Number of child geometries in the group.
    #[must_use]
    pub fn child_count(&self) -> usize {
        // SAFETY: self.ptr is a live GeometryGroup*.
        unsafe { dm_noesis_geometry_group_child_count(self.ptr.as_ptr()) }.max(0) as usize
    }

    /// Set the fill rule.
    pub fn set_fill_rule(&mut self, rule: FillRule) {
        // SAFETY: self.ptr is a live GeometryGroup*.
        unsafe { dm_noesis_geometry_group_set_fill_rule(self.ptr.as_ptr(), rule.to_ordinal()) };
    }

    /// Read the fill rule back from the live object.
    #[must_use]
    pub fn fill_rule(&self) -> FillRule {
        // SAFETY: self.ptr is a live GeometryGroup*.
        FillRule::from_ordinal(unsafe { dm_noesis_geometry_group_get_fill_rule(self.ptr.as_ptr()) })
    }
}
