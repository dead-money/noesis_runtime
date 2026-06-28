// C++ wrappers for the XAML loading variants surface (TODO §15):
//
//   * GetXamlDependencies — walk an in-memory XAML buffer's referenced
//     resources (other XAMLs, fonts, textures, prefixed UserControl nodes,
//     the root type) without instantiating the tree, forwarding each hit
//     into a Rust callback.
//
//   * Scheme- / assembly-scoped provider setters — thin pass-throughs to the
//     `GUI::SetSchemeXamlProvider` / `SetAssemblyXamlProvider` /
//     `SetSchemeAssemblyXamlProvider` overloads (and the identical Texture +
//     Font triples). These REUSE the provider handles produced by
//     `noesis_{xaml,font,texture}_provider_create` in the existing shim
//     files — only the install call differs from the global setter.
//
//   * Typed component load — `GUI::LoadXaml` for a root that need not be a
//     FrameworkElement (e.g. a ResourceDictionary), plus a reflection helper
//     that reports the loaded root's class-type name so the typed-load path
//     is observable headlessly.

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

// The C ABI dependency-kind ints must match Noesis::XamlDependencyType so the
// Rust enum can map them 1:1. Guard each ordinal at compile time.
static_assert((int32_t)Noesis::XamlDependencyType_Filename == 0, "XamlDependencyType::Filename");
static_assert((int32_t)Noesis::XamlDependencyType_Font == 1, "XamlDependencyType::Font");
static_assert((int32_t)Noesis::XamlDependencyType_UserControl == 2, "XamlDependencyType::UserControl");
static_assert((int32_t)Noesis::XamlDependencyType_Root == 3, "XamlDependencyType::Root");

namespace {

// Bundles the Rust callback + its userdata so the capture-less trampoline can
// recover both from the single `void* user` slot GetXamlDependencies forwards.
struct DependencyCtx {
    void* user;
    noesis_xaml_dependency_fn cb;
};

// Capture-less so it converts to the plain `XamlDependencyCallback` function
// pointer the API wants. Forwards each dependency into the Rust callback,
// passing the URI as a borrowed NUL-terminated string (Rust copies it).
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

// Load XAML by URI WITHOUT narrowing the root to FrameworkElement. Returns the
// loaded root as a BaseComponent* at +1 (release via
// noesis_base_component_release), or NULL when the URI is unknown to the
// installed provider / the XAML is malformed. Unlike noesis_gui_load_xaml,
// this keeps non-FrameworkElement roots (e.g. ResourceDictionary).
extern "C" void* noesis_gui_load_xaml_component(const char* uri) {
    if (!uri) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> component =
        Noesis::GUI::LoadXaml(Noesis::Uri(uri));
    if (!component) return nullptr;
    return component.GiveOwnership();
}

// Reflected class-type name of any BaseComponent (e.g. "ResourceDictionary",
// "Grid"). Returns Noesis's interned `const char*` (owned by the type system,
// stable for the process lifetime); Rust copies it immediately. NULL on a
// NULL object or a type with no class.
extern "C" const char* noesis_base_component_type_name(void* obj) {
    if (!obj) return nullptr;
    const Noesis::TypeClass* tc = static_cast<Noesis::BaseComponent*>(obj)->GetClassType();
    if (!tc) return nullptr;
    return tc->GetName();
}

// ── Scheme- / assembly-scoped provider setters ─────────────────────────────
//
// `provider` is a handle from the matching `noesis_*_provider_create`. A
// NULL scheme/assembly is a no-op (the C++ API requires a valid C string).
// Passing a NULL provider clears the scoped registration.

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
