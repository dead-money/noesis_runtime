// C++ wrapper for Noesis::TextureProvider (Phase 4.E ImageBrush support).
//
//   `RustTextureProvider` subclasses `Noesis::TextureProvider`. Two virtuals
//   are overridden:
//
//     - `GetTextureInfo(uri)` — returns dimensions + optional atlas rect
//       without decoding pixels. Trampolines to `vtable.get_info`; on a
//       `false` / null return Noesis falls back to LoadTexture.
//
//     - `LoadTexture(uri, device)` — decodes the image bytes through the
//       Rust vtable, then calls the *same device Noesis passed us* to
//       create a `Ptr<Texture>` from the RGBA8 payload. That device is our
//       RustRenderDevice, so the created texture is a `RustTexture`
//       referenced back into our Rust-side `textures` map — the same
//       lifecycle as textures Noesis creates for its own glyph / ramps
//       atlases.
//
//   The Rust side is responsible for:
//     - Mapping URIs to asset paths.
//     - Decoding (PNG/JPEG/etc.) into tightly-packed RGBA8.
//     - Keeping the byte buffer alive for the duration of `load_texture`.
//       Once the call returns the bytes have been copied into the wgpu
//       texture, so Rust may release or reuse the buffer.

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
    RustTextureProvider(const dm_noesis_texture_provider_vtable* vtable, void* userdata)
        : mVtable(*vtable), mUserdata(userdata)
    {}

    Noesis::TextureInfo GetTextureInfo(const Noesis::Uri& uri) override {
        Noesis::TextureInfo info;
        if (!mVtable.get_info) return info;
        const char* uriStr = uri.Str();
        dm_noesis_texture_info raw{};
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
        // The Rust side must pack exactly width*height*4 bytes for RGBA8;
        // bail defensively rather than handing bad data to the device.
        if (len != width * height * 4u) {
            return nullptr;
        }
        // `data` is a single mip level. CreateTexture takes a
        // `const void**` array, so wrap the single pointer in a local
        // array. The device copies into the GPU texture synchronously.
        const void* levels[] = { data };
        return device->CreateTexture(
            uriStr ? uriStr : "",
            width, height, 1,
            Noesis::TextureFormat::RGBA8,
            levels);
    }

private:
    dm_noesis_texture_provider_vtable mVtable;
    void* mUserdata;
};

}  // namespace

// ── TextureProvider C ABI ──────────────────────────────────────────────────

extern "C" void* dm_noesis_texture_provider_create(
    const dm_noesis_texture_provider_vtable* vtable, void* userdata)
{
    if (!vtable) return nullptr;
    Noesis::Ptr<RustTextureProvider> p =
        Noesis::MakePtr<RustTextureProvider>(vtable, userdata);
    return p.GiveOwnership();
}

extern "C" void dm_noesis_texture_provider_destroy(void* provider) {
    if (!provider) return;
    static_cast<Noesis::TextureProvider*>(provider)->Release();
}

extern "C" void dm_noesis_set_texture_provider(void* provider) {
    Noesis::GUI::SetTextureProvider(static_cast<Noesis::TextureProvider*>(provider));
}
