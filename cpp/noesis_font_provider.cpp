// Rust-backed font provider: a CachedFontProvider subclass overriding two
// virtuals.
//
//   * ScanFolder(folder): runs the first time a font is requested from a
//     folder. Rust reports each filename through `register_fn`, and each is
//     passed to CachedFontProvider::RegisterFont, which opens it to read face
//     metadata.
//   * OpenFont(folder, filename): Rust returns a borrowed slice, valid only for
//     the call; it is copied into an OwnedFontStream.
//
// Rust never sees FontWeight / Stretch / Style; CachedFontProvider does the
// matching from the scanned face metadata.

#include <cstdint>
#include <cstring>

#include <NsCore/Ptr.h>
#include <NsCore/String.h>
#include <NsCore/Vector.h>
#include <NsGui/CachedFontProvider.h>
#include <NsGui/IntegrationAPI.h>
#include <NsGui/Stream.h>
#include <NsGui/Uri.h>

#include "noesis_shim.h"

namespace {

// Owns a copy of its bytes. MemoryStream only borrows, but the FontSource keeps
// the stream and reads it lazily at glyph-raster time, after open_font returns.
class OwnedFontStream final : public Noesis::Stream {
public:
    OwnedFontStream(const uint8_t* data, uint32_t len) : mOffset(0) {
        if (data && len > 0) mData.Append(data, data + len);
    }

    void SetPosition(uint32_t pos) override {
        mOffset = pos < mData.Size() ? pos : mData.Size();
    }
    uint32_t GetPosition() const override { return mOffset; }
    uint32_t GetLength() const override { return mData.Size(); }
    uint32_t Read(void* buffer, uint32_t size) override {
        uint32_t remaining = mData.Size() - mOffset;
        uint32_t n = size < remaining ? size : remaining;
        if (n > 0) memcpy(buffer, mData.Begin() + mOffset, n);
        mOffset += n;
        return n;
    }
    const void* GetMemoryBase() const override {
        return mData.Size() > 0 ? mData.Begin() : nullptr;
    }
    void Close() override {}

private:
    Noesis::Vector<uint8_t> mData;
    uint32_t mOffset;
};

class RustFontProvider final : public Noesis::CachedFontProvider {
public:
    RustFontProvider(const noesis_font_provider_vtable* vtable, void* userdata)
        : mVtable(*vtable), mUserdata(userdata)
    {}

    // public access to the protected RegisterFont
    void RegisterFontFromRust(const Noesis::Uri& folder, const char* filename) {
        RegisterFont(folder, filename);
    }

protected:
    void ScanFolder(const Noesis::Uri& folder) override {
        if (!mVtable.scan_folder) return;
        const char* folderUri = folder.Str();

        // Register only after scan_folder returns. RegisterFont calls OpenFont
        // synchronously, which takes its own &mut to the Rust provider; doing
        // that while scan_folder's &mut is live would be aliasing UB.
        Noesis::Vector<Noesis::String> names;
        mVtable.scan_folder(
            mUserdata,
            folderUri ? folderUri : "",
            [](void* raw, const char* filename) {
                if (!raw || !filename) return;
                static_cast<Noesis::Vector<Noesis::String>*>(raw)->EmplaceBack(filename);
            },
            &names);
        for (uint32_t i = 0; i < names.Size(); ++i) {
            RegisterFontFromRust(folder, names[i].Str());
        }
    }

    Noesis::Ptr<Noesis::Stream> OpenFont(
        const Noesis::Uri& folder, const char* filename) const override
    {
        if (!mVtable.open_font) return nullptr;
        const char* folderUri = folder.Str();
        const uint8_t* data = nullptr;
        uint32_t len = 0;
        bool ok = mVtable.open_font(
            mUserdata,
            folderUri ? folderUri : "",
            filename ? filename : "",
            &data, &len);
        if (!ok || data == nullptr) {
            return nullptr;
        }
        return Noesis::MakePtr<OwnedFontStream>(data, len);
    }

private:
    noesis_font_provider_vtable mVtable;
    void* mUserdata;
};

}  // namespace

// ── FontProvider C ABI ─────────────────────────────────────────────────────

extern "C" void* noesis_font_provider_create(
    const noesis_font_provider_vtable* vtable, void* userdata)
{
    if (!vtable) return nullptr;
    Noesis::Ptr<RustFontProvider> p =
        Noesis::MakePtr<RustFontProvider>(vtable, userdata);
    return p.GiveOwnership();
}

extern "C" void noesis_font_provider_destroy(void* provider) {
    if (!provider) return;
    static_cast<Noesis::FontProvider*>(provider)->Release();
}

extern "C" void noesis_set_font_provider(void* provider) {
    Noesis::GUI::SetFontProvider(static_cast<Noesis::FontProvider*>(provider));
}

extern "C" void noesis_font_provider_register_font(
    void* provider, const char* folder_uri, const char* filename)
{
    if (!provider || !filename) return;
    auto* p = static_cast<RustFontProvider*>(provider);
    Noesis::Uri folder{folder_uri ? folder_uri : ""};
    p->RegisterFontFromRust(folder, filename);
}

extern "C" void noesis_set_font_fallbacks(const char* const* families, uint32_t count) {
    if (!families || count == 0) {
        Noesis::GUI::SetFontFallbacks(nullptr, 0);
        return;
    }
    // SDK takes `const char**` but only reads the array
    Noesis::GUI::SetFontFallbacks(const_cast<const char**>(families), count);
}

extern "C" void noesis_set_font_default_properties(
    float size, int32_t weight, int32_t stretch, int32_t style)
{
    Noesis::GUI::SetFontDefaultProperties(
        size,
        static_cast<Noesis::FontWeight>(weight),
        static_cast<Noesis::FontStretch>(stretch),
        static_cast<Noesis::FontStyle>(style));
}
