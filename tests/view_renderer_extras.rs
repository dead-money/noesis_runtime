//! Integration tests for the `Rendering` per-frame event, gesture/touch
//! thresholds, stereo render paths, and `RenderDevice` offscreen/glyph-cache
//! tuning.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::render_device::types::{Batch, DeviceCaps, Tile};
use noesis_runtime::render_device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect, register,
};
use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

// A TextBlock so a render pass produces real glyph geometry (proving the view
// is alive and rendering after the threshold/stereo setters run).
const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <TextBlock Text="The quick brown fox" Foreground="White" FontSize="28"/>
</Grid>"##;

struct InMem {
    bytes: HashMap<String, Vec<u8>>,
}

impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.bytes.get(uri).map(Vec::as_slice)
    }
}

// Minimal headless RenderDevice that drops everything except monotonic handles
// and scratch buffers. The `draws` counter lets stereo render paths be
// verified to have actually issued geometry rather than no-op'd.
struct NullDevice {
    next: u64,
    vb: Vec<u8>,
    ib: Vec<u8>,
    draws: Arc<AtomicU32>,
}

impl NullDevice {
    fn new(draws: Arc<AtomicU32>) -> Self {
        Self {
            next: 1,
            vb: vec![0; 512 * 1024],
            ib: vec![0; 128 * 1024],
            draws,
        }
    }

    fn handle(&mut self) -> NonZeroU64 {
        let h = self.next;
        self.next += 1;
        NonZeroU64::new(h).expect("handles start at 1")
    }
}

impl RenderDevice for NullDevice {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn caps(&self) -> DeviceCaps {
        DeviceCaps::default()
    }
    fn create_texture(&mut self, desc: TextureDesc<'_>) -> TextureBinding {
        TextureBinding {
            handle: TextureHandle(self.handle()),
            width: desc.width,
            height: desc.height,
            has_mipmaps: desc.num_levels > 1,
            inverted: false,
            has_alpha: true,
        }
    }
    fn update_texture(&mut self, _: TextureHandle, _: u32, _: TextureRect, _: &[u8]) {}
    fn end_updating_textures(&mut self, _: &[TextureHandle]) {}
    fn drop_texture(&mut self, _: TextureHandle) {}
    fn create_render_target(&mut self, desc: RenderTargetDesc<'_>) -> RenderTargetBinding {
        RenderTargetBinding {
            handle: RenderTargetHandle(self.handle()),
            resolve_texture: TextureBinding {
                handle: TextureHandle(self.handle()),
                width: desc.width,
                height: desc.height,
                has_mipmaps: false,
                inverted: false,
                has_alpha: true,
            },
        }
    }
    fn clone_render_target(&mut self, _: &str, _: RenderTargetHandle) -> RenderTargetBinding {
        RenderTargetBinding {
            handle: RenderTargetHandle(self.handle()),
            resolve_texture: TextureBinding {
                handle: TextureHandle(self.handle()),
                width: 0,
                height: 0,
                has_mipmaps: false,
                inverted: false,
                has_alpha: true,
            },
        }
    }
    fn drop_render_target(&mut self, _: RenderTargetHandle) {}
    fn begin_offscreen_render(&mut self) {}
    fn end_offscreen_render(&mut self) {}
    fn begin_onscreen_render(&mut self) {}
    fn end_onscreen_render(&mut self) {}
    fn set_render_target(&mut self, _: RenderTargetHandle) {}
    fn begin_tile(&mut self, _: RenderTargetHandle, _: Tile) {}
    fn end_tile(&mut self, _: RenderTargetHandle) {}
    fn resolve_render_target(&mut self, _: RenderTargetHandle, _: &[Tile]) {}
    fn map_vertices(&mut self, bytes: u32) -> &mut [u8] {
        &mut self.vb[..bytes as usize]
    }
    fn unmap_vertices(&mut self) {}
    fn map_indices(&mut self, bytes: u32) -> &mut [u8] {
        &mut self.ib[..bytes as usize]
    }
    fn unmap_indices(&mut self) {}
    fn draw_batch(&mut self, _: &Batch) {
        self.draws.fetch_add(1, Ordering::SeqCst);
    }
}

// With an identity projection any eye matrix is trivially "enclosed",
// satisfying RenderStereo's culling precondition.
const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0,
];

#[test]
#[allow(clippy::too_many_lines)]
fn rendering_event_thresholds_stereo_and_device_tuning() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let render_ticks = Arc::new(AtomicU32::new(0));
    let draws = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("ui.xaml".to_string(), XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        // Defaults are version-dependent (e.g. SDK ships a 2048 glyph cache,
        // not the 1024 the header suggests) so we print but do not assert them;
        // we only assert writes made with values distinct from any plausible default.
        let mut device = register(NullDevice::new(Arc::clone(&draws)));
        eprintln!(
            "RenderDevice defaults: offscreen {}x{} samples={} default_surf={} max_surf={} glyph {}x{}",
            device.offscreen_width(),
            device.offscreen_height(),
            device.offscreen_sample_count(),
            device.offscreen_default_num_surfaces(),
            device.offscreen_max_num_surfaces(),
            device.glyph_cache_width(),
            device.glyph_cache_height(),
        );

        device.set_offscreen_width(512);
        device.set_offscreen_height(256);
        device.set_offscreen_sample_count(4);
        device.set_offscreen_default_num_surfaces(3);
        device.set_offscreen_max_num_surfaces(7);
        device.set_glyph_cache_width(768);
        device.set_glyph_cache_height(1536);

        assert_eq!(device.offscreen_width(), 512, "offscreen width round-trip");
        assert_eq!(
            device.offscreen_height(),
            256,
            "offscreen height round-trip"
        );
        assert_eq!(
            device.offscreen_sample_count(),
            4,
            "offscreen sample count round-trip"
        );
        assert_eq!(
            device.offscreen_default_num_surfaces(),
            3,
            "offscreen default-num-surfaces round-trip"
        );
        assert_eq!(
            device.offscreen_max_num_surfaces(),
            7,
            "offscreen max-num-surfaces round-trip"
        );
        assert_eq!(
            device.glyph_cache_width(),
            768,
            "glyph-cache width round-trip"
        );
        assert_eq!(
            device.glyph_cache_height(),
            1536,
            "glyph-cache height round-trip"
        );

        // Restore sane sizes for the real render pass below.
        device.set_offscreen_width(0);
        device.set_offscreen_height(0);
        device.set_offscreen_sample_count(1);
        device.set_glyph_cache_width(1024);
        device.set_glyph_cache_height(1024);

        let element = FrameworkElement::load("ui.xaml").expect("ui.xaml load failed");
        let mut view = View::create(element);
        view.set_size(200, 200);
        // Identity projection: eye matrices must be enclosed by the view projection;
        // identity makes that trivially true.
        view.set_projection_matrix(&IDENTITY);
        view.activate();
        assert!(view.update(0.0), "first Update should report change");

        // These threshold setters have no SDK getter; correctness is confirmed
        // by the view remaining healthy through the render pass at the end.
        view.set_holding_time_threshold(750);
        view.set_holding_distance_threshold(20);
        view.set_manipulation_distance_threshold(15);
        view.set_double_tap_time_threshold(400);
        view.set_double_tap_distance_threshold(12);
        view.set_emulate_touch(true);
        // EmulateTouch makes the mouse drive touch input.
        let _ = view.mouse_move(100, 100);
        view.update(0.05);
        let _ = view.mouse_button_down(100, 100, noesis_runtime::view::MouseButton::Left);
        let _ = view.mouse_button_up(100, 100, noesis_runtime::view::MouseButton::Left);
        view.set_emulate_touch(false);

        // Getter-less; the stereo path below will run with this factor in effect.
        view.set_stereo_offscreen_scale_factor(2.5);

        let rt = Arc::clone(&render_ticks);
        let sub = view
            .add_rendering_handler(move || {
                rt.fetch_add(1, Ordering::SeqCst);
            })
            .expect("add_rendering_handler returned None");

        {
            let mut renderer = view.renderer();
            renderer.init(&device);
        }

        // Drive several full frames to let the Rendering event fire.
        let mut t = 0.1_f64;
        for _ in 0..5 {
            view.update(t);
            {
                let mut renderer = view.renderer();
                renderer.update_render_tree();
                renderer.render_offscreen();
                renderer.render(false, true);
            }
            t += 0.05;
        }

        let fired = render_ticks.load(Ordering::SeqCst);
        assert!(
            fired > 0,
            "Rendering handler should fire during the render frames, got {fired}"
        );

        drop(sub);
        for _ in 0..5 {
            view.update(t);
            {
                let mut renderer = view.renderer();
                renderer.update_render_tree();
                renderer.render_offscreen();
                renderer.render(false, true);
            }
            t += 0.05;
        }
        assert_eq!(
            render_ticks.load(Ordering::SeqCst),
            fired,
            "Rendering handler must stop firing after its subscription is dropped"
        );

        // Count draw_batch calls across the stereo block to prove the entrypoints
        // drove the device to actual geometry rather than no-op'd.
        view.update(t);
        let draws_before_stereo = draws.load(Ordering::SeqCst);
        {
            let mut renderer = view.renderer();
            renderer.update_render_tree();
            renderer.render_offscreen();
            renderer.render_stereo(&IDENTITY, false, true);
            renderer.render_stereo_both(&IDENTITY, &IDENTITY, false, false);
        }
        let draws_after_stereo = draws.load(Ordering::SeqCst);
        eprintln!(
            "draw_batch calls during stereo block: {draws_before_stereo} -> {draws_after_stereo}"
        );
        assert!(
            draws_after_stereo > draws_before_stereo,
            "RenderStereo should have issued draw batches ({draws_before_stereo} -> {draws_after_stereo})"
        );
        t += 0.05;

        view.update(t);
        {
            let mut renderer = view.renderer();
            renderer.update_render_tree();
            renderer.render_offscreen();
            renderer.render(false, true);
        }
        let stats = view.stats();
        eprintln!("ViewStats after stereo + render: {stats:?}");
        assert!(
            stats.triangles > 0,
            "the glyph content should still render after the stereo path, got {} triangles",
            stats.triangles
        );

        {
            let mut renderer = view.renderer();
            renderer.shutdown();
        }
        view.deactivate();
        drop(view);
        drop(device);
    }

    noesis_runtime::shutdown();
}
