// Noesis::TextureProvider backed by a Rust vtable (ImageBrush and Image
// sources).
//
// GetTextureInfo returns size and atlas rect without decoding; a false return
// from `get_info` leaves the info zeroed. LoadTexture gets tightly packed RGBA8
// from `load_texture` and creates the texture on the device Noesis passes in,
// which is the RustRenderDevice, so the texture follows the same lifecycle as
// Noesis's own glyph and ramp textures. The returned bytes need only stay
// valid until LoadTexture returns; CreateTexture copies them.

#include <cstdint>

#include <NsCore/Ptr.h>
#include <NsGui/IntegrationAPI.h>
#include <NsGui/TextureProvider.h>
#include <NsGui/Uri.h>
#include <NsRender/RenderDevice.h>
#include <NsRender/Texture.h>

#include "noesis_shim.h"

namespace {

class RustTextureProvider final : public Noesis::TextureProvider {
public:
    RustTextureProvider(const noesis_texture_provider_vtable* vtable, void* userdata)
        : mVtable(*vtable), mUserdata(userdata)
    {}

    Noesis::TextureInfo GetTextureInfo(const Noesis::Uri& uri) override {
        Noesis::TextureInfo info;
        if (!mVtable.get_info) return info;
        const char* uriStr = uri.Str();
        noesis_texture_info raw{};
        bool ok = mVtable.get_info(mUserdata, uriStr ? uriStr : "", &raw);
        if (!ok) return info;
        info.width = raw.width;
        info.height = raw.height;
        info.x = raw.x;
        info.y = raw.y;
        info.dpiScale = raw.dpi_scale;
        return info;
    }

    Noesis::Ptr<Noesis::Texture> LoadTexture(
        const Noesis::Uri& uri, Noesis::RenderDevice* device) override
    {
        if (!mVtable.load_texture || !device) return nullptr;
        const char* uriStr = uri.Str();
        uint32_t width = 0;
        uint32_t height = 0;
        const uint8_t* data = nullptr;
        uint32_t len = 0;
        bool ok = mVtable.load_texture(
            mUserdata,
            uriStr ? uriStr : "",
            &width, &height,
            &data, &len);
        if (!ok || data == nullptr || width == 0 || height == 0) {
            return nullptr;
        }
        // 64-bit product: a u32 width*height*4 can wrap and spuriously match `len`.
        if (static_cast<uint64_t>(width) * height * 4u != len) {
            return nullptr;
        }
        // Single mip level; the device copies synchronously.
        const void* levels[] = { data };
        return device->CreateTexture(
            uriStr ? uriStr : "",
            width, height, 1,
            Noesis::TextureFormat::RGBA8,
            levels);
    }

private:
    noesis_texture_provider_vtable mVtable;
    void* mUserdata;
};

}  // namespace

// ── TextureProvider C ABI ──────────────────────────────────────────────────

extern "C" void* noesis_texture_provider_create(
    const noesis_texture_provider_vtable* vtable, void* userdata)
{
    if (!vtable) return nullptr;
    Noesis::Ptr<RustTextureProvider> p =
        Noesis::MakePtr<RustTextureProvider>(vtable, userdata);
    return p.GiveOwnership();
}

extern "C" void noesis_texture_provider_destroy(void* provider) {
    if (!provider) return;
    static_cast<Noesis::TextureProvider*>(provider)->Release();
}

extern "C" void noesis_set_texture_provider(void* provider) {
    Noesis::GUI::SetTextureProvider(static_cast<Noesis::TextureProvider*>(provider));
}
