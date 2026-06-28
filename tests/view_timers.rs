//! Integration tests for view timers, typed `RenderFlags` + `GetFlags`,
//! `ViewStats`, tessellation quality, and `MouseHWheel`.

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::render_device::types::{Batch, DeviceCaps, Tile};
use noesis_runtime::render_device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect, register,
};
use noesis_runtime::view::{FrameworkElement, Quality, RenderFlag, RenderFlags, View};
use noesis_runtime::xaml_provider::XamlProvider;

// ScrollViewer with overflowing content so MouseHWheel has something to handle
// and glyph geometry appears in ViewStats.
const SCROLL_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<ScrollViewer xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      x:Name="Scroller"
      HorizontalScrollBarVisibility="Auto" VerticalScrollBarVisibility="Disabled"
      Background="#FF202020" Width="200" Height="200">
  <StackPanel Orientation="Horizontal">
    <TextBlock Text="The quick brown fox jumps over the lazy dog" Foreground="White"
               FontSize="32" Width="1200"/>
  </StackPanel>
</ScrollViewer>"##;

// No scrollable surface. The negative control for the MouseHWheel assertion.
const GRID_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <TextBlock Text="hi" Foreground="White" FontSize="20"/>
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

// Minimal headless RenderDevice: hands out monotonic handles and scratch
// buffers, dropping everything else. Enough to drive a real Noesis render
// pass (which fills ViewStats) without a GPU.
struct NullDevice {
    next: u64,
    vb: Vec<u8>,
    ib: Vec<u8>,
}

impl NullDevice {
    fn new() -> Self {
        Self {
            next: 1,
            // Sizes from DYNAMIC_VB_SIZE / DYNAMIC_IB_SIZE in RenderDevice.h.
            vb: vec![0; 512 * 1024],
            ib: vec![0; 128 * 1024],
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
    fn draw_batch(&mut self, _: &Batch) {}
}

#[test]
#[allow(clippy::too_many_lines)]
fn view_timers_flags_stats_quality_hwheel() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let const_ticks = Arc::new(AtomicU32::new(0));
    let once_ticks = Arc::new(AtomicU32::new(0));
    let cancelled_ticks = Arc::new(AtomicU32::new(0));
    let restart_ticks = Arc::new(AtomicU32::new(0));

    {
        // Every owning wrapper must drop before shutdown().
        let mut bytes = HashMap::new();
        bytes.insert("scroll.xaml".to_string(), SCROLL_XAML.as_bytes().to_vec());
        bytes.insert("grid.xaml".to_string(), GRID_XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        {
            let grid = FrameworkElement::load("grid.xaml").expect("grid.xaml load failed");
            let mut grid_view = View::create(grid);
            grid_view.set_size(200, 200);
            grid_view.activate();
            let mut t = 0.0;
            for _ in 0..3 {
                grid_view.update(t);
                t += 0.05;
            }
            let _ = grid_view.mouse_move(100, 100);
            grid_view.update(t);
            assert!(
                !grid_view.mouse_hwheel(100, 100, 120),
                "MouseHWheel over a non-scrollable Grid should not be handled"
            );
            grid_view.deactivate();
            drop(grid_view);
        }

        let element =
            FrameworkElement::load("scroll.xaml").expect("load_xaml returned None for scroll.xaml");

        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        // First update establishes the origin time the timer clock counts from.
        assert!(view.update(0.0), "first Update should report change");

        let flags = RenderFlags::from_iter([
            RenderFlag::Ppaa,
            RenderFlag::Wireframe,
            RenderFlag::DepthTesting,
        ]);
        view.set_render_flags(flags);
        assert_eq!(view.get_flags(), flags.bits(), "raw GetFlags should match");
        assert_eq!(view.flags(), flags, "typed flags should round-trip");
        assert!(view.flags().contains(RenderFlag::Ppaa));
        assert!(view.flags().contains(RenderFlag::Wireframe));
        assert!(
            !view.flags().contains(RenderFlag::Overdraw),
            "an unset flag must not be reported present"
        );
        let built = RenderFlags::empty()
            .with(RenderFlag::Ppaa)
            .with(RenderFlag::Wireframe)
            .with(RenderFlag::DepthTesting);
        assert_eq!(built, flags);
        view.set_flags(0);
        assert_eq!(view.get_flags(), 0, "flags should clear to 0");

        view.set_quality(Quality::High);
        assert!(
            (view.tessellation_max_pixel_error() - 0.2).abs() < 1e-4,
            "High quality should map to ~0.2 px error, got {}",
            view.tessellation_max_pixel_error()
        );
        view.set_quality(Quality::Low);
        assert!(
            (view.tessellation_max_pixel_error() - 0.7).abs() < 1e-4,
            "Low quality should map to ~0.7 px error"
        );
        view.set_tessellation_max_pixel_error(0.55);
        assert!(
            (view.tessellation_max_pixel_error() - 0.55).abs() < 1e-4,
            "the raw setter should round-trip"
        );
        // Reset to the default before the render pass below.
        view.set_quality(Quality::Medium);

        let c = Arc::clone(&const_ticks);
        let sub_const = view
            .create_timer(16, move || {
                c.fetch_add(1, Ordering::SeqCst);
                16
            })
            .expect("create_timer returned None");

        // Returning 0 from the callback stops the timer.
        let o = Arc::clone(&once_ticks);
        let _sub_once = view
            .create_timer(10, move || {
                o.fetch_add(1, Ordering::SeqCst);
                0
            })
            .expect("create_timer returned None");

        let cc = Arc::clone(&cancelled_ticks);
        let sub_cancel = view
            .create_timer(10, move || {
                cc.fetch_add(1, Ordering::SeqCst);
                10
            })
            .expect("create_timer returned None");
        drop(sub_cancel);

        // 50 ms steps cover both the 16 ms and 10 ms intervals each iteration.
        let mut t = 0.0_f64;
        for _ in 0..20 {
            t += 0.05;
            view.update(t);
        }

        let after_pump = const_ticks.load(Ordering::SeqCst);
        assert!(
            after_pump >= 5,
            "constant-cadence timer should have fired several times, got {after_pump}"
        );
        assert_eq!(
            once_ticks.load(Ordering::SeqCst),
            1,
            "zero-return timer must fire exactly once then stop"
        );
        assert_eq!(
            cancelled_ticks.load(Ordering::SeqCst),
            0,
            "a timer cancelled before any update must never fire"
        );

        // Necessary but not sufficient: the 16 ms interval already fires once
        // per 50 ms step, so this alone cannot distinguish a working restart
        // from a no-op. See the huge->short restart below for the real proof.
        sub_const.restart(8);
        for _ in 0..10 {
            t += 0.05;
            view.update(t);
        }
        assert!(
            const_ticks.load(Ordering::SeqCst) > after_pump,
            "timer should keep firing after restart()"
        );

        // A no-op restart() leaves the ~10000 s interval in place, so the
        // counter stays 0; this assertion fails iff RestartTimer did not cross
        // into IView.
        let r = Arc::clone(&restart_ticks);
        let sub_restart = view
            .create_timer(10_000_000, move || {
                r.fetch_add(1, Ordering::SeqCst);
                10_000_000
            })
            .expect("create_timer returned None");
        for _ in 0..10 {
            t += 0.05;
            view.update(t);
        }
        assert_eq!(
            restart_ticks.load(Ordering::SeqCst),
            0,
            "huge-interval timer must not fire before restart"
        );
        sub_restart.restart(10);
        for _ in 0..10 {
            t += 0.05;
            view.update(t);
        }
        assert!(
            restart_ticks.load(Ordering::SeqCst) > 0,
            "timer must fire after restart() shortens the interval"
        );

        let _ = view.mouse_move(100, 100);
        t += 0.05;
        view.update(t);
        assert!(
            view.mouse_hwheel(100, 100, 120),
            "MouseHWheel over a horizontally-scrollable ScrollViewer should be handled"
        );

        let device = register(NullDevice::new());
        {
            let mut renderer = view.renderer();
            renderer.init(&device);
        }
        t += 0.05;
        view.update(t);
        {
            let mut renderer = view.renderer();
            renderer.update_render_tree();
            renderer.render_offscreen();
            renderer.render(false, true);
        }

        let stats = view.stats();
        eprintln!("ViewStats after render: {stats:?}");
        assert!(
            stats.triangles > 0,
            "a rendered frame with glyph content should report triangles, got {}",
            stats.triangles
        );
        assert!(
            stats.batches > 0,
            "a rendered frame should report at least one batch, got {}",
            stats.batches
        );

        {
            let mut renderer = view.renderer();
            renderer.shutdown();
        }

        drop(sub_const);
        view.deactivate();
        drop(view);
        drop(device);
    }

    noesis_runtime::shutdown();
}
