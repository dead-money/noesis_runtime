// Code-built image sources: CroppedBitmap, TextureSource, BitmapImage,
// DynamicTextureSource.
//
// Each create hands out one reference owned by the caller (src/imaging.rs
// releases it with noesis_base_component_release). Assigning the object to an
// Image.Source or ImageBrush.ImageSource takes Noesis's own reference.
//
// TextureSource::GetTexture and the BitmapSource pixel-size / DPI getters
// resolve only after a real texture or texture provider has run in a
// RenderDevice render pass. Without one they read null / 0.

#include "noesis_shim.h"

#include <NsCore/BaseComponent.h>
#include <NsCore/DynamicCast.h>
#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsDrawing/Int32Rect.h>
#include <NsGui/BitmapImage.h>
#include <NsGui/BitmapSource.h>
#include <NsGui/CroppedBitmap.h>
#include <NsGui/DynamicTextureSource.h>
#include <NsGui/ImageSource.h>
#include <NsGui/TextureSource.h>
#include <NsGui/Uri.h>
#include <NsRender/Texture.h>

namespace {

class CallbackTextureRenderer final : public Noesis::DynamicTextureSource::Renderer {
public:
    CallbackTextureRenderer(noesis_texture_renderer_callbacks callbacks, void* user)
        : mCallbacks(callbacks), mUser(user) {}

    void Init(Noesis::RenderDevice* device) override { mCallbacks.init(device, mUser); }
    Noesis::Texture* Render(uint32_t width, uint32_t height) override {
        return static_cast<Noesis::Texture*>(mCallbacks.render(width, height, mUser));
    }
    void Shutdown() override { mCallbacks.shutdown(mUser); }

private:
    noesis_texture_renderer_callbacks mCallbacks;
    void* mUser;
};

// Adds the caller's reference; the creating Ptr releases its own at scope exit.
void* handout(Noesis::BaseComponent* c) {
    if (!c) return nullptr;
    c->AddReference();
    return c;
}

template <class T>
T* cast(void* p) {
    if (!p) return nullptr;
    return Noesis::DynamicCast<T*>(static_cast<Noesis::BaseComponent*>(p));
}

// Canonical BaseComponent* so it compares equal to the pointer handout() gave.
void* borrow(Noesis::BitmapSource* s) {
    if (!s) return nullptr;
    return static_cast<Noesis::BaseComponent*>(s);
}

}  // namespace

// ── CroppedBitmap ────────────────────────────────────────────────────────────

extern "C" void* noesis_cropped_bitmap_create() {
    Noesis::Ptr<Noesis::CroppedBitmap> c = *new Noesis::CroppedBitmap();
    return handout(c.GetPtr());
}

// `source` is a borrowed BitmapSource* (or null); Noesis takes its own reference.
extern "C" bool noesis_cropped_bitmap_set_source(void* crop, void* source) {
    auto* c = cast<Noesis::CroppedBitmap>(crop);
    if (!c) return false;
    c->SetSource(cast<Noesis::BitmapSource>(source));
    return true;
}

// Borrowed (no +1) BitmapSource* currently set as the source, or null.
extern "C" void* noesis_cropped_bitmap_get_source(void* crop) {
    auto* c = cast<Noesis::CroppedBitmap>(crop);
    if (!c) return nullptr;
    return borrow(c->GetSource());
}

extern "C" bool noesis_cropped_bitmap_set_source_rect(void* crop, int32_t x, int32_t y,
                                                         uint32_t width, uint32_t height) {
    auto* c = cast<Noesis::CroppedBitmap>(crop);
    if (!c) return false;
    c->SetSourceRect(Noesis::Int32Rect(x, y, width, height));
    return true;
}

extern "C" bool noesis_cropped_bitmap_get_source_rect(void* crop, int32_t* x, int32_t* y,
                                                         uint32_t* width, uint32_t* height) {
    auto* c = cast<Noesis::CroppedBitmap>(crop);
    if (!c) return false;
    const Noesis::Int32Rect& r = c->GetSourceRect();
    if (x) *x = r.x;
    if (y) *y = r.y;
    if (width) *width = r.width;
    if (height) *height = r.height;
    return true;
}

// ── TextureSource ────────────────────────────────────────────────────────────

// Default-construct when `texture` is null, else TextureSource(Texture*).
// `texture` is a borrowed Noesis::Texture* (a BaseComponent*, e.g. from a host
// RenderDevice); Noesis stores it in an owning Ptr<Texture>.
extern "C" void* noesis_texture_source_create(void* texture) {
    auto* tex = cast<Noesis::Texture>(texture);
    Noesis::Ptr<Noesis::TextureSource> s =
        tex ? *new Noesis::TextureSource(tex) : *new Noesis::TextureSource();
    return handout(s.GetPtr());
}

extern "C" bool noesis_texture_source_set_texture(void* source, void* texture) {
    auto* s = cast<Noesis::TextureSource>(source);
    if (!s) return false;
    s->SetTexture(cast<Noesis::Texture>(texture));
    return true;
}

// Borrowed (no +1) Texture* currently bound, or null (null until a host
// RenderDevice-created Texture is bound).
extern "C" void* noesis_texture_source_get_texture(void* source) {
    auto* s = cast<Noesis::TextureSource>(source);
    if (!s) return nullptr;
    return static_cast<Noesis::BaseComponent*>(s->GetTexture());
}

// ── BitmapImage ──────────────────────────────────────────────────────────────

// Default-construct when `uri` is null, else BitmapImage(Uri(uri)).
extern "C" void* noesis_bitmap_image_create(const char* uri) {
    Noesis::Ptr<Noesis::BitmapImage> b =
        uri ? *new Noesis::BitmapImage(Noesis::Uri(uri)) : *new Noesis::BitmapImage();
    return handout(b.GetPtr());
}

extern "C" bool noesis_bitmap_image_set_uri_source(void* image, const char* uri) {
    auto* b = cast<Noesis::BitmapImage>(image);
    if (!b) return false;
    b->SetUriSource(Noesis::Uri(uri ? uri : ""));
    return true;
}

// Borrowed (no +1) canonicalized UriSource string, valid while `image` lives and
// its UriSource is unchanged. Returns null on a non-BitmapImage pointer.
extern "C" const char* noesis_bitmap_image_get_uri_source(void* image) {
    auto* b = cast<Noesis::BitmapImage>(image);
    if (!b) return nullptr;
    return b->GetUriSource().Str();
}

// ── BitmapSource getters (any BitmapSource subclass) ─────────────────────────

// Pixel dimensions; 0 until a texture provider resolves the bitmap on a render
// pass. Returns false on a non-BitmapSource pointer.
extern "C" bool noesis_bitmap_source_get_pixel_size(void* source, int32_t* width,
                                                       int32_t* height) {
    auto* s = cast<Noesis::BitmapSource>(source);
    if (!s) return false;
    if (width) *width = s->GetPixelWidth();
    if (height) *height = s->GetPixelHeight();
    return true;
}

// Horizontal / vertical DPI; defaults until resolved on a render pass.
extern "C" bool noesis_bitmap_source_get_dpi(void* source, float* dpi_x, float* dpi_y) {
    auto* s = cast<Noesis::BitmapSource>(source);
    if (!s) return false;
    if (dpi_x) *dpi_x = s->GetDpiX();
    if (dpi_y) *dpi_y = s->GetDpiY();
    return true;
}

// ── DynamicTextureSource ─────────────────────────────────────────────────────

extern "C" void* noesis_dynamic_texture_source_create(
    uint32_t width, uint32_t height, noesis_texture_renderer_callbacks callbacks, void* user) {
    if (!callbacks.init || !callbacks.render || !callbacks.shutdown) return nullptr;
    auto renderer = Noesis::MakePtr<CallbackTextureRenderer>(callbacks, user);
    Noesis::Ptr<Noesis::DynamicTextureSource> s =
        *new Noesis::DynamicTextureSource(width, height, renderer.GetPtr());
    return handout(s.GetPtr());
}

extern "C" bool noesis_dynamic_texture_source_update(void* source) {
    auto* s = cast<Noesis::DynamicTextureSource>(source);
    if (!s) return false;
    s->Update();
    return true;
}

#ifdef NOESIS_TEST_UTILS
extern "C" void noesis_test_texture_renderer_lifecycle(
    noesis_texture_renderer_callbacks callbacks, void* user) {
    auto renderer = Noesis::MakePtr<CallbackTextureRenderer>(callbacks, user);
    renderer->Init(nullptr);
    renderer->Render(64, 48);
    renderer->DetachDevice();
    renderer->AttachDevice(nullptr);
    renderer->Render(128, 96);
    renderer->Shutdown();
}
#endif

extern "C" bool noesis_dynamic_texture_source_resize(void* source, uint32_t width,
                                                       uint32_t height) {
    auto* s = cast<Noesis::DynamicTextureSource>(source);
    if (!s) return false;
    s->Resize(width, height);
    return true;
}

extern "C" bool noesis_dynamic_texture_source_get_pixel_size(void* source, uint32_t* width,
                                                               uint32_t* height) {
    auto* s = cast<Noesis::DynamicTextureSource>(source);
    if (!s) return false;
    if (width) *width = s->GetPixelWidth();
    if (height) *height = s->GetPixelHeight();
    return true;
}
