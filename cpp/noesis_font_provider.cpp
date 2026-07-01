// C++ wrapper for Noesis::FontProvider.
//
//   `RustFontProvider` subclasses `Noesis::CachedFontProvider`. Two virtuals
//   are overridden:
//     - `ScanFolder(folder)`: called the first time a font is requested
//       from a folder. Trampolines into Rust, which walks its own registry
//       and calls `register_fn(cx, filename)` once per font; the C++ side
//       forwards each call to `CachedFontProvider::RegisterFont(folder,
//       filename)`. Noesis then opens the returned stream to scan face
//       metadata.
//     - `OpenFont(folder, filename)`: called by `MatchFont` once a face
//       has been registered and Noesis needs the bytes. Trampolines to
//       Rust, which returns a borrowed slice; we copy it into an owning
//       stream (see OwnedFontStream) because the stream outlives the
//       open_font call. The Rust provider only needs to keep the bytes
//       alive for the duration of that call.
//
//   The Rust side doesn't see FontWeight/Stretch/Style; CachedFontProvider
//   handles matching internally after RegisterFont(folder, filename) scans
//   the file for face metadata.

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

// Stream that owns a private copy of its bytes. Noesis's stock MemoryStream
// only *borrows* the caller's buffer, but a font stream is retained inside the
// resulting FontSource and read lazily at glyph-raster time — long after
// open_font has returned — so a borrowed Rust buffer would dangle. Copying the
// bytes here makes the documented open_font contract (bytes valid only for the
// duration of the call) actually true.
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

// RustFontProvider: subclass of CachedFontProvider. We expose
// `scan_folder` and `open_font` through the Rust vtable; the rest of the
// base class handles font matching / weight-stretch-style lookup.
class RustFontProvider final : public Noesis::CachedFontProvider {
public:
    RustFontProvider(const noesis_font_provider_vtable* vtable, void* userdata)
        : mVtable(*vtable), mUserdata(userdata)
    {}

    // Public trampoline helper: the Rust scan_folder callback hands us
    // filenames one at a time; we forward each to the base class's
    // protected RegisterFont. Callable only through our own subclass so
    // the protected-access check is satisfied.
    void RegisterFontFromRust(const Noesis::Uri& folder, const char* filename) {
        RegisterFont(folder, filename);
    }

protected:
    void ScanFolder(const Noesis::Uri& folder) override {
        if (!mVtable.scan_folder) return;
        const char* folderUri = folder.Str();

        // Collect the filenames the Rust callback hands us, then RegisterFont
        // them *after* scan_folder returns. RegisterFont synchronously opens
        // and scans each file through OpenFont (→ the Rust open_font
        // trampoline), which mints its own &mut to the Rust provider; running
        // it while the provider's scan_folder &mut is still live on the stack
        // would be aliasing UB. Deferring keeps the two &mut disjoint.
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
        // Copy into an owning stream: Noesis retains the returned stream inside
        // the FontSource and reads it lazily at raster time, so it cannot
        // borrow the Rust-owned bytes (which are only valid for this call).
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
    // SDK signature takes `const char**`, but the array it reads is const-
    // correct; cast away the pointer-to-pointer const qualifier.
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
