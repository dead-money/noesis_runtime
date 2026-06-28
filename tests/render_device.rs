//! Phase 1 integration test: drives the C++ `RustRenderDevice` subclass
//! through one representative frame via the test-only entrypoint
//! `dm_noesis_test_run_frame_scenario`, with a [`MockDevice`] on the Rust
//! side recording every virtual call. Asserts the recorded sequence matches
//! the expected ordering verbatim (including the destructor-driven
//! `drop_texture` / `drop_render_target` callbacks at scenario exit).
//!
//! Requires the `test-utils` Cargo feature:
//!
//! ```sh
//! NOESIS_SDK_DIR=~/deadmoney/sdk/noesis-3.2.12 \
//!   cargo test --features test-utils --test render_device
//! ```

#![cfg(feature = "test-utils")]

use std::num::NonZeroU64;
use std::sync::{Arc, Mutex};

use dm_noesis_runtime::render_device::ffi::dm_noesis_test_run_frame_scenario;
use dm_noesis_runtime::render_device::types::{Batch, DeviceCaps, Shader, TextureFormat, Tile};
use dm_noesis_runtime::render_device::{
    RenderDevice, RenderTargetBinding, RenderTargetDesc, RenderTargetHandle, TextureBinding,
    TextureDesc, TextureHandle, TextureRect, register,
};

// ────────────────────────────────────────────────────────────────────────────
// Recorded call type — one variant per Noesis virtual the device implements.
// Compared verbatim against the expected sequence below.
// ────────────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum Op {
    GetCaps,
    CreateTexture {
        label: String,
        width: u32,
        height: u32,
        num_levels: u32,
        format: TextureFormat,
        has_init_data: bool,
    },
    UpdateTexture {
        handle: u64,
        level: u32,
        rect: TextureRect,
        bytes_len: usize,
    },
    EndUpdatingTextures(Vec<u64>),
    DropTexture(u64),
    CreateRenderTarget {
        label: String,
        width: u32,
        height: u32,
        sample_count: u32,
        needs_stencil: bool,
    },
    CloneRenderTarget {
        label: String,
        src: u64,
    },
    DropRenderTarget(u64),
    BeginOffscreenRender,
    EndOffscreenRender,
    BeginOnscreenRender,
    EndOnscreenRender,
    SetRenderTarget(u64),
    BeginTile {
        handle: u64,
        tile: Tile,
    },
    EndTile(u64),
    ResolveRenderTarget {
        handle: u64,
        tiles: Vec<Tile>,
    },
    MapVertices(u32),
    UnmapVertices,
    MapIndices(u32),
    UnmapIndices,
    DrawBatch {
        shader: u8,
        num_vertices: u32,
        num_indices: u32,
    },
}

// ────────────────────────────────────────────────────────────────────────────
// MockDevice — RenderDevice impl that records into a shared op log and hands
// out monotonically-increasing handle IDs (so the assertion can pin specific
// handle values without coupling to the Mock's internal state).
// ────────────────────────────────────────────────────────────────────────────

struct MockDevice {
    ops: Arc<Mutex<Vec<Op>>>,
    next_handle: u64,
    vb: Vec<u8>,
    ib: Vec<u8>,
}

impl MockDevice {
    fn new(ops: Arc<Mutex<Vec<Op>>>) -> Self {
        Self {
            ops,
            next_handle: 1,
            // DYNAMIC_VB_SIZE / DYNAMIC_IB_SIZE from RenderDevice.h.
            vb: vec![0; 512 * 1024],
            ib: vec![0; 128 * 1024],
        }
    }

    fn alloc_handle(&mut self) -> NonZeroU64 {
        let h = self.next_handle;
        self.next_handle += 1;
        NonZeroU64::new(h).expect("alloc_handle starts at 1")
    }

    fn push(&self, op: Op) {
        self.ops.lock().expect("ops poisoned").push(op);
    }
}

impl RenderDevice for MockDevice {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn caps(&self) -> DeviceCaps {
        self.push(Op::GetCaps);
        DeviceCaps::default()
    }

    fn create_texture(&mut self, desc: TextureDesc<'_>) -> TextureBinding {
        self.push(Op::CreateTexture {
            label: desc.label.to_owned(),
            width: desc.width,
            height: desc.height,
            num_levels: desc.num_levels,
            format: desc.format,
            has_init_data: desc.data.is_some(),
        });
        TextureBinding {
            handle: TextureHandle(self.alloc_handle()),
            width: desc.width,
            height: desc.height,
            has_mipmaps: desc.num_levels > 1,
            inverted: false,
            has_alpha: !matches!(desc.format, TextureFormat::Rgbx8),
        }
    }

    fn update_texture(
        &mut self,
        handle: TextureHandle,
        level: u32,
        rect: TextureRect,
        data: &[u8],
    ) {
        self.push(Op::UpdateTexture {
            handle: handle.0.get(),
            level,
            rect,
            bytes_len: data.len(),
        });
    }

    fn end_updating_textures(&mut self, textures: &[TextureHandle]) {
        self.push(Op::EndUpdatingTextures(
            textures.iter().map(|h| h.0.get()).collect(),
        ));
    }

    fn drop_texture(&mut self, handle: TextureHandle) {
        self.push(Op::DropTexture(handle.0.get()));
    }

    fn create_render_target(&mut self, desc: RenderTargetDesc<'_>) -> RenderTargetBinding {
        self.push(Op::CreateRenderTarget {
            label: desc.label.to_owned(),
            width: desc.width,
            height: desc.height,
            sample_count: desc.sample_count,
            needs_stencil: desc.needs_stencil,
        });
        let rt_handle = RenderTargetHandle(self.alloc_handle());
        let tex_handle = TextureHandle(self.alloc_handle());
        RenderTargetBinding {
            handle: rt_handle,
            resolve_texture: TextureBinding {
                handle: tex_handle,
                width: desc.width,
                height: desc.height,
                has_mipmaps: false,
                inverted: false,
                has_alpha: true,
            },
        }
    }

    fn clone_render_target(&mut self, label: &str, src: RenderTargetHandle) -> RenderTargetBinding {
        self.push(Op::CloneRenderTarget {
            label: label.to_owned(),
            src: src.0.get(),
        });
        let rt_handle = RenderTargetHandle(self.alloc_handle());
        let tex_handle = TextureHandle(self.alloc_handle());
        RenderTargetBinding {
            handle: rt_handle,
            resolve_texture: TextureBinding {
                handle: tex_handle,
                width: 0,
                height: 0,
                has_mipmaps: false,
                inverted: false,
                has_alpha: true,
            },
        }
    }

    fn drop_render_target(&mut self, handle: RenderTargetHandle) {
        self.push(Op::DropRenderTarget(handle.0.get()));
    }

    fn begin_offscreen_render(&mut self) {
        self.push(Op::BeginOffscreenRender);
    }
    fn end_offscreen_render(&mut self) {
        self.push(Op::EndOffscreenRender);
    }
    fn begin_onscreen_render(&mut self) {
        self.push(Op::BeginOnscreenRender);
    }
    fn end_onscreen_render(&mut self) {
        self.push(Op::EndOnscreenRender);
    }

    fn set_render_target(&mut self, handle: RenderTargetHandle) {
        self.push(Op::SetRenderTarget(handle.0.get()));
    }

    fn begin_tile(&mut self, handle: RenderTargetHandle, tile: Tile) {
        self.push(Op::BeginTile {
            handle: handle.0.get(),
            tile,
        });
    }

    fn end_tile(&mut self, handle: RenderTargetHandle) {
        self.push(Op::EndTile(handle.0.get()));
    }

    fn resolve_render_target(&mut self, handle: RenderTargetHandle, tiles: &[Tile]) {
        self.push(Op::ResolveRenderTarget {
            handle: handle.0.get(),
            tiles: tiles.to_vec(),
        });
    }

    fn map_vertices(&mut self, bytes: u32) -> &mut [u8] {
        self.push(Op::MapVertices(bytes));
        &mut self.vb[..bytes as usize]
    }
    fn unmap_vertices(&mut self) {
        self.push(Op::UnmapVertices);
    }
    fn map_indices(&mut self, bytes: u32) -> &mut [u8] {
        self.push(Op::MapIndices(bytes));
        &mut self.ib[..bytes as usize]
    }
    fn unmap_indices(&mut self) {
        self.push(Op::UnmapIndices);
    }

    fn draw_batch(&mut self, batch: &Batch) {
        self.push(Op::DrawBatch {
            shader: batch.shader.0,
            num_vertices: batch.num_vertices,
            num_indices: batch.num_indices,
        });
    }
}

// ────────────────────────────────────────────────────────────────────────────
// The test
// ────────────────────────────────────────────────────────────────────────────

#[test]
#[allow(clippy::too_many_lines)] // expected-op vec dominates the line count
fn frame_scenario_records_expected_op_sequence() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let log: Arc<Mutex<Vec<Op>>> = Arc::new(Mutex::new(Vec::new()));
    let registered = register(MockDevice::new(log.clone()));

    // SAFETY: registered.raw() points to a live Noesis::RenderDevice* that
    // remains valid until `drop(registered)` below.
    unsafe { dm_noesis_test_run_frame_scenario(registered.raw()) };

    // Tear down the device — but the scenario already dropped its Ptr<>s, so
    // this just releases our +1 reference and finalises the C++ instance.
    drop(registered);

    let ops = log.lock().expect("ops poisoned");

    // Handle allocation order in MockDevice (each `create_*` bumps `next_handle`):
    //   1 = t_immutable
    //   2 = t_dynamic
    //   3 = rt_main           4 = rt_main resolve texture
    //   5 = rt_clone          6 = rt_clone resolve texture
    //
    // Drop order at scenario exit (reverse declaration in the C++ scenario,
    // and within each render target: outer RT first, then its Ptr<RustTexture>
    // resolve member):
    //   rt_clone     → drop_render_target(5), drop_texture(6)
    //   rt_main      → drop_render_target(3), drop_texture(4)
    //   t_dynamic    → drop_texture(2)
    //   t_immutable  → drop_texture(1)
    let full_tile = Tile {
        x: 0,
        y: 0,
        width: 256,
        height: 256,
    };
    let expected: Vec<Op> = vec![
        Op::GetCaps,
        Op::CreateTexture {
            label: "t_immutable".to_owned(),
            width: 4,
            height: 4,
            num_levels: 1,
            format: TextureFormat::Rgba8,
            has_init_data: true,
        },
        Op::CreateTexture {
            label: "t_dynamic".to_owned(),
            width: 16,
            height: 16,
            num_levels: 1,
            format: TextureFormat::R8,
            has_init_data: false,
        },
        Op::UpdateTexture {
            handle: 2,
            level: 0,
            rect: TextureRect {
                x: 2,
                y: 2,
                width: 4,
                height: 4,
            },
            bytes_len: 16,
        },
        Op::EndUpdatingTextures(vec![2]),
        Op::CreateRenderTarget {
            label: "rt_main".to_owned(),
            width: 256,
            height: 256,
            sample_count: 1,
            needs_stencil: true,
        },
        Op::BeginOffscreenRender,
        Op::SetRenderTarget(3),
        Op::BeginTile {
            handle: 3,
            tile: full_tile,
        },
        Op::MapVertices(96),
        Op::UnmapVertices,
        Op::MapIndices(36),
        Op::UnmapIndices,
        Op::DrawBatch {
            shader: Shader::PATH_SOLID.0,
            num_vertices: 4,
            num_indices: 6,
        },
        Op::EndTile(3),
        Op::ResolveRenderTarget {
            handle: 3,
            tiles: vec![full_tile],
        },
        Op::EndOffscreenRender,
        Op::BeginOnscreenRender,
        Op::MapVertices(96),
        Op::UnmapVertices,
        Op::MapIndices(36),
        Op::UnmapIndices,
        Op::DrawBatch {
            shader: Shader::RGBA.0,
            num_vertices: 4,
            num_indices: 6,
        },
        Op::EndOnscreenRender,
        Op::CloneRenderTarget {
            label: "rt_clone".to_owned(),
            src: 3,
        },
        Op::DropRenderTarget(5),
        Op::DropTexture(6),
        Op::DropRenderTarget(3),
        Op::DropTexture(4),
        Op::DropTexture(2),
        Op::DropTexture(1),
    ];

    assert_eq!(
        ops.len(),
        expected.len(),
        "op count mismatch\n  actual ({}): {:#?}\n  expected ({}): {:#?}",
        ops.len(),
        &*ops,
        expected.len(),
        expected,
    );
    for (i, (got, want)) in ops.iter().zip(expected.iter()).enumerate() {
        assert_eq!(got, want, "op {i} mismatch");
    }

    drop(ops);
    dm_noesis_runtime::shutdown();
}
