//! `MeshData` / `Mesh` + `DrawingContext::draw_mesh` / `draw_text`: headless
//! buffer round-trips and a render-drive batch-count proof.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use noesis_runtime::brushes::SolidColorBrush;
use noesis_runtime::classes::{
    ClassBuilder, Instance, PropertyChangeHandler, PropertyValue, RenderHandler,
};
use noesis_runtime::drawing::DrawingContext;
use noesis_runtime::ffi::ClassBase;
use noesis_runtime::formatted_text::FormattedText;
use noesis_runtime::mesh::{Mesh, MeshData};
use noesis_runtime::render_device::types::{Batch, DeviceCaps, Tile};
use noesis_runtime::render_device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect, register,
};
use noesis_runtime::view::{FrameworkElement, View};

struct NoopChange;
impl PropertyChangeHandler for NoopChange {
    fn on_changed(&self, _instance: Instance, _prop_index: u32, _value: PropertyValue<'_>) {}
}

#[derive(Clone, Default)]
struct Signals {
    renders: Arc<AtomicU32>,
    all_draws_ok: Arc<AtomicBool>,
}

fn build_quad() -> MeshData {
    let mut md = MeshData::new();
    md.set_vertices(&[[0.0, 0.0], [100.0, 0.0], [100.0, 80.0], [0.0, 80.0]]);
    md.set_uvs(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]]);
    md.set_indices(&[0, 1, 2, 0, 2, 3]);
    md.set_bounds([0.0, 0.0, 100.0, 80.0]);
    md
}

struct MeshRender {
    draw: bool,
    signals: Signals,
    brush: SolidColorBrush,
    mesh: MeshData,
    text: FormattedText,
}

impl RenderHandler for MeshRender {
    fn render(&self, _instance: Instance, ctx: DrawingContext<'_>) {
        self.signals.renders.fetch_add(1, Ordering::SeqCst);
        if !self.draw {
            return;
        }
        let mut ok = true;
        ok &= ctx.draw_mesh(Some(&self.brush), &self.mesh);
        // No font provider is wired here, so the text shapes zero glyphs; the
        // call still reaches the live DrawingContext (returns true), which is
        // what this asserts. Glyph rasterization is covered by the FormattedText
        // metrics tests.
        ok &= ctx.draw_text(&self.text, [0.0, 0.0, 100.0, 80.0]);
        self.signals.all_draws_ok.store(ok, Ordering::SeqCst);
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

fn render_batches(class_name: &str, draw: bool, signals: Signals) -> u32 {
    let painter = MeshRender {
        draw,
        signals,
        brush: SolidColorBrush::new([0.2, 0.6, 0.9, 1.0]),
        mesh: build_quad(),
        text: FormattedText::builder("Mesh", "Arial", 16.0).build(),
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
fn mesh_round_trips_and_draws() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let md = build_quad();
        assert_eq!(md.num_vertices(), 4, "vertex count");
        assert_eq!(md.num_uvs(), 4, "uv count");
        assert_eq!(md.num_indices(), 6, "index count");
        assert_eq!(
            md.vertices(),
            vec![[0.0, 0.0], [100.0, 0.0], [100.0, 80.0], [0.0, 80.0]],
            "vertices round-trip"
        );
        assert_eq!(
            md.uvs(),
            vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]],
            "uvs round-trip"
        );
        assert_eq!(md.indices(), vec![0, 1, 2, 0, 2, 3], "indices round-trip");
        let b = md.bounds();
        assert!(
            (b[0]).abs() < 1.0e-4
                && (b[1]).abs() < 1.0e-4
                && (b[2] - 100.0).abs() < 1.0e-4
                && (b[3] - 80.0).abs() < 1.0e-4,
            "bounds round-trip: {b:?}"
        );

        let mut md2 = MeshData::new();
        md2.set_vertices(&[[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]]);
        assert_eq!(md2.num_vertices(), 3, "resized vertex count");
        assert_eq!(
            md2.vertices(),
            vec![[1.0, 2.0], [3.0, 4.0], [5.0, 6.0]],
            "resized vertices round-trip"
        );

        let mut mesh = Mesh::new();
        assert!(mesh.data().is_none(), "fresh Mesh has no data");
        assert!(mesh.brush().is_none(), "fresh Mesh has no brush");
        assert!(mesh.set_data(&md), "set_data should succeed");
        assert_eq!(
            mesh.data().map(std::ptr::NonNull::as_ptr),
            Some(md.raw()),
            "Mesh.Data round-trips the same MeshData*"
        );
        let fill = SolidColorBrush::new([1.0, 0.0, 0.0, 1.0]);
        assert!(mesh.set_brush(&fill), "set_brush should succeed");
        assert!(mesh.brush().is_some(), "Mesh.Brush set");
        drop(mesh);
        drop(md);

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
            "draw_mesh / draw_text failed to reach the DrawingContext"
        );
        assert!(
            full > baseline,
            "filled mesh produced no extra GPU batches: painter={full} baseline={baseline}"
        );
    }

    noesis_runtime::shutdown();
}

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
