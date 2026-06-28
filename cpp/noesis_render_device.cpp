// C++ subclasses that satisfy the Noesis pure-virtual `RenderDevice`,
// `Texture`, and `RenderTarget` contracts by trampolining into the Rust-side
// vtable supplied at construction. Plus the C-ABI factory functions the Rust
// `register()` helper calls.
//
// See ../docs/PHASE_1_PLAN.md for the design context. The C ABI surface is
// declared in noesis_shim.h.

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsRender/RenderDevice.h>
#include <NsRender/RenderTarget.h>
#include <NsRender/Texture.h>

#include <cstdint>
#include <utility>
#include <vector>

namespace {

class RustRenderDevice;

// ─── RustTexture ────────────────────────────────────────────────────────────
//
// Stores the metadata Noesis exposes through const-getters as plain members so
// the getters are zero-overhead. Holds a back-pointer to its parent device so
// the destructor can call `drop_texture`. The device outlives all textures it
// produced because Rust drops the device only AFTER dropping the
// `dm_noesis_render_device_destroy` reference, which transitively releases
// every Noesis-held `Ptr<Texture>`.

class RustTexture final : public Noesis::Texture {
public:
    RustTexture(RustRenderDevice* device, uint64_t handle,
                uint32_t width, uint32_t height,
                Noesis::TextureFormat::Enum format,
                bool has_mipmaps, bool inverted, bool has_alpha)
        : mDevice(device)
        , mHandle(handle)
        , mWidth(width)
        , mHeight(height)
        , mFormat(format)
        , mHasMipMaps(has_mipmaps)
        , mInverted(inverted)
        , mHasAlpha(has_alpha)
    {}

    ~RustTexture();

    uint32_t GetWidth() const override { return mWidth; }
    uint32_t GetHeight() const override { return mHeight; }
    bool HasMipMaps() const override { return mHasMipMaps; }
    bool IsInverted() const override { return mInverted; }
    bool HasAlpha() const override { return mHasAlpha; }

    uint64_t handle() const { return mHandle; }
    Noesis::TextureFormat::Enum format() const { return mFormat; }

private:
    RustRenderDevice* mDevice;
    uint64_t mHandle;
    uint32_t mWidth;
    uint32_t mHeight;
    Noesis::TextureFormat::Enum mFormat;
    bool mHasMipMaps;
    bool mInverted;
    bool mHasAlpha;
};

// ─── RustRenderTarget ───────────────────────────────────────────────────────
//
// Holds the resolve `RustTexture` as a `Ptr<>` so its lifetime is tied to the
// render target. `GetTexture` returns the raw pointer; Noesis treats the
// returned `Texture*` as borrowed.

class RustRenderTarget final : public Noesis::RenderTarget {
public:
    RustRenderTarget(RustRenderDevice* device, uint64_t handle,
                     Noesis::Ptr<RustTexture> resolve)
        : mDevice(device)
        , mHandle(handle)
        , mResolve(std::move(resolve))
    {}

    ~RustRenderTarget();

    Noesis::Texture* GetTexture() override { return mResolve.GetPtr(); }

    uint64_t handle() const { return mHandle; }

private:
    RustRenderDevice* mDevice;
    uint64_t mHandle;
    Noesis::Ptr<RustTexture> mResolve;
};

// ─── RustRenderDevice ───────────────────────────────────────────────────────

class RustRenderDevice final : public Noesis::RenderDevice {
public:
    RustRenderDevice(const dm_noesis_render_device_vtable* vtable, void* userdata)
        : mVtable(*vtable)
        , mUserdata(userdata)
    {}

    void dropTexture(uint64_t h) { mVtable.drop_texture(mUserdata, h); }
    void dropRenderTarget(uint64_t h) { mVtable.drop_render_target(mUserdata, h); }

    // ── RenderDevice virtuals ──────────────────────────────────────────────

    const Noesis::DeviceCaps& GetCaps() const override {
        if (!mCapsValid) {
            mVtable.get_caps(mUserdata, &mCaps);
            mCapsValid = true;
        }
        return mCaps;
    }

    Noesis::Ptr<Noesis::RenderTarget> CreateRenderTarget(
        const char* label, uint32_t width, uint32_t height,
        uint32_t sampleCount, bool needsStencil) override
    {
        dm_noesis_render_target_binding b{};
        mVtable.create_render_target(mUserdata, label, width, height,
                                     sampleCount, needsStencil, &b);
        return makeRenderTarget(b);
    }

    Noesis::Ptr<Noesis::RenderTarget> CloneRenderTarget(
        const char* label, Noesis::RenderTarget* surface) override
    {
        const auto src = static_cast<RustRenderTarget*>(surface);
        dm_noesis_render_target_binding b{};
        mVtable.clone_render_target(mUserdata, label, src->handle(), &b);
        return makeRenderTarget(b);
    }

    Noesis::Ptr<Noesis::Texture> CreateTexture(
        const char* label, uint32_t width, uint32_t height, uint32_t numLevels,
        Noesis::TextureFormat::Enum format, const void** data) override
    {
        dm_noesis_texture_binding b{};
        mVtable.create_texture(mUserdata, label, width, height, numLevels,
                               static_cast<uint32_t>(format), data, &b);
        return makeTexture(b, format);
    }

    void UpdateTexture(Noesis::Texture* texture, uint32_t level,
                       uint32_t x, uint32_t y, uint32_t width, uint32_t height,
                       const void* data) override
    {
        const auto* t = static_cast<RustTexture*>(texture);
        mVtable.update_texture(mUserdata, t->handle(), level, x, y, width, height,
                               static_cast<uint32_t>(t->format()), data);
    }

    void EndUpdatingTextures(Noesis::Texture** textures, uint32_t count) override {
        if (count == 0) return;
        std::vector<uint64_t> handles(count);
        for (uint32_t i = 0; i < count; ++i) {
            handles[i] = static_cast<RustTexture*>(textures[i])->handle();
        }
        mVtable.end_updating_textures(mUserdata, handles.data(), count);
    }

    void BeginOffscreenRender() override { mVtable.begin_offscreen_render(mUserdata); }
    void EndOffscreenRender()   override { mVtable.end_offscreen_render(mUserdata); }
    void BeginOnscreenRender()  override { mVtable.begin_onscreen_render(mUserdata); }
    void EndOnscreenRender()    override { mVtable.end_onscreen_render(mUserdata); }

    void SetRenderTarget(Noesis::RenderTarget* surface) override {
        mVtable.set_render_target(mUserdata,
            static_cast<RustRenderTarget*>(surface)->handle());
    }

    void BeginTile(Noesis::RenderTarget* surface, const Noesis::Tile& tile) override {
        mVtable.begin_tile(mUserdata,
            static_cast<RustRenderTarget*>(surface)->handle(), &tile);
    }

    void EndTile(Noesis::RenderTarget* surface) override {
        mVtable.end_tile(mUserdata,
            static_cast<RustRenderTarget*>(surface)->handle());
    }

    void ResolveRenderTarget(Noesis::RenderTarget* surface,
                             const Noesis::Tile* tiles, uint32_t numTiles) override
    {
        mVtable.resolve_render_target(mUserdata,
            static_cast<RustRenderTarget*>(surface)->handle(), tiles, numTiles);
    }

    void* MapVertices(uint32_t bytes) override { return mVtable.map_vertices(mUserdata, bytes); }
    void  UnmapVertices() override            { mVtable.unmap_vertices(mUserdata); }
    void* MapIndices(uint32_t bytes) override  { return mVtable.map_indices(mUserdata, bytes); }
    void  UnmapIndices() override             { mVtable.unmap_indices(mUserdata); }

    void DrawBatch(const Noesis::Batch& batch) override {
        mVtable.draw_batch(mUserdata, &batch);
    }

private:
    Noesis::Ptr<RustTexture> makeTexture(const dm_noesis_texture_binding& b,
                                         Noesis::TextureFormat::Enum format) {
        return Noesis::MakePtr<RustTexture>(this, b.handle, b.width, b.height,
                                            format, b.has_mipmaps, b.inverted, b.has_alpha);
    }

    Noesis::Ptr<Noesis::RenderTarget> makeRenderTarget(
        const dm_noesis_render_target_binding& b)
    {
        // Resolve textures are always RGBA8 — that's what Noesis uses for the
        // composited surface. (The Rust impl is free to pick a wgpu format
        // internally as long as that mapping is consistent with how it reads
        // back via UpdateTexture.)
        return Noesis::MakePtr<RustRenderTarget>(
            this, b.handle,
            makeTexture(b.resolve_texture, Noesis::TextureFormat::RGBA8));
    }

    dm_noesis_render_device_vtable mVtable;
    void* mUserdata;
    mutable Noesis::DeviceCaps mCaps{};
    mutable bool mCapsValid = false;
};

// Definitions outside the class bodies because each destructor needs the full
// `RustRenderDevice` definition to call `drop*`.

RustTexture::~RustTexture() {
    mDevice->dropTexture(mHandle);
}

RustRenderTarget::~RustRenderTarget() {
    mDevice->dropRenderTarget(mHandle);
}

}  // namespace

// ─── Factory C ABI ──────────────────────────────────────────────────────────

extern "C" void* dm_noesis_render_device_create(
    const dm_noesis_render_device_vtable* vtable, void* userdata)
{
    if (!vtable) return nullptr;
    Noesis::Ptr<RustRenderDevice> device =
        Noesis::MakePtr<RustRenderDevice>(vtable, userdata);
    // MakePtr returns refcount = 1. GiveOwnership clears the smart pointer
    // without decrementing, transferring the +1 to the C-ABI caller.
    return device.GiveOwnership();
}

extern "C" void dm_noesis_render_device_destroy(void* device) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->Release();
}

extern "C" uint64_t dm_noesis_texture_get_handle(const void* texture) {
    if (!texture) return 0;
    return static_cast<const RustTexture*>(
               static_cast<const Noesis::Texture*>(texture))->handle();
}

extern "C" uint64_t dm_noesis_render_target_get_handle(const void* surface) {
    if (!surface) return 0;
    return static_cast<const RustRenderTarget*>(
               static_cast<const Noesis::RenderTarget*>(surface))->handle();
}

// ─── Test-only entrypoints ─────────────────────────────────────────────────
//
// Gated by the `test-utils` Cargo feature (which sets DM_NOESIS_TEST_UTILS).
// Production builds omit them entirely.

#ifdef DM_NOESIS_TEST_UTILS

// One-shot frame scenario that exercises every Noesis virtual the device
// implements, in the documented frame-protocol order. Lets all Ptr<>s die at
// function exit so drop_texture / drop_render_target fire and the Rust mock
// can observe the cleanup ordering.
//
// Used by tests/render_device.rs.
extern "C" void dm_noesis_test_run_frame_scenario(void* device_ptr) {
    auto* device = static_cast<RustRenderDevice*>(device_ptr);

    // ── Caps query (cached after first call) ───────────────────────────────
    (void)device->GetCaps();

    // ── Create textures ────────────────────────────────────────────────────
    static const uint32_t pixels4x4_rgba8[16] = {
        0xff0000ff, 0xff00ff00, 0xffff0000, 0xffffffff,
        0xff0000ff, 0xff00ff00, 0xffff0000, 0xffffffff,
        0xff0000ff, 0xff00ff00, 0xffff0000, 0xffffffff,
        0xff0000ff, 0xff00ff00, 0xffff0000, 0xffffffff,
    };
    const void* immutable_data[1] = { pixels4x4_rgba8 };

    Noesis::Ptr<Noesis::Texture> t_immutable = device->CreateTexture(
        "t_immutable", 4, 4, 1, Noesis::TextureFormat::RGBA8, immutable_data);

    Noesis::Ptr<Noesis::Texture> t_dynamic = device->CreateTexture(
        "t_dynamic", 16, 16, 1, Noesis::TextureFormat::R8, nullptr);

    static const uint8_t patch4x4_r8[16] = {
        0x10, 0x20, 0x30, 0x40,
        0x50, 0x60, 0x70, 0x80,
        0x90, 0xa0, 0xb0, 0xc0,
        0xd0, 0xe0, 0xf0, 0xff,
    };
    device->UpdateTexture(t_dynamic.GetPtr(), 0, 2, 2, 4, 4, patch4x4_r8);

    Noesis::Texture* dirty[1] = { t_dynamic.GetPtr() };
    device->EndUpdatingTextures(dirty, 1);

    // ── Create render target ───────────────────────────────────────────────
    Noesis::Ptr<Noesis::RenderTarget> rt = device->CreateRenderTarget(
        "rt_main", 256, 256, 1, true);

    // ── Offscreen phase ────────────────────────────────────────────────────
    device->BeginOffscreenRender();
    device->SetRenderTarget(rt.GetPtr());
    Noesis::Tile tile = { 0, 0, 256, 256 };
    device->BeginTile(rt.GetPtr(), tile);

    (void)device->MapVertices(96);
    device->UnmapVertices();
    (void)device->MapIndices(36);
    device->UnmapIndices();

    Noesis::Batch offscreen_batch{};
    offscreen_batch.shader.v = Noesis::Shader::Path_Solid;
    offscreen_batch.numVertices = 4;
    offscreen_batch.numIndices = 6;
    device->DrawBatch(offscreen_batch);

    device->EndTile(rt.GetPtr());
    device->ResolveRenderTarget(rt.GetPtr(), &tile, 1);
    device->EndOffscreenRender();

    // ── Onscreen phase ─────────────────────────────────────────────────────
    device->BeginOnscreenRender();

    (void)device->MapVertices(96);
    device->UnmapVertices();
    (void)device->MapIndices(36);
    device->UnmapIndices();

    Noesis::Batch onscreen_batch{};
    onscreen_batch.shader.v = Noesis::Shader::RGBA;
    onscreen_batch.numVertices = 4;
    onscreen_batch.numIndices = 6;
    device->DrawBatch(onscreen_batch);

    device->EndOnscreenRender();

    // ── Clone (exercises clone_render_target) ──────────────────────────────
    Noesis::Ptr<Noesis::RenderTarget> rt_clone = device->CloneRenderTarget(
        "rt_clone", rt.GetPtr());
    (void)rt_clone;

    // Function exit destroys (in reverse declaration order):
    //   rt_clone     → drop_render_target(clone) + drop_texture(clone resolve)
    //   rt           → drop_render_target(main)  + drop_texture(main resolve)
    //   t_dynamic    → drop_texture(dynamic)
    //   t_immutable  → drop_texture(immutable)
}

#endif  // DM_NOESIS_TEST_UTILS
