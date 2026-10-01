// XAML dependency scanning, scheme- and assembly-scoped provider setters, and
// loading a XAML root that need not be a FrameworkElement.
//
// The scoped setters take the same provider handles as the global setters
// (noesis_{xaml,font,texture}_provider_create).

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/BaseComponent.h>
#include <NsCore/Type.h>
#include <NsCore/TypeClass.h>
#include <NsGui/IntegrationAPI.h>
#include <NsGui/MemoryStream.h>
#include <NsGui/Stream.h>
#include <NsGui/Uri.h>
#include <NsGui/XamlProvider.h>
#include <NsGui/TextureProvider.h>
#include <NsGui/FontProvider.h>
#include <NsGui/Enums.h>

#include <cstdint>

// The Rust dependency-kind enum maps these ordinals 1:1.
static_assert((int32_t)Noesis::XamlDependencyType_Filename == 0, "XamlDependencyType::Filename");
static_assert((int32_t)Noesis::XamlDependencyType_Font == 1, "XamlDependencyType::Font");
static_assert((int32_t)Noesis::XamlDependencyType_UserControl == 2, "XamlDependencyType::UserControl");
static_assert((int32_t)Noesis::XamlDependencyType_Root == 3, "XamlDependencyType::Root");

namespace {

struct DependencyCtx {
    void* user;
    noesis_xaml_dependency_fn cb;
};

// The URI is borrowed for the callback's duration.
void DependencyTrampoline(void* user, const Noesis::Uri& uri, Noesis::XamlDependencyType type) {
    auto* ctx = static_cast<DependencyCtx*>(user);
    if (!ctx || !ctx->cb) return;
    const char* s = uri.Str();
    ctx->cb(ctx->user, s ? s : "", static_cast<int32_t>(type));
}

}  // namespace

// ── GetXamlDependencies ────────────────────────────────────────────────────

extern "C" void noesis_get_xaml_dependencies(
    const uint8_t* xaml, uint32_t len, const char* base_uri,
    void* user, noesis_xaml_dependency_fn cb)
{
    if (!xaml || !cb) return;
    // MemoryStream wraps the buffer without copying; GetXamlDependencies reads
    // it synchronously, so the Rust-owned slice need only outlive this call.
    Noesis::Ptr<Noesis::MemoryStream> stream =
        Noesis::MakePtr<Noesis::MemoryStream>(xaml, len);
    DependencyCtx ctx{user, cb};
    Noesis::GUI::GetXamlDependencies(
        stream, Noesis::Uri(base_uri ? base_uri : ""), &ctx, &DependencyTrampoline);
}

// ── Typed component load + reflected type name ─────────────────────────────

// Unlike noesis_gui_load_xaml, keeps non-FrameworkElement roots. Returns the
// root at +1, or NULL for an unknown URI or malformed XAML.
extern "C" void* noesis_gui_load_xaml_component(const char* uri) {
    if (!uri) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> component =
        Noesis::GUI::LoadXaml(Noesis::Uri(uri));
    if (!component) return nullptr;
    return component.GiveOwnership();
}

// Interned by the type system, valid for the process lifetime. NULL on NULL
// or a type with no class.
extern "C" const char* noesis_base_component_type_name(void* obj) {
    if (!obj) return nullptr;
    const Noesis::TypeClass* tc = static_cast<Noesis::BaseComponent*>(obj)->GetClassType();
    if (!tc) return nullptr;
    return tc->GetName();
}

// ── Scheme- / assembly-scoped provider setters ─────────────────────────────
//
// A NULL scheme or assembly is a no-op. A NULL provider clears the scoped
// registration.

extern "C" void noesis_set_xaml_provider_scheme(const char* scheme, void* provider) {
    if (!scheme) return;
    Noesis::GUI::SetSchemeXamlProvider(
        scheme, static_cast<Noesis::XamlProvider*>(provider));
}

extern "C" void noesis_set_xaml_provider_assembly(const char* assembly, void* provider) {
    if (!assembly) return;
    Noesis::GUI::SetAssemblyXamlProvider(
        assembly, static_cast<Noesis::XamlProvider*>(provider));
}

extern "C" void noesis_set_xaml_provider_scheme_assembly(
    const char* scheme, const char* assembly, void* provider)
{
    if (!scheme || !assembly) return;
    Noesis::GUI::SetSchemeAssemblyXamlProvider(
        scheme, assembly, static_cast<Noesis::XamlProvider*>(provider));
}

extern "C" void noesis_set_texture_provider_scheme(const char* scheme, void* provider) {
    if (!scheme) return;
    Noesis::GUI::SetSchemeTextureProvider(
        scheme, static_cast<Noesis::TextureProvider*>(provider));
}

extern "C" void noesis_set_texture_provider_assembly(const char* assembly, void* provider) {
    if (!assembly) return;
    Noesis::GUI::SetAssemblyTextureProvider(
        assembly, static_cast<Noesis::TextureProvider*>(provider));
}

extern "C" void noesis_set_texture_provider_scheme_assembly(
    const char* scheme, const char* assembly, void* provider)
{
    if (!scheme || !assembly) return;
    Noesis::GUI::SetSchemeAssemblyTextureProvider(
        scheme, assembly, static_cast<Noesis::TextureProvider*>(provider));
}

extern "C" void noesis_set_font_provider_scheme(const char* scheme, void* provider) {
    if (!scheme) return;
    Noesis::GUI::SetSchemeFontProvider(
        scheme, static_cast<Noesis::FontProvider*>(provider));
}

extern "C" void noesis_set_font_provider_assembly(const char* assembly, void* provider) {
    if (!assembly) return;
    Noesis::GUI::SetAssemblyFontProvider(
        assembly, static_cast<Noesis::FontProvider*>(provider));
}

extern "C" void noesis_set_font_provider_scheme_assembly(
    const char* scheme, const char* assembly, void* provider)
{
    if (!scheme || !assembly) return;
    Noesis::GUI::SetSchemeAssemblyFontProvider(
        scheme, assembly, static_cast<Noesis::FontProvider*>(provider));
}
