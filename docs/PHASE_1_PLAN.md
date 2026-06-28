# Phase 1 — Render Device Skeleton

## Goal

Implement enough of the `Noesis::RenderDevice` C++ contract to satisfy every pure virtual via Rust trait dispatch. **No real GPU work in this phase** — `DrawBatch` is a no-op, `MapVertices`/`MapIndices` return scratch buffers, textures are opaque handles. Phase 2 introduces wgpu.

## Success criteria

- `cargo test -p dm_noesis_runtime --test render_device` passes.
- A `MockDevice` records every virtual the C++ subclass dispatches to, and the recorded op sequence matches an asserted ordering.
- No use of Noesis's `IView` / `IRenderer` in this phase — driving the device into a real Noesis pipeline is Phase 4.

## Surface to mirror (from `$NOESIS_SDK_DIR/Include/NsRender/RenderDevice.h`)

**Enums** (`#[repr(u32)]`, layout-checked with `static_assertions`):
- `TextureFormat::Enum` — RGBA8, RGBX8, R8.
- `Shader::Enum` — ~80 variants (RGBA, Mask, Clear, Path_*, Path_AA_*, SDF_*, SDF_LCD_*, Opacity_*, Upsample, Downsample, Shadow, Blur, Custom_Effect).
- `Shader::Vertex::Enum` — 21 vertex-shader variants.
- `Shader::Vertex::Format::Enum` — 16 vertex-format variants (semantic only; per-format byte layouts live in `GLRenderDevice` and become Phase 3 work).
- `WrapMode::Enum`, `MinMagFilter::Enum`, `MipFilter::Enum`.
- `BlendMode::Enum`, `StencilMode::Enum` (referenced by `IsValidBlendMode` / `IsValidStencilMode` validation helpers; declarations live in `RenderState.h` or similar — locate first, see Open Questions).

**Structs** (`#[repr(C)]`, must be binary-compatible — C++ passes some by reference):
- `DeviceCaps` — 4 bools + 1 float.
- `SamplerState` — 1-byte bitfield union.
- `RenderState` — TBD; flags for blend / stencil / color-write / wireframe.
- `UniformData` — `{ *const c_void, u32, u32 }`.
- `Tile` — 4 × `u32`.
- `Batch` — shader + render state + 1 byte stencil ref + bool single-pass-stereo + 4 × draw range u32 + 5 × `Texture*` + 5 × `SamplerState` + 4 × `UniformData` + `void* pixelShader`. Pointer fields stay raw.

**Pure virtuals** to override (each gets a vtable fn pointer + a `dm_noesis_test_*` entrypoint for verification):
- `GetCaps`
- `CreateRenderTarget`, `CloneRenderTarget`
- `CreateTexture`, `UpdateTexture`, `EndUpdatingTextures`
- `BeginOffscreenRender`, `EndOffscreenRender`, `BeginOnscreenRender`, `EndOnscreenRender`
- `SetRenderTarget`, `BeginTile`, `EndTile`, `ResolveRenderTarget`
- `MapVertices`, `UnmapVertices`, `MapIndices`, `UnmapIndices`
- `DrawBatch`

The base class's non-virtual setters (`SetOffscreenWidth` etc.) and the `DeviceDestroyed` delegate are inherited as-is; we don't override them.

## C++ shim design (`dm_noesis_runtime/cpp/`)

Three new subclasses. Each stores its vtable + a `void* userdata` that points back to the Rust resource:

### `RustRenderDevice : public Noesis::RenderDevice`

Members: `RustRenderDeviceVTable vtable; void* userdata;`. Every pure-virtual override forwards to the corresponding fn pointer. `CreateTexture` / `CreateRenderTarget` instantiate `RustTexture` / `RustRenderTarget` from the Rust-returned descriptions and return them as `Ptr<…>`.

### `RustTexture : public Noesis::Texture`

Stores width / height / has_mipmaps / inverted / has_alpha as fields (the const-getter virtuals just return them). Stores `void* handle` + `RustTextureDropFn drop_fn` + `void* userdata`. Destructor calls `drop_fn(userdata, handle)`.

### `RustRenderTarget : public Noesis::RenderTarget`

Stores `Ptr<RustTexture> resolve_texture` + `void* handle` + `RustRtDropFn drop_fn` + `void* userdata`. `GetTexture()` returns `resolve_texture.GetPtr()`.

### Factory C ABI (added to `cpp/noesis_shim.h`)

```c
void* dm_noesis_render_device_create(const dm_noesis_render_device_vtable* vtable, void* userdata);
void  dm_noesis_render_device_destroy(void* device);
```

The returned `void*` is a raw `Noesis::RenderDevice*`. Phase 4 hands it to `IView::GetRenderer()->Init(device)`.

## Rust API (`dm_noesis_runtime/src/render_device/`)

```text
src/render_device/
    mod.rs        // pub use, RenderDevice trait, register()
    types.rs      // #[repr(C)] mirrors of Noesis enums/structs
    vtable.rs     // RustRenderDeviceVTable + trampoline fns
    handle.rs     // TextureHandle, RenderTargetHandle (NonZeroU64 newtypes)
```

The trait (sketch):

```rust
pub trait RenderDevice {
    fn caps(&self) -> DeviceCaps;
    fn create_texture(&mut self, desc: TextureDesc) -> TextureBinding;
    fn update_texture(&mut self, h: TextureHandle, level: u32, rect: TextureRect, data: &[u8]);
    fn end_updating_textures(&mut self, textures: &[TextureHandle]);
    fn create_render_target(&mut self, desc: RenderTargetDesc) -> RenderTargetBinding;
    fn clone_render_target(&mut self, label: &str, src: RenderTargetHandle) -> RenderTargetBinding;
    fn drop_texture(&mut self, h: TextureHandle);
    fn drop_render_target(&mut self, h: RenderTargetHandle);
    fn begin_offscreen_render(&mut self);
    fn end_offscreen_render(&mut self);
    fn begin_onscreen_render(&mut self);
    fn end_onscreen_render(&mut self);
    fn set_render_target(&mut self, h: RenderTargetHandle);
    fn begin_tile(&mut self, h: RenderTargetHandle, tile: Tile);
    fn end_tile(&mut self, h: RenderTargetHandle);
    fn resolve_render_target(&mut self, h: RenderTargetHandle, tiles: &[Tile]);
    fn map_vertices(&mut self, bytes: u32) -> &mut [u8];
    fn unmap_vertices(&mut self);
    fn map_indices(&mut self, bytes: u32) -> &mut [u8];
    fn unmap_indices(&mut self);
    fn draw_batch(&mut self, batch: &Batch);
}

pub struct Registered { /* owns boxed device + C++ handle, drops both */ }
pub fn register<D: RenderDevice + 'static>(device: D) -> Registered;
```

`TextureBinding` carries the Rust handle plus the metadata the C++ wrapper needs to satisfy its const-getters (width, height, has_mipmaps, inverted, has_alpha). Same shape for `RenderTargetBinding`.

## Verification strategy

`tests/render_device.rs`:

```rust
#[derive(Default)]
struct MockDevice { ops: Vec<Op>, next_id: u64, vb: Vec<u8>, ib: Vec<u8> }

#[derive(Debug, PartialEq)]
enum Op {
    GetCaps,
    CreateTexture { label: String, w: u32, h: u32, levels: u32, format: TextureFormat },
    UpdateTexture { handle: u64, level: u32, rect: (u32,u32,u32,u32), bytes: usize },
    EndUpdatingTextures(Vec<u64>),
    CreateRenderTarget { label: String, w: u32, h: u32, samples: u32, stencil: bool },
    BeginOffscreenRender, EndOffscreenRender,
    BeginOnscreenRender,  EndOnscreenRender,
    SetRenderTarget(u64), BeginTile(u64, Tile), EndTile(u64),
    ResolveRenderTarget(u64, Vec<Tile>),
    MapVertices(u32), UnmapVertices,
    MapIndices(u32),  UnmapIndices,
    DrawBatch { shader: Shader, num_indices: u32 },
    DropTexture(u64), DropRenderTarget(u64),
}

impl RenderDevice for MockDevice { /* each method records its Op */ }
```

Test-only C entrypoints in the shim, one per virtual, gated by Cargo feature `test-utils`:

```c
void  dm_noesis_test_get_caps(void* device, dm_noesis_device_caps* out);
void* dm_noesis_test_create_texture(void* device, const char* label, uint32_t w, uint32_t h,
                                    uint32_t levels, dm_noesis_texture_format format);
/* ... one per virtual ... */
```

The test exercises a representative frame:

```text
GetCaps
→ CreateTexture (immutable, w=64, h=64, levels=1, RGBA8) → t1
→ CreateTexture (dynamic, w=256, h=256, levels=1, R8)    → t2
→ UpdateTexture(t2, ...)
→ EndUpdatingTextures([t2])
→ CreateRenderTarget (w=512, h=512, samples=1, stencil=true) → r1
→ BeginOffscreenRender
  → SetRenderTarget(r1)
  → BeginTile(r1, full)
    → MapVertices(96) → write → UnmapVertices
    → MapIndices(36)  → write → UnmapIndices
    → DrawBatch { shader: Path_Solid, num_indices: 6 }
  → EndTile(r1)
  → ResolveRenderTarget(r1, [full])
→ EndOffscreenRender
→ BeginOnscreenRender → MapVertices/Indices → DrawBatch → EndOnscreenRender
→ Drop everything (triggers DropTexture / DropRenderTarget callbacks)
```

Asserts the recorded `ops` match this script verbatim.

## Sub-phase sequencing

Each row is a single PR-sized commit. Order chosen so each step compiles without the next.

- **P1.1 — Enum mirrors.** `TextureFormat`, `WrapMode`, `MinMagFilter`, `MipFilter`, `Shader`, `Shader::Vertex`, `Shader::Vertex::Format`, `SamplerState`, `DeviceCaps`, `Tile`, `UniformData`. Compile-time size assertions.
- **P1.2 — `RenderState` + `Batch`.** First locate the `RenderState` / `BlendMode` / `StencilMode` declarations (likely a sibling header — scan first). Then mirror.
- **P1.3 — `RenderDevice` trait + handle / desc / binding types.** Pure Rust; no FFI yet.
- **P1.4 — C++ subclasses + factory.** `RustRenderDevice`, `RustTexture`, `RustRenderTarget` in `cpp/noesis_render_device.{h,cpp}`. Factory functions in `noesis_shim.{h,cpp}`. Compiles against the SDK; not yet callable from Rust.
- **P1.5 — Rust trampolines + `register()`.** `RustRenderDeviceVTable` Rust mirror; trampoline fns that downcast `userdata` to `&mut dyn RenderDevice` and dispatch. `Registered` lifecycle.
- **P1.6 — `dm_noesis_test_*` entrypoints.** One per virtual, gated by `test-utils` feature so production builds don't carry them.
- **P1.7 — `MockDevice` + integration test.** Wire it all up; the asserted op sequence above lights up green.

## Open questions to resolve in flight

- **`RenderState` / `BlendMode` / `StencilMode` declarations.** Not in `RenderDevice.h`. Scan `Include/NsRender/` and `Include/NsDrawing/` first; affects P1.2 layout.
- **`MapVertices` lifetime semantics.** Buffer stays valid until `UnmapVertices`. Mock returns a slice into a `Vec<u8>` field. Phase 2's wgpu impl will need a persistently-mapped ring buffer or staged `Queue::write_buffer`; out of scope here.
- **Threading.** Header docs imply Noesis calls the device from a single render thread. We'll assume `&mut self` access is fine. Revisit if Phase 4 surfaces multi-threaded calls.
- **`Batch.pixelShader` opaque pointer.** Phase 1 round-trips it; Phase 6 (custom effects) consumes it.
- **`Ptr<T>` lifetime.** Noesis uses intrusive ref counting. The C++ subclasses inherit ref-count semantics from `BaseComponent`; destruction triggers our drop callbacks. Verify the destructor sequencing in P1.4.
- **`OnDestroy` override.** `RenderDevice` overrides `OnDestroy` from `BaseComponent`. Probably leave alone; confirm no resource-cleanup hook is needed.
