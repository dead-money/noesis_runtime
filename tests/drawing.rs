//! Immediate-mode drawing via `OnRender` (TODO §10).
//!
//! Registers custom `FrameworkElement`s whose [`RenderHandler`] issues
//! immediate-mode draw commands (`DrawLine` / `DrawRectangle` /
//! `DrawRoundedRectangle` / `DrawEllipse` / `DrawGeometry`, plus `PushTransform`
//! / `PushClip` / `PushBlendingMode` + `Pop`), places one in a `View`, binds a
//! counting [`RenderDevice`], and drives a real update + offscreen + onscreen
//! render pass so `OnRender` actually fires.
//!
//! Assertions (each fails against a stub):
//!   * the render callback fired with a non-null `DrawingContext`,
//!   * every draw / push / pop call reported success (the context + pen + brush +
//!     geometry casts crossed the FFI),
//!   * the filled draws produced real GPU geometry — a painting element records
//!     STRICTLY MORE `draw_batch` calls than an otherwise-identical element that
//!     draws nothing (this cancels the constant trial-watermark batches, so a
//!     no-op draw fn — which adds zero batches — fails),
//!   * a code-built `Pen` round-trips its thickness / line caps / join, and a
//!     `RectangleGeometry` round-trips its rect, through the live Noesis object.
//!
//! A render handler that never fires, or draw fns that no-op, fail these.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use noesis_runtime::brushes::SolidColorBrush;
use noesis_runtime::classes::{
    ClassBuilder, Instance, PropertyChangeHandler, PropertyValue, RenderHandler,
};
use noesis_runtime::drawing::{
    BlendingMode, DrawingContext, Pen, PenLineCap, PenLineJoin, RectangleGeometry,
};
use noesis_runtime::ffi::ClassBase;
use noesis_runtime::geometry::{
    EllipseGeometry, Geometry, LineSegment, PathFigure, PathGeometry,
};
use noesis_runtime::render_device::types::{Batch, DeviceCaps, Tile};
use noesis_runtime::render_device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect, register,
};
use noesis_runtime::transforms::TranslateTransform;
use noesis_runtime::view::{FrameworkElement, View};

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[derive(Clone, Default)]
struct Signals {
    renders: Arc<AtomicU32>,
    all_draws_ok: Arc<AtomicBool>,
    image_null_rejected: Arc<AtomicBool>,
}

/// Render handler that owns its drawing resources (so they outlive each
/// `OnRender`) and, when `draw` is set, exercises every `DrawingContext`
/// entrypoint. With `draw` clear it records the same signals but issues no draw
/// commands — the baseline used to cancel the trial watermark's batches.
struct PainterRender {
    draw: bool,
    signals: Signals,
    brush: SolidColorBrush,
    pen: Pen,
    geometry: RectangleGeometry,
    // Rich code-built geometries from `crate::geometry`, drawn / clipped through
    // the unified `geometry::Geometry` trait — proving `draw_geometry` /
    // `push_clip` accept real built geometry, not just a drawing-local rect.
    path: PathGeometry,
    ellipse: EllipseGeometry,
    transform: TranslateTransform,
}

impl RenderHandler for PainterRender {
    fn render(&self, _instance: Instance, ctx: DrawingContext<'_>) {
        self.signals.renders.fetch_add(1, Ordering::SeqCst);
        if !self.draw {
            return;
        }

        let mut ok = true;
        // Fill + stroke shapes covering the element — produces real geometry.
        ok &= ctx.draw_rectangle(Some(&self.brush), Some(&self.pen), [0.0, 0.0, 100.0, 80.0]);
        ok &= ctx.draw_line(&self.pen, (0.0, 0.0), (100.0, 80.0));
        ok &= ctx.draw_rounded_rectangle(
            Some(&self.brush),
            Some(&self.pen),
            [10.0, 10.0, 40.0, 30.0],
            6.0,
            6.0,
        );
        ok &= ctx.draw_ellipse(Some(&self.brush), Some(&self.pen), (50.0, 40.0), 20.0, 15.0);
        ok &= ctx.draw_geometry(Some(&self.brush), Some(&self.pen), &self.geometry);

        // Draw real built geometry via the unified `geometry::Geometry` trait:
        // a `PathGeometry` (triangle figure) and an `EllipseGeometry`.
        ok &= ctx.draw_geometry(Some(&self.brush), Some(&self.pen), &self.path);
        ok &= ctx.draw_geometry(Some(&self.brush), Some(&self.pen), &self.ellipse);

        // Clip with the `EllipseGeometry`, draw inside it, then pop.
        ok &= ctx.push_clip(&self.ellipse);
        ok &= ctx.draw_geometry(Some(&self.brush), None, &self.path);
        ok &= ctx.pop(); // ellipse clip

        // Push / draw / pop a transformed + clipped + blended layer.
        ok &= ctx.push_transform(&self.transform);
        ok &= ctx.push_clip(&self.geometry);
        ok &= ctx.push_blending_mode(BlendingMode::Additive);
        ok &= ctx.draw_rectangle(Some(&self.brush), None, [0.0, 0.0, 50.0, 50.0]);
        ok &= ctx.pop(); // blending
        ok &= ctx.pop(); // clip
        ok &= ctx.pop(); // transform

        self.signals.all_draws_ok.store(ok, Ordering::SeqCst);

        // DrawImage requires a real ImageSource (TODO §12); a null source must be
        // rejected gracefully (false) rather than reaching Noesis.
        // SAFETY: passing null is explicitly the rejected path under test.
        let rejected = !unsafe { ctx.draw_image(std::ptr::null_mut(), [0.0, 0.0, 10.0, 10.0]) };
        self.signals
            .image_null_rejected
            .store(rejected, Ordering::SeqCst);
    }
}

fn xaml(ns_class: &str) -> String {
    format!(
        r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      xmlns:d="clr-namespace:Draw" Width="200" Height="200">
  <d:{ns_class} x:Name="P" Width="100" Height="80"
                HorizontalAlignment="Left" VerticalAlignment="Top"/>
</Grid>"##
    )
}

/// Register `class_name`, mount it in a View, drive a full render pass with a
/// fresh counting device, and return the number of `draw_batch` calls recorded.
fn render_batches(class_name: &str, draw: bool, signals: Signals) -> u32 {
    // Build a real PathGeometry: a closed triangle figure.
    let mut path = PathGeometry::new();
    let mut figure = PathFigure::new();
    figure.set_start_point(0.0, 0.0);
    figure.add_segment(&LineSegment::new(80.0, 0.0));
    figure.add_segment(&LineSegment::new(40.0, 60.0));
    figure.set_is_closed(true);
    path.add_figure(&figure);
    assert!(path.figure_count() >= 1, "path figure not added");
    assert!(!path.is_empty(), "path geometry built empty");

    let ellipse = EllipseGeometry::new(50.0, 40.0, 30.0, 20.0);

    let painter = PainterRender {
        draw,
        signals,
        brush: SolidColorBrush::new([0.2, 0.6, 0.9, 1.0]),
        pen: Pen::new(&SolidColorBrush::new([1.0, 1.0, 1.0, 1.0]), 1.5),
        geometry: RectangleGeometry::new(0.0, 0.0, 100.0, 80.0, 0.0, 0.0),
        path,
        ellipse,
        transform: TranslateTransform::new(5.0, 5.0),
    };

    let mut b = ClassBuilder::new(
        &format!("Draw.{class_name}"),
        ClassBase::FrameworkElement,
        NoopChange,
    );
    b.set_render(painter);
    let reg = b.register().expect("class registration failed");

    let root = FrameworkElement::parse(&xaml(class_name)).expect("parse XAML");
    let mut view = View::create(root);
    view.set_size(200, 200);
    view.activate();
    assert!(view.update(0.0), "first update produced no snapshot");

    let batches = Arc::new(AtomicU32::new(0));
    let device = register(CountingDevice::new(Arc::clone(&batches)));
    {
        let mut renderer = view.renderer();
        renderer.init(&device);
        // OnRender fires while the renderer collects the render tree.
        renderer.update_render_tree();
        renderer.render_offscreen();
        renderer.render(false, true);
        renderer.shutdown();
    }

    view.deactivate();
    drop(view);
    drop(device);
    drop(reg);

    batches.load(Ordering::SeqCst)
}

#[test]
fn on_render_fires_and_draws() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        // ── Pen read-back round-trip (proves the create + setters crossed FFI).
        let stroke = SolidColorBrush::new([0.0, 0.0, 0.0, 1.0]);
        let mut pen = Pen::new(&stroke, 3.5);
        assert!(
            (pen.thickness() - 3.5).abs() < 1.0e-4,
            "pen thickness round-trip"
        );
        assert!(pen.set_thickness(2.0), "setter should succeed");
        assert!(
            (pen.thickness() - 2.0).abs() < 1.0e-4,
            "pen set_thickness round-trip"
        );
        assert!(pen.set_line_caps(PenLineCap::Round, PenLineCap::Triangle, PenLineCap::Square));
        assert_eq!(
            pen.line_caps(),
            Some((PenLineCap::Round, PenLineCap::Triangle, PenLineCap::Square)),
            "pen line caps round-trip"
        );
        assert!(pen.set_line_join(PenLineJoin::Round, 7.0));
        let (join, miter) = pen.line_join().expect("line join");
        assert_eq!(join, PenLineJoin::Round, "pen line join round-trip");
        assert!((miter - 7.0).abs() < 1.0e-4, "pen miter limit round-trip");
        assert!(pen.brush().is_some(), "pen brush set at construction");
        drop(pen);

        // RectangleGeometry read-back round-trip.
        let geo = RectangleGeometry::new(5.0, 6.0, 30.0, 20.0, 0.0, 0.0);
        let r = geo.rect();
        assert!(
            (r[0] - 5.0).abs() < 1.0e-4 && (r[2] - 30.0).abs() < 1.0e-4,
            "geometry rect round-trip"
        );
        drop(geo);

        // ── OnRender drive: baseline (no draws) vs painter (full draws) ──────
        let blank = Signals::default();
        let baseline = render_batches("Blank", false, blank.clone());
        assert!(
            blank.renders.load(Ordering::SeqCst) > 0,
            "baseline OnRender trampoline never fired"
        );

        let painted = Signals::default();
        let full = render_batches("Painter", true, painted.clone());

        assert!(
            painted.renders.load(Ordering::SeqCst) > 0,
            "painter OnRender trampoline never fired"
        );
        assert!(
            painted.all_draws_ok.load(Ordering::SeqCst),
            "a draw / push / pop command failed to reach the DrawingContext"
        );
        assert!(
            painted.image_null_rejected.load(Ordering::SeqCst),
            "DrawImage(null) was not rejected"
        );
        // The painting element adds real geometry on top of the identical
        // watermark baseline — a no-op draw fn would leave the counts equal.
        assert!(
            full > baseline,
            "filled draws produced no extra GPU batches (no-op draw fns): \
             painter={full} baseline={baseline}"
        );
    }

    noesis_runtime::shutdown();
}

// ────────────────────────────────────────────────────────────────────────────
// CountingDevice — a minimal RenderDevice that hands out monotonic handles and
// counts draw batches, so the test can prove the immediate-mode draws produced
// real geometry without coupling to a GPU backend.
// ────────────────────────────────────────────────────────────────────────────

struct CountingDevice {
    next_handle: u64,
    batches: Arc<AtomicU32>,
    vb: Vec<u8>,
    ib: Vec<u8>,
}

impl CountingDevice {
    fn new(batches: Arc<AtomicU32>) -> Self {
        Self {
            next_handle: 1,
            batches,
            // DYNAMIC_VB_SIZE / DYNAMIC_IB_SIZE from RenderDevice.h.
            vb: vec![0; 512 * 1024],
            ib: vec![0; 128 * 1024],
        }
    }

    fn alloc(&mut self) -> std::num::NonZeroU64 {
        let h = self.next_handle;
        self.next_handle += 1;
        std::num::NonZeroU64::new(h).expect("handles start at 1")
    }
}

impl RenderDevice for CountingDevice {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn caps(&self) -> DeviceCaps {
        DeviceCaps::default()
    }

    fn create_texture(&mut self, desc: TextureDesc<'_>) -> TextureBinding {
        TextureBinding {
            handle: TextureHandle(self.alloc()),
            width: desc.width,
            height: desc.height,
            has_mipmaps: desc.num_levels > 1,
            inverted: false,
            has_alpha: true,
        }
    }

    fn update_texture(
        &mut self,
        _handle: TextureHandle,
        _level: u32,
        _rect: TextureRect,
        _data: &[u8],
    ) {
    }

    fn end_updating_textures(&mut self, _textures: &[TextureHandle]) {}

    fn drop_texture(&mut self, _handle: TextureHandle) {}

    fn create_render_target(&mut self, desc: RenderTargetDesc<'_>) -> RenderTargetBinding {
        let rt = RenderTargetHandle(self.alloc());
        let tex = TextureHandle(self.alloc());
        RenderTargetBinding {
            handle: rt,
            resolve_texture: TextureBinding {
                handle: tex,
                width: desc.width,
                height: desc.height,
                has_mipmaps: false,
                inverted: false,
                has_alpha: true,
            },
        }
    }

    fn clone_render_target(
        &mut self,
        _label: &str,
        _src: RenderTargetHandle,
    ) -> RenderTargetBinding {
        let rt = RenderTargetHandle(self.alloc());
        let tex = TextureHandle(self.alloc());
        RenderTargetBinding {
            handle: rt,
            resolve_texture: TextureBinding {
                handle: tex,
                width: 0,
                height: 0,
                has_mipmaps: false,
                inverted: false,
                has_alpha: true,
            },
        }
    }

    fn drop_render_target(&mut self, _handle: RenderTargetHandle) {}

    fn begin_offscreen_render(&mut self) {}
    fn end_offscreen_render(&mut self) {}
    fn begin_onscreen_render(&mut self) {}
    fn end_onscreen_render(&mut self) {}

    fn set_render_target(&mut self, _handle: RenderTargetHandle) {}
    fn begin_tile(&mut self, _handle: RenderTargetHandle, _tile: Tile) {}
    fn end_tile(&mut self, _handle: RenderTargetHandle) {}
    fn resolve_render_target(&mut self, _handle: RenderTargetHandle, _tiles: &[Tile]) {}

    fn map_vertices(&mut self, bytes: u32) -> &mut [u8] {
        &mut self.vb[..bytes as usize]
    }
    fn unmap_vertices(&mut self) {}
    fn map_indices(&mut self, bytes: u32) -> &mut [u8] {
        &mut self.ib[..bytes as usize]
    }
    fn unmap_indices(&mut self) {}

    fn draw_batch(&mut self, _batch: &Batch) {
        self.batches.fetch_add(1, Ordering::SeqCst);
    }
}
