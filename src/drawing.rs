//! Immediate-mode drawing via `DrawingContext` (TODO §10).
//!
//! In Noesis 3.2.13 a [`DrawingContext`] has a private constructor (friend
//! `UIElement`) and is delivered ONLY to `UIElement::OnRender` — there is no
//! public `DrawingVisual`/`RenderOpen` and no `Drawing`/`DrawingGroup` object
//! model. So immediate-mode drawing is reachable exactly one way: override
//! `OnRender` on a custom element. This module provides:
//!
//! * [`Pen`] — a code-built `Noesis::Pen` (brush + thickness + line caps /
//!   join), the stroke descriptor several draw calls need. Owning handle with a
//!   `+1` reference released on [`Drop`], like the brushes in [`crate::brushes`].
//!   Read-back getters ([`Pen::thickness`], [`Pen::line_caps`], …) re-read the
//!   live object so a test proves the value crossed the FFI.
//! * [`RectangleGeometry`] — a minimal [`Geometry`] primitive so the
//!   [`DrawingContext::draw_geometry`] / [`DrawingContext::push_clip`] paths are
//!   reachable (full geometry construction is still TODO §10).
//! * [`DrawingContext`] — a **borrowed** handle over the `DrawingContext*`
//!   handed to a [`crate::classes::RenderHandler`]. Valid only for the duration
//!   of the render callback; the draw / push / pop methods forward straight into
//!   Noesis.
//!
//! Wire a render handler with
//! [`ClassBuilder::set_render`](crate::classes::ClassBuilder::set_render).

use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::c_void;

use crate::brushes::Brush;
use crate::ffi::{
    dm_noesis_base_component_release, dm_noesis_drawing_draw_ellipse,
    dm_noesis_drawing_draw_geometry, dm_noesis_drawing_draw_image, dm_noesis_drawing_draw_line,
    dm_noesis_drawing_draw_rectangle, dm_noesis_drawing_draw_rounded_rectangle,
    dm_noesis_drawing_pop, dm_noesis_drawing_push_blending_mode, dm_noesis_drawing_push_clip,
    dm_noesis_drawing_push_transform, dm_noesis_drawing_rect_geometry_create, dm_noesis_pen_create,
    dm_noesis_pen_get_brush, dm_noesis_pen_get_line_caps, dm_noesis_pen_get_line_join,
    dm_noesis_pen_get_thickness, dm_noesis_pen_set_brush, dm_noesis_pen_set_line_caps,
    dm_noesis_pen_set_line_join, dm_noesis_pen_set_thickness,
    dm_noesis_rectangle_geometry_get_rect,
};
use crate::transforms::Transform;

/// How the ends of a stroked line are drawn. Mirrors `Noesis::PenLineCap`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PenLineCap {
    /// No cap past the last point (default).
    Flat = 0,
    /// A square extending half the thickness past the end.
    Square = 1,
    /// A semicircle of diameter equal to the thickness.
    Round = 2,
    /// An isosceles right triangle.
    Triangle = 3,
}

/// How the vertices of a stroked shape are joined. Mirrors `Noesis::PenLineJoin`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PenLineJoin {
    /// Sharp angular vertices (default).
    Miter = 0,
    /// Beveled vertices.
    Bevel = 1,
    /// Rounded vertices.
    Round = 2,
}

/// How drawn content is mixed with what's behind it. Mirrors
/// `Noesis::BlendingMode`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum BlendingMode {
    Normal = 0,
    Multiply = 1,
    Screen = 2,
    Additive = 3,
}

/// A handle to a Noesis `Geometry` — the shape argument of
/// [`DrawingContext::draw_geometry`] / [`DrawingContext::push_clip`].
pub trait Geometry {
    /// Borrowed `Noesis::Geometry*` (a `BaseComponent*`), valid for `self`'s
    /// lifetime.
    fn geometry_raw(&self) -> *mut c_void;
}

// ── Pen ──────────────────────────────────────────────────────────────────────

/// A code-built `Noesis::Pen`: the stroke (outline) descriptor — a [`Brush`] +
/// thickness + line caps / join — that the `Draw*` calls stroke with.
pub struct Pen {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Pen {}

impl Pen {
    /// Create a pen of `thickness` painted by `brush` (any [`Brush`]). Noesis
    /// takes its own reference to the brush, so the brush handle may be dropped
    /// afterwards.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the pen (not expected after
    /// [`crate::init`]).
    #[must_use]
    pub fn new(brush: &dyn Brush, thickness: f32) -> Self {
        // SAFETY: brush.brush_raw() is a live Brush* for the borrow; the C side
        // copies the reference. Returns a +1-owned Pen* this handle releases.
        let ptr = unsafe { dm_noesis_pen_create(brush.brush_raw(), thickness) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_pen_create returned null"),
        }
    }

    /// Create a pen of `thickness` with no brush set yet.
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the pen.
    #[must_use]
    pub fn with_thickness(thickness: f32) -> Self {
        // SAFETY: a null brush is allowed (set one later via set_brush).
        let ptr = unsafe { dm_noesis_pen_create(core::ptr::null_mut(), thickness) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_pen_create returned null"),
        }
    }

    /// Raw `Noesis::Pen*`. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Point the pen at `brush` (Noesis takes its own reference).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_brush(&mut self, brush: &dyn Brush) -> bool {
        // SAFETY: self.ptr is a live Pen*; brush_raw() is a live Brush*.
        unsafe { dm_noesis_pen_set_brush(self.ptr.as_ptr(), brush.brush_raw()) }
    }

    /// Borrowed `Noesis::Brush*` currently set on the pen, or `None`. The
    /// pointer has no `+1` reference; do not release it.
    #[must_use]
    pub fn brush(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live Pen*; the returned pointer is borrowed.
        let p = unsafe { dm_noesis_pen_get_brush(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set the stroke thickness (in DIPs).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_thickness(&mut self, thickness: f32) -> bool {
        // SAFETY: self.ptr is a live Pen*.
        unsafe { dm_noesis_pen_set_thickness(self.ptr.as_ptr(), thickness) }
    }

    /// Read the stroke thickness back from the live object.
    #[must_use]
    pub fn thickness(&self) -> f32 {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is a live Pen*; `out` is a valid float.
        unsafe { dm_noesis_pen_get_thickness(self.ptr.as_ptr(), &mut out) };
        out
    }

    /// Set the start, end, and dash line caps.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_line_caps(&mut self, start: PenLineCap, end: PenLineCap, dash: PenLineCap) -> bool {
        // SAFETY: self.ptr is a live Pen*; the enum ordinals match Noesis's.
        unsafe {
            dm_noesis_pen_set_line_caps(self.ptr.as_ptr(), start as i32, end as i32, dash as i32)
        }
    }

    /// Read `(start, end, dash)` line caps back from the live object.
    #[must_use]
    pub fn line_caps(&self) -> Option<(PenLineCap, PenLineCap, PenLineCap)> {
        let mut out = [0i32; 3];
        // SAFETY: self.ptr is a live Pen*; `out` is a 3-int buffer.
        let ok = unsafe { dm_noesis_pen_get_line_caps(self.ptr.as_ptr(), out.as_mut_ptr()) };
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
        unsafe { dm_noesis_pen_set_line_join(self.ptr.as_ptr(), join as i32, miter_limit) }
    }

    /// Read `(join, miter_limit)` back from the live object.
    #[must_use]
    pub fn line_join(&self) -> Option<(PenLineJoin, f32)> {
        let mut join = 0i32;
        let mut miter = 0.0f32;
        // SAFETY: self.ptr is a live Pen*; both out params are valid.
        let ok = unsafe { dm_noesis_pen_get_line_join(self.ptr.as_ptr(), &mut join, &mut miter) };
        ok.then(|| (join_from_i32(join), miter))
    }
}

impl Drop for Pen {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_pen_create with a +1 ref we own.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
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

// ── RectangleGeometry ────────────────────────────────────────────────────────

/// A code-built `Noesis::RectangleGeometry` (an axis-aligned rectangle, with
/// optional corner radii) — a minimal [`Geometry`] so the `draw_geometry` /
/// `push_clip` paths are exercisable.
pub struct RectangleGeometry {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for RectangleGeometry {}

impl RectangleGeometry {
    /// Create a rectangle geometry of `(x, y, w, h)` with corner radii
    /// `(r_x, r_y)` (pass `0.0` for sharp corners).
    ///
    /// # Panics
    ///
    /// Panics if Noesis fails to allocate the geometry.
    #[must_use]
    pub fn new(x: f32, y: f32, w: f32, h: f32, r_x: f32, r_y: f32) -> Self {
        let ptr = unsafe { dm_noesis_drawing_rect_geometry_create(x, y, w, h, r_x, r_y) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_drawing_rect_geometry_create returned null"),
        }
    }

    /// Raw `Noesis::RectangleGeometry*`. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Read the rectangle back from the live object as `[x, y, w, h]`.
    #[must_use]
    pub fn rect(&self) -> [f32; 4] {
        let mut out = [0.0f32; 4];
        // SAFETY: self.ptr is a live RectangleGeometry*; `out` is 4 floats.
        unsafe { dm_noesis_rectangle_geometry_get_rect(self.ptr.as_ptr(), out.as_mut_ptr()) };
        out
    }
}

impl Drop for RectangleGeometry {
    fn drop(&mut self) {
        // SAFETY: produced by *_create with a +1 ref we own.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

impl Geometry for RectangleGeometry {
    fn geometry_raw(&self) -> *mut c_void {
        self.raw()
    }
}

// ── DrawingContext ───────────────────────────────────────────────────────────

/// A **borrowed** drawing context, valid only for the duration of a
/// [`crate::classes::RenderHandler::render`] callback. Issues immediate-mode
/// draw / push / pop commands straight into Noesis. Do not store it past the
/// callback — the underlying `Noesis::DrawingContext*` is owned by the element's
/// render pass.
///
/// Coordinates are in DIPs in the element's local space. A null `brush` (`None`)
/// fills nothing; a null `pen` (`None`) strokes nothing — matching Noesis's own
/// behaviour, so passing both `None` draws nothing.
pub struct DrawingContext<'a> {
    ptr: NonNull<c_void>,
    _marker: PhantomData<&'a ()>,
}

impl DrawingContext<'_> {
    /// Wrap a borrowed `Noesis::DrawingContext*` received via the FFI render
    /// callback.
    ///
    /// # Safety
    ///
    /// `ptr` must be the non-null context pointer delivered to the render
    /// callback; it is borrowed and valid only for that call.
    #[must_use]
    pub unsafe fn from_raw(ptr: NonNull<c_void>) -> Self {
        Self {
            ptr,
            _marker: PhantomData,
        }
    }

    /// Raw `Noesis::DrawingContext*`. Borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Draw a line between two points with `pen`.
    pub fn draw_line(&self, pen: &Pen, p0: (f32, f32), p1: (f32, f32)) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; pen.raw() is a live Pen*.
        unsafe { dm_noesis_drawing_draw_line(self.ptr.as_ptr(), pen.raw(), p0.0, p0.1, p1.0, p1.1) }
    }

    /// Fill and/or stroke a rectangle `[x, y, w, h]`.
    pub fn draw_rectangle(
        &self,
        brush: Option<&dyn Brush>,
        pen: Option<&Pen>,
        rect: [f32; 4],
    ) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; the brush / pen pointers
        // (or null) are live for the borrow.
        unsafe {
            dm_noesis_drawing_draw_rectangle(
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

    /// Fill and/or stroke a rounded rectangle `[x, y, w, h]` with corner radii
    /// `(r_x, r_y)`.
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
            dm_noesis_drawing_draw_rounded_rectangle(
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

    /// Fill and/or stroke an ellipse centered at `(cx, cy)` with radii
    /// `(r_x, r_y)`.
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
            dm_noesis_drawing_draw_ellipse(
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
            dm_noesis_drawing_draw_geometry(
                self.ptr.as_ptr(),
                brush_ptr(brush),
                pen_ptr(pen),
                geometry.geometry_raw(),
            )
        }
    }

    /// Draw a borrowed `Noesis::ImageSource*` into `[x, y, w, h]`. Returns
    /// `false` if `image_source` is null / not an `ImageSource`.
    ///
    /// # Safety
    ///
    /// `image_source` must be a live `Noesis::ImageSource*` (e.g. from
    /// [`FrameworkElement::get_component`](crate::view::FrameworkElement::get_component)).
    pub unsafe fn draw_image(&self, image_source: *mut c_void, rect: [f32; 4]) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; `image_source` per contract.
        unsafe {
            dm_noesis_drawing_draw_image(
                self.ptr.as_ptr(),
                image_source,
                rect[0],
                rect[1],
                rect[2],
                rect[3],
            )
        }
    }

    /// Pop the last `push_*` operation off the context.
    pub fn pop(&self) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*.
        unsafe { dm_noesis_drawing_pop(self.ptr.as_ptr()) }
    }

    /// Push a clip [`Geometry`]; pair with [`Self::pop`].
    pub fn push_clip(&self, geometry: &dyn Geometry) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; geometry_raw() is live.
        unsafe { dm_noesis_drawing_push_clip(self.ptr.as_ptr(), geometry.geometry_raw()) }
    }

    /// Push a [`Transform`]; pair with [`Self::pop`].
    pub fn push_transform(&self, transform: &dyn Transform) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; transform_raw() is live.
        unsafe { dm_noesis_drawing_push_transform(self.ptr.as_ptr(), transform.transform_raw()) }
    }

    /// Push a [`BlendingMode`]; pair with [`Self::pop`].
    pub fn push_blending_mode(&self, mode: BlendingMode) -> bool {
        // SAFETY: self.ptr is a live DrawingContext*; the ordinal matches Noesis.
        unsafe { dm_noesis_drawing_push_blending_mode(self.ptr.as_ptr(), mode as i32) }
    }
}

fn brush_ptr(brush: Option<&dyn Brush>) -> *mut c_void {
    brush.map_or(core::ptr::null_mut(), Brush::brush_raw)
}

fn pen_ptr(pen: Option<&Pen>) -> *mut c_void {
    pen.map_or(core::ptr::null_mut(), Pen::raw)
}
