//! Immediate-mode drawing from a custom element's render callback.
//!
//! Noesis hands out a `DrawingContext` only to `UIElement::OnRender`; it has
//! no `DrawingVisual` or `Drawing` object model. To draw, register a custom
//! class with a [`RenderHandler`](crate::classes::RenderHandler) via
//! [`ClassBuilder::set_render`](crate::classes::ClassBuilder::set_render). The
//! handler receives a [`DrawingContext`] for the duration of the call.
//!
//! * [`DrawingContext`] draws lines, rectangles, ellipses, geometry, text,
//!   meshes, and images, and pushes clips, transforms, and blending modes.
//! * [`Pen`] describes a stroke: brush, thickness, caps, joins, and dashes.
//! * Geometry types live in [`crate::geometry`]; [`Geometry`] and
//!   [`RectangleGeometry`] are re-exported here, along with [`PenLineCap`] and
//!   [`PenLineJoin`] from [`crate::shapes`].

use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::{CStr, c_void};

use crate::brushes::Brush;
use crate::ffi::{
    noesis_base_component_release, noesis_drawing_draw_ellipse, noesis_drawing_draw_geometry,
    noesis_drawing_draw_image, noesis_drawing_draw_line, noesis_drawing_draw_mesh,
    noesis_drawing_draw_rectangle, noesis_drawing_draw_rounded_rectangle, noesis_drawing_draw_text,
    noesis_drawing_pop, noesis_drawing_push_blending_mode, noesis_drawing_push_clip,
    noesis_drawing_push_transform, noesis_pen_create, noesis_pen_get_brush,
    noesis_pen_get_dash_offset, noesis_pen_get_dashes, noesis_pen_get_line_caps,
    noesis_pen_get_line_join, noesis_pen_get_thickness, noesis_pen_set_brush,
    noesis_pen_set_dash_style, noesis_pen_set_line_caps, noesis_pen_set_line_join,
    noesis_pen_set_thickness,
};
pub use crate::geometry::{Geometry, RectangleGeometry};
pub use crate::shapes::{PenLineCap, PenLineJoin};
use crate::transforms::Transform;

/// How drawn content combines with what is already behind it. Used with
/// [`DrawingContext::push_blending_mode`].
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum BlendingMode {
    Normal = 0,
    Multiply = 1,
    Screen = 2,
    Additive = 3,
}

/// A `Noesis::Pen`: how [`DrawingContext`] strokes outlines. Combines a
/// [`Brush`], a thickness, line caps and join, and an optional dash pattern.
///
/// Getters read the live Noesis object. Setters return `false` only if the
/// handle fails its type check, which doesn't happen for a live `Pen`. Holds
/// one reference, released on drop.
pub struct Pen {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Pen {}

impl Pen {
    /// Create a pen of `thickness` DIPs painted with `brush`. The pen keeps its
    /// own reference to the brush, so you can drop the brush handle.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the pen (not expected after
    /// [`crate::init`]).
    #[must_use]
    pub fn new(brush: &dyn Brush, thickness: f32) -> Self {
        // SAFETY: brush.brush_raw() is a live Brush* for the borrow; the C side
        // copies the reference. Returns a +1-owned Pen* this handle releases.
        let ptr = unsafe { noesis_pen_create(brush.brush_raw(), thickness) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_pen_create returned null"),
        }
    }

    /// Create a pen of `thickness` DIPs with no brush. It strokes nothing until
    /// you call [`Self::set_brush`].
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the pen.
    #[must_use]
    pub fn with_thickness(thickness: f32) -> Self {
        // SAFETY: a null brush is allowed (set one later via set_brush).
        let ptr = unsafe { noesis_pen_create(core::ptr::null_mut(), thickness) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_pen_create returned null"),
        }
    }

    /// Raw `Noesis::Pen*`, valid while `self` is alive.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Paint the stroke with `brush`. The pen keeps its own reference to it.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_brush(&mut self, brush: &dyn Brush) -> bool {
        // SAFETY: self.ptr is a live Pen*; brush_raw() is a live Brush*.
        unsafe { noesis_pen_set_brush(self.ptr.as_ptr(), brush.brush_raw()) }
    }

    /// The pen's brush as a borrowed `Noesis::Brush*`, or `None`. Don't
    /// release it.
    #[must_use]
    pub fn brush(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live Pen*; the returned pointer is borrowed.
        let p = unsafe { noesis_pen_get_brush(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set the stroke thickness (in DIPs).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_thickness(&mut self, thickness: f32) -> bool {
        // SAFETY: self.ptr is a live Pen*.
        unsafe { noesis_pen_set_thickness(self.ptr.as_ptr(), thickness) }
    }

    /// The stroke thickness, in DIPs.
    #[must_use]
    pub fn thickness(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is a live Pen*; `out` is a valid float.
        unsafe { noesis_pen_get_thickness(self.ptr.as_ptr(), &mut out) };
        out
    }

    /// Set the start, end, and dash line caps.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_line_caps(&mut self, start: PenLineCap, end: PenLineCap, dash: PenLineCap) -> bool {
        // SAFETY: self.ptr is a live Pen*; the enum ordinals match Noesis's.
        unsafe {
            noesis_pen_set_line_caps(self.ptr.as_ptr(), start as i32, end as i32, dash as i32)
        }
    }

    /// The `(start, end, dash)` line caps.
    #[must_use]
    pub fn line_caps(&self) -> Option<(PenLineCap, PenLineCap, PenLineCap)> {
        let mut out = [0i32; 3];
        // SAFETY: self.ptr is a live Pen*; `out` is a 3-int buffer.
        let ok = unsafe { noesis_pen_get_line_caps(self.ptr.as_ptr(), out.as_mut_ptr()) };
        if !ok {
            return None;
        }
        Some((
            cap_from_i32(out[0]),
            cap_from_i32(out[1]),
            cap_from_i32(out[2]),
        ))
    }

    /// Set the line join and miter limit.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_line_join(&mut self, join: PenLineJoin, miter_limit: f32) -> bool {
        // SAFETY: self.ptr is a live Pen*; the enum ordinal matches Noesis's.
        unsafe { noesis_pen_set_line_join(self.ptr.as_ptr(), join as i32, miter_limit) }
    }

    /// The `(join, miter_limit)`.
    #[must_use]
    pub fn line_join(&self) -> Option<(PenLineJoin, f32)> {
        let mut join = 0i32;
        let mut miter = 0.0f32;
        // SAFETY: self.ptr is a live Pen*; both out params are valid.
        let ok = unsafe { noesis_pen_get_line_join(self.ptr.as_ptr(), &mut join, &mut miter) };
        ok.then(|| (join_from_i32(join), miter))
    }

    /// Set the dash pattern. `dashes` alternates dash and gap lengths, in
    /// multiples of the pen thickness; `offset` is how far into the pattern the
    /// stroke starts. An empty `dashes` restores a solid stroke.
    pub fn set_dash_style(&mut self, dashes: &[f32], offset: f32) -> bool {
        let count = u32::try_from(dashes.len()).unwrap_or(u32::MAX);
        // SAFETY: self.ptr is a live Pen*; `dashes`/`count` describe a valid
        // (possibly empty) slice read only for the duration of the call.
        unsafe { noesis_pen_set_dash_style(self.ptr.as_ptr(), dashes.as_ptr(), count, offset) }
    }

    /// The dash offset, or `None` for a solid stroke.
    #[must_use]
    pub fn dash_offset(&self) -> Option<f32> {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is a live Pen*; `out` is a valid float.
        let ok = unsafe { noesis_pen_get_dash_offset(self.ptr.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// The dash pattern, or `None` for a solid stroke. Parsed from Noesis's
    /// string form, so values may differ from what you set by float rounding.
    #[must_use]
    pub fn dashes(&self) -> Option<Vec<f32>> {
        // SAFETY: self.ptr is a live Pen*; the returned pointer (if non-null) is
        // a borrowed NUL-terminated string valid until the next pen mutation,
        // copied out immediately here.
        let p = unsafe { noesis_pen_get_dashes(self.ptr.as_ptr()) };
        if p.is_null() {
            return None;
        }
        // SAFETY: p is a NUL-terminated string owned by the live DashStyle.
        let s = unsafe { CStr::from_ptr(p) }.to_string_lossy();
        Some(
            s.split([' ', ','])
                .filter(|t| !t.is_empty())
                .filter_map(|t| t.parse::<f32>().ok())
                .collect(),
        )
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_pen_create with a +1 ref we own.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

fn cap_from_i32(v: i32) -> PenLineCap {
    match v {
        1 => PenLineCap::Square,
        2 => PenLineCap::Round,
        3 => PenLineCap::Triangle,
        _ => PenLineCap::Flat,
    }
}

fn join_from_i32(v: i32) -> PenLineJoin {
    match v {
        1 => PenLineJoin::Bevel,
        2 => PenLineJoin::Round,
        _ => PenLineJoin::Miter,
    }
}

/// The drawing surface passed to
/// [`RenderHandler::render`](crate::classes::RenderHandler::render), valid only
/// during that call.
///
/// Coordinates are DIPs in the element's local space; rectangles are
/// `[x, y, width, height]`. A `None` brush fills nothing and a `None` pen
/// strokes nothing. Each `push_*` must be matched by a [`Self::pop`].
///
/// Methods return `false` when an argument fails its type check (e.g. a null
/// image); with live handles from this crate they return `true`.
pub struct DrawingContext<'a> {
    ptr: NonNull<c_void>,
    _marker: PhantomData<&'a ()>,
}

impl DrawingContext<'_> {
    /// Wrap a raw `Noesis::DrawingContext*`.
    ///
    /// # Safety
    ///
    /// `ptr` must be a live `DrawingContext*` from a render callback, and the
    /// result must not outlive that callback.
    #[must_use]
    pub unsafe fn from_raw(ptr: NonNull<c_void>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    /// Raw `Noesis::DrawingContext*`, valid during the render callback.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Stroke a line from `p0` to `p1`.
    pub fn draw_line(&self, pen: &Pen, p0: (f32, f32), p1: (f32, f32)) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; pen.raw() is a live Pen*.
        unsafe { noesis_drawing_draw_line(self.ptr.as_ptr(), pen.raw(), p0.0, p0.1, p1.0, p1.1) }
    }

    /// Fill and/or stroke a rectangle.
    pub fn draw_rectangle(
        &self,
        brush: Option<&dyn Brush>,
        pen: Option<&Pen>,
        rect: [f32; 4],
    ) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; the brush / pen pointers
        // (or null) are live for the borrow.
        unsafe {
            noesis_drawing_draw_rectangle(
                self.ptr.as_ptr(),
                brush_ptr(brush),
                pen_ptr(pen),
                rect[0],
                rect[1],
                rect[2],
                rect[3],
            )
        }
    }

    /// Fill and/or stroke a rectangle with corner radii `r_x` and `r_y`.
    pub fn draw_rounded_rectangle(
        &self,
        brush: Option<&dyn Brush>,
        pen: Option<&Pen>,
        rect: [f32; 4],
        r_x: f32,
        r_y: f32,
    ) -> bool {
        // SAFETY: as `draw_rectangle`.
        unsafe {
            noesis_drawing_draw_rounded_rectangle(
                self.ptr.as_ptr(),
                brush_ptr(brush),
                pen_ptr(pen),
                rect[0],
                rect[1],
                rect[2],
                rect[3],
                r_x,
                r_y,
            )
        }
    }

    /// Fill and/or stroke an ellipse at `center` with radii `r_x` and `r_y`.
    pub fn draw_ellipse(
        &self,
        brush: Option<&dyn Brush>,
        pen: Option<&Pen>,
        center: (f32, f32),
        r_x: f32,
        r_y: f32,
    ) -> bool {
        // SAFETY: as `draw_rectangle`.
        unsafe {
            noesis_drawing_draw_ellipse(
                self.ptr.as_ptr(),
                brush_ptr(brush),
                pen_ptr(pen),
                center.0,
                center.1,
                r_x,
                r_y,
            )
        }
    }

    /// Fill and/or stroke a [`Geometry`].
    pub fn draw_geometry(
        &self,
        brush: Option<&dyn Brush>,
        pen: Option<&Pen>,
        geometry: &dyn Geometry,
    ) -> bool {
        // SAFETY: as `draw_rectangle`; geometry_raw() is a live Geometry*.
        unsafe {
            noesis_drawing_draw_geometry(
                self.ptr.as_ptr(),
                brush_ptr(brush),
                pen_ptr(pen),
                geometry.geometry_raw(),
            )
        }
    }

    /// Draw [`FormattedText`](crate::formatted_text::FormattedText) inside
    /// `bounds`. The text carries its own brush.
    pub fn draw_text(
        &self,
        formatted_text: &crate::formatted_text::FormattedText,
        bounds: [f32; 4],
    ) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; raw() is a live
        // FormattedText* borrowed for the call.
        unsafe {
            noesis_drawing_draw_text(
                self.ptr.as_ptr(),
                formatted_text.raw(),
                bounds[0],
                bounds[1],
                bounds[2],
                bounds[3],
            )
        }
    }

    /// Fill a [`MeshData`](crate::mesh::MeshData) with `brush`.
    pub fn draw_mesh(&self, brush: Option<&dyn Brush>, mesh: &crate::mesh::MeshData) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; mesh.raw() is a live
        // MeshData*; the brush pointer (or null) is live for the borrow.
        unsafe { noesis_drawing_draw_mesh(self.ptr.as_ptr(), brush_ptr(brush), mesh.raw()) }
    }

    /// Draw an image stretched to `rect`. Returns `false` if `image_source` is
    /// null or not an `ImageSource`.
    ///
    /// # Safety
    ///
    /// `image_source` must be null or a live `Noesis::BaseComponent*`, e.g. from
    /// [`FrameworkElement::get_component`](crate::view::FrameworkElement::get_component).
    pub unsafe fn draw_image(&self, image_source: *mut c_void, rect: [f32; 4]) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; `image_source` per contract.
        unsafe {
            noesis_drawing_draw_image(
                self.ptr.as_ptr(),
                image_source,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
            )
        }
    }

    /// Undo the most recent `push_*`.
    pub fn pop(&self) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*.
        unsafe { noesis_drawing_pop(self.ptr.as_ptr()) }
    }

    /// Clip later drawing to `geometry` until the matching [`Self::pop`].
    pub fn push_clip(&self, geometry: &dyn Geometry) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; geometry_raw() is live.
        unsafe { noesis_drawing_push_clip(self.ptr.as_ptr(), geometry.geometry_raw()) }
    }

    /// Apply `transform` to later drawing until the matching [`Self::pop`].
    pub fn push_transform(&self, transform: &dyn Transform) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; transform_raw() is live.
        unsafe { noesis_drawing_push_transform(self.ptr.as_ptr(), transform.transform_raw()) }
    }

    /// Blend later drawing with `mode` until the matching [`Self::pop`].
    pub fn push_blending_mode(&self, mode: BlendingMode) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; the ordinal matches Noesis.
        unsafe { noesis_drawing_push_blending_mode(self.ptr.as_ptr(), mode as i32) }
    }
}

fn brush_ptr(brush: Option<&dyn Brush>) -> *mut c_void {
    brush.map_or(core::ptr::null_mut(), Brush::brush_raw)
}

fn pen_ptr(pen: Option<&Pen>) -> *mut c_void {
    pen.map_or(core::ptr::null_mut(), Pen::raw)
}
