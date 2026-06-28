//! SVG parsing and geometry queries, all CPU-side and headless — no GPU
//! `RenderDevice` or render pass is needed, so you can use these in tests,
//! tooling, or hit-testing without standing up a renderer.
//!
//! There are two surfaces:
//!
//! * [`SvgPath`] wraps `Noesis::SVGPath`, a single outline. Parse it from an
//!   SVG *path data* string with [`SvgPath::parse`], or build one up with
//!   [`SvgPath::move_to`], [`SvgPath::line_to`], [`SvgPath::add_rect`], and
//!   friends. Then query it: [`SvgPath::bounds`] for the bounding box,
//!   [`SvgPath::fill_contains`] and [`SvgPath::stroke_contains`] for
//!   hit-testing. The queries read back from the live Noesis object each call.
//!
//! * [`SvgImage`] wraps `Noesis::SVG::Image`, the shape collection parsed from
//!   a whole `<svg>` document via [`SvgImage::parse`]. Inspect it with
//!   [`SvgImage::size`], [`SvgImage::shape_count`], and
//!   [`SvgImage::shape_fill_type`].
//!
//! Neither underlying type is a `BaseComponent`; each handle owns a plain heap
//! object freed on [`Drop`].

use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::ffi::{
    noesis_svg_image_destroy, noesis_svg_image_get_size, noesis_svg_image_parse,
    noesis_svg_image_shape_count, noesis_svg_image_shape_fill_type, noesis_svg_path_add_ellipse,
    noesis_svg_path_add_rect, noesis_svg_path_calculate_bounds, noesis_svg_path_close,
    noesis_svg_path_command_count, noesis_svg_path_create, noesis_svg_path_destroy,
    noesis_svg_path_fill_contains, noesis_svg_path_line_to, noesis_svg_path_move_to,
    noesis_svg_path_parse, noesis_svg_path_stroke_contains,
};

/// The fill (winding) rule used by [`SvgPath::fill_contains`].
///
/// Re-exported from [`crate::geometry`] so the crate has a single `FillRule`
/// type. Its ordinals (`EvenOdd` = 0, `Nonzero` = 1) match
/// `Noesis::SVGPath::Fill_EvenOdd` / `Fill_NonZero`.
pub use crate::geometry::FillRule;

/// Stroke join style for [`Pen`] (`Noesis::SVGPath::StrokeJoinStyle`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StrokeJoin {
    /// Mitered (sharp) corners.
    Miter = 0,
    /// Beveled (flattened) corners. The Noesis default.
    Bevel = 1,
    /// Rounded corners.
    Round = 2,
}

/// Stroke cap style for [`Pen`] (`Noesis::SVGPath::StrokeCapStyle`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum StrokeCap {
    /// Flat cap flush with the endpoint.
    Butt = 0,
    /// Square cap extending past the endpoint.
    Square = 1,
    /// Rounded cap.
    Round = 2,
    /// Triangular cap.
    Triangle = 3,
}

/// A stroke pen for [`SvgPath::stroke_contains`], mirroring
/// `Noesis::SVGPath::Pen`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct Pen {
    /// Stroke width.
    pub width: f32,
    /// Corner join style.
    pub join: StrokeJoin,
    /// Start cap style.
    pub start_cap: StrokeCap,
    /// End cap style.
    pub end_cap: StrokeCap,
    /// Miter limit (only relevant for [`StrokeJoin::Miter`]).
    pub miter_limit: f32,
}

impl Default for Pen {
    /// Matches the `Noesis::SVGPath::Pen` member defaults: width 1, bevel join,
    /// butt caps, miter limit 1.
    fn default() -> Self {
        Self {
            width: 1.0,
            join: StrokeJoin::Bevel,
            start_cap: StrokeCap::Butt,
            end_cap: StrokeCap::Butt,
            miter_limit: 1.0,
        }
    }
}

/// An owned `Noesis::SVGPath`: a CPU command buffer describing an outline.
pub struct SvgPath {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for SvgPath {}

impl SvgPath {
    /// Parse an SVG *path data* string (e.g. `"M0 0 L100 0 L100 50 Z"`).
    ///
    /// Returns `None` if the string fails to parse.
    #[must_use]
    pub fn parse(path_data: &str) -> Option<Self> {
        let c = CString::new(path_data).ok()?;
        // SAFETY: `c` is a valid NUL-terminated string for the call; the C side
        // copies what it needs.
        let ptr = unsafe { noesis_svg_path_parse(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Create an empty path to populate with the builder methods.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the path.
    #[must_use]
    pub fn new() -> Self {
        let ptr = unsafe { noesis_svg_path_create() };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_svg_path_create returned null"),
        }
    }

    /// Number of `uint32` entries in the command buffer. Zero for a freshly
    /// [`SvgPath::new`] path; a successfully parsed path is non-empty.
    #[must_use]
    pub fn command_count(&self) -> u32 {
        // SAFETY: self.ptr is a live SVGPath*.
        unsafe { noesis_svg_path_command_count(self.ptr.as_ptr()) }
    }

    /// Start a new sub-path at `(x, y)`.
    pub fn move_to(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live SVGPath*.
        unsafe { noesis_svg_path_move_to(self.ptr.as_ptr(), x, y) };
    }

    /// Add a straight line segment to `(x, y)`.
    pub fn line_to(&mut self, x: f32, y: f32) {
        // SAFETY: self.ptr is a live SVGPath*.
        unsafe { noesis_svg_path_line_to(self.ptr.as_ptr(), x, y) };
    }

    /// Close the current sub-path.
    pub fn close(&mut self) {
        // SAFETY: self.ptr is a live SVGPath*.
        unsafe { noesis_svg_path_close(self.ptr.as_ptr()) };
    }

    /// Append a rectangle with its top-left corner at `(x, y)`.
    pub fn add_rect(&mut self, x: f32, y: f32, width: f32, height: f32) {
        // SAFETY: self.ptr is a live SVGPath*.
        unsafe { noesis_svg_path_add_rect(self.ptr.as_ptr(), x, y, width, height) };
    }

    /// Append an ellipse centered at `(x, y)` with radii `rx`, `ry`.
    pub fn add_ellipse(&mut self, x: f32, y: f32, rx: f32, ry: f32) {
        // SAFETY: self.ptr is a live SVGPath*.
        unsafe { noesis_svg_path_add_ellipse(self.ptr.as_ptr(), x, y, rx, ry) };
    }

    /// Tight axis-aligned bounding box `[x, y, width, height]` of the path
    /// geometry, read back from the live Noesis object.
    #[must_use]
    pub fn bounds(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live SVGPath*; `out` holds 4 floats.
        unsafe { noesis_svg_path_calculate_bounds(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }

    /// Whether `(x, y)` lies inside the filled region under `rule`.
    #[must_use]
    pub fn fill_contains(&self, x: f32, y: f32, rule: FillRule) -> bool {
        // SAFETY: self.ptr is a live SVGPath*.
        unsafe { noesis_svg_path_fill_contains(self.ptr.as_ptr(), x, y, rule as i32) }
    }

    /// Whether `(x, y)` falls within the stroked outline for `pen`.
    #[must_use]
    pub fn stroke_contains(&self, x: f32, y: f32, pen: Pen) -> bool {
        // SAFETY: self.ptr is a live SVGPath*.
        unsafe {
            noesis_svg_path_stroke_contains(
                self.ptr.as_ptr(),
                x,
                y,
                pen.width,
                pen.join as i32,
                pen.start_cap as i32,
                pen.end_cap as i32,
                pen.miter_limit,
            )
        }
    }

    /// Raw `Noesis::SVGPath*`. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Default for SvgPath {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for SvgPath {
    fn drop(&mut self) {
        // SAFETY: produced by a parse/create entrypoint we own; freed once here.
        unsafe { noesis_svg_path_destroy(self.ptr.as_ptr()) };
    }
}

/// The fill-brush kind of a parsed SVG shape, mirroring
/// `Noesis::SVG::Brush::Type`.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum SvgBrushType {
    /// No fill.
    None = 0,
    /// Solid color.
    Solid = 1,
    /// Linear gradient.
    Linear = 2,
    /// Radial gradient.
    Radial = 3,
}

impl SvgBrushType {
    fn from_raw(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::None),
            1 => Some(Self::Solid),
            2 => Some(Self::Linear),
            3 => Some(Self::Radial),
            _ => None,
        }
    }
}

/// An owned `Noesis::SVG::Image`: the path collection parsed from a whole
/// `<svg>` document by `Noesis::SVG::Parse`.
pub struct SvgImage {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for SvgImage {}

impl SvgImage {
    /// Parse a full `<svg>...</svg>` document string into a path collection.
    ///
    /// Returns `None` only if `document` contains an interior NUL. A malformed
    /// document parses into an image with zero shapes (observable via
    /// [`SvgImage::shape_count`]).
    #[must_use]
    pub fn parse(document: &str) -> Option<Self> {
        let c = CString::new(document).ok()?;
        // SAFETY: `c` is a valid NUL-terminated string for the call.
        let ptr = unsafe { noesis_svg_image_parse(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// The parsed document `(width, height)` (the `<svg>` size).
    #[must_use]
    pub fn size(&self) -> (f32, f32) {
        let mut w = 0.0f32;
        let mut h = 0.0f32;
        // SAFETY: self.ptr is a live SVG::Image*; out params valid.
        unsafe { noesis_svg_image_get_size(self.ptr.as_ptr(), &mut w, &mut h) };
        (w, h)
    }

    /// Number of parsed shapes (paths) in the document.
    #[must_use]
    pub fn shape_count(&self) -> u32 {
        // SAFETY: self.ptr is a live SVG::Image*.
        unsafe { noesis_svg_image_shape_count(self.ptr.as_ptr()) }
    }

    /// Fill-brush type of shape `index`, or `None` if the index is out of range.
    #[must_use]
    pub fn shape_fill_type(&self, index: u32) -> Option<SvgBrushType> {
        // SAFETY: self.ptr is a live SVG::Image*.
        let v = unsafe { noesis_svg_image_shape_fill_type(self.ptr.as_ptr(), index) };
        SvgBrushType::from_raw(v)
    }

    /// Raw `Noesis::SVG::Image*`. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for SvgImage {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_svg_image_parse; freed once here.
        unsafe { noesis_svg_image_destroy(self.ptr.as_ptr()) };
    }
}
