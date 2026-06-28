// C++ wrappers for the XamlProvider / IView / IRenderer surface (Phase 4.C).
//
// Mirrors the RustRenderDevice pattern in noesis_render_device.cpp:
//   * `RustXamlProvider` subclasses `Noesis::XamlProvider` and trampolines
//     `LoadXaml` into a Rust vtable. The Rust side owns the bytes; this shim
//     wraps them in a `Noesis::MemoryStream` whose `const void*` buffer is
//     the Rust-owned storage.
//   * Thin extern "C" entrypoints over `GUI::LoadXaml`, `GUI::CreateView`,
//     and the `IView` / `IRenderer` methods Phase 4.D will drive.
//
// No Rust callback fires on XamlProvider teardown — the Rust side manages
// the boxed trait object's lifetime via its `Drop` impl (mirrors the
// `Registered` pattern for RenderDevice).

#include "noesis_shim.h"

#include <NsCore/Noesis.h>
#include <NsCore/Ptr.h>
#include <NsCore/Delegate.h>
#include <NsCore/DynamicCast.h>
#include <NsGui/FrameworkElement.h>
#include <NsGui/InputEnums.h>
#include <NsGui/IntegrationAPI.h>
#include <NsGui/IRenderer.h>
#include <NsGui/IView.h>
#include <NsGui/MemoryStream.h>
#include <NsGui/ResourceDictionary.h>
#include <NsGui/UICollection.h>
#include <NsGui/Stream.h>
#include <NsGui/Uri.h>
#include <NsGui/XamlProvider.h>
#include <NsMath/Matrix.h>
#include <NsRender/RenderDevice.h>

#include <cstdint>
#include <cstring>

namespace {

// ── RustXamlProvider ───────────────────────────────────────────────────────

class RustXamlProvider final : public Noesis::XamlProvider {
public:
    RustXamlProvider(const dm_noesis_xaml_provider_vtable* vtable, void* userdata)
        : mVtable(*vtable), mUserdata(userdata)
    {}

    Noesis::Ptr<Noesis::Stream> LoadXaml(const Noesis::Uri& uri) override {
        const char* uriChars = uri.Str();
        const uint8_t* data = nullptr;
        uint32_t len = 0;
        bool ok = mVtable.load_xaml(mUserdata, uriChars ? uriChars : "", &data, &len);
        if (!ok || data == nullptr) {
            return nullptr;
        }
        // MemoryStream stores the buffer pointer without copying. The Rust
        // side guarantees the bytes stay valid until parsing completes (which
        // is synchronous with this call's return).
        return Noesis::MakePtr<Noesis::MemoryStream>(data, len);
    }

private:
    dm_noesis_xaml_provider_vtable mVtable;
    void* mUserdata;
};

}  // namespace

// ── XamlProvider C ABI ─────────────────────────────────────────────────────

extern "C" void* dm_noesis_xaml_provider_create(
    const dm_noesis_xaml_provider_vtable* vtable, void* userdata)
{
    if (!vtable) return nullptr;
    Noesis::Ptr<RustXamlProvider> p =
        Noesis::MakePtr<RustXamlProvider>(vtable, userdata);
    return p.GiveOwnership();
}

extern "C" void dm_noesis_xaml_provider_destroy(void* provider) {
    if (!provider) return;
    static_cast<Noesis::XamlProvider*>(provider)->Release();
}

extern "C" void dm_noesis_set_xaml_provider(void* provider) {
    Noesis::GUI::SetXamlProvider(static_cast<Noesis::XamlProvider*>(provider));
}

// ── XAML load + generic release ────────────────────────────────────────────

extern "C" void* dm_noesis_gui_load_xaml(const char* uri) {
    if (!uri) return nullptr;
    Noesis::Ptr<Noesis::BaseComponent> component =
        Noesis::GUI::LoadXaml(Noesis::Uri(uri));
    if (!component) return nullptr;
    // GUI::CreateView wants a FrameworkElement*. DynamicPtrCast fails
    // predictably if the loaded root isn't one (e.g. a ResourceDictionary).
    Noesis::Ptr<Noesis::FrameworkElement> element =
        Noesis::DynamicPtrCast<Noesis::FrameworkElement>(component);
    if (!element) return nullptr;
    return element.GiveOwnership();
}

extern "C" void* dm_noesis_gui_parse_xaml(const char* text) {
    if (!text) return nullptr;
    // ParseXaml builds an object tree directly from the string — no
    // XamlProvider URI round-trip. Mirrors dm_noesis_gui_load_xaml's
    // ownership: cast the root to FrameworkElement and hand out a +1 ref.
    // Malformed XAML yields a null Ptr (the error is routed through the log
    // handler), so this returns NULL rather than crashing.
    Noesis::Ptr<Noesis::BaseComponent> component = Noesis::GUI::ParseXaml(text);
    if (!component) return nullptr;
    Noesis::Ptr<Noesis::FrameworkElement> element =
        Noesis::DynamicPtrCast<Noesis::FrameworkElement>(component);
    if (!element) return nullptr;
    return element.GiveOwnership();
}

extern "C" bool dm_noesis_gui_load_component(void* component, const char* uri) {
    if (!component || !uri) return false;
    // LoadComponent populates an existing instance (the code-behind / x:Class
    // pattern). `component` is borrowed; Noesis does not take ownership of the
    // caller's ref. Meaningful population requires the instance's reflected
    // type to match the XAML root's x:Class.
    Noesis::GUI::LoadComponent(
        static_cast<Noesis::BaseComponent*>(component), Noesis::Uri(uri));
    return true;
}

extern "C" void dm_noesis_base_component_release(void* obj) {
    if (!obj) return;
    static_cast<Noesis::BaseComponent*>(obj)->Release();
}

// Add a +1 reference to any BaseComponent and return it (NULL on NULL input).
// Lets Rust promote a borrowed component pointer (e.g. a hit-test visual handed
// to a callback, or GetRenderTransform's borrowed result) into an owning
// handle. Balance with dm_noesis_base_component_release.
extern "C" void* dm_noesis_base_component_add_reference(void* obj) {
    if (!obj) return nullptr;
    static_cast<Noesis::BaseComponent*>(obj)->AddReference();
    return obj;
}

// Current strong reference count of any BaseComponent (BaseRefCounted::
// GetNumReferences). Returns 0 on NULL input. The absolute value is an internal
// detail — callers should reason about deltas (AddReference => +1, Release =>
// -1), not the raw number.
extern "C" int32_t dm_noesis_base_component_get_num_references(void* obj) {
    if (!obj) return 0;
    return static_cast<Noesis::BaseComponent*>(obj)->GetNumReferences();
}

extern "C" bool dm_noesis_gui_load_application_resources(const char* uri) {
    if (!uri) return false;
    Noesis::Ptr<Noesis::ResourceDictionary> dict =
        Noesis::GUI::LoadXaml<Noesis::ResourceDictionary>(Noesis::Uri(uri));
    if (!dict) return false;
    Noesis::GUI::SetApplicationResources(dict);
    return true;
}

// Experimental: install application resources by building the merged-
// dictionary chain manually in C++ so each leaf loads with the parent
// `ResourceDictionary` already wired into application resources.
//
// Why: `LoadXaml<ResourceDictionary>(parent_uri)` parses the parent and
// recursively parses each `<ResourceDictionary Source="..."/>` in
// `MergedDictionaries`. The leaves are parsed in isolation — their
// internal `{StaticResource SiblingKey}` lookups can't see siblings that
// haven't been parsed yet (or even the ones that already have, if the
// resolver only walks the leaf's own logical tree). This is the
// hommlet-side "Brushes.xaml self-merges Colors.xaml" workaround
// territory.
//
// This variant takes the list of leaf URIs explicitly (in dependency
// order). It constructs an empty parent `ResourceDictionary`, installs
// it as application resources before loading anything, then for each
// leaf URI: creates an empty child, adds it to `parent.MergedDictionaries`
// (parent scope is now visible to the child), and assigns
// `child.Source = uri` to trigger the load. Each leaf parses with the
// growing parent context already live.
extern "C" bool dm_noesis_gui_install_app_resources_chain(
    const char* const* uris, uint32_t count)
{
    if (!uris || count == 0) return false;
    Noesis::Ptr<Noesis::ResourceDictionary> parent = *new Noesis::ResourceDictionary();
    Noesis::GUI::SetApplicationResources(parent);
    for (uint32_t i = 0; i < count; ++i) {
        const char* leafUri = uris[i];
        if (!leafUri) continue;
        Noesis::Ptr<Noesis::ResourceDictionary> child = *new Noesis::ResourceDictionary();
        parent->GetMergedDictionaries()->Add(child);
        child->SetSource(Noesis::Uri(leafUri));
    }
    return true;
}

// ── View lifecycle ─────────────────────────────────────────────────────────

extern "C" void* dm_noesis_view_create(void* framework_element) {
    if (!framework_element) return nullptr;
    Noesis::Ptr<Noesis::IView> view = Noesis::GUI::CreateView(
        static_cast<Noesis::FrameworkElement*>(framework_element));
    if (!view) return nullptr;
    return view.GiveOwnership();
}

extern "C" void dm_noesis_view_destroy(void* view) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->Release();
}

// Add a +1 reference to an IView and return it. Lets an owned renderer handle
// keep the view (and the IRenderer it owns) alive independently of the View
// wrapper, so the two can drive different threads (Update on the UI thread;
// UpdateRenderTree / Render on the render thread). Balance with
// dm_noesis_view_destroy. No-op (returns NULL) on a NULL view.
extern "C" void* dm_noesis_view_add_reference(void* view) {
    if (!view) return nullptr;
    static_cast<Noesis::IView*>(view)->AddReference();
    return view;
}

// ── View setters ───────────────────────────────────────────────────────────

extern "C" void dm_noesis_view_set_size(void* view, uint32_t width, uint32_t height) {
    static_cast<Noesis::IView*>(view)->SetSize(width, height);
}

extern "C" void dm_noesis_view_set_scale(void* view, float scale) {
    // DPI scale: 1.0 == 96 ppi. Scales content + hit-testing without changing
    // the surface size, so the UI stays crisp (vector re-tessellation) at any
    // display density.
    static_cast<Noesis::IView*>(view)->SetScale(scale);
}

extern "C" void dm_noesis_view_set_projection_matrix(void* view, const float* matrix) {
    // Matrix4(const float*) reads 16 floats; the native GetData() layout is
    // row-major (Vector4 mVal[4] holding rows), so we pass the Rust array
    // through untouched.
    Noesis::Matrix4 m(matrix);
    static_cast<Noesis::IView*>(view)->SetProjectionMatrix(m);
}

extern "C" bool dm_noesis_view_update(void* view, double time_seconds) {
    return static_cast<Noesis::IView*>(view)->Update(time_seconds);
}

extern "C" void dm_noesis_view_set_flags(void* view, uint32_t flags) {
    static_cast<Noesis::IView*>(view)->SetFlags(flags);
}

extern "C" void* dm_noesis_view_get_renderer(void* view) {
    return static_cast<Noesis::IView*>(view)->GetRenderer();
}

extern "C" void* dm_noesis_view_get_content(void* view) {
    if (!view) return nullptr;
    Noesis::FrameworkElement* content = static_cast<Noesis::IView*>(view)->GetContent();
    if (!content) return nullptr;
    // GetContent returns a non-owning raw pointer (the View owns the +1 ref
    // it took at CreateView time). Bump the count so callers can release it
    // through the standard FrameworkElement drop path.
    content->AddReference();
    return content;
}

// ── Renderer ───────────────────────────────────────────────────────────────

extern "C" void dm_noesis_renderer_init(void* renderer, void* render_device) {
    static_cast<Noesis::IRenderer*>(renderer)->Init(
        static_cast<Noesis::RenderDevice*>(render_device));
}

extern "C" void dm_noesis_renderer_shutdown(void* renderer) {
    static_cast<Noesis::IRenderer*>(renderer)->Shutdown();
}

extern "C" bool dm_noesis_renderer_update_render_tree(void* renderer) {
    return static_cast<Noesis::IRenderer*>(renderer)->UpdateRenderTree();
}

extern "C" bool dm_noesis_renderer_render_offscreen(void* renderer) {
    return static_cast<Noesis::IRenderer*>(renderer)->RenderOffscreen();
}

extern "C" void dm_noesis_renderer_render(void* renderer, bool flip_y, bool clear) {
    static_cast<Noesis::IRenderer*>(renderer)->Render(flip_y, clear);
}

// ── Stereo / VR rendering (TODO §1) ─────────────────────────────────────────
// `RenderStereo` overloads (the non-deprecated VR path). Each eye matrix is a
// row-major 4×4 read straight from 16 Rust floats, same layout convention as
// dm_noesis_view_set_projection_matrix. Culling always uses the view's
// projection matrix, so the eye matrices must be enclosed by it.

// Multi-pass stereo: one eye per call (call twice, once per eye, into the
// matching render target).
extern "C" void dm_noesis_renderer_render_stereo(
    void* renderer, const float* eye_matrix, bool flip_y, bool clear)
{
    if (!renderer || !eye_matrix) return;
    Noesis::Matrix4 eye(eye_matrix);
    static_cast<Noesis::IRenderer*>(renderer)->RenderStereo(eye, flip_y, clear);
}

// Single-pass stereo: both eyes in one call (e.g. multiview / instanced VR).
extern "C" void dm_noesis_renderer_render_stereo_both(
    void* renderer, const float* left_eye_matrix, const float* right_eye_matrix,
    bool flip_y, bool clear)
{
    if (!renderer || !left_eye_matrix || !right_eye_matrix) return;
    Noesis::Matrix4 left(left_eye_matrix);
    Noesis::Matrix4 right(right_eye_matrix);
    static_cast<Noesis::IRenderer*>(renderer)->RenderStereo(left, right, flip_y, clear);
}

// ── View input ─────────────────────────────────────────────────────────────
//
// The safe wrappers in `src/view.rs` define `MouseButton` and `Key` enums with
// explicit discriminants. Assert each ordinal here so any accidental drift
// between Noesis SDK versions fails dm_noesis's own C++ compile, long before
// a wrong-key bug shows up at runtime.

static_assert((int32_t)Noesis::MouseButton_Left == 0, "MouseButton::Left");
static_assert((int32_t)Noesis::MouseButton_Right == 1, "MouseButton::Right");
static_assert((int32_t)Noesis::MouseButton_Middle == 2, "MouseButton::Middle");
static_assert((int32_t)Noesis::MouseButton_XButton1 == 3, "MouseButton::XButton1");
static_assert((int32_t)Noesis::MouseButton_XButton2 == 4, "MouseButton::XButton2");

static_assert((int32_t)Noesis::Key_None == 0, "Key::None");
static_assert((int32_t)Noesis::Key_Back == 2, "Key::Back");
static_assert((int32_t)Noesis::Key_Tab == 3, "Key::Tab");
static_assert((int32_t)Noesis::Key_Return == 6, "Key::Return");
static_assert((int32_t)Noesis::Key_Pause == 7, "Key::Pause");
static_assert((int32_t)Noesis::Key_CapsLock == 8, "Key::CapsLock");
static_assert((int32_t)Noesis::Key_Escape == 13, "Key::Escape");
static_assert((int32_t)Noesis::Key_Space == 18, "Key::Space");
static_assert((int32_t)Noesis::Key_PageUp == 19, "Key::PageUp");
static_assert((int32_t)Noesis::Key_PageDown == 20, "Key::PageDown");
static_assert((int32_t)Noesis::Key_End == 21, "Key::End");
static_assert((int32_t)Noesis::Key_Home == 22, "Key::Home");
static_assert((int32_t)Noesis::Key_Left == 23, "Key::Left");
static_assert((int32_t)Noesis::Key_Up == 24, "Key::Up");
static_assert((int32_t)Noesis::Key_Right == 25, "Key::Right");
static_assert((int32_t)Noesis::Key_Down == 26, "Key::Down");
static_assert((int32_t)Noesis::Key_PrintScreen == 30, "Key::PrintScreen");
static_assert((int32_t)Noesis::Key_Insert == 31, "Key::Insert");
static_assert((int32_t)Noesis::Key_Delete == 32, "Key::Delete");
static_assert((int32_t)Noesis::Key_Help == 33, "Key::Help");
static_assert((int32_t)Noesis::Key_D0 == 34, "Key::D0");
static_assert((int32_t)Noesis::Key_D9 == 43, "Key::D9");
static_assert((int32_t)Noesis::Key_A == 44, "Key::A");
static_assert((int32_t)Noesis::Key_Z == 69, "Key::Z");
static_assert((int32_t)Noesis::Key_LWin == 70, "Key::LWin");
static_assert((int32_t)Noesis::Key_RWin == 71, "Key::RWin");
static_assert((int32_t)Noesis::Key_Apps == 72, "Key::Apps");
static_assert((int32_t)Noesis::Key_NumPad0 == 74, "Key::NumPad0");
static_assert((int32_t)Noesis::Key_NumPad9 == 83, "Key::NumPad9");
static_assert((int32_t)Noesis::Key_Multiply == 84, "Key::Multiply");
static_assert((int32_t)Noesis::Key_Add == 85, "Key::Add");
static_assert((int32_t)Noesis::Key_Subtract == 87, "Key::Subtract");
static_assert((int32_t)Noesis::Key_Decimal == 88, "Key::Decimal");
static_assert((int32_t)Noesis::Key_Divide == 89, "Key::Divide");
static_assert((int32_t)Noesis::Key_F1 == 90, "Key::F1");
static_assert((int32_t)Noesis::Key_F24 == 113, "Key::F24");
static_assert((int32_t)Noesis::Key_NumLock == 114, "Key::NumLock");
static_assert((int32_t)Noesis::Key_Scroll == 115, "Key::ScrollLock");
static_assert((int32_t)Noesis::Key_LeftShift == 116, "Key::LeftShift");
static_assert((int32_t)Noesis::Key_RightShift == 117, "Key::RightShift");
static_assert((int32_t)Noesis::Key_LeftCtrl == 118, "Key::LeftCtrl");
static_assert((int32_t)Noesis::Key_RightCtrl == 119, "Key::RightCtrl");
static_assert((int32_t)Noesis::Key_LeftAlt == 120, "Key::LeftAlt");
static_assert((int32_t)Noesis::Key_RightAlt == 121, "Key::RightAlt");
static_assert((int32_t)Noesis::Key_OemSemicolon == 140, "Key::OemSemicolon");
static_assert((int32_t)Noesis::Key_OemPlus == 141, "Key::OemPlus");
static_assert((int32_t)Noesis::Key_OemComma == 142, "Key::OemComma");
static_assert((int32_t)Noesis::Key_OemMinus == 143, "Key::OemMinus");
static_assert((int32_t)Noesis::Key_OemPeriod == 144, "Key::OemPeriod");
static_assert((int32_t)Noesis::Key_OemQuestion == 145, "Key::OemSlash");
static_assert((int32_t)Noesis::Key_OemTilde == 146, "Key::OemTilde");
static_assert((int32_t)Noesis::Key_OemOpenBrackets == 149, "Key::OemOpenBrackets");
static_assert((int32_t)Noesis::Key_OemPipe == 150, "Key::OemPipe");
static_assert((int32_t)Noesis::Key_OemCloseBrackets == 151, "Key::OemCloseBrackets");
static_assert((int32_t)Noesis::Key_OemQuotes == 152, "Key::OemQuotes");

extern "C" bool dm_noesis_view_mouse_move(void* view, int32_t x, int32_t y) {
    return static_cast<Noesis::IView*>(view)->MouseMove(x, y);
}

extern "C" bool dm_noesis_view_mouse_button_down(void* view, int32_t x, int32_t y, int32_t button) {
    return static_cast<Noesis::IView*>(view)
        ->MouseButtonDown(x, y, static_cast<Noesis::MouseButton>(button));
}

extern "C" bool dm_noesis_view_mouse_button_up(void* view, int32_t x, int32_t y, int32_t button) {
    return static_cast<Noesis::IView*>(view)
        ->MouseButtonUp(x, y, static_cast<Noesis::MouseButton>(button));
}

extern "C" bool dm_noesis_view_mouse_double_click(void* view, int32_t x, int32_t y, int32_t button) {
    return static_cast<Noesis::IView*>(view)
        ->MouseDoubleClick(x, y, static_cast<Noesis::MouseButton>(button));
}

extern "C" bool dm_noesis_view_mouse_wheel(void* view, int32_t x, int32_t y, int32_t delta) {
    return static_cast<Noesis::IView*>(view)->MouseWheel(x, y, delta);
}

extern "C" bool dm_noesis_view_scroll(void* view, int32_t x, int32_t y, float value) {
    return static_cast<Noesis::IView*>(view)->Scroll(x, y, value);
}

extern "C" bool dm_noesis_view_hscroll(void* view, int32_t x, int32_t y, float value) {
    return static_cast<Noesis::IView*>(view)->HScroll(x, y, value);
}

extern "C" bool dm_noesis_view_touch_down(void* view, int32_t x, int32_t y, uint64_t id) {
    return static_cast<Noesis::IView*>(view)->TouchDown(x, y, id);
}

extern "C" bool dm_noesis_view_touch_move(void* view, int32_t x, int32_t y, uint64_t id) {
    return static_cast<Noesis::IView*>(view)->TouchMove(x, y, id);
}

extern "C" bool dm_noesis_view_touch_up(void* view, int32_t x, int32_t y, uint64_t id) {
    return static_cast<Noesis::IView*>(view)->TouchUp(x, y, id);
}

extern "C" bool dm_noesis_view_key_down(void* view, int32_t key) {
    return static_cast<Noesis::IView*>(view)->KeyDown(static_cast<Noesis::Key>(key));
}

extern "C" bool dm_noesis_view_key_up(void* view, int32_t key) {
    return static_cast<Noesis::IView*>(view)->KeyUp(static_cast<Noesis::Key>(key));
}

extern "C" bool dm_noesis_view_char(void* view, uint32_t codepoint) {
    return static_cast<Noesis::IView*>(view)->Char(codepoint);
}

extern "C" void dm_noesis_view_activate(void* view) {
    static_cast<Noesis::IView*>(view)->Activate();
}

extern "C" void dm_noesis_view_deactivate(void* view) {
    static_cast<Noesis::IView*>(view)->Deactivate();
}

extern "C" bool dm_noesis_view_mouse_hwheel(void* view, int32_t x, int32_t y, int32_t delta) {
    return static_cast<Noesis::IView*>(view)->MouseHWheel(x, y, delta);
}

// ── View flags / quality / stats (TODO §1) ─────────────────────────────────

extern "C" uint32_t dm_noesis_view_get_flags(void* view) {
    if (!view) return 0;
    return static_cast<Noesis::IView*>(view)->GetFlags();
}

extern "C" void dm_noesis_view_set_tessellation_max_pixel_error(void* view, float error) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->SetTessellationMaxPixelError(
        Noesis::TessellationMaxPixelError(error));
}

extern "C" float dm_noesis_view_get_tessellation_max_pixel_error(void* view) {
    if (!view) return 0.0f;
    return static_cast<Noesis::IView*>(view)->GetTessellationMaxPixelError().error;
}

// ── Gesture / touch thresholds (TODO §1) ───────────────────────────────────
// Pure pass-through setters; all are no-ops on a null view.

extern "C" void dm_noesis_view_set_holding_time_threshold(void* view, uint32_t ms) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->SetHoldingTimeThreshold(ms);
}

extern "C" void dm_noesis_view_set_holding_distance_threshold(void* view, uint32_t pixels) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->SetHoldingDistanceThreshold(pixels);
}

extern "C" void dm_noesis_view_set_manipulation_distance_threshold(void* view, uint32_t pixels) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->SetManipulationDistanceThreshold(pixels);
}

extern "C" void dm_noesis_view_set_double_tap_time_threshold(void* view, uint32_t ms) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->SetDoubleTapTimeThreshold(ms);
}

extern "C" void dm_noesis_view_set_double_tap_distance_threshold(void* view, uint32_t pixels) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->SetDoubleTapDistanceThreshold(pixels);
}

extern "C" void dm_noesis_view_set_emulate_touch(void* view, bool emulate) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->SetEmulateTouch(emulate);
}

// ── Stereo / VR (TODO §1) ──────────────────────────────────────────────────
// Adjusts the offscreen-phase scale used when stereo eye matrices differ from
// the view projection. Must be 1.0 for non-VR; 2–3 is recommended for VR.

extern "C" void dm_noesis_view_set_stereo_offscreen_scale_factor(void* view, float factor) {
    if (!view) return;
    static_cast<Noesis::IView*>(view)->SetStereoOffscreenScaleFactor(factor);
}

// The C ABI struct mirrors `Noesis::ViewStats` field-for-field; guard the size
// so a future SDK field addition can't silently desync the copy below.
static_assert(sizeof(dm_noesis_view_stats) == sizeof(Noesis::ViewStats),
    "dm_noesis_view_stats must match Noesis::ViewStats layout");

extern "C" void dm_noesis_view_get_stats(void* view, dm_noesis_view_stats* out) {
    if (!view || !out) return;
    const Noesis::ViewStats s = static_cast<Noesis::IView*>(view)->GetStats();
    out->frame_time = s.frameTime;
    out->update_time = s.updateTime;
    out->render_time = s.renderTime;
    out->triangles = s.triangles;
    out->draws = s.draws;
    out->batches = s.batches;
    out->tessellations = s.tessellations;
    out->flushes = s.flushes;
    out->geometry_size = s.geometrySize;
    out->masks = s.masks;
    out->opacities = s.opacities;
    out->render_target_switches = s.renderTargetSwitches;
    out->uploaded_ramps = s.uploadedRamps;
    out->rasterized_glyphs = s.rasterizedGlyphs;
    out->discarded_glyph_tiles = s.discardedGlyphTiles;
}

// ── View-driven timers (TODO §1) ───────────────────────────────────────────

namespace {

// Trampoline between Noesis's `Delegate<uint32_t()>` timer callback and the C
// ABI. Modelled on RustCommand (noesis_commands.cpp): the donated Rust handler
// box is owned here and freed via `mFree` in the destructor, exactly once.
// Holds a +1 ref on the IView so the token may safely outlive the caller's
// other view handles (the only constraint is dropping it before the Noesis
// runtime shuts down, like every other owning handle in this crate). Noesis
// stores a copy of the `Delegate` bound to this object until CancelTimer, so
// the object must stay alive until then — which the Rust RAII handle enforces.
class RustTimer {
public:
    RustTimer(Noesis::IView* view, dm_noesis_timer_fn cb, void* userdata,
              dm_noesis_timer_free_fn free_handler)
        : mView(view), mCb(cb), mUserdata(userdata), mFree(free_handler), mId(0)
    {
        if (mView) {
            mView->AddReference();
        }
    }

    ~RustTimer() {
        // Donated ownership: drop the Rust handler box here, exactly once.
        // Null first so a (currently-impossible) re-entrant teardown can't
        // double-free.
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
        if (mView) {
            mView->Release();
        }
    }

    RustTimer(const RustTimer&) = delete;
    RustTimer& operator=(const RustTimer&) = delete;

    // The Noesis timer callback: forwards to Rust and returns the next interval
    // (0 stops the timer).
    uint32_t Tick() {
        if (mCb) {
            return mCb(mUserdata);
        }
        return 0;
    }

    void setId(uint32_t id) { mId = id; }
    uint32_t id() const { return mId; }
    Noesis::IView* view() const { return mView; }

private:
    Noesis::IView* mView;  // raw + manual AddRef/Release — see ctor/dtor.
    dm_noesis_timer_fn mCb;
    void* mUserdata;
    dm_noesis_timer_free_fn mFree;
    uint32_t mId;
};

}  // namespace

extern "C" void* dm_noesis_view_create_timer(
    void* view, uint32_t interval_ms, dm_noesis_timer_fn cb, void* userdata,
    dm_noesis_timer_free_fn free_handler)
{
    if (!view || !cb) return nullptr;
    auto* v = static_cast<Noesis::IView*>(view);
    auto* timer = new RustTimer(v, cb, userdata, free_handler);
    const uint32_t id = v->CreateTimer(interval_ms, Noesis::MakeDelegate(timer, &RustTimer::Tick));
    timer->setId(id);
    return timer;
}

extern "C" void dm_noesis_view_restart_timer(void* token, uint32_t interval_ms) {
    if (!token) return;
    auto* timer = static_cast<RustTimer*>(token);
    if (auto* v = timer->view()) {
        v->RestartTimer(timer->id(), interval_ms);
    }
}

extern "C" void dm_noesis_view_cancel_timer(void* token) {
    if (!token) return;
    auto* timer = static_cast<RustTimer*>(token);
    if (auto* v = timer->view()) {
        v->CancelTimer(timer->id());
    }
    delete timer;  // dtor frees the donated userdata + releases the view ref.
}

// ── Rendering event (TODO §1) ──────────────────────────────────────────────

namespace {

// Trampoline for `IView::Rendering()` — a `Delegate<void(IView*)>` fired after
// animation/layout, just before the composition tree is rendered. Lifetime
// mirrors RustTimer: the donated Rust handler box is owned here and freed via
// `mFree` in the destructor exactly once, and a +1 ref on the IView keeps the
// handle safely usable until it is removed. The Delegate is registered with
// `+=` on construction and detached with `-=` on destruction, so the bound
// object must (and does, via the Rust RAII handle) outlive registration.
class RustRenderingHandler {
public:
    RustRenderingHandler(Noesis::IView* view, dm_noesis_rendering_fn cb,
                         void* userdata, dm_noesis_rendering_free_fn free_handler)
        : mView(view), mCb(cb), mUserdata(userdata), mFree(free_handler)
    {
        mView->AddReference();
        mView->Rendering() += Noesis::MakeDelegate(this, &RustRenderingHandler::OnRendering);
    }

    ~RustRenderingHandler() {
        mView->Rendering() -= Noesis::MakeDelegate(this, &RustRenderingHandler::OnRendering);
        // Donated ownership: drop the Rust handler box here, exactly once.
        void* ud = mUserdata;
        mUserdata = nullptr;
        if (mFree && ud) {
            mFree(ud);
        }
        mView->Release();
    }

    RustRenderingHandler(const RustRenderingHandler&) = delete;
    RustRenderingHandler& operator=(const RustRenderingHandler&) = delete;

private:
    void OnRendering(Noesis::IView* view) {
        if (mCb) {
            mCb(mUserdata, view);
        }
    }

    Noesis::IView* mView;  // raw + manual AddRef/Release — see ctor/dtor.
    dm_noesis_rendering_fn mCb;
    void* mUserdata;
    dm_noesis_rendering_free_fn mFree;
};

}  // namespace

extern "C" void* dm_noesis_view_add_rendering_handler(
    void* view, dm_noesis_rendering_fn cb, void* userdata,
    dm_noesis_rendering_free_fn free_handler)
{
    if (!view || !cb) return nullptr;
    auto* v = static_cast<Noesis::IView*>(view);
    return new RustRenderingHandler(v, cb, userdata, free_handler);
}

extern "C" void dm_noesis_view_remove_rendering_handler(void* token) {
    if (!token) return;
    // dtor detaches the delegate, frees the donated userdata, releases the ref.
    delete static_cast<RustRenderingHandler*>(token);
}
