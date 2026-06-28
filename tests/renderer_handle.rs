//! TODO §1 — the owned, thread-movable [`RendererHandle`] (the render-thread /
//! UI-thread split). Proves the two properties that make it more than the
//! borrowed `Renderer<'a>`:
//!   1. it keeps the renderer alive after the `View` wrapper is dropped (its own
//!      `+1` ref on the `IView`), and
//!   2. it is `Send` and actually drives a render pass from another thread.
//!
//! Single `#[test]` per file (Noesis can't be re-init'd in a process): all work
//! happens in an inner scope so every owning wrapper drops before `shutdown()`.
//! Reuses the headless `RenderDevice` harness from the sibling view tests, with
//! a `draw_batch` counter so "rendering actually happened" is observable.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test --features test-utils --test renderer_handle -- --nocapture`

use std::collections::HashMap;
use std::num::NonZeroU64;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::render_device::types::{Batch, DeviceCaps, Tile};
use noesis_runtime::render_device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect, register,
};
use noesis_runtime::view::{FrameworkElement, RendererHandle, View};
use noesis_runtime::xaml_provider::XamlProvider;

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

// Headless RenderDevice with a draw_batch counter (so a render pass is
// observable). Matches the one in tests/view_renderer_extras.rs.
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

// Compile-time proof that the handle is `Send` (the property that lets it move
// to a render thread). If `RendererHandle` lost its `Send` impl this stops
// compiling.
fn assert_send<T: Send>() {}

#[test]
fn renderer_handle_outlives_view_and_renders_cross_thread() {
    assert_send::<RendererHandle>();

    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let draws = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("ui.xaml".to_string(), XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let device = register(NullDevice::new(Arc::clone(&draws)));

        let element = FrameworkElement::load("ui.xaml").expect("ui.xaml load failed");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        // Take the owned handle BEFORE driving frames. The IView now has two
        // refs: this handle's and the `view` wrapper's.
        let mut handle = view.renderer_handle();
        handle.renderer().init(&device);

        // Drive a few frames on the "UI thread" (here, the test thread) so a
        // snapshot exists, then render through the handle to confirm it drives a
        // real pass.
        let mut t = 0.0_f64;
        assert!(view.update(t), "first Update should report change");
        for _ in 0..3 {
            t += 0.05;
            view.update(t);
        }
        {
            let mut r = handle.renderer();
            r.update_render_tree();
            r.render_offscreen();
            r.render(false, true);
        }
        let after_main_render = draws.load(Ordering::SeqCst);
        assert!(
            after_main_render > 0,
            "rendering through the handle should issue draw batches, got {after_main_render}"
        );

        // ── (2) Cross-thread: move the handle to a render thread and render
        //        there. `update` already ran on this thread, so the snapshot is
        //        ready; the scoped thread only consumes it. Returns the handle
        //        so we keep ownership afterwards. ─────────────────────────────
        view.update(t); // fresh snapshot for the render thread to grab
        let mut handle = std::thread::scope(|s| {
            s.spawn(move || {
                let mut r = handle.renderer();
                r.update_render_tree();
                r.render_offscreen();
                r.render(false, true);
                handle
            })
            .join()
            .expect("render thread panicked")
        });
        let after_thread_render = draws.load(Ordering::SeqCst);
        assert!(
            after_thread_render > after_main_render,
            "rendering on the moved-to render thread should issue more draws \
             ({after_main_render} -> {after_thread_render})"
        );

        // ── (1) Independent lifetime: drop the `View` wrapper. The handle's own
        //        IView ref must keep the view + renderer alive, so a render still
        //        succeeds (a dangling renderer would crash / fail to draw). ────
        drop(view);
        let before_post_drop = draws.load(Ordering::SeqCst);
        {
            let mut r = handle.renderer();
            r.update_render_tree();
            r.render_offscreen();
            r.render(false, true);
        }
        let after_post_drop = draws.load(Ordering::SeqCst);
        assert!(
            after_post_drop > before_post_drop,
            "the handle must keep the renderer alive and drawable after the View \
             is dropped ({before_post_drop} -> {after_post_drop})"
        );

        // Tear down through the handle, then release it (drops the last IView
        // ref) before the device and shutdown.
        handle.renderer().shutdown();
        drop(handle);
        drop(device);
    }

    noesis_runtime::shutdown();
}
