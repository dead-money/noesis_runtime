// Narrow C ABI shim over the Noesis Native SDK.
//
// This is the ONLY header dm_noesis/src binds against. Rust declarations live
// in src/ffi.rs and are hand-mirrored — we do NOT bindgen NsCore/NsGui (their
// templates + Ptr<T> + virtual-dispatch surface does not translate cleanly).
//
// Phase 0 surface: lifecycle and version. Render device, View, input, XAML
// loading land in subsequent phases — see ../dm_noesis_bevy/CLAUDE.md for the
// phase plan.

#ifndef DM_NOESIS_SHIM_H
#define DM_NOESIS_SHIM_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef enum dm_noesis_log_level {
    DM_NOESIS_LOG_TRACE   = 0,
    DM_NOESIS_LOG_DEBUG   = 1,
    DM_NOESIS_LOG_INFO    = 2,
    DM_NOESIS_LOG_WARNING = 3,
    DM_NOESIS_LOG_ERROR   = 4
} dm_noesis_log_level;

typedef void (*dm_noesis_log_fn)(
    void* userdata,
    const char* file,
    uint32_t line,
    dm_noesis_log_level level,
    const char* channel,
    const char* message);

// Optional. Apply per-developer Indie license credentials. Call BEFORE
// dm_noesis_init. Pass empty strings to leave Noesis in trial mode.
void dm_noesis_set_license(const char* name, const char* key);

// Optional. Install a logging callback. Call BEFORE dm_noesis_init to capture
// init-time messages. Pass NULL to clear.
void dm_noesis_set_log_handler(dm_noesis_log_fn cb, void* userdata);

// Initialize Noesis subsystems. Call exactly once per process; Noesis does not
// support re-init after shutdown.
void dm_noesis_init(void);

// Shut Noesis down. Call once at process exit, after all Noesis-owned objects
// have been released.
void dm_noesis_shutdown(void);

// Returns the Noesis runtime build version (e.g. "3.2.12"). The pointer is
// owned by the Noesis runtime; do not free.
const char* dm_noesis_version(void);

// ── Render device (Phase 1) ────────────────────────────────────────────────
//
// The Rust side implements `Noesis::RenderDevice` by:
//   1. Constructing a `dm_noesis_render_device_vtable` of trampoline fn ptrs.
//   2. Calling `dm_noesis_render_device_create(&vtable, userdata)`.
//   3. Receiving back an opaque `void*` that is actually a Noesis::RenderDevice*
//      (specifically, an instance of the C++-internal RustRenderDevice subclass
//      that forwards every virtual into the vtable).
//   4. Calling `dm_noesis_render_device_destroy(device)` exactly once at end of
//      life. The C++-side intrusive ref count handles transitively-owned
//      textures and render targets.

// Texture metadata returned by the `create_texture` vtable slot. Mirrored on
// the Rust side as `crate::ffi::TextureBindingFfi` with the same layout.
typedef struct dm_noesis_texture_binding {
    uint64_t handle;       // 0 reserved invalid; valid handles are nonzero
    uint32_t width;
    uint32_t height;
    bool has_mipmaps;
    bool inverted;
    bool has_alpha;
    uint8_t pad;           // explicit so Rust mirror is unambiguous
} dm_noesis_texture_binding;

// Render-target metadata returned by `create_render_target` / `clone_render_target`.
typedef struct dm_noesis_render_target_binding {
    uint64_t handle;
    dm_noesis_texture_binding resolve_texture;
} dm_noesis_render_target_binding;

// vtable of fn pointers the Rust side fills in. The C++ subclass copies this
// struct on construction and dispatches every virtual through it.
//
// Pointer params marked `void*` carry POD struct pointers whose layouts the
// Rust side mirrors with `#[repr(C)]`:
//   - `out_caps`     → `Noesis::DeviceCaps*`     (= Rust `types::DeviceCaps`)
//   - `tile`/`tiles` → `const Noesis::Tile*`     (= Rust `types::Tile`)
//   - `batch`        → `const Noesis::Batch*`    (= Rust `types::Batch`)
//
// `data` in `create_texture` is `NULL` for dynamic textures, otherwise an
// array of `levels` `const void*` mip pointers (each tightly packed).
typedef struct dm_noesis_render_device_vtable {
    void (*get_caps)(void* userdata, void* out_caps);

    void (*create_texture)(
        void* userdata,
        const char* label, uint32_t width, uint32_t height, uint32_t levels,
        uint32_t format, const void* const* data,
        dm_noesis_texture_binding* out);
    // `format` is forwarded from the texture's create-time format so the Rust
    // side can construct an exact-length `&[u8]` from `data` without having to
    // track per-handle metadata separately.
    void (*update_texture)(
        void* userdata, uint64_t handle, uint32_t level,
        uint32_t x, uint32_t y, uint32_t width, uint32_t height,
        uint32_t format, const void* data);
    void (*end_updating_textures)(void* userdata, const uint64_t* handles, uint32_t count);
    void (*drop_texture)(void* userdata, uint64_t handle);

    void (*create_render_target)(
        void* userdata,
        const char* label, uint32_t width, uint32_t height,
        uint32_t sample_count, bool needs_stencil,
        dm_noesis_render_target_binding* out);
    void (*clone_render_target)(
        void* userdata, const char* label, uint64_t src_handle,
        dm_noesis_render_target_binding* out);
    void (*drop_render_target)(void* userdata, uint64_t handle);

    void (*begin_offscreen_render)(void* userdata);
    void (*end_offscreen_render)(void* userdata);
    void (*begin_onscreen_render)(void* userdata);
    void (*end_onscreen_render)(void* userdata);

    void (*set_render_target)(void* userdata, uint64_t handle);
    void (*begin_tile)(void* userdata, uint64_t handle, const void* tile);
    void (*end_tile)(void* userdata, uint64_t handle);
    void (*resolve_render_target)(
        void* userdata, uint64_t handle, const void* tiles, uint32_t count);

    void* (*map_vertices)(void* userdata, uint32_t bytes);
    void  (*unmap_vertices)(void* userdata);
    void* (*map_indices)(void* userdata, uint32_t bytes);
    void  (*unmap_indices)(void* userdata);

    void (*draw_batch)(void* userdata, const void* batch);
} dm_noesis_render_device_vtable;

// Create a `RustRenderDevice` instance, returning an opaque
// `Noesis::RenderDevice*` with intrusive ref count = 1. Call
// `dm_noesis_render_device_destroy` exactly once to release.
//
// Returns `NULL` on bad input (null vtable).
void* dm_noesis_render_device_create(
    const dm_noesis_render_device_vtable* vtable, void* userdata);

// Release the +1 reference held by `_create`'s caller. The actual destruction
// happens when the last `Ptr<>` goes away — including any Noesis-internal
// references — which transitively releases all `RustTexture` / `RustRenderTarget`
// instances allocated through the device, each calling `drop_texture` /
// `drop_render_target` on the vtable.
void dm_noesis_render_device_destroy(void* device);

// Extract the Rust-side handle stored in a `RustTexture` / `RustRenderTarget`
// instance. Return 0 if the input is null.
//
// Used by the Rust `draw_batch` impl to translate `Batch.pattern/ramps/...`
// pointers back into Rust-side `TextureHandle` values.
uint64_t dm_noesis_texture_get_handle(const void* texture);
uint64_t dm_noesis_render_target_get_handle(const void* surface);

// ── XAML provider (Phase 4.C) ──────────────────────────────────────────────
//
// The Rust side subclasses `Noesis::XamlProvider` via a vtable of fn pointers.
// `dm_noesis_xaml_provider_create` returns a `Noesis::XamlProvider*` (refcount
// = 1) wrapping that vtable; pair with `_destroy`. Install it globally with
// `dm_noesis_set_xaml_provider`.
//
// `load_xaml` callback contract:
//   - Return `true` with `*out_data` / `*out_len` set on success. The pointed
//     bytes must stay valid until Noesis finishes parsing the XAML, which is
//     synchronous with the `GUI::LoadXaml` call that triggered it. In practice
//     the Rust impl owns the bytes (e.g. in a HashMap) and returns a slice
//     into them.
//   - Return `false` to signal not-found; Noesis will produce a load error.

typedef struct dm_noesis_xaml_provider_vtable {
    bool (*load_xaml)(
        void* userdata,
        const char* uri,
        const uint8_t** out_data,
        uint32_t* out_len);
} dm_noesis_xaml_provider_vtable;

void* dm_noesis_xaml_provider_create(
    const dm_noesis_xaml_provider_vtable* vtable, void* userdata);
void dm_noesis_xaml_provider_destroy(void* provider);

// Install `provider` as the global XAML provider, or pass NULL to clear.
void dm_noesis_set_xaml_provider(void* provider);

// ── Font provider (Phase 4.F.1) ────────────────────────────────────────────
//
// Subclass of `Noesis::CachedFontProvider`. CachedFontProvider handles font
// matching (weight/stretch/style) internally once faces are registered; we
// only need two callbacks:
//
//   - `scan_folder(userdata, folder_uri, register_fn, register_cx)` — called
//     the first time a font is requested from a folder. Rust walks its
//     registry and invokes `register_fn(register_cx, filename)` once per
//     font file in that folder. The C++ side forwards each call to
//     `CachedFontProvider::RegisterFont(folder, filename)`, which opens
//     the file via `open_font` below to scan face metadata.
//
//   - `open_font(userdata, folder_uri, filename, out_data, out_len)` —
//     return `true` with `*out_data`/`*out_len` set; the pointed bytes
//     must stay valid until the font-stream reader finishes (same
//     contract as `load_xaml`). Return `false` to signal "not found".

typedef void (*dm_noesis_register_font_fn)(void* register_cx, const char* filename);

typedef struct dm_noesis_font_provider_vtable {
    void (*scan_folder)(
        void* userdata,
        const char* folder_uri,
        dm_noesis_register_font_fn register_fn,
        void* register_cx);

    bool (*open_font)(
        void* userdata,
        const char* folder_uri,
        const char* filename,
        const uint8_t** out_data,
        uint32_t* out_len);
} dm_noesis_font_provider_vtable;

void* dm_noesis_font_provider_create(
    const dm_noesis_font_provider_vtable* vtable, void* userdata);
void dm_noesis_font_provider_destroy(void* provider);

// Install `provider` as the global font provider, or pass NULL to clear.
void dm_noesis_set_font_provider(void* provider);

// `families` is an array of `count` NUL-terminated UTF-8 strings. Each may be
// a plain family name ("Arial") or a Noesis path-rooted family
// ("Fonts/#Bitter"). Noesis uses this list to resolve glyphs that are not
// present in the element's explicit FontFamily.
void dm_noesis_set_font_fallbacks(const char* const* families, uint32_t count);

// Register a font face directly with the provider's underlying
// `CachedFontProvider` cache, bypassing Noesis's lazy `ScanFolder` model.
// `provider` must be a pointer returned from
// `dm_noesis_font_provider_create` (a `RustFontProvider`); the folder/
// filename pair must resolve through the same `open_font` callback that
// would normally service `ScanFolder` registrations. Calling this for a
// `(folder, filename)` already registered is safe — Noesis re-opens the
// stream and re-scans face metadata; the duplicate face entry is ignored
// during `MatchFont`.
//
// Use case: when font assets land asynchronously (e.g. a Bevy
// `AssetServer`), the synchronous `ScanFolder` flow can run before all
// faces are present. Eagerly calling this once per loaded font ensures
// every face is in the cache before XAML's first `FontFamily` lookup,
// without depending on which font happened to be referenced from a
// fallback chain at scan time.
void dm_noesis_font_provider_register_font(
    void* provider, const char* folder_uri, const char* filename);

// Default font size/weight/stretch/style applied when elements don't
// specify them. `weight`, `stretch`, `style` mirror `NsGui/InputEnums.h`
// enums; see their declarations for values.
void dm_noesis_set_font_default_properties(
    float size, int32_t weight, int32_t stretch, int32_t style);

// ── Texture provider (Phase 4.E ImageBrush support) ────────────────────────
//
// Subclass of `Noesis::TextureProvider`. Two callbacks:
//
//   - `get_info(userdata, uri, out)` — return metadata (width/height and
//     optional atlas rect + dpi scale) without decoding pixels. Returning
//     `false` (or an all-zero out) signals "texture not found"; Noesis
//     falls back to the image-load path below.
//
//   - `load_texture(userdata, uri, out_width, out_height, out_data, out_len)`
//     — return RGBA8-packed pixel bytes plus dimensions. Return `true` on
//     success; the pointed bytes must stay valid for the duration of the
//     call. The C++ shim will immediately turn around and call
//     `device->CreateTexture(...)` with the data, so the ownership lifetime
//     is exactly the callback — no need to keep the pixels alive beyond.
//     Return `false` to signal "not found".

typedef struct dm_noesis_texture_info {
    uint32_t width;
    uint32_t height;
    uint32_t x;        // atlas sub-rect x; 0 for a plain image
    uint32_t y;        // atlas sub-rect y; 0 for a plain image
    float dpi_scale;   // 1.0 for 96dpi / 1:1
} dm_noesis_texture_info;

typedef struct dm_noesis_texture_provider_vtable {
    bool (*get_info)(
        void* userdata,
        const char* uri,
        dm_noesis_texture_info* out);

    bool (*load_texture)(
        void* userdata,
        const char* uri,
        uint32_t* out_width,
        uint32_t* out_height,
        const uint8_t** out_data,
        uint32_t* out_len);
} dm_noesis_texture_provider_vtable;

void* dm_noesis_texture_provider_create(
    const dm_noesis_texture_provider_vtable* vtable, void* userdata);
void dm_noesis_texture_provider_destroy(void* provider);

// Install `provider` as the global texture provider, or pass NULL to clear.
void dm_noesis_set_texture_provider(void* provider);

// ── XAML loading + View + Renderer (Phase 4.C) ─────────────────────────────
//
// Opaque pointer contracts:
//   - dm_noesis_gui_load_xaml returns a FrameworkElement* with refcount = 1.
//     Release with dm_noesis_base_component_release.
//   - dm_noesis_view_create returns an IView* with refcount = 1. Release with
//     dm_noesis_view_destroy.
//   - dm_noesis_view_get_renderer returns a borrowed IRenderer* owned by the
//     View. Do NOT release.

// Load XAML by URI. Returns a FrameworkElement* (+1 ref), or NULL if the
// resolved root isn't a FrameworkElement or the URI wasn't found.
void* dm_noesis_gui_load_xaml(const char* uri);

// Install an application-scope `ResourceDictionary` loaded from `uri`.
// Replaces any previously-installed application resources. Styles and
// brushes in the dictionary are visible to every subsequent view.
// Returns `true` if the URI resolved + parsed as a ResourceDictionary.
bool dm_noesis_gui_load_application_resources(const char* uri);

// Install application resources by building the merged-dictionary chain
// manually, leaf by leaf. `uris` is `count` leaf `ResourceDictionary`
// URIs in dependency order — earlier entries must be loadable without
// referencing later ones. Returns `true` on success; `false` for null /
// empty input. Replaces any previously-installed application resources.
//
// Sidesteps a Noesis behaviour where a top-level `LoadXaml` of a parent
// dictionary parses its `MergedDictionaries` children in isolation,
// leaving cross-sibling `{StaticResource SiblingKey}` references inside
// child bodies null-resolved at parse time. This variant creates each
// child empty, adds it to the parent's `MergedDictionaries` first, and
// only then assigns `Source` — so the parent + previously-loaded
// siblings are visible to the child during parsing.
//
// Relative-URI gotcha: each leaf is loaded via `SetSource(Uri)`, so
// relative URIs *inside* a leaf — most notably
// `<FontFamily>Folder/#Family</FontFamily>` resources — resolve against
// the leaf's own location. A `Theme/Fonts.xaml` leaf declaring
// `<FontFamily>Fonts/#X</FontFamily>` will look for family `X` in
// folder `Theme/Fonts/`, not the project-root `Fonts/`. If the
// FontProvider's `RegisterFont` calls register under `Fonts/`, the
// leaf needs `../Fonts/#X` (or absolute `/Fonts/#X` if your XAML URI
// resolver supports leading slashes).
bool dm_noesis_gui_install_app_resources_chain(
    const char* const* uris, uint32_t count);

// Release a BaseComponent-derived object.
void dm_noesis_base_component_release(void* obj);

// Create an IView whose root is `framework_element`. The view retains its own
// reference to the element; the caller's reference is still held by the
// FrameworkElement wrapper until it's dropped.
void* dm_noesis_view_create(void* framework_element);

// Release an IView* obtained from dm_noesis_view_create.
void dm_noesis_view_destroy(void* view);

void dm_noesis_view_set_size(void* view, uint32_t width, uint32_t height);

// DPI scale for the view's content (1.0 == 96 ppi). Crisp at any density.
void dm_noesis_view_set_scale(void* view, float scale);

// `matrix` is 16 floats, row-major (the native Matrix4::GetData() layout).
void dm_noesis_view_set_projection_matrix(void* view, const float* matrix);

bool dm_noesis_view_update(void* view, double time_seconds);

void dm_noesis_view_set_flags(void* view, uint32_t flags);

// Returns the IRenderer* owned by the View. Do NOT release.
void* dm_noesis_view_get_renderer(void* view);

// Borrow the View's content as an owning FrameworkElement* (refcount = +1).
// Returns NULL if the view is null or has no content. Release through
// dm_noesis_base_component_release like any other FrameworkElement* the API
// hands out.
void* dm_noesis_view_get_content(void* view);

// Initialize the renderer with `render_device`. The RenderDevice pointer is
// the opaque value returned from dm_noesis_render_device_create.
void dm_noesis_renderer_init(void* renderer, void* render_device);
void dm_noesis_renderer_shutdown(void* renderer);
bool dm_noesis_renderer_update_render_tree(void* renderer);
bool dm_noesis_renderer_render_offscreen(void* renderer);
void dm_noesis_renderer_render(void* renderer, bool flip_y, bool clear);

// ── View input (Phase 5) ───────────────────────────────────────────────────
//
// Thin trampolines over `Noesis::IView` input methods. `button` takes a
// `Noesis::MouseButton` value (see InputEnums.h); `key` takes a `Noesis::Key`.
// Out-of-range values are passed through — Noesis ignores unknown keys.
//
// Noesis requires a `MouseMove` at the press coordinate before a
// `MouseButtonDown` hits the correct element; callers must enqueue moves
// before buttons themselves.

bool dm_noesis_view_mouse_move(void* view, int32_t x, int32_t y);
bool dm_noesis_view_mouse_button_down(void* view, int32_t x, int32_t y, int32_t button);
bool dm_noesis_view_mouse_button_up(void* view, int32_t x, int32_t y, int32_t button);
bool dm_noesis_view_mouse_double_click(void* view, int32_t x, int32_t y, int32_t button);
bool dm_noesis_view_mouse_wheel(void* view, int32_t x, int32_t y, int32_t delta);
bool dm_noesis_view_scroll(void* view, int32_t x, int32_t y, float value);
bool dm_noesis_view_hscroll(void* view, int32_t x, int32_t y, float value);

bool dm_noesis_view_touch_down(void* view, int32_t x, int32_t y, uint64_t id);
bool dm_noesis_view_touch_move(void* view, int32_t x, int32_t y, uint64_t id);
bool dm_noesis_view_touch_up(void* view, int32_t x, int32_t y, uint64_t id);

bool dm_noesis_view_key_down(void* view, int32_t key);
bool dm_noesis_view_key_up(void* view, int32_t key);
bool dm_noesis_view_char(void* view, uint32_t codepoint);

void dm_noesis_view_activate(void* view);
void dm_noesis_view_deactivate(void* view);

// ── Element traversal + events (Phase 5.B) ─────────────────────────────────
//
// Look up named elements in the logical / visual tree and subscribe Rust
// callbacks to routed events. Currently exposes `BaseButton::Click` only —
// extend with sibling functions when other events earn it. The pattern (a
// heap-allocated handler that owns its registration) generalizes cleanly.

// Look up an element by `x:Name` rooted at `element`. Returns a
// FrameworkElement* with refcount = +1 for the caller (release via
// dm_noesis_base_component_release), or NULL if `name` is not found or
// if the resolved object is not a FrameworkElement (e.g. it's a Brush
// stored in a ResourceDictionary that happens to share the namescope).
void* dm_noesis_framework_element_find_name(void* element, const char* name);

// Borrowed view of an element's `x:Name`. NULL when the element has no name.
// The string is owned by Noesis; caller must not free, must not assume it
// outlives the next layout pass (in practice Noesis stores names as static
// strings, but the contract is "don't keep the pointer past your borrow").
const char* dm_noesis_framework_element_get_name(void* element);

// Set `UIElement::Visibility` on `element` — `true` → Visible, `false` →
// Collapsed. (Hidden — the third Visibility value, where the element
// reserves layout space but doesn't paint — isn't exposed; modal/overlay
// patterns want Collapsed, and a future API can add the third state if
// needed.) Safe to call with NULL.
void dm_noesis_framework_element_set_visibility(void* element, bool visible);

// Set `FrameworkElement::Margin` on `element` (layout offsets in DIPs: left,
// top, right, bottom). Paired with a Left/Top-anchored element, a margin of
// (x, y, 0, 0) places its corner at (x, y) — the positioning primitive a
// floating menu/popup needs (Noesis's Canvas.Left/Top attached property isn't
// exposed here). Safe to call with NULL.
void dm_noesis_framework_element_set_margin(
    void* element, float left, float top, float right, float bottom);

// Click-event callback. Invoked from inside `IView::Update` (or another
// input-pump method, depending on which event raised the click) on whatever
// thread is driving the view. Keep work in the callback small — push to a
// queue and process from a regular system step if you need anything heavy.
typedef void (*dm_noesis_click_fn)(void* userdata);

// Subscribe `cb(userdata)` to `BaseButton::Click` on `element`. Returns an
// opaque token (an internal handler) that you must pass to
// `dm_noesis_unsubscribe_click` exactly once when you're done. Returns NULL
// if `element` is not castable to `BaseButton` (e.g. it's a ContentControl
// or a UserControl with no inner button), or if `cb` is NULL.
//
// The token holds a +1 ref on the underlying button so the subscription
// stays valid even if the caller drops every other reference to the
// element. Release the token before `dm_noesis_shutdown` like every other
// owning handle in this API.
void* dm_noesis_subscribe_click(
    void* element, dm_noesis_click_fn cb, void* userdata);

// Unsubscribe and free the handler. Safe to call with NULL.
void dm_noesis_unsubscribe_click(void* token);

// KeyDown-event callback. Invoked from inside the input pump on whatever
// thread is driving the view, same threading contract as `dm_noesis_click_fn`.
//
// `key` is the raw `Noesis::Key` ordinal — see `view::Key` in src/view.rs for
// the safe enum mirror. `out_handled` is a borrowed pointer pre-cleared to
// `false`; the callback may set `*out_handled = true` to stop the routed
// event propagating (equivalent to setting `KeyEventArgs::handled` in C++).
typedef void (*dm_noesis_keydown_fn)(void* userdata, int32_t key, bool* out_handled);

// Subscribe `cb(userdata, key, out_handled)` to `UIElement::KeyDown` on
// `element`. Returns an opaque token (an internal handler) that you must
// pass to `dm_noesis_unsubscribe_keydown` exactly once when you're done.
// Returns NULL if `element` is not castable to `UIElement` (essentially
// every visual element is, but the cast can fail e.g. for a raw `Brush`
// returned from a ResourceDictionary lookup) or if `cb` is NULL.
//
// The token holds a +1 ref on the element so the subscription stays valid
// even if the caller drops every other reference. Release the token before
// `dm_noesis_shutdown` like every other owning handle in this API.
void* dm_noesis_subscribe_keydown(
    void* element, dm_noesis_keydown_fn cb, void* userdata);

// Unsubscribe and free the keydown handler. Safe to call with NULL.
void dm_noesis_unsubscribe_keydown(void* token);

// ── Generic routed-event subscription (TODO §5) ─────────────────────────────
//
// One name-keyed mechanism for the whole routed-event surface (mouse, keyboard,
// focus, lifecycle, touch/manipulation, drag/drop) on top of
// `UIElement::AddHandler`. Supersedes the bespoke click/keydown wrappers above
// (which are kept for source compatibility).

// Generic routed-event callback. `args` is an opaque handle to the live event
// arguments — pass it to the `dm_noesis_*_args_*` accessors below to read typed
// fields (position, button, key, wheel delta, new size, source). It is valid
// ONLY for the duration of the call. `out_handled` is a borrowed bool the shim
// pre-seeds with the event's current handled state; write `true` to mark the
// routed event handled (stops same-element handlers that opted out of
// handledEventsToo, and cross-element bubbling/tunneling). Same threading
// contract as `dm_noesis_click_fn`.
typedef void (*dm_noesis_routed_event_fn)(void* userdata, const void* args, bool* out_handled);

// Subscribe `cb` to the routed event named `event_name` on `element` (which is
// DynamicCast to `UIElement*`). Names are the WPF/Noesis event names:
// "MouseMove", "MouseLeftButtonDown", "MouseWheel", "KeyDown", "KeyUp",
// "GotFocus", "LostFocus", "Loaded", "Unloaded", "SizeChanged", "TextInput",
// "Drop", "Tapped", ... A curated table maps the common names to the precise
// arg shape; any other name falls back to the SDK's `FindRoutedEvent` lookup
// over the element's class hierarchy (only the source/handled accessors apply).
//
// `handled_too`: this SDK's `AddHandler` has no `handledEventsToo` parameter,
// so already-handled events are not re-routed to the handler regardless. The
// flag is honoured WITHIN a single element's handler chain: when `false`, the
// callback is skipped if a prior handler on the same element already set
// handled. Pass `true` to always run.
//
// Returns an opaque token to pass once to `dm_noesis_unsubscribe_event`, or
// NULL if `element` is not a `UIElement`, `event_name` is unknown, or `cb` is
// NULL. The token holds a +1 ref on the element so the subscription outlives
// every other handle the caller drops.
void* dm_noesis_subscribe_event(
    void* element, const char* event_name, bool handled_too, dm_noesis_routed_event_fn cb,
    void* userdata);

// Unsubscribe and free the routed-event handler. Safe to call with NULL.
void dm_noesis_unsubscribe_event(void* token);

// Event-arg accessors. Each takes the opaque `args` handed to the callback and
// returns a sentinel when the live event isn't of the matching kind (so one
// generic callback can probe whatever arrived).

// Mouse pointer position in the source element's coordinate space. Works for
// mouse, mouse-button and mouse-wheel events. Returns false (writes nothing)
// for other event kinds or a NULL handle.
bool dm_noesis_mouse_args_position(const void* args, float* x, float* y);

// Changed mouse button as a `Noesis::MouseButton` ordinal (mirror in
// `view::MouseButton`). Returns -1 unless the event is a mouse-button event.
int32_t dm_noesis_mouse_button_args_button(const void* args);

// Mouse wheel rotation delta (signed; ~120 per notch). Returns 0 unless the
// event is a mouse-wheel event.
int32_t dm_noesis_mouse_wheel_args_delta(const void* args);

// Pressed/released key as a `Noesis::Key` ordinal (mirror in `view::Key`).
// Returns -1 unless the event is a key event.
int32_t dm_noesis_key_args_key(const void* args);

// Input character (UTF-32 code point) for a TextInput event. Returns -1 unless
// the event is a text-composition event.
int32_t dm_noesis_text_args_ch(const void* args);

// New size for a SizeChanged event (DIPs). Returns false (writes nothing)
// unless the event is a SizeChanged event.
bool dm_noesis_size_changed_args_new_size(const void* args, float* width, float* height);

// Borrowed pointer to the event's originating element (`RoutedEventArgs::source`),
// or NULL. Not ref-counted — do not release; valid only for the callback.
void* dm_noesis_routed_args_source(const void* args);

// ── Text + focus helpers ───────────────────────────────────────────────────
//
// Read / write the `Text` property of a `TextBox` or `TextBlock`, and move
// keyboard focus to a named element. The console plugin uses these to
// populate the log surface, mirror the input box, and grab focus on open.
//
// Callers should resolve the element via `dm_noesis_framework_element_find_name`
// first; the helpers `DynamicCast` to the concrete type and no-op safely if
// the element is not a Text* / not a UIElement.

// Read `Text` from a TextBox or TextBlock. Returns NULL if `element` is null
// or not a Text* element. The returned string is owned by Noesis (specifically
// the BaseTextBox::TextContainer / TextBlock::Text storage); do not free, do
// not assume it outlives the next layout pass — copy if needed.
const char* dm_noesis_text_get(void* element);

// Write `Text` on a TextBox or TextBlock. `text == NULL` is treated as the
// empty string. Returns `false` if `element` is null or not a Text* element.
bool dm_noesis_text_set(void* element, const char* text);

// Move the caret of a TextBox to the end of its current text (i.e. set
// `CaretIndex = strlen(Text)`). No-op (returns `false`) if `element` is null
// or not a TextBox. Used by command-history navigation so the cursor sits
// past the end of the just-restored entry.
bool dm_noesis_text_caret_to_end(void* element);

// Move keyboard focus to `element`. Equivalent to `UIElement::Focus()` —
// returns the focusable result Noesis reports (the element accepted focus).
// `false` for null input or an element that cannot receive focus (e.g. a
// disabled or non-focusable element).
bool dm_noesis_focus_element(void* element);

// Assign a `Path` element's `Data` to an open polyline through `count` (x, y)
// pairs in `xy` (length `2*count`, in the Path's local coordinate space). Built
// via a StreamGeometry, so it is a real vector trace (the live oscilloscope).
// Returns `false` for null/short input or an element that is not a `Path`.
bool dm_noesis_path_set_points(void* element, const float* xy, uint32_t count);

// Transition a templated control to the visual state named `state` via
// `VisualStateManager::GoToState`, optionally running the state's
// VisualTransition (`use_transitions`). `element` is DynamicCast to
// `FrameworkElement*`; GoToState only finds state groups on a control's
// ControlTemplate, so a non-templated element or an unknown state name both
// return `false`. Returns `false` for null input as well.
bool dm_noesis_visual_state_go_to_state(
    void* element, const char* state, bool use_transitions);

// ── Custom XAML class registration (Phase 5.C) ─────────────────────────────
//
// Register Rust-backed types so XAML can instantiate them by name (`<aor:Foo>`)
// and bind their dependency properties. This is the C++/Rust analogue of
// what Noesis's C# / Unity binding does for managed code: a per-base-type
// trampoline subclass + a runtime-built `TypeClassBuilder` per consumer-named
// type + Factory creator + UIElementData with the consumer's DPs.
//
// Usage flow (Rust side):
//   1. dm_noesis_class_register("AOR.NineSlicer", DM_NOESIS_BASE_CONTENT_CONTROL,
//      cb, userdata) → class_token.
//   2. dm_noesis_class_register_property(token, "Source",
//      DM_NOESIS_PROP_BASE_COMPONENT, NULL) → prop_index.
//      ...repeat for each DP. Indices are dense (0, 1, 2, ...) in registration
//      order and identify the DP in the changed callback.
//   3. Load XAML that uses `<aor:NineSlicer Source="..." />`.
//      Noesis instantiates a trampoline; every property write fires `cb` with
//      `(userdata, instance, prop_index, value_ptr)`.
//   4. From Rust, dm_noesis_instance_set_property(instance, idx, value_ptr)
//      writes back computed values; dm_noesis_instance_get_property reads.
//   5. dm_noesis_class_unregister(token) at process shutdown, after all
//      instances are released.
//
// Registration must complete BEFORE the first XAML referencing the class
// loads. Unregistration must happen AFTER the last instance is released
// (typically: just before dm_noesis_shutdown).

typedef enum dm_noesis_class_base {
    DM_NOESIS_BASE_CONTENT_CONTROL = 0,
    // Future bases (Control, UserControl, FrameworkElement, Panel) plug in
    // by adding sibling trampoline subclasses in noesis_classes.cpp.
} dm_noesis_class_base;

// Property value-type tag. Determines the layout of `value_ptr` /
// `default_ptr` / `out_value` buffers in the FFI:
//
//   INT32         → const int32_t*
//   UINT32        → const uint32_t* (4 bytes; e.g. Grid.Row / Grid.Column,
//                   declared uint32_t in Noesis)
//   FLOAT         → const float*
//   DOUBLE        → const double*
//   BOOL          → const bool* (one byte; nonzero = true)
//   STRING        → const char* const* (pointer to a NUL-terminated UTF-8 string;
//                   on `set` the bytes are copied; on `get`/changed callback the
//                   pointer borrows from Noesis-owned storage and must not be
//                   freed; copy if you need to keep it past the next layout pass)
//   THICKNESS     → const float[4]: left, top, right, bottom (matches
//                   Noesis::Thickness layout)
//   COLOR         → const float[4]: r, g, b, a (matches Noesis::Color layout)
//   RECT          → const float[4]: x, y, width, height (matches Noesis::Rect)
//   IMAGE_SOURCE  → BaseComponent* (a Noesis::ImageSource subclass; ownership
//                   convention matches dm_noesis_base_component_release — the
//                   `set` path does NOT consume the caller's ref; the `get`
//                   / changed callback yields a borrowed pointer)
//   BASE_COMPONENT → BaseComponent* (any Noesis::BaseComponent subclass; same
//                    ownership convention as IMAGE_SOURCE)
typedef enum dm_noesis_prop_type {
    DM_NOESIS_PROP_INT32          = 0,
    DM_NOESIS_PROP_FLOAT          = 1,
    DM_NOESIS_PROP_DOUBLE         = 2,
    DM_NOESIS_PROP_BOOL           = 3,
    DM_NOESIS_PROP_STRING         = 4,
    DM_NOESIS_PROP_THICKNESS      = 5,
    DM_NOESIS_PROP_COLOR          = 6,
    DM_NOESIS_PROP_RECT           = 7,
    DM_NOESIS_PROP_IMAGE_SOURCE   = 8,
    DM_NOESIS_PROP_BASE_COMPONENT = 9,
    DM_NOESIS_PROP_UINT32         = 10
} dm_noesis_prop_type;

// Callback fired by the trampoline subclass's `OnPropertyChanged` override.
// `instance` is the C++ object pointer (an opaque BaseComponent*), useful as
// a stable per-instance identity for the Rust side; `prop_index` is the dense
// index returned from dm_noesis_class_register_property; `value_ptr` is the
// new value in the layout determined by the property's registered type.
//
// The callback fires from inside Noesis's property pump — typically the main
// thread during XAML parse + layout + input. Keep work small; queue if heavy.
typedef void (*dm_noesis_prop_changed_fn)(
    void* userdata,
    void* instance,
    uint32_t prop_index,
    const void* value_ptr);

// Free callback invoked when the underlying ClassData is finally torn down —
// either immediately at `dm_noesis_class_unregister` (if no instances exist)
// or deferred until the last live instance is released. Receives the
// `userdata` passed to `dm_noesis_class_register` so the Rust trampoline can
// drop its boxed handler. Called exactly once per successfully-registered
// class.
typedef void (*dm_noesis_class_free_fn)(void* userdata);

// Register a Rust-backed class. Returns an opaque token to use for property
// registration + unregistration. NULL on bad input (null name, unsupported
// base, init not yet called, name already registered).
//
// `free_handler` (optional, may be NULL) is invoked exactly once when
// ClassData is finally freed — see `dm_noesis_class_free_fn`. Ownership of
// `userdata` transfers to the C++ side at registration; the Rust side must
// not free it.
void* dm_noesis_class_register(
    const char* name,
    dm_noesis_class_base base,
    dm_noesis_prop_changed_fn cb,
    void* userdata,
    dm_noesis_class_free_fn free_handler);

// Add a DependencyProperty to a registered class. `default_ptr` follows the
// per-type layout above (or NULL for a type-default zero/empty). Returns the
// dense property index, or UINT32_MAX on failure (null token, unknown type,
// duplicate property name on the same class).
//
// All properties must be registered BEFORE the first XAML referencing the
// class loads — Noesis caches the property set on the TypeClass.
uint32_t dm_noesis_class_register_property(
    void* class_token,
    const char* prop_name,
    dm_noesis_prop_type prop_type,
    const void* default_ptr);

// Unregister a class: removes from Factory + Reflection so no NEW instances
// can be created, then releases the Rust caller's reference on the
// underlying ClassData. Existing live instances retain their own references
// — the actual free + `free_handler` callback runs when the last instance
// is destroyed (which may be later than this call, e.g. when a View
// holding the instances is finally torn down). Safe to call with NULL.
void dm_noesis_class_unregister(void* class_token);

// Set a property on an instance. `instance` is the BaseComponent* delivered
// to the changed callback; `prop_index` is the dense index from registration;
// `value_ptr` follows the per-type layout. Setting fires the changed callback
// recursively if the new value differs from the current — the Rust side is
// responsible for any re-entrancy guard.
void dm_noesis_instance_set_property(
    void* instance,
    uint32_t prop_index,
    const void* value_ptr);

// Read a property from an instance. `out_value` must point to a buffer of the
// appropriate size for the property type (4 bytes for INT32/FLOAT/BOOL,
// 8 for DOUBLE, 16 for THICKNESS/COLOR/RECT, sizeof(void*) for STRING /
// IMAGE_SOURCE / BASE_COMPONENT). For STRING/component types the buffer
// receives a borrowed pointer (do not free). Returns true on success, false
// on bad input (null pointers, index out of range, type mismatch).
bool dm_noesis_instance_get_property(
    void* instance,
    uint32_t prop_index,
    void* out_value);

// Read width / height of a Noesis::ImageSource (or a subclass). Returns
// `false` and leaves the out-params untouched if `image_source` is null or
// not an ImageSource. Useful for custom controls (NineSlicer / ThreeSlicer)
// that need to compute viewboxes from the source dimensions.
//
// The pointer convention matches what the property-changed callback hands
// out for `IMAGE_SOURCE` properties: a borrowed `BaseComponent*` whose
// runtime type is an ImageSource subclass. Caller does not own a ref.
bool dm_noesis_image_source_get_size(
    void* image_source,
    float* out_width,
    float* out_height);

// ── Generic name-keyed DependencyProperty access ───────────────────────────
//
// Set / get any dependency property on any `Noesis::DependencyObject` by name,
// without registering a Rust-backed class. `obj` is an opaque
// `BaseComponent*` (e.g. a `FrameworkElement*` from find-by-name); it is
// `DynamicCast` to `DependencyObject*` internally. The property is resolved
// by `name` through the inherited class hierarchy
// (`FindDependencyProperty`).
//
// `prop_type` is a `dm_noesis_prop_type` and selects the layout of
// `value_ptr` / `out_value` exactly as on the instance path (see the enum
// docs above). Because the caller supplies the tag, it is validated against
// the property's real reflected type before any cast: value / struct types
// must match exactly; `IMAGE_SOURCE` / `BASE_COMPONENT` accept any property
// whose type is assignable to `ImageSource` / `BaseComponent`.
//
// Returns false (no-op) on: null obj/name, obj is not a DependencyObject,
// unknown property name, type-tag mismatch, or (set only) a read-only
// property. String / component `get` results borrow Noesis-owned storage —
// copy immediately (same contract as the instance getter). Never throws; does
// not call VerifyAccess(), so the caller must respect the View's thread
// affinity.
bool dm_noesis_dependency_object_set_property(
    void* obj,
    const char* name,
    uint32_t prop_type,
    const void* value_ptr);

bool dm_noesis_dependency_object_get_property(
    void* obj,
    const char* name,
    uint32_t prop_type,
    void* out_value);

// ── Element tree access (TODO §2) ───────────────────────────────────────────
//
// Visual / logical tree traversal, attached + advanced dependency-property
// access, dynamic type inference, alignment, namescope register/unregister, and
// thread-affinity queries. Owning returns hand the caller a +1 BaseComponent*
// (release via dm_noesis_base_component_release), matching find_name. None of
// these call VerifyAccess(); respect the View's thread affinity.

// ── A. Tree traversal ───────────────────────────────────────────────────────
//
// VisualTreeHelper variants treat `element` as a `Visual*`. Children may be
// plain Visuals (not FrameworkElements); they're returned as raw +1
// BaseComponent* handles without null-filtering, so indexed traversal has no
// holes. All return NULL on null / not-a-Visual / out-of-bounds.

// Number of visual children, or 0 if `element` is not a Visual.
uint32_t dm_noesis_visual_children_count(void* element);
// Visual child at `index` (+1), or NULL.
void* dm_noesis_visual_child(void* element, uint32_t index);
// Visual parent (+1), or NULL.
void* dm_noesis_visual_parent(void* element);
// Hit-test a single point in `element`-local DIPs; returns the topmost hit
// Visual* (+1) or NULL.
void* dm_noesis_visual_hit_test(void* element, float x, float y);

// Logical-tree + FrameworkElement navigation.
//
// Logical parent (+1), via FrameworkElement::GetParent. NULL if `element` is
// not a FrameworkElement or has no logical parent.
void* dm_noesis_framework_element_logical_parent(void* element);
// Number of logical children (LogicalTreeHelper::GetChildrenCount), or 0 if
// `element` is not a FrameworkElement.
uint32_t dm_noesis_logical_children_count(void* element);
// Logical child at `index` (+1), or NULL. (GetChild returns a Ptr<> already
// at +1; the shim AddReference()s so the caller nets +1.)
void* dm_noesis_logical_child(void* element, uint32_t index);
// Templated child named `name` from this control's applied template (+1), via
// FrameworkElement::GetTemplateChild. NULL if not a FrameworkElement or no such
// named part exists.
void* dm_noesis_framework_element_template_child(void* element, const char* name);

// ── B. Attached properties ──────────────────────────────────────────────────
//
// Resolve `prop_name` on `owner_type`'s reflected TypeClass (e.g.
// owner="Grid", prop="Row"; owner="Canvas", prop="Left"), then set / get on
// `obj`. Same prop_type tag layout + validation as the generic name-keyed
// path. The owner type must already be registered with Reflection (referencing
// it from XAML forces registration). Returns false on null, obj-not-a-
// DependencyObject, unknown owner type, unknown property, tag mismatch, or
// (set) a read-only property.
bool dm_noesis_dependency_object_set_attached(
    void* obj, const char* owner_type, const char* prop_name,
    uint32_t prop_type, const void* value_ptr);
bool dm_noesis_dependency_object_get_attached(
    void* obj, const char* owner_type, const char* prop_name,
    uint32_t prop_type, void* out_value);

// ── C. ClearValue / SetCurrentValue / GetBaseValue ──────────────────────────
//
// clear_value resolves the DP by name and calls ClearLocalValue (returns false
// if unknown / read-only). set_current_value marshals like the generic setter
// but calls SetCurrentValue<T> / SetCurrentValueObject (coerce field only,
// leaving any local / source value intact). get_base_value reads
// GetBaseValue<T> (value before animation / coerce); since Noesis exposes no
// boxed GetBaseValueObject, the IMAGE_SOURCE / BASE_COMPONENT tags are
// unsupported and return false.
bool dm_noesis_dependency_object_clear_value(void* obj, const char* name);
bool dm_noesis_dependency_object_set_current_value(
    void* obj, const char* name, uint32_t prop_type, const void* value_ptr);
bool dm_noesis_dependency_object_get_base_value(
    void* obj, const char* name, uint32_t prop_type, void* out_value);

// ── D. Dynamic tag inference ────────────────────────────────────────────────
//
// Returns the dm_noesis_prop_type tag (>=0) for the named DP on `obj`, or -1 if
// `obj` is not a DependencyObject, the property is unknown, or its reflected
// type maps to no tag. The inverse of the tag validation the setters apply.
int32_t dm_noesis_dependency_object_property_tag(void* obj, const char* name);

// ── E. HorizontalAlignment / VerticalAlignment ──────────────────────────────
//
// A bespoke path: the alignment enums don't match the generic INT32 tag's
// reflected Type, so these go through the FrameworkElement accessors. `value`
// mirrors Noesis::HorizontalAlignment (Left/Center/Right/Stretch, 0..=3) and
// Noesis::VerticalAlignment (Top/Center/Bottom/Stretch, 0..=3). Getters return
// -1 if `element` is not a FrameworkElement; setters no-op.
void dm_noesis_framework_element_set_halign(void* element, int32_t value);
void dm_noesis_framework_element_set_valign(void* element, int32_t value);
int32_t dm_noesis_framework_element_get_halign(void* element);
int32_t dm_noesis_framework_element_get_valign(void* element);

// ── F. Namescope register / unregister ──────────────────────────────────────
//
// Register / unregister an x:Name in the namescope hosting `element`. `object`
// is a borrowed BaseComponent* (the scope takes its own ref). Returns false if
// `element` is not a FrameworkElement. The element must live within a namescope
// (the XAML root hosts one); registering a name already present updates it.
bool dm_noesis_framework_element_register_name(void* element, const char* name, void* object);
bool dm_noesis_framework_element_unregister_name(void* element, const char* name);

// ── G. Thread affinity (DispatcherObject) ───────────────────────────────────
//
// Only the affinity queries are exposed — NsGui has no public BeginInvoke
// surface (cross-thread marshalling would need IView timers, TODO §1). True if
// the calling thread owns `obj` (DispatcherObject::CheckAccess); false if `obj`
// is not a DispatcherObject. thread_id returns the owning thread id
// (GetThreadId), or UINT32_MAX when unattached or not a DispatcherObject.
bool dm_noesis_dependency_object_check_access(void* obj);
uint32_t dm_noesis_dependency_object_thread_id(void* obj);

// ── Custom MarkupExtension registration (Phase 5.D) ────────────────────────
//
// Register Rust-backed `MarkupExtension` subclasses so XAML's
// `{myns:Foo positional_arg}` syntax dispatches to a Rust callback.
// AoR's `LocalizeExtension` is the motivating example —
// `{aor:Localize menu.main_menu.new_game}` resolves the key through a
// LocalizationManager and substitutes the result.
//
// Architecture mirrors the custom-class FFI: a per-base C++ trampoline
// (`RustMarkupExtension : Noesis::MarkupExtension`) with a `Key` string
// field declared as the ContentProperty (so XAML's positional-argument
// syntax sets it). Each consumer-named extension gets a synthetic
// `TypeClassBuilder` that AddBases from the trampoline; consumer
// callbacks are dispatched per-name via a Symbol → ClassData side table.
//
// ## v1 scope
//
// * Single positional `Key` argument (matches `[ContentProperty("Key")]`).
// * Callback returns either a borrowed C string (most common) or a
//   borrowed `BaseComponent*` (for value types that can't be expressed
//   as text).
// * No reactive bindings — the callback runs at XAML parse time and the
//   returned value is substituted statically. Locale switching requires
//   re-loading the XAML (matches the existing byte-substitution shim's
//   semantics; full reactivity follows in a separate PR via a
//   `LocalizationManager`-style indexer + Binding).
//
// ## Lifecycle
//
// 1. dm_noesis_markup_extension_register("AOR.Localize", cb, userdata)
//    → opaque token.
// 2. Load XAML using `{aor:Localize SomeKey}`. Noesis instantiates the
//    extension, sets `Key = "SomeKey"`, calls ProvideValue, which
//    fires `cb(userdata, "SomeKey", out_string, out_component)`.
// 3. Callback writes either out_string OR out_component (not both) and
//    returns `true`. Returning `false` = no value (Noesis substitutes
//    UnsetValue).
// 4. dm_noesis_markup_extension_unregister(token) at shutdown.

// MarkupExtension callback. `key` is the ContentProperty value the XAML
// parser set on the extension (the bit between `{aor:Localize` and `}`).
// Output slots: write *exactly one* of them (set the other to NULL):
//   * `*out_string` — borrowed UTF-8 C string. Must outlive the call;
//     Noesis copies into its own String storage immediately. Pointing into
//     userdata-owned long-lived storage is the simplest pattern.
//   * `*out_component` — borrowed BaseComponent* (e.g. an existing
//     resource lookup). Caller does NOT consume a ref; Noesis adds its
//     own AddReference if it stores the value.
// Return `true` to signal "value produced"; `false` for "no value, use
// UnsetValue."
typedef bool (*dm_noesis_markup_provide_fn)(
    void* userdata,
    const char* key,
    const char** out_string,
    void** out_component);

// Free callback invoked exactly once when the underlying MarkupClassData
// is finally torn down — either at unregister (no instances alive) or
// deferred to the last live extension instance's destruction. Mirrors
// `dm_noesis_class_free_fn`. Ownership of `userdata` transfers to the
// C++ side at registration; the Rust side must not free it.
typedef void (*dm_noesis_markup_free_fn)(void* userdata);

// Register a Rust-backed MarkupExtension class. NULL on bad input
// (null name, init not yet called, name already registered).
//
// `free_handler` (optional, may be NULL) is invoked exactly once when
// MarkupClassData is finally freed.
void* dm_noesis_markup_extension_register(
    const char* name,
    dm_noesis_markup_provide_fn cb,
    void* userdata,
    dm_noesis_markup_free_fn free_handler);

// Unregister a markup extension class — removes from Factory + Reflection
// so no NEW instances can be created, then drops the Rust caller's ref
// on MarkupClassData. Existing live extension instances retain their
// references; the actual free + `free_handler` callback runs when the
// last instance is destroyed. Safe to call with NULL.
void dm_noesis_markup_extension_unregister(void* token);

// Instantiate a registered class (see dm_noesis_class_register) directly from
// Rust, without a XAML reference. Returns a BaseComponent* with +1 ref for the
// caller (release via dm_noesis_base_component_release), or NULL on null token.
//
// The instance is a DependencyObject carrying the class's registered DPs, so it
// works as a data-binding source / view model: set it as an element's
// DataContext (dm_noesis_framework_element_set_data_context) and bind to its
// DPs in XAML. Writing a DP from Rust (dm_noesis_instance_set_property) raises
// the change notification the binding engine observes.
void* dm_noesis_class_create_instance(void* class_token);

// ── Data binding bridge (Phase 5.E / TODO §3) ──────────────────────────────
//
// Drive XAML from Rust-owned data. Bindings are authored in XAML
// (`{Binding Path}` / `ItemsSource="{Binding}"`); these entrypoints supply the
// runtime data they resolve against.

// Box a UTF-8 C string into a `BoxedValue<String>`. Returns a BaseComponent*
// with +1 ref (release via dm_noesis_base_component_release). NULL text is
// treated as empty. Use it for ObservableCollection items rendered by a
// `<DataTemplate>` with `{Binding}` (the whole item), and anywhere a string
// must cross as a BaseComponent.
void* dm_noesis_box_string(const char* text);

// Create an `ObservableCollection<BaseComponent>`. Returns a BaseComponent*
// with +1 ref (release via dm_noesis_base_component_release). It implements
// INotifyCollectionChanged, so once bound to an ItemsControl.ItemsSource every
// mutation below raises CollectionChanged and the control regenerates.
void* dm_noesis_observable_collection_create(void);

// Append `item` (a borrowed BaseComponent*; the collection takes its own ref).
// Returns the insertion index, or -1 if `collection` is not an
// ObservableCollection.
int32_t dm_noesis_observable_collection_add(void* collection, void* item);

// Insert / replace at `index`. Return false on a null/non-collection pointer or
// an out-of-range index (insert allows index == count; set requires
// index < count).
bool dm_noesis_observable_collection_insert(void* collection, uint32_t index, void* item);
bool dm_noesis_observable_collection_set(void* collection, uint32_t index, void* item);

// Remove the item at `index`. False on null/non-collection or out-of-range.
bool dm_noesis_observable_collection_remove_at(void* collection, uint32_t index);

// Remove every item.
void dm_noesis_observable_collection_clear(void* collection);

// Item count, or -1 if `collection` is not an ObservableCollection.
int32_t dm_noesis_observable_collection_count(void* collection);

// Borrowed (no +1) pointer to the item at `index`, or NULL on
// null/non-collection/out-of-range. The collection owns the reference.
void* dm_noesis_observable_collection_get(void* collection, uint32_t index);

// Set / get a FrameworkElement's `DataContext`. `set` stores its own ref on
// `context` (pass NULL to clear) and returns false if `element` is not a
// FrameworkElement. `get` returns a borrowed (no +1) pointer or NULL.
bool dm_noesis_framework_element_set_data_context(void* element, void* context);
void* dm_noesis_framework_element_get_data_context(void* element);

// Set an ItemsControl's `ItemsSource` (e.g. an ObservableCollection). Returns
// false if `element` is not an ItemsControl. Pass NULL to clear.
bool dm_noesis_items_control_set_items_source(void* element, void* items);

// Number of items the ItemsControl sees through its bound source (a live
// passthrough). -1 if `element` is not an ItemsControl.
int32_t dm_noesis_items_control_items_count(void* element);

// Number of *realized* item containers the generator has materialized. Only
// grows when the generator regenerates, which for a source mutated after first
// layout requires INotifyCollectionChanged to have fired — so it is a genuine
// signal that change notification reached the control (vs. items_count, which
// passes through regardless). -1 if `element` is not an ItemsControl.
int32_t dm_noesis_items_control_realized_count(void* element);

// ── Commands: ICommand from Rust (TODO §4) ─────────────────────────────────
//
// Expose Rust logic to XAML `Command="{Binding ...}"`. The C++ side wraps a
// Rust vtable in a `RustCommand : Noesis::BaseCommand` (which implements the
// `ICommand` interface), so the returned object is a `BaseComponent*` that a
// Button / MenuItem can bind its `Command` to. Reach XAML by storing the
// command as a `BASE_COMPONENT` property on a Rust view-model instance (set
// via dm_noesis_instance_set_property) and exposing that instance as a
// DataContext; XAML then binds `Command="{Binding TheProperty}"`.
//
// `CanExecute` / `Execute` forward into the vtable. `param` is the borrowed
// command-parameter `BaseComponent*` the control passes (CommandParameter),
// and may be NULL. Keep work small — these fire from inside Noesis's input
// pump on whatever thread drives the view.

typedef struct dm_noesis_command_vtable {
    // Whether the command can run now. Drives Button.IsEnabled when the
    // button is bound to this command. `param` is borrowed; may be NULL.
    bool (*can_execute)(void* userdata, void* param);
    // Invoke the command. `param` is borrowed; may be NULL.
    void (*execute)(void* userdata, void* param);
} dm_noesis_command_vtable;

// Free callback invoked exactly once when the underlying RustCommand is
// finally destroyed (last reference released — which may be the binding
// long after dm_noesis_command_destroy). Receives the `userdata` passed to
// dm_noesis_command_create; ownership of `userdata` transfers to the C++
// side at creation. Optional (may be NULL).
typedef void (*dm_noesis_command_free_fn)(void* userdata);

// Create a Rust-backed ICommand. Returns a `BaseComponent*` (an ICommand)
// with +1 ref for the caller; release via dm_noesis_command_destroy. The
// `vtable` is copied (need not outlive the call). Returns NULL if `vt` is
// NULL.
void* dm_noesis_command_create(
    const dm_noesis_command_vtable* vt,
    void* userdata,
    dm_noesis_command_free_fn free_handler);

// Release the caller's +1 reference from dm_noesis_command_create. If a
// binding still references the command it stays alive (and the free handler
// is deferred) until that reference also drops. Safe to call with NULL.
void dm_noesis_command_destroy(void* command);

// Fire `CanExecuteChanged` so any control bound to this command re-queries
// `CanExecute` (e.g. a Button re-evaluates IsEnabled on the next update).
// Safe to call with NULL or a non-command pointer (no-op).
void dm_noesis_command_raise_can_execute_changed(void* command);

#ifdef __cplusplus
}
#endif

#endif  // DM_NOESIS_SHIM_H
