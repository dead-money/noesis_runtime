// Noesis RenderDevice / Texture / RenderTarget implementations that forward
// every virtual into the Rust vtable supplied at construction.
//
// Handle 0 marks an inert wrapper: the Rust create callback panicked or
// rejected the request and left its out-binding zeroed. Inert handles are never
// forwarded back to Rust.

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

// Raw back-pointer to the device for `drop_texture`: a texture must not
// outlive the device that created it.
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

class RustRenderDevice final : public Noesis::RenderDevice {
public:
    RustRenderDevice(const noesis_render_device_vtable* vtable, void* userdata)
        : mVtable(*vtable)
        , mUserdata(userdata)
    {}

    // Last reference gone, so no further callback can fire. Freeing the boxed
    // impl here rather than in noesis_render_device_destroy keeps mUserdata
    // valid for callbacks Noesis issues after the Rust guard drops.
    ~RustRenderDevice() override { mVtable.drop_userdata(mUserdata); }

    void dropTexture(uint64_t h) { mVtable.drop_texture(mUserdata, h); }
    void dropRenderTarget(uint64_t h) { mVtable.drop_render_target(mUserdata, h); }

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
        noesis_render_target_binding b{};
        mVtable.create_render_target(mUserdata, label, width, height,
                                     sampleCount, needsStencil, &b);
        return makeRenderTarget(b);
    }

    Noesis::Ptr<Noesis::RenderTarget> CloneRenderTarget(
        const char* label, Noesis::RenderTarget* surface) override
    {
        const auto src = static_cast<RustRenderTarget*>(surface);
        noesis_render_target_binding b{};
        // An inert source clones to another inert wrapper.
        if (src->handle() != 0) {
            mVtable.clone_render_target(mUserdata, label, src->handle(), &b);
        }
        return makeRenderTarget(b);
    }

    Noesis::Ptr<Noesis::Texture> CreateTexture(
        const char* label, uint32_t width, uint32_t height, uint32_t numLevels,
        Noesis::TextureFormat::Enum format, const void** data) override
    {
        noesis_texture_binding b{};
        mVtable.create_texture(mUserdata, label, width, height, numLevels,
                               static_cast<uint32_t>(format), data, &b);
        return makeTexture(b, format);
    }

    void UpdateTexture(Noesis::Texture* texture, uint32_t level,
                       uint32_t x, uint32_t y, uint32_t width, uint32_t height,
                       const void* data) override
    {
        const auto* t = static_cast<RustTexture*>(texture);
        if (t->handle() == 0) return;
        mVtable.update_texture(mUserdata, t->handle(), level, x, y, width, height,
                               static_cast<uint32_t>(t->format()), data);
    }

    void EndUpdatingTextures(Noesis::Texture** textures, uint32_t count) override {
        if (count == 0) return;
        std::vector<uint64_t> handles;
        handles.reserve(count);
        for (uint32_t i = 0; i < count; ++i) {
            const uint64_t h = static_cast<RustTexture*>(textures[i])->handle();
            if (h != 0) handles.push_back(h);
        }
        if (handles.empty()) return;
        mVtable.end_updating_textures(mUserdata, handles.data(),
                                      static_cast<uint32_t>(handles.size()));
    }

    void BeginOffscreenRender() override { mVtable.begin_offscreen_render(mUserdata); }
    void EndOffscreenRender()   override { mVtable.end_offscreen_render(mUserdata); }
    void BeginOnscreenRender()  override { mVtable.begin_onscreen_render(mUserdata); }
    void EndOnscreenRender()    override { mVtable.end_onscreen_render(mUserdata); }

    void SetRenderTarget(Noesis::RenderTarget* surface) override {
        const uint64_t h = static_cast<RustRenderTarget*>(surface)->handle();
        if (h == 0) return;
        mVtable.set_render_target(mUserdata, h);
    }

    void BeginTile(Noesis::RenderTarget* surface, const Noesis::Tile& tile) override {
        const uint64_t h = static_cast<RustRenderTarget*>(surface)->handle();
        if (h == 0) return;
        mVtable.begin_tile(mUserdata, h, &tile);
    }

    void EndTile(Noesis::RenderTarget* surface) override {
        const uint64_t h = static_cast<RustRenderTarget*>(surface)->handle();
        if (h == 0) return;
        mVtable.end_tile(mUserdata, h);
    }

    void ResolveRenderTarget(Noesis::RenderTarget* surface,
                             const Noesis::Tile* tiles, uint32_t numTiles) override
    {
        const uint64_t h = static_cast<RustRenderTarget*>(surface)->handle();
        if (h == 0) return;
        mVtable.resolve_render_target(mUserdata, h, tiles, numTiles);
    }

    void* MapVertices(uint32_t bytes) override { return mVtable.map_vertices(mUserdata, bytes); }
    void  UnmapVertices() override            { mVtable.unmap_vertices(mUserdata); }
    void* MapIndices(uint32_t bytes) override  { return mVtable.map_indices(mUserdata, bytes); }
    void  UnmapIndices() override             { mVtable.unmap_indices(mUserdata); }

    void DrawBatch(const Noesis::Batch& batch) override {
        mVtable.draw_batch(mUserdata, &batch);
    }

private:
    Noesis::Ptr<RustTexture> makeTexture(const noesis_texture_binding& b,
                                         Noesis::TextureFormat::Enum format) {
        return Noesis::MakePtr<RustTexture>(this, b.handle, b.width, b.height,
                                            format, b.has_mipmaps, b.inverted, b.has_alpha);
    }

    Noesis::Ptr<Noesis::RenderTarget> makeRenderTarget(
        const noesis_render_target_binding& b)
    {
        // Noesis composites into RGBA8; the binding carries no format field.
        return Noesis::MakePtr<RustRenderTarget>(
            this, b.handle,
            makeTexture(b.resolve_texture, Noesis::TextureFormat::RGBA8));
    }

    noesis_render_device_vtable mVtable;
    void* mUserdata;
    mutable Noesis::DeviceCaps mCaps{};
    mutable bool mCapsValid = false;
};

RustTexture::~RustTexture() {
    // Forwarding 0 would panic in the Rust trampoline's NonZeroU64 conversion.
    if (mHandle != 0) mDevice->dropTexture(mHandle);
}

RustRenderTarget::~RustRenderTarget() {
    if (mHandle != 0) mDevice->dropRenderTarget(mHandle);
}

}  // namespace

extern "C" void* noesis_render_device_create(
    const noesis_render_device_vtable* vtable, void* userdata)
{
    if (!vtable) return nullptr;
    Noesis::Ptr<RustRenderDevice> device =
        Noesis::MakePtr<RustRenderDevice>(vtable, userdata);
    // GiveOwnership hands MakePtr's +1 to the caller without a release.
    return device.GiveOwnership();
}

extern "C" void noesis_render_device_destroy(void* device) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->Release();
}

extern "C" void noesis_render_device_set_offscreen_width(void* device, uint32_t width) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->SetOffscreenWidth(width);
}

extern "C" void noesis_render_device_set_offscreen_height(void* device, uint32_t height) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->SetOffscreenHeight(height);
}

extern "C" void noesis_render_device_set_offscreen_sample_count(void* device, uint32_t count) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->SetOffscreenSampleCount(count);
}

extern "C" void noesis_render_device_set_offscreen_default_num_surfaces(void* device, uint32_t num) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->SetOffscreenDefaultNumSurfaces(num);
}

extern "C" void noesis_render_device_set_offscreen_max_num_surfaces(void* device, uint32_t num) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->SetOffscreenMaxNumSurfaces(num);
}

extern "C" void noesis_render_device_set_glyph_cache_width(void* device, uint32_t width) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->SetGlyphCacheWidth(width);
}

extern "C" void noesis_render_device_set_glyph_cache_height(void* device, uint32_t height) {
    if (!device) return;
    static_cast<Noesis::RenderDevice*>(device)->SetGlyphCacheHeight(height);
}

extern "C" uint32_t noesis_render_device_get_offscreen_width(const void* device) {
    if (!device) return 0;
    return static_cast<const Noesis::RenderDevice*>(device)->GetOffscreenWidth();
}

extern "C" uint32_t noesis_render_device_get_offscreen_height(const void* device) {
    if (!device) return 0;
    return static_cast<const Noesis::RenderDevice*>(device)->GetOffscreenHeight();
}

extern "C" uint32_t noesis_render_device_get_offscreen_sample_count(const void* device) {
    if (!device) return 0;
    return static_cast<const Noesis::RenderDevice*>(device)->GetOffscreenSampleCount();
}

extern "C" uint32_t noesis_render_device_get_offscreen_default_num_surfaces(const void* device) {
    if (!device) return 0;
    return static_cast<const Noesis::RenderDevice*>(device)->GetOffscreenDefaultNumSurfaces();
}

extern "C" uint32_t noesis_render_device_get_offscreen_max_num_surfaces(const void* device) {
    if (!device) return 0;
    return static_cast<const Noesis::RenderDevice*>(device)->GetOffscreenMaxNumSurfaces();
}

extern "C" uint32_t noesis_render_device_get_glyph_cache_width(const void* device) {
    if (!device) return 0;
    return static_cast<const Noesis::RenderDevice*>(device)->GetGlyphCacheWidth();
}

extern "C" uint32_t noesis_render_device_get_glyph_cache_height(const void* device) {
    if (!device) return 0;
    return static_cast<const Noesis::RenderDevice*>(device)->GetGlyphCacheHeight();
}

extern "C" uint64_t noesis_texture_get_handle(const void* texture) {
    if (!texture) return 0;
    return static_cast<const RustTexture*>(
               static_cast<const Noesis::Texture*>(texture))->handle();
}

extern "C" uint64_t noesis_render_target_get_handle(const void* surface) {
    if (!surface) return 0;
    return static_cast<const RustRenderTarget*>(
               static_cast<const Noesis::RenderTarget*>(surface))->handle();
}

// NOESIS_TEST_UTILS is set by the `test-utils` Cargo feature.
#ifdef NOESIS_TEST_UTILS

// Calls every RenderDevice virtual once in frame-protocol order, then lets the
// Ptr<>s die so drop_texture / drop_render_target fire in a known order.
extern "C" void noesis_test_run_frame_scenario(void* device_ptr) {
    auto* device = static_cast<RustRenderDevice*>(device_ptr);

    (void)device->GetCaps();

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

    Noesis::Ptr<Noesis::RenderTarget> rt = device->CreateRenderTarget(
        "rt_main", 256, 256, 1, true);

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

    Noesis::Ptr<Noesis::RenderTarget> rt_clone = device->CloneRenderTarget(
        "rt_clone", rt.GetPtr());
    (void)rt_clone;

    // Destroyed in reverse declaration order:
    //   rt_clone     → drop_render_target(clone) + drop_texture(clone resolve)
    //   rt           → drop_render_target(main)  + drop_texture(main resolve)
    //   t_dynamic    → drop_texture(dynamic)
    //   t_immutable  → drop_texture(immutable)
}

#endif  // NOESIS_TEST_UTILS
