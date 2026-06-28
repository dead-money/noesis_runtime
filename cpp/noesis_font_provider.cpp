// C++ wrapper for Noesis::FontProvider (Phase 4.F.1).
//
//   `RustFontProvider` subclasses `Noesis::CachedFontProvider`. Two virtuals
//   are overridden:
//     - `ScanFolder(folder)` — called the first time a font is requested
//       from a folder. Trampolines into Rust, which walks its own registry
//       and calls `register_fn(cx, filename)` once per font; the C++ side
//       forwards each call to `CachedFontProvider::RegisterFont(folder,
//       filename)`. Noesis then opens the returned stream to scan face
//       metadata.
//     - `OpenFont(folder, filename)` — called by `MatchFont` once a face
//       has been registered and Noesis needs the bytes. Trampolines to
//       Rust, which returns a borrowed slice; we wrap it in a MemoryStream
//       without copying. The Rust provider is responsible for keeping the
//       bytes alive for the duration of Noesis's scan/parse.
//
//   The Rust side doesn't see FontWeight/Stretch/Style — CachedFontProvider
//   handles matching internally after RegisterFont(folder, filename) scans
//   the file for face metadata.

#include <cstdint>

#include <NsCore/Ptr.h>
#include <NsCore/String.h>
#include <NsGui/CachedFontProvider.h>
#include <NsGui/IntegrationAPI.h>
#include <NsGui/MemoryStream.h>
#include <NsGui/Stream.h>
#include <NsGui/Uri.h>

#include "noesis_shim.h"

namespace {

// RustFontProvider — subclass of CachedFontProvider. We expose
// `scan_folder` and `open_font` through the Rust vtable; the rest of the
// base class handles font matching / weight-stretch-style lookup.
class RustFontProvider final : public Noesis::CachedFontProvider {
public:
    RustFontProvider(const dm_noesis_font_provider_vtable* vtable, void* userdata)
        : mVtable(*vtable), mUserdata(userdata)
    {}

    // Public trampoline helper — the Rust scan_folder callback hands us
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

        // Trampoline ctx: carries `this` + folder so each Rust callback
        // can route back into our RegisterFontFromRust helper. Safe
        // because Rust calls `register_fn` synchronously from within
        // `scan_folder` before the call returns.
        struct Ctx {
            RustFontProvider* self;
            const Noesis::Uri* folder;
        };
        Ctx ctx{this, &folder};
        mVtable.scan_folder(
            mUserdata,
            folderUri,
            [](void* raw, const char* filename) {
                if (!raw || !filename) return;
                auto* c = static_cast<Ctx*>(raw);
                c->self->RegisterFontFromRust(*c->folder, filename);
            },
            &ctx);
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
        // Same contract as XamlProvider: MemoryStream holds a borrowed
        // pointer; Rust guarantees the bytes stay valid until font loading
        // completes. In practice the Rust provider owns the bytes in a
        // HashMap and returns a slice into them, same pattern as XAML.
        return Noesis::MakePtr<Noesis::MemoryStream>(data, len);
    }

private:
    dm_noesis_font_provider_vtable mVtable;
    void* mUserdata;
};

}  // namespace

// ── FontProvider C ABI ─────────────────────────────────────────────────────

extern "C" void* dm_noesis_font_provider_create(
    const dm_noesis_font_provider_vtable* vtable, void* userdata)
{
    if (!vtable) return nullptr;
    Noesis::Ptr<RustFontProvider> p =
        Noesis::MakePtr<RustFontProvider>(vtable, userdata);
    return p.GiveOwnership();
}

extern "C" void dm_noesis_font_provider_destroy(void* provider) {
    if (!provider) return;
    static_cast<Noesis::FontProvider*>(provider)->Release();
}

extern "C" void dm_noesis_set_font_provider(void* provider) {
    Noesis::GUI::SetFontProvider(static_cast<Noesis::FontProvider*>(provider));
}

extern "C" void dm_noesis_font_provider_register_font(
    void* provider, const char* folder_uri, const char* filename)
{
    if (!provider || !filename) return;
    auto* p = static_cast<RustFontProvider*>(provider);
    Noesis::Uri folder{folder_uri ? folder_uri : ""};
    p->RegisterFontFromRust(folder, filename);
}

extern "C" void dm_noesis_set_font_fallbacks(const char* const* families, uint32_t count) {
    if (!families || count == 0) {
        Noesis::GUI::SetFontFallbacks(nullptr, 0);
        return;
    }
    // SDK signature takes `const char**`, but the array it reads is const-
    // correct; cast away the pointer-to-pointer const qualifier.
    Noesis::GUI::SetFontFallbacks(const_cast<const char**>(families), count);
}

extern "C" void dm_noesis_set_font_default_properties(
    float size, int32_t weight, int32_t stretch, int32_t style)
{
    Noesis::GUI::SetFontDefaultProperties(
        size,
        static_cast<Noesis::FontWeight>(weight),
        static_cast<Noesis::FontStretch>(stretch),
        static_cast<Noesis::FontStyle>(style));
}
