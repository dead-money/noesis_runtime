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

// Returns the Noesis runtime build version (e.g. "3.2.13"). The pointer is
// owned by the Noesis runtime; do not free.
const char* dm_noesis_version(void);

// ── Inspector / hot-reload toggles + queries (TODO §17) ─────────────────────
//
// The Disable* switches map to `GUI::Disable*` and MUST be called BEFORE
// dm_noesis_init — they have no effect afterwards. There is no matching
// "enable": the Inspector / Hot Reload are on by default in Debug/Profile SDK
// builds; we only expose the off switches plus the runtime connection query
// and keep-alive pump. On a Release dylib these features are compiled out, so
// the Disable* calls are harmless no-ops and dm_noesis_is_inspector_connected
// always returns false.

// Disable the Hot Reload feature (saves a little memory). Call BEFORE init.
void dm_noesis_disable_hot_reload(void);
// Skip Inspector socket initialization (e.g. WSAStartup) when the host has
// already initialized sockets. Call BEFORE init.
void dm_noesis_disable_socket_init(void);
// Disable all remote Inspector connections. Call BEFORE init.
void dm_noesis_disable_inspector(void);
// Returns whether a remote Inspector is currently connected. Always false on
// a Release dylib (Inspector compiled out) or when nothing is attached.
bool dm_noesis_is_inspector_connected(void);
// Keep the Inspector connection alive. Views call this internally on update;
// only needed if the Inspector connects before any view exists.
void dm_noesis_update_inspector(void);

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

// ── Offscreen / glyph-cache tuning (TODO §1) ───────────────────────────────
//
// Configure resource sizing on a `Noesis::RenderDevice` (the opaque value from
// `dm_noesis_render_device_create`). Set these before the renderer draws its
// first frame. Offscreen width/height of 0 selects automatic sizing (the
// default). Glyph-cache dimensions have a build-dependent default (read it back
// via the getter rather than assuming a value). All are no-ops on a NULL
// device.
void dm_noesis_render_device_set_offscreen_width(void* device, uint32_t width);
void dm_noesis_render_device_set_offscreen_height(void* device, uint32_t height);
void dm_noesis_render_device_set_offscreen_sample_count(void* device, uint32_t count);
void dm_noesis_render_device_set_offscreen_default_num_surfaces(void* device, uint32_t num);
void dm_noesis_render_device_set_offscreen_max_num_surfaces(void* device, uint32_t num);
void dm_noesis_render_device_set_glyph_cache_width(void* device, uint32_t width);
void dm_noesis_render_device_set_glyph_cache_height(void* device, uint32_t height);

// Read back the configured values (the companion getters). Return 0 on a NULL
// device. Width/height of 0 means automatic for the offscreen knobs.
uint32_t dm_noesis_render_device_get_offscreen_width(const void* device);
uint32_t dm_noesis_render_device_get_offscreen_height(const void* device);
uint32_t dm_noesis_render_device_get_offscreen_sample_count(const void* device);
uint32_t dm_noesis_render_device_get_offscreen_default_num_surfaces(const void* device);
uint32_t dm_noesis_render_device_get_offscreen_max_num_surfaces(const void* device);
uint32_t dm_noesis_render_device_get_glyph_cache_width(const void* device);
uint32_t dm_noesis_render_device_get_glyph_cache_height(const void* device);

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

// Parse XAML from an in-memory NUL-terminated string (no XamlProvider URI
// needed). Returns a FrameworkElement* (+1 ref), or NULL if `text` is NULL,
// the XAML is malformed, or the parsed root isn't a FrameworkElement (e.g. a
// bare ResourceDictionary). Release with dm_noesis_base_component_release.
void* dm_noesis_gui_parse_xaml(const char* text);

// Load the XAML at `uri` into an existing `component` instance — the
// code-behind / x:Class pattern, where the root object already exists and
// LoadComponent populates its children + named fields. `component` is an
// opaque BaseComponent* (borrowed; ownership is not taken). Returns false if
// either argument is NULL. Meaningful use requires the component's reflected
// type to match the XAML root's x:Class.
bool dm_noesis_gui_load_component(void* component, const char* uri);

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

// Add a +1 reference to a BaseComponent-derived object and return it (NULL on
// NULL input). Promotes a borrowed component pointer into an owning handle;
// balance with dm_noesis_base_component_release.
void* dm_noesis_base_component_add_reference(void* obj);

// Create an IView whose root is `framework_element`. The view retains its own
// reference to the element; the caller's reference is still held by the
// FrameworkElement wrapper until it's dropped.
void* dm_noesis_view_create(void* framework_element);

// Release an IView* obtained from dm_noesis_view_create.
void dm_noesis_view_destroy(void* view);

// Add a +1 reference to an IView and return it (NULL on a NULL view). Used to
// build an owned, thread-movable renderer handle that keeps the view alive
// independently of the View wrapper. Balance each call with
// dm_noesis_view_destroy.
void* dm_noesis_view_add_reference(void* view);

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

// ── Stereo / VR rendering (TODO §1) ────────────────────────────────────────
// `IRenderer::RenderStereo` overloads (the non-deprecated VR render path). Each
// eye matrix is 16 floats, row-major (same layout as
// dm_noesis_view_set_projection_matrix). Culling uses the view's projection
// matrix, so the eye matrices must be enclosed by it. No-ops on NULL args.
//
// Multi-pass: render one eye per call (call twice, into each eye's target).
void dm_noesis_renderer_render_stereo(
    void* renderer, const float* eye_matrix, bool flip_y, bool clear);
// Single-pass: render both eyes in one call (multiview / instanced VR).
void dm_noesis_renderer_render_stereo_both(
    void* renderer, const float* left_eye_matrix, const float* right_eye_matrix,
    bool flip_y, bool clear);

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

// Horizontal mouse wheel. `delta` mirrors `dm_noesis_view_mouse_wheel`'s
// Windows-style 120-units-per-notch convention; positive scrolls right.
bool dm_noesis_view_mouse_hwheel(void* view, int32_t x, int32_t y, int32_t delta);

// ── View flags / quality / stats (TODO §1) ─────────────────────────────────

// Current render flags (a bitmask of `Noesis::RenderFlags`). Companion to
// dm_noesis_view_set_flags.
uint32_t dm_noesis_view_get_flags(void* view);

// Tessellation curve tolerance in screen-space pixels (smaller == higher
// quality). LowQuality is 0.7, MediumQuality 0.4, HighQuality 0.2.
void dm_noesis_view_set_tessellation_max_pixel_error(void* view, float error);
float dm_noesis_view_get_tessellation_max_pixel_error(void* view);

// ── Gesture / touch thresholds (TODO §1) ───────────────────────────────────
// Tune when interactions promote to Holding / Tapped / DoubleTapped /
// Manipulation gestures, and whether the mouse emulates touch input. All are
// pass-through setters; no-ops on a NULL view. Defaults (per the SDK): holding
// time 500ms, holding distance 10px, manipulation distance 10px, double-tap
// time 500ms, double-tap distance 10px.
void dm_noesis_view_set_holding_time_threshold(void* view, uint32_t ms);
void dm_noesis_view_set_holding_distance_threshold(void* view, uint32_t pixels);
void dm_noesis_view_set_manipulation_distance_threshold(void* view, uint32_t pixels);
void dm_noesis_view_set_double_tap_time_threshold(void* view, uint32_t ms);
void dm_noesis_view_set_double_tap_distance_threshold(void* view, uint32_t pixels);
void dm_noesis_view_set_emulate_touch(void* view, bool emulate);

// ── Stereo / VR (TODO §1) ──────────────────────────────────────────────────
// Scale applied to the offscreen phase to account for stereo eye matrices
// differing from the view projection. Must be 1.0 (the default) for non-VR;
// 2–3 is recommended for VR. No-op on a NULL view.
void dm_noesis_view_set_stereo_offscreen_scale_factor(void* view, float factor);

// Performance counters for the last rendered frame. Field order / types match
// `Noesis::ViewStats` exactly (3 floats then 12 uint32_t); a static_assert in
// noesis_view.cpp guards the size. `out` is written only when both pointers
// are non-null.
typedef struct dm_noesis_view_stats {
    float frame_time;
    float update_time;
    float render_time;

    uint32_t triangles;
    uint32_t draws;
    uint32_t batches;

    uint32_t tessellations;
    uint32_t flushes;
    uint32_t geometry_size;

    uint32_t masks;
    uint32_t opacities;
    uint32_t render_target_switches;

    uint32_t uploaded_ramps;
    uint32_t rasterized_glyphs;
    uint32_t discarded_glyph_tiles;
} dm_noesis_view_stats;

void dm_noesis_view_get_stats(void* view, dm_noesis_view_stats* out);

// ── View-driven timers (TODO §1) ───────────────────────────────────────────
//
// `IView::CreateTimer(interval, Delegate<uint32_t()>)` fires from inside
// View::Update on the thread driving the view. The callback returns the next
// interval in milliseconds, or 0 to stop. Lifetime mirrors the RustCommand
// donated-free-fn pattern: a heap `RustTimer` holds the Rust callback + the
// donated userdata + a free handler + the assigned timer id + the IView (with
// a +1 ref so the token can safely outlive the caller's other view handles).
// The token returned here is that `RustTimer*`.

// Callback fired on each timer tick. Returns the next interval in ms (0 stops
// the timer). Fires from inside View::Update on the view-driving thread.
typedef uint32_t (*dm_noesis_timer_fn)(void* userdata);

// Free callback invoked exactly once when the timer token is cancelled (its
// RustTimer destroyed). Receives the `userdata` passed to create; ownership of
// `userdata` transfers to the C++ side at creation. Optional (may be NULL).
typedef void (*dm_noesis_timer_free_fn)(void* userdata);

// Create a view timer firing every `interval_ms`. Returns an opaque token (a
// RustTimer*) or NULL on failure (`view`/`cb` null). Cancel + free via
// dm_noesis_view_cancel_timer.
void* dm_noesis_view_create_timer(
    void* view, uint32_t interval_ms, dm_noesis_timer_fn cb, void* userdata,
    dm_noesis_timer_free_fn free_handler);

// Restart the timer with a new interval (ms). No-op on a NULL token.
void dm_noesis_view_restart_timer(void* token, uint32_t interval_ms);

// Cancel the timer and destroy the token: calls IView::CancelTimer(id), then
// deletes the RustTimer (invoking the donated free handler exactly once and
// releasing the +1 view ref). Safe to call with NULL.
void dm_noesis_view_cancel_timer(void* token);

// ── Rendering event (TODO §1) ──────────────────────────────────────────────
//
// `IView::Rendering()` is a `Delegate<void(IView*)>` raised after animation and
// layout are applied to the composition tree, just before it is rendered — a
// per-frame hook on the view-driving thread. Lifetime mirrors the timer
// donated-free-fn pattern: a heap handler holds the Rust callback + donated
// userdata + free handler + a +1 ref on the IView, registers the delegate with
// `+=`, and detaches it with `-=` when the token is removed. The returned token
// is that handler pointer.

// Callback fired on each Rendering event. `view` is the borrowed IView* raising
// the event (do not release). Fires on the view-driving thread.
typedef void (*dm_noesis_rendering_fn)(void* userdata, void* view);

// Free callback invoked exactly once when the handler token is removed.
// Receives the `userdata` passed to add; ownership transfers to the C++ side at
// registration. Optional (may be NULL).
typedef void (*dm_noesis_rendering_free_fn)(void* userdata);

// Subscribe a Rust callback to the view's Rendering event. Returns an opaque
// token or NULL on failure (`view`/`cb` null). Remove + free via
// dm_noesis_view_remove_rendering_handler.
void* dm_noesis_view_add_rendering_handler(
    void* view, dm_noesis_rendering_fn cb, void* userdata,
    dm_noesis_rendering_free_fn free_handler);

// Detach the Rendering delegate and destroy the token (invoking the donated
// free handler exactly once and releasing the +1 view ref). Safe to call with
// NULL.
void dm_noesis_view_remove_rendering_handler(void* token);

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

// ── Non-routed lifecycle events (TODO §5) ───────────────────────────────────
//
// `Initialized`, `LayoutUpdated`, `DataContextChanged` and the `Is*Changed`
// notifications ride the `Event_<T>` mechanism (AddEventHandler(Symbol,
// EventHandler)), not AddHandler(RoutedEvent, ...). This name-keyed surface
// drives the public accessors' `+=` / `-=` so the internal Symbol keys never
// have to be guessed. None of these notifications carry args we surface, so the
// callback is a bare `void(userdata)`.

// Lifecycle-event callback. Same threading contract as `dm_noesis_click_fn`
// (fires from inside the layout / property pump on the view-driving thread).
typedef void (*dm_noesis_lifecycle_fn)(void* userdata);

// Subscribe `cb(userdata)` to the non-routed lifecycle event named `event_name`
// on `element` (DynamicCast to FrameworkElement*). Supported names:
// "Initialized", "LayoutUpdated", "DataContextChanged", "IsEnabledChanged",
// "IsVisibleChanged", "IsHitTestVisibleChanged", "IsKeyboardFocusedChanged",
// "IsKeyboardFocusWithinChanged", "IsMouseCapturedChanged",
// "IsMouseCaptureWithinChanged", "IsMouseDirectlyOverChanged",
// "FocusableChanged". Returns an opaque token to pass once to
// `dm_noesis_unsubscribe_lifecycle`, or NULL if `element` is not a
// FrameworkElement, `event_name` is unknown, or `cb` is NULL. The token holds a
// +1 ref on the element so the subscription outlives every other handle the
// caller drops.
void* dm_noesis_subscribe_lifecycle(
    void* element, const char* event_name, dm_noesis_lifecycle_fn cb, void* userdata);

// Unsubscribe and free the lifecycle handler. Safe to call with NULL.
void dm_noesis_unsubscribe_lifecycle(void* token);

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
    DM_NOESIS_BASE_CONTENT_CONTROL   = 0,
    DM_NOESIS_BASE_CONTROL           = 1,
    DM_NOESIS_BASE_FRAMEWORK_ELEMENT = 2,
    DM_NOESIS_BASE_USER_CONTROL      = 3,
    DM_NOESIS_BASE_PANEL             = 4,
    DM_NOESIS_BASE_DECORATOR         = 5,
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

// ── Custom base classes + richer DP metadata + layout (TODO §9) ─────────────
//
// `dm_noesis_class_register` accepts any `dm_noesis_class_base` value above —
// each maps to a sibling trampoline subclass (`RustControl`, `RustPanel`, …)
// that shares the synthetic-TypeClass + ClassData machinery with
// `RustContentControl`. The additions below layer richer DP metadata
// (coercion / FrameworkPropertyMetadataOptions / read-only) and layout
// participation (MeasureOverride / ArrangeOverride) onto any registered class.

// Richer DependencyProperty registration. Superset of
// `dm_noesis_class_register_property`:
//   * `fpm_options` — bitmask of Noesis::FrameworkPropertyMetadataOptions
//     (AffectsMeasure=0x1, AffectsArrange=0x2, AffectsParentMeasure=0x4,
//     AffectsParentArrange=0x8, AffectsRender=0x10, Inherits=0x20, …). When
//     non-zero the DP is created with a FrameworkPropertyMetadata so Noesis
//     invalidates the matching layout/render pass on change.
//   * `read_only` — registers the DP with PropertyAccess_ReadOnly. The public
//     setter paths (dm_noesis_*_set_property / bindings / XAML) then reject
//     writes; the privileged `dm_noesis_instance_set_readonly_property` is the
//     only way to mutate it (mirrors a WPF DependencyPropertyKey).
//   * `coerce` — attaches the class-level coerce callback (installed via
//     `dm_noesis_class_set_coerce`) to THIS property. Limited to the first 32
//     properties of a class (the coerce-thunk pool size); registration returns
//     UINT32_MAX if a 33rd coerced property is requested.
// Returns the dense property index, or UINT32_MAX on failure.
uint32_t dm_noesis_class_register_property_ex(
    void* class_token,
    const char* prop_name,
    dm_noesis_prop_type prop_type,
    const void* default_ptr,
    uint32_t fpm_options,
    bool read_only,
    bool coerce);

// Set a read-only DP on an instance via the privileged path (the analogue of
// setting through a WPF DependencyPropertyKey). `value_ptr` follows the
// per-type layout. Returns false on bad input (null/invalid instance, index
// out of range). Fires the changed callback like any other write.
bool dm_noesis_instance_set_readonly_property(
    void* instance,
    uint32_t prop_index,
    const void* value_ptr);

// Coerce callback. Invoked synchronously inside Noesis's value pipeline when a
// coerced DP's effective value is computed. `in_value` is the pre-coercion
// value (per the DP's prop_type layout); `out_value` is pre-initialized to a
// copy of `in_value` and the implementation overwrites it with the coerced
// result. Only scalar / Thickness / Color / Rect tags are coercible; object /
// string tags pass through unchanged. `instance` is the owning object's
// BaseComponent*.
typedef void (*dm_noesis_coerce_fn)(
    void* userdata,
    void* instance,
    uint32_t prop_index,
    const void* in_value,
    void* out_value);

// Install a class-level coerce callback. Individual DPs opt in by passing
// `coerce=true` to dm_noesis_class_register_property_ex. `userdata` ownership
// transfers to the C++ side and is released via `free_handler` when ClassData
// is finally torn down (same lifetime contract as the change callback). NULL
// `cb` detaches.
void dm_noesis_class_set_coerce(
    void* class_token,
    dm_noesis_coerce_fn cb,
    void* userdata,
    dm_noesis_class_free_fn free_handler);

// Layout vtable: the trampoline subclass's MeasureOverride / ArrangeOverride
// forward into these. `instance` is the owning object's BaseComponent* (use it
// with dm_noesis_visual_children_count / dm_noesis_visual_child +
// dm_noesis_uielement_measure / _arrange to lay out children). Sizes are in
// DIPs. Write the desired (measure) / used (arrange) size to out_w/out_h. When
// no layout handler is installed the base class's default layout runs.
typedef struct dm_noesis_layout_vtable {
    void (*measure)(void* userdata, void* instance,
        float avail_w, float avail_h, float* out_w, float* out_h);
    void (*arrange)(void* userdata, void* instance,
        float final_w, float final_h, float* out_w, float* out_h);
} dm_noesis_layout_vtable;

typedef void (*dm_noesis_layout_free_fn)(void* userdata);

// Install a layout handler on a registered class. Meaningful for any base
// (all current bases derive from FrameworkElement). Pass a null `vtable` to
// detach. `userdata` ownership transfers; released via `free_handler` at
// ClassData teardown. Copies the vtable by value.
void dm_noesis_class_set_layout(
    void* class_token,
    const dm_noesis_layout_vtable* vtable,
    void* userdata,
    dm_noesis_layout_free_fn free_handler);

// Render callback (TODO §10). The trampoline subclass's `OnRender` override
// forwards here after the base `OnRender` runs. `instance` is the owning
// object's BaseComponent*; `context` is a BORROWED Noesis::DrawingContext*
// (do not release) valid ONLY for the duration of the call — issue immediate
// mode draw commands through the dm_noesis_drawing_* entrypoints. OnRender
// fires from inside the renderer's render-tree update (typically the view
// thread); keep work small.
typedef void (*dm_noesis_render_fn)(void* userdata, void* instance, void* context);

typedef void (*dm_noesis_render_free_fn)(void* userdata);

// Install a render handler on a registered class. Meaningful for any base (all
// current bases derive from UIElement). Pass a null `cb` to detach. `userdata`
// ownership transfers; released via `free_handler` at ClassData teardown (same
// lifetime contract as the change / coerce / layout callbacks).
void dm_noesis_class_set_render(
    void* class_token,
    dm_noesis_render_fn cb,
    void* userdata,
    dm_noesis_render_free_fn free_handler);

// UIElement layout primitives for custom MeasureOverride / ArrangeOverride
// implementations. `element` is a borrowed UIElement* (e.g. from
// dm_noesis_visual_child). measure/arrange return false if `element` is null
// or not a UIElement; desired_size additionally writes the post-Measure
// DesiredSize. arrange rect is (x, y, w, h) in the parent's coordinate space.
bool dm_noesis_uielement_measure(void* element, float avail_w, float avail_h);
bool dm_noesis_uielement_arrange(void* element, float x, float y, float w, float h);
bool dm_noesis_uielement_desired_size(void* element, float* out_w, float* out_h);

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

// Filtered hit test (TODO §2) — the callback overload of
// VisualTreeHelper::HitTest. `filter` is called per visual as the tree is
// walked; its return is a `HitTestFilterBehavior` (0 ContinueSkipSelfAndChildren,
// 1 ContinueSkipChildren, 2 ContinueSkipSelf, 3 Continue, 4 Stop). `result` is
// called per hit; its return is a `HitTestResultBehavior` (0 Stop, 1 Continue).
// Both receive BORROWED Visual* (valid only for that call; AddRef via
// dm_noesis_base_component_add_reference to keep one). `filter` may be NULL
// (treated as Continue); `result` must be non-NULL or the call is a no-op.
typedef int32_t (*dm_noesis_hit_filter_fn)(void* userdata, void* visual);
typedef int32_t (*dm_noesis_hit_result_fn)(void* userdata, void* visual);
void dm_noesis_visual_hit_test_filtered(
    void* element, float x, float y, dm_noesis_hit_filter_fn filter,
    dm_noesis_hit_result_fn result, void* userdata);

// RenderTransform origin (TODO §2) — UIElement's (0..1, 0..1) relative pivot.
// The getter writes 0,0 when `element` is not a UIElement; the setter is then a
// no-op returning false.
void dm_noesis_ui_element_get_render_transform_origin(void* element, float* out_x, float* out_y);
bool dm_noesis_ui_element_set_render_transform_origin(void* element, float x, float y);

// Standalone NameScope (TODO §2). The freestanding NameScope object, distinct
// from the per-FrameworkElement RegisterName path. Owning returns (+1, release
// via dm_noesis_base_component_release): _create, _get, _find_name.
void* dm_noesis_name_scope_create();
void* dm_noesis_name_scope_get(void* element);
bool dm_noesis_name_scope_set(void* element, void* scope);
void* dm_noesis_name_scope_find_name(void* scope, const char* name);
void dm_noesis_name_scope_register_name(void* scope, const char* name, void* obj);
void dm_noesis_name_scope_unregister_name(void* scope, const char* name);
void dm_noesis_name_scope_update_name(void* scope, const char* name, void* obj);
// Reverse lookup: registered name of `obj`, or NULL. Borrowed (owned by the
// scope); copy it before mutating the scope.
const char* dm_noesis_name_scope_find_object(void* scope, void* obj);
// Enumerate (name, object) pairs; callback gets BORROWED pointers per call.
typedef void (*dm_noesis_name_scope_enum_fn)(void* userdata, const char* name, void* obj);
void dm_noesis_name_scope_enum(void* scope, dm_noesis_name_scope_enum_fn cb, void* userdata);

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

// ── Value boxing / unboxing primitives (TODO §3) ───────────────────────────
//
// Binding values cross the FFI as `Noesis::BaseComponent*` (boxed). These wrap
// primitives so Rust can produce / read binding values — the currency a
// converter speaks. `dm_noesis_box_string` (above) handles strings; its unbox
// peer lives here. Each `box_*` returns a BaseComponent* with +1 ref (release
// via dm_noesis_base_component_release). Each `unbox_*` returns false / NULL if
// the boxed runtime type doesn't match the requested type.

void* dm_noesis_box_bool(bool value);
void* dm_noesis_box_int32(int32_t value);
void* dm_noesis_box_double(double value);

bool dm_noesis_unbox_bool(void* boxed, bool* out);
bool dm_noesis_unbox_int32(void* boxed, int32_t* out);
bool dm_noesis_unbox_double(void* boxed, double* out);

// Borrowed (no +1) view of a boxed string's bytes, valid only while `boxed` is
// alive (copy if you need to keep it). NULL if `boxed` is not a
// BoxedValue<String>.
const char* dm_noesis_unbox_string(void* boxed);

// ── Value converters: IValueConverter from Rust (TODO §3) ──────────────────
//
// A `RustValueConverter : Noesis::BaseValueConverter` forwards TryConvert /
// TryConvertBack into a Rust vtable. The returned object is a `BaseComponent*`
// (and an `IValueConverter`); set it on a code-built binding
// (`dm_noesis_binding_set_converter`) or insert it into an element's resources
// (`dm_noesis_framework_element_add_resource`) so XAML
// `{Binding ..., Converter={StaticResource Key}}` can reach it.
//
// `value` / `parameter` are borrowed boxed `BaseComponent*` (may be NULL) —
// unbox with the helpers above. `target_type` is an opaque `const Noesis::Type*`
// (forward-compatible; ignore it for simple converters). Write a +1-owned
// `BaseComponent*` into `*out_result` (ownership transfers to Noesis) and
// return `true`; return `false` to signal UnsetValue (Noesis uses the
// FallbackValue / property default). Returning `true` with `*out_result == NULL`
// yields a null value. Same threading contract as the command vtable — fires
// from inside Noesis's binding pump.

typedef struct dm_noesis_value_converter_vtable {
    bool (*convert)(
        void* userdata, void* value, const void* target_type,
        void* parameter, void** out_result);
    bool (*convert_back)(
        void* userdata, void* value, const void* target_type,
        void* parameter, void** out_result);
} dm_noesis_value_converter_vtable;

// Free callback invoked exactly once when the underlying RustValueConverter is
// finally destroyed (last reference released — which may be a Binding long
// after dm_noesis_value_converter_destroy). Ownership of `userdata` transfers
// to C++ at creation. Optional (may be NULL).
typedef void (*dm_noesis_value_converter_free_fn)(void* userdata);

// Create a Rust-backed IValueConverter. Returns a `BaseComponent*` with +1 ref
// for the caller; release via dm_noesis_value_converter_destroy. The `vtable`
// is copied (need not outlive the call). Returns NULL if `vt` is NULL.
void* dm_noesis_value_converter_create(
    const dm_noesis_value_converter_vtable* vt,
    void* userdata,
    dm_noesis_value_converter_free_fn free_handler);

// Release the caller's +1 reference from dm_noesis_value_converter_create. If a
// binding still references the converter it stays alive (and the free handler
// is deferred) until that reference also drops. Safe to call with NULL.
void dm_noesis_value_converter_destroy(void* converter);

// ── Code-built Binding + SetBinding (TODO §3) ──────────────────────────────
//
// `new Binding(path)` plus setters for the common knobs, then wire it onto a
// target DP with `dm_noesis_set_binding` — the code path that mirrors XAML
// `{Binding ...}` authoring. The Binding is a `BaseComponent*` with +1 ref;
// release via dm_noesis_binding_destroy. SetBinding takes its own reference, so
// the Binding may be destroyed right after wiring. All setters no-op on a NULL
// / non-Binding pointer. Pointer-valued setters take a borrowed BaseComponent*
// (the Binding stores its own reference; pass NULL to clear).

// Create a Binding with an initial property path (NULL → empty path / bind to
// the whole DataContext). +1 ref for the caller.
void* dm_noesis_binding_create(const char* path);
void dm_noesis_binding_destroy(void* binding);

// Source object (an explicit binding source, e.g. a Rust view model). Setting
// this overrides the inherited DataContext for this binding.
void dm_noesis_binding_set_source(void* binding, void* source);
// Bind against another element resolved by its x:Name in the same namescope.
void dm_noesis_binding_set_element_name(void* binding, const char* name);
// BindingMode ordinal: 0 Default, 1 TwoWay, 2 OneWay, 3 OneTime, 4 OneWayToSource.
void dm_noesis_binding_set_mode(void* binding, int32_t mode);
// IValueConverter (a dm_noesis_value_converter_create result, or any Noesis
// converter BaseComponent*). NULL clears.
void dm_noesis_binding_set_converter(void* binding, void* converter);
// Borrowed parameter passed to the converter on every Convert / ConvertBack.
void dm_noesis_binding_set_converter_parameter(void* binding, void* parameter);
// .NET-style composite format string (e.g. "F2", "Value is {0:F2}").
void dm_noesis_binding_set_string_format(void* binding, const char* format);
// Borrowed value used when the binding can't produce one.
void dm_noesis_binding_set_fallback_value(void* binding, void* value);
// UpdateSourceTrigger ordinal: 0 Default, 1 PropertyChanged, 2 LostFocus, 3 Explicit.
void dm_noesis_binding_set_update_source_trigger(void* binding, int32_t trigger);
// Bind relative to the target element itself (RelativeSource Self) — e.g. bind
// one property of an element to another on the same element.
void dm_noesis_binding_set_relative_source_self(void* binding);
// RelativeSource FindAncestor: resolve `type_name` through the reflection
// registry and bind to the `level`-th ancestor of that type (1 = nearest;
// 0 is coerced to 1). The ancestor type must already be registered with
// Reflection (referencing it from XAML forces registration). Returns false
// (no-op) on a NULL/non-Binding pointer or an unknown / unregistered type name.
bool dm_noesis_binding_set_relative_source_find_ancestor(
    void* binding, const char* type_name, uint32_t level);
// RelativeSource PreviousData: bind to the previous item in a data-bound
// collection. Uses the shared static singleton.
void dm_noesis_binding_set_relative_source_previous_data(void* binding);
// RelativeSource TemplatedParent: bind to the control a ControlTemplate is
// applied to. Uses the shared static singleton.
void dm_noesis_binding_set_relative_source_templated_parent(void* binding);

// Borrowed BindingExpression* for the binding on `element`'s `dp_name` property
// (BindingOperations::GetBindingExpression). OWNED by the target — do NOT
// release; valid only while the binding stays live on that property. NULL if
// `element` is not a DependencyObject, the DP name is unknown, or no binding is
// set. Pass the result to the update entrypoints below.
void* dm_noesis_get_binding_expression(void* element, const char* dp_name);
// Force a source -> target data transfer (re-pull the source value). No-op on
// NULL.
void dm_noesis_binding_expression_update_target(void* expr);
// Push the current target value back to the source — commits a binding whose
// UpdateSourceTrigger is Explicit. No-op (per Noesis) unless the binding's Mode
// is TwoWay / OneWayToSource. No-op on NULL.
void dm_noesis_binding_expression_update_source(void* expr);

// Resolve `dp_name` on `element`'s class hierarchy and wire `binding` onto it
// via BindingOperations::SetBinding. Returns false if `element` is not a
// DependencyObject, `binding` is not a Binding, or the DP name is unknown.
bool dm_noesis_set_binding(void* element, const char* dp_name, void* binding);

// Insert `object` into `element`'s ResourceDictionary under `key` (creating the
// dictionary if the element has none). Makes a Rust-built converter / value
// reachable from XAML via `{StaticResource Key}`. The dictionary stores its own
// reference to `object`. Returns false if `element` is not a FrameworkElement.
bool dm_noesis_framework_element_add_resource(
    void* element, const char* key, void* object);

// ── Brushes, transforms, effects, RenderOptions (TODO §11) ──────────────────
//
// Object construction from Rust. Every `*_create` returns a freshly-built
// BaseComponent* with a single owned reference (the caller releases it via
// dm_noesis_base_component_release, mirrored by the owning Rust handle's Drop).
// Colors are float[4] = {r, g, b, a} in 0..=1 (NsDrawing/Color.h). `cast`-style
// type checks make every setter/getter a no-op (false / -1) on a wrong-type
// pointer. Assign a built object to an element via the generic
// dm_noesis_*_set_property BASE_COMPONENT path (FrameworkElement::set_component);
// Noesis then takes its own reference, so the Rust handle may drop afterwards.

// SolidColorBrush
void* dm_noesis_solid_color_brush_create(const float color[4]);
bool dm_noesis_solid_color_brush_set_color(void* brush, const float color[4]);
bool dm_noesis_solid_color_brush_get_color(void* brush, float out[4]);

// LinearGradientBrush
void* dm_noesis_linear_gradient_brush_create(void);
bool dm_noesis_linear_gradient_brush_set_start_point(void* brush, float x, float y);
bool dm_noesis_linear_gradient_brush_set_end_point(void* brush, float x, float y);
// out = {startX, startY, endX, endY}
bool dm_noesis_linear_gradient_brush_get_points(void* brush, float out[4]);

// RadialGradientBrush
void* dm_noesis_radial_gradient_brush_create(void);
bool dm_noesis_radial_gradient_brush_set_center(void* brush, float x, float y);
bool dm_noesis_radial_gradient_brush_set_gradient_origin(void* brush, float x, float y);
bool dm_noesis_radial_gradient_brush_set_radius(void* brush, float rx, float ry);
bool dm_noesis_radial_gradient_brush_get_radius(void* brush, float* rx, float* ry);

// GradientBrush stops (works on any LinearGradientBrush / RadialGradientBrush).
// add_stop returns the new stop index or -1; stop_count returns count or -1 on
// a non-GradientBrush pointer.
int32_t dm_noesis_gradient_brush_add_stop(void* brush, float offset, const float color[4]);
int32_t dm_noesis_gradient_brush_stop_count(void* brush);
bool dm_noesis_gradient_brush_get_stop(void* brush, uint32_t index, float* out_offset,
                                       float out_color[4]);

// ImageBrush. `image_source` is a borrowed ImageSource* (or null); Noesis takes
// its own reference. get returns a borrowed ImageSource* (no +1) or null.
void* dm_noesis_image_brush_create(void* image_source);
bool dm_noesis_image_brush_set_image_source(void* brush, void* image_source);
void* dm_noesis_image_brush_get_image_source(void* brush);

// VisualBrush. `visual` is a borrowed Visual* (any element is a Visual; or null);
// Noesis takes its own reference. get returns a borrowed Visual* (no +1) or null.
// VisualBrush only renders when the visual is in the logical tree, but the
// property assignment is headless-verifiable through GetVisual pointer identity.
void* dm_noesis_visual_brush_create(void* visual);
bool dm_noesis_visual_brush_set_visual(void* brush, void* visual);
void* dm_noesis_visual_brush_get_visual(void* brush);

// TileBrush tiling knobs (base of ImageBrush AND VisualBrush). Enum ordinals
// match NsGui/Enums.h: AlignmentX {Left,Center,Right}, AlignmentY {Top,Center,
// Bottom}, Stretch {None,Fill,Uniform,UniformToFill}, TileMode {None,Tile,FlipX,
// FlipY,FlipXY}, BrushMappingMode {Absolute,RelativeToBoundingBox}. The enum
// getters return -1 if `brush` is not a TileBrush. Viewport/Viewbox are Rects
// expressed as {x, y, width, height}.
bool dm_noesis_tile_brush_set_alignment_x(void* brush, int32_t value);
int32_t dm_noesis_tile_brush_get_alignment_x(void* brush);
bool dm_noesis_tile_brush_set_alignment_y(void* brush, int32_t value);
int32_t dm_noesis_tile_brush_get_alignment_y(void* brush);
bool dm_noesis_tile_brush_set_stretch(void* brush, int32_t value);
int32_t dm_noesis_tile_brush_get_stretch(void* brush);
bool dm_noesis_tile_brush_set_tile_mode(void* brush, int32_t value);
int32_t dm_noesis_tile_brush_get_tile_mode(void* brush);
bool dm_noesis_tile_brush_set_viewport_units(void* brush, int32_t value);
int32_t dm_noesis_tile_brush_get_viewport_units(void* brush);
bool dm_noesis_tile_brush_set_viewbox_units(void* brush, int32_t value);
int32_t dm_noesis_tile_brush_get_viewbox_units(void* brush);
bool dm_noesis_tile_brush_set_viewport(void* brush, float x, float y, float w, float h);
bool dm_noesis_tile_brush_get_viewport(void* brush, float out[4]);
bool dm_noesis_tile_brush_set_viewbox(void* brush, float x, float y, float w, float h);
bool dm_noesis_tile_brush_get_viewbox(void* brush, float out[4]);

// Transforms (NsGui/*Transform.h). Assign via set_component("RenderTransform").
void* dm_noesis_translate_transform_create(float x, float y);
bool dm_noesis_translate_transform_set(void* transform, float x, float y);
bool dm_noesis_translate_transform_get(void* transform, float* x, float* y);

void* dm_noesis_scale_transform_create(float sx, float sy, float cx, float cy);
bool dm_noesis_scale_transform_set(void* transform, float sx, float sy, float cx, float cy);
// out = {scaleX, scaleY, centerX, centerY}
bool dm_noesis_scale_transform_get(void* transform, float out[4]);

void* dm_noesis_rotate_transform_create(float angle, float cx, float cy);
bool dm_noesis_rotate_transform_set_angle(void* transform, float angle);
// out = {angle, centerX, centerY}
bool dm_noesis_rotate_transform_get(void* transform, float out[3]);

void* dm_noesis_skew_transform_create(float ax, float ay, float cx, float cy);
// out = {angleX, angleY, centerX, centerY}
bool dm_noesis_skew_transform_get(void* transform, float out[4]);

// matrix = {m00, m01, m10, m11, m20, m21} (Transform2 row-major layout).
void* dm_noesis_matrix_transform_create(const float matrix[6]);
bool dm_noesis_matrix_transform_set(void* transform, const float matrix[6]);
bool dm_noesis_matrix_transform_get(void* transform, float out[6]);

void* dm_noesis_transform_group_create(void);
bool dm_noesis_transform_group_add_child(void* group, void* child);
int32_t dm_noesis_transform_group_child_count(void* group);

// fields = {centerX, centerY, scaleX, scaleY, skewX, skewY, rotation,
//           translateX, translateY}
void* dm_noesis_composite_transform_create(const float fields[9]);
bool dm_noesis_composite_transform_get(void* transform, float out[9]);

// 3D transforms (NsGui/CompositeTransform3D.h, MatrixTransform3D.h). Assigned to
// an element via dm_noesis_element_set_transform3d (UIElement::SetTransform3D),
// NOT via RenderTransform.
//
// fields = {centerX, centerY, centerZ, rotationX, rotationY, rotationZ,
//           scaleX, scaleY, scaleZ, translateX, translateY, translateZ}
void* dm_noesis_composite_transform3d_create(const float fields[12]);
bool dm_noesis_composite_transform3d_set(void* transform, const float fields[12]);
bool dm_noesis_composite_transform3d_get(void* transform, float out[12]);

// matrix = 12 floats = Noesis::Transform3 (4 rows of Vector3, row-major).
void* dm_noesis_matrix_transform3d_create(const float matrix[12]);
bool dm_noesis_matrix_transform3d_set(void* transform, const float matrix[12]);
bool dm_noesis_matrix_transform3d_get(void* transform, float out[12]);

// Element Transform3D assignment (UIElement::SetTransform3D / GetTransform3D).
// `transform` is a borrowed Transform3D* (or null to clear); Noesis takes its
// own reference. set returns false if `element` is not a UIElement (or the
// non-null `transform` is not a Transform3D). get returns a borrowed Transform3D*
// (no +1) or null.
bool dm_noesis_element_set_transform3d(void* element, void* transform);
void* dm_noesis_element_get_transform3d(void* element);

// Effects (NsGui/BlurEffect.h, DropShadowEffect.h). Assign via
// set_component("Effect").
void* dm_noesis_blur_effect_create(float radius);
bool dm_noesis_blur_effect_set_radius(void* effect, float radius);
bool dm_noesis_blur_effect_get_radius(void* effect, float* out);

void* dm_noesis_drop_shadow_effect_create(const float color[4], float blur_radius,
                                          float direction, float shadow_depth, float opacity);
// out_color = {r,g,b,a}; any out pointer may be null to skip that field.
bool dm_noesis_drop_shadow_effect_get(void* effect, float out_color[4], float* out_blur,
                                      float* out_direction, float* out_shadow_depth,
                                      float* out_opacity);

// RenderOptions.BitmapScalingMode attached property (ordinals match
// Noesis::BitmapScalingMode: 0 Unspecified, 1 LowQuality, 2 HighQuality).
// get returns the ordinal or -1 if `obj` is not a DependencyObject.
bool dm_noesis_render_options_set_bitmap_scaling_mode(void* obj, int32_t mode);
int32_t dm_noesis_render_options_get_bitmap_scaling_mode(void* obj);

// ── Shape elements (TODO §10 / Phase D) ─────────────────────────────────────
//
// Implemented in cpp/noesis_shapes.cpp. *_create hands out a freshly-built
// shape with one owned +1 reference (the brushes handout() idiom); the Rust
// handle's Drop releases it. `shape` is a Shape* / FrameworkElement* /
// BaseComponent* (the same opaque handle used elsewhere); every entrypoint
// DynamicCasts and fails gracefully (false / null / -1) on a type mismatch.
// Noesis 3.2.13 ships only Rectangle/Ellipse/Line/Path shape elements — there
// is no Polygon/Polyline (see TODO §10 + Known SDK limitations).
void* dm_noesis_rectangle_create(void);
void* dm_noesis_ellipse_create(void);
void* dm_noesis_line_create(void);

// FrameworkElement Width/Height (a Shape's own size lives on these inherited DPs).
bool dm_noesis_shape_set_width(void* shape, float width);
bool dm_noesis_shape_get_width(void* shape, float* out);
bool dm_noesis_shape_set_height(void* shape, float height);
bool dm_noesis_shape_get_height(void* shape, float* out);

// Fill/Stroke reuse the brush wrappers: setters take any Brush* (null clears),
// getters return the live Brush* BORROWED (no +1) so tests can match by identity.
bool dm_noesis_shape_set_fill(void* shape, void* brush);
void* dm_noesis_shape_get_fill(void* shape);
bool dm_noesis_shape_set_stroke(void* shape, void* brush);
void* dm_noesis_shape_get_stroke(void* shape);

// Stroke scalar properties.
bool dm_noesis_shape_set_stroke_thickness(void* shape, float value);
bool dm_noesis_shape_get_stroke_thickness(void* shape, float* out);
bool dm_noesis_shape_set_stroke_miter_limit(void* shape, float value);
bool dm_noesis_shape_get_stroke_miter_limit(void* shape, float* out);
bool dm_noesis_shape_set_stroke_dash_offset(void* shape, float value);
bool dm_noesis_shape_get_stroke_dash_offset(void* shape, float* out);
bool dm_noesis_shape_set_trim_start(void* shape, float value);
bool dm_noesis_shape_get_trim_start(void* shape, float* out);
bool dm_noesis_shape_set_trim_end(void* shape, float value);
bool dm_noesis_shape_get_trim_end(void* shape, float* out);
bool dm_noesis_shape_set_trim_offset(void* shape, float value);
bool dm_noesis_shape_get_trim_offset(void* shape, float* out);

// Stroke enum properties: set returns false on type mismatch, get returns the
// ordinal or -1. Ordinals match Noesis::PenLineCap / PenLineJoin / Stretch.
bool dm_noesis_shape_set_stroke_dash_cap(void* shape, int32_t value);
int32_t dm_noesis_shape_get_stroke_dash_cap(void* shape);
bool dm_noesis_shape_set_stroke_start_line_cap(void* shape, int32_t value);
int32_t dm_noesis_shape_get_stroke_start_line_cap(void* shape);
bool dm_noesis_shape_set_stroke_end_line_cap(void* shape, int32_t value);
int32_t dm_noesis_shape_get_stroke_end_line_cap(void* shape);
bool dm_noesis_shape_set_stroke_line_join(void* shape, int32_t value);
int32_t dm_noesis_shape_get_stroke_line_join(void* shape);
bool dm_noesis_shape_set_stretch(void* shape, int32_t value);
int32_t dm_noesis_shape_get_stretch(void* shape);

// StrokeDashArray — Noesis exposes this as a string ("2 1 3"); get returns a
// borrowed pointer owned by the Shape (null if unset / not a Shape).
bool dm_noesis_shape_set_stroke_dash_array(void* shape, const char* dashes);
const char* dm_noesis_shape_get_stroke_dash_array(void* shape);

// Rectangle::RadiusX / RadiusY.
bool dm_noesis_rectangle_set_radius_x(void* shape, float value);
bool dm_noesis_rectangle_get_radius_x(void* shape, float* out);
bool dm_noesis_rectangle_set_radius_y(void* shape, float value);
bool dm_noesis_rectangle_get_radius_y(void* shape, float* out);

// Line::X1/Y1/X2/Y2 (set/get all four; out = {x1, y1, x2, y2}).
bool dm_noesis_line_set(void* shape, float x1, float y1, float x2, float y2);
bool dm_noesis_line_get(void* shape, float out[4]);
// ── ImageSource / BitmapSource family (TODO §12 "Bitmaps") ──────────────────
//
// Implemented in cpp/noesis_imaging.cpp. Every `*_create` returns a freshly-
// built BaseComponent* with a single owned reference (released by the owning
// Rust handle's Drop via dm_noesis_base_component_release). `cast`-style type
// checks make every setter/getter a no-op (false / null) on a wrong-type
// pointer. Headless, the GPU-resolved values (TextureSource texture, pixel
// dims, dpi) read back null / 0 — that resolution needs a RenderDevice render
// pass (see "Known SDK limitations" in TODO.md).

// CroppedBitmap (no GPU needed). `source` / the get return are borrowed
// BitmapSource* (Noesis takes its own reference on set; the get adds no +1).
// SourceRect is an Int32Rect {x, y (int32); width, height (uint32)}.
void* dm_noesis_cropped_bitmap_create(void);
bool dm_noesis_cropped_bitmap_set_source(void* crop, void* source);
void* dm_noesis_cropped_bitmap_get_source(void* crop);
bool dm_noesis_cropped_bitmap_set_source_rect(void* crop, int32_t x, int32_t y, uint32_t width,
                                              uint32_t height);
bool dm_noesis_cropped_bitmap_get_source_rect(void* crop, int32_t* x, int32_t* y, uint32_t* width,
                                              uint32_t* height);

// TextureSource. `texture` is a borrowed Noesis::Texture* (null => default ctor;
// real ones come from a host RenderDevice). get returns a borrowed Texture* or
// null (null until a host RenderDevice-created Texture is bound).
void* dm_noesis_texture_source_create(void* texture);
bool dm_noesis_texture_source_set_texture(void* source, void* texture);
void* dm_noesis_texture_source_get_texture(void* source);

// BitmapImage. `uri` is a UTF-8 string (null => default ctor). get returns a
// borrowed canonicalized UriSource string (valid while the image + its
// UriSource are unchanged), or null on a non-BitmapImage pointer.
void* dm_noesis_bitmap_image_create(const char* uri);
bool dm_noesis_bitmap_image_set_uri_source(void* image, const char* uri);
const char* dm_noesis_bitmap_image_get_uri_source(void* image);

// BitmapSource base getters (work on any BitmapSource subclass). Pixel dims /
// dpi default until resolved on a render pass. false on a non-BitmapSource.
bool dm_noesis_bitmap_source_get_pixel_size(void* source, int32_t* width, int32_t* height);
bool dm_noesis_bitmap_source_get_dpi(void* source, float* dpi_x, float* dpi_y);

// DynamicTextureSource. `callback` is pointer-ABI-compatible with
// Noesis::DynamicTextureSource::TextureRenderCallback
// (Texture* (*)(RenderDevice*, void*)); it is invoked from the render thread, so
// it only fires under a live RenderDevice render pass. create returns null if
// `callback` is null.
typedef void* (*dm_noesis_texture_render_callback)(void* device, void* user);
void* dm_noesis_dynamic_texture_source_create(uint32_t width, uint32_t height,
                                              dm_noesis_texture_render_callback callback,
                                              void* user);
bool dm_noesis_dynamic_texture_source_resize(void* source, uint32_t width, uint32_t height);
bool dm_noesis_dynamic_texture_source_get_pixel_size(void* source, uint32_t* width,
                                                     uint32_t* height);
// ── Typography & text properties (TODO §13) ─────────────────────────────────
//
// Implemented in cpp/noesis_typography.cpp. FontFamily is handed out with a +1
// reference (release via dm_noesis_base_component_release). The TextElement and
// Typography accessors take a borrowed DependencyObject* (`element`); every
// setter has a getter that re-reads from the live object. Enum ordinals match
// the Noesis headers: FontWeight (FontProperties.h, e.g. Normal=400, Bold=700),
// FontStyle (Normal=0, Oblique=1, Italic=2), FontStretch (UltraCondensed=1 …
// UltraExpanded=9), and the Typography enums (Typography.h).

// FontFamily(source) — `source` may be NULL for the default family. Returns a
// +1 FontFamily* (BaseComponent*).
void* dm_noesis_typography_font_family_create(const char* source);
// Borrowed source string (the text used to construct it); NULL on type mismatch.
const char* dm_noesis_typography_font_family_get_source(void* family);
// Number of concrete fonts the family resolved to via the registered font
// provider (0 with no provider, or if `family` is not a FontFamily). NOTE:
// 3.2.13 exposes per-family enumeration only — there is no API to enumerate the
// set of available family names from the font system (see TODO limitations).
uint32_t dm_noesis_typography_font_family_get_num_fonts(void* family);
// Borrowed name of the resolved font at `index`, or NULL if out of range.
const char* dm_noesis_typography_font_family_get_font_name(void* family, uint32_t index);

// TextElement attached font properties (static Get/Set on a DependencyObject).
// FontSize is in device-independent pixels. set returns false on type mismatch;
// get writes through `out` and returns false on type mismatch / null out.
bool dm_noesis_typography_text_element_set_font_size(void* element, float size);
bool dm_noesis_typography_text_element_get_font_size(void* element, float* out);
// FontFamily attached DP: set takes a borrowed FontFamily* (Noesis +1s it);
// get returns the borrowed FontFamily* currently set (no +1), or NULL.
bool dm_noesis_typography_text_element_set_font_family(void* element, void* family);
void* dm_noesis_typography_text_element_get_font_family(void* element);
// Foreground attached DP: set takes a borrowed Brush* (Noesis +1s it); get
// returns the borrowed Brush* currently set (no +1), or NULL.
bool dm_noesis_typography_text_element_set_foreground(void* element, void* brush);
void* dm_noesis_typography_text_element_get_foreground(void* element);
// FontWeight / FontStyle / FontStretch enums (ordinals as above).
bool dm_noesis_typography_text_element_set_font_weight(void* element, int32_t weight);
bool dm_noesis_typography_text_element_get_font_weight(void* element, int32_t* out);
bool dm_noesis_typography_text_element_set_font_style(void* element, int32_t style);
bool dm_noesis_typography_text_element_get_font_style(void* element, int32_t* out);
bool dm_noesis_typography_text_element_set_font_stretch(void* element, int32_t stretch);
bool dm_noesis_typography_text_element_get_font_stretch(void* element, int32_t* out);

// Typography attached DPs (representative subset; the remaining ~30 follow the
// identical SetValue/GetValue-with-DP-pointer pattern). Enum values use the
// Typography.h ordinals; the bool flags map directly.
bool dm_noesis_typography_set_capitals(void* element, int32_t value);
bool dm_noesis_typography_get_capitals(void* element, int32_t* out);
bool dm_noesis_typography_set_numeral_style(void* element, int32_t value);
bool dm_noesis_typography_get_numeral_style(void* element, int32_t* out);
bool dm_noesis_typography_set_fraction(void* element, int32_t value);
bool dm_noesis_typography_get_fraction(void* element, int32_t* out);
bool dm_noesis_typography_set_variants(void* element, int32_t value);
bool dm_noesis_typography_get_variants(void* element, int32_t* out);
bool dm_noesis_typography_set_standard_ligatures(void* element, bool value);
bool dm_noesis_typography_get_standard_ligatures(void* element, bool* out);
bool dm_noesis_typography_set_kerning(void* element, bool value);
bool dm_noesis_typography_get_kerning(void* element, bool* out);

// CompositionUnderline (IME composition ranges) on a TextBox. `style` matches
// Noesis::CompositionLineStyle (0 None, 1 Solid, 2 Dot, 3 Dash, 4 Squiggle).
// add/clear return false if `element` is not a TextBox; num returns -1; get
// writes the requested fields (any out may be NULL) and returns false on out of
// range / type mismatch.
bool dm_noesis_typography_text_box_add_composition_underline(void* element, uint32_t start,
                                                             uint32_t end, int32_t style,
                                                             bool bold);
int32_t dm_noesis_typography_text_box_num_composition_underlines(void* element);
bool dm_noesis_typography_text_box_get_composition_underline(void* element, uint32_t index,
                                                             uint32_t* out_start, uint32_t* out_end,
                                                             int32_t* out_style, bool* out_bold);
bool dm_noesis_typography_text_box_clear_composition_underlines(void* element);
// ── Immediate-mode drawing: Pen + DrawingContext (TODO §10) ─────────────────
//
// Implemented in cpp/noesis_drawing.cpp. The `Pen` is a code-built
// BaseComponent built/owned like the brushes above (handout +1, released via
// dm_noesis_base_component_release). The DrawingContext entrypoints take the
// BORROWED context handed to a class render callback (dm_noesis_render_fn);
// they DynamicCast it to a Noesis::DrawingContext* and fail gracefully (false)
// on a null / wrong-type context. Brush / Pen / Geometry / Transform /
// ImageSource arguments are borrowed BaseComponent* (or null for "none").

// Pen (NsGui/Pen.h). `brush` is a borrowed Brush* (or null); Noesis takes its
// own reference. Line-cap / join ordinals match Noesis::PenLineCap (0 Flat,
// 1 Square, 2 Round, 3 Triangle) / Noesis::PenLineJoin (0 Miter, 1 Bevel,
// 2 Round).
void* dm_noesis_pen_create(void* brush, float thickness);
bool dm_noesis_pen_set_brush(void* pen, void* brush);
void* dm_noesis_pen_get_brush(void* pen);
bool dm_noesis_pen_set_thickness(void* pen, float thickness);
bool dm_noesis_pen_get_thickness(void* pen, float* out);
bool dm_noesis_pen_set_line_caps(void* pen, int32_t start_cap, int32_t end_cap, int32_t dash_cap);
// out = {startCap, endCap, dashCap} ordinals.
bool dm_noesis_pen_get_line_caps(void* pen, int32_t out[3]);
bool dm_noesis_pen_set_line_join(void* pen, int32_t join, float miter_limit);
bool dm_noesis_pen_get_line_join(void* pen, int32_t* out_join, float* out_miter_limit);

// RectangleGeometry (NsGui/RectangleGeometry.h). A minimal Geometry primitive so
// the DrawGeometry / PushClip context entrypoints are reachable; rect is
// (x, y, w, h) with optional corner radii rX / rY. get reads the rect back as
// {x, y, w, h}.
void* dm_noesis_rectangle_geometry_create(float x, float y, float w, float h, float rX, float rY);
bool dm_noesis_rectangle_geometry_get_rect(void* geometry, float out[4]);

// DrawingContext draw / push / pop commands (NsGui/DrawingContext.h). `context`
// is the borrowed pointer from the render callback. Each returns false if the
// context is null / not a DrawingContext. Coordinates are in DIPs in the
// element's local space. A null brush / pen draws only the part the non-null
// argument covers (matching Noesis's own behaviour).
bool dm_noesis_drawing_draw_line(void* context, void* pen, float x0, float y0, float x1, float y1);
bool dm_noesis_drawing_draw_rectangle(void* context, void* brush, void* pen,
                                      float x, float y, float w, float h);
bool dm_noesis_drawing_draw_rounded_rectangle(void* context, void* brush, void* pen,
                                              float x, float y, float w, float h,
                                              float rX, float rY);
bool dm_noesis_drawing_draw_ellipse(void* context, void* brush, void* pen,
                                    float cx, float cy, float rX, float rY);
bool dm_noesis_drawing_draw_geometry(void* context, void* brush, void* pen, void* geometry);
// Returns false if `image_source` is null / not an ImageSource (DrawImage
// requires a real source — see Known SDK limitations re: building one headless).
bool dm_noesis_drawing_draw_image(void* context, void* image_source,
                                  float x, float y, float w, float h);
bool dm_noesis_drawing_pop(void* context);
bool dm_noesis_drawing_push_clip(void* context, void* geometry);
bool dm_noesis_drawing_push_transform(void* context, void* transform);
// `mode` ordinal matches Noesis::BlendingMode (0 Normal, 1 Multiply, 2 Screen,
// 3 Additive).
bool dm_noesis_drawing_push_blending_mode(void* context, int32_t mode);

// ── Controls — programmatic access (TODO §8 / Phase B) ──────────────────────
//
// Implemented in cpp/noesis_controls.cpp. Each entrypoint DynamicCasts to the
// right control type and fails gracefully (false / null / sentinel) on a type
// mismatch. `element` is a FrameworkElement* / BaseComponent* (the same opaque
// handle the rest of the FrameworkElement surface uses).

// Selector — SelectedIndex / SelectedItem (ListBox/ComboBox/TabControl/ListView).
// get_selected_index writes *out (-1 == empty selection); both return false if
// `element` is not a Selector. set_selected_index coerces out-of-range to -1.
// get_selected_item returns a BORROWED (no +1) pointer (the data item for an
// ItemsSource-bound control, else the container), null if empty / not a Selector.
// set_selected_item takes a borrowed item (Noesis takes its own ref); null clears.
bool dm_noesis_selector_get_selected_index(void* element, int32_t* out);
bool dm_noesis_selector_set_selected_index(void* element, int32_t index);
void* dm_noesis_selector_get_selected_item(void* element);
bool dm_noesis_selector_set_selected_item(void* element, void* item);

// ItemsControl.Items direct mutation (NOT ItemsSource — no-op when an external
// ItemsSource is set, since Items is then read-only). `item` is a borrowed
// BaseComponent* (typically a boxed value); the collection takes its own ref.
// items_add returns the new index, or -1 on a non-ItemsControl / rejected add.
int32_t dm_noesis_items_control_items_add(void* element, void* item);
bool dm_noesis_items_control_items_insert(void* element, uint32_t index, void* item);
bool dm_noesis_items_control_items_remove_at(void* element, uint32_t index);
bool dm_noesis_items_control_items_clear(void* element);

// RangeBase — `which`: 0 = Value, 1 = Minimum, 2 = Maximum (Slider/ProgressBar/
// ScrollBar). Getter writes *out, returns false on a non-RangeBase / bad `which`.
// Setter runs Noesis coercion (Value clamped to [Minimum, Maximum]).
bool dm_noesis_rangebase_get(void* element, int32_t which, float* out);
bool dm_noesis_rangebase_set(void* element, int32_t which, float value);

// ToggleButton.IsChecked tri-state. `state`: 0 = unchecked, 1 = checked,
// 2 = indeterminate (null). Getter writes *out_state, returns false on a
// non-ToggleButton. (CheckBox/RadioButton.)
bool dm_noesis_toggle_get_is_checked(void* element, int8_t* out_state);
bool dm_noesis_toggle_set_is_checked(void* element, int8_t state);

// Popup.IsOpen / Expander.IsExpanded. Getter writes *out, returns false on a
// type mismatch.
bool dm_noesis_popup_get_is_open(void* element, bool* out);
bool dm_noesis_popup_set_is_open(void* element, bool open);
bool dm_noesis_expander_get_is_expanded(void* element, bool* out);
bool dm_noesis_expander_set_is_expanded(void* element, bool expanded);

// ScrollViewer — `which`: 0 = HorizontalOffset, 1 = VerticalOffset,
// 2 = ScrollableWidth, 3 = ScrollableHeight, 4 = ExtentHeight,
// 5 = ViewportHeight (all read-only computed metrics). Getter writes *out,
// returns false on a non-ScrollViewer / bad `which`. Scrolling is via methods.
bool dm_noesis_scrollviewer_get(void* element, int32_t which, float* out);
bool dm_noesis_scrollviewer_scroll_to_horizontal(void* element, float offset);
bool dm_noesis_scrollviewer_scroll_to_vertical(void* element, float offset);
bool dm_noesis_scrollviewer_scroll_to_home(void* element);
bool dm_noesis_scrollviewer_scroll_to_end(void* element);

// TextBox selection / caret. `which` for the int get/set: 0 = SelectionStart,
// 1 = SelectionLength, 2 = CaretIndex. Getter writes *out, returns false on a
// non-TextBox. get_selected_text returns a BORROWED pointer (copy immediately),
// null on a non-TextBox.
bool dm_noesis_textbox_get_int(void* element, int32_t which, int32_t* out);
bool dm_noesis_textbox_set_int(void* element, int32_t which, int32_t value);
bool dm_noesis_textbox_select(void* element, int32_t start, int32_t length);
bool dm_noesis_textbox_select_all(void* element);
const char* dm_noesis_textbox_get_selected_text(void* element);

// PasswordBox password. get returns a BORROWED pointer (copy immediately), null
// on a non-PasswordBox.
const char* dm_noesis_passwordbox_get_password(void* element);
bool dm_noesis_passwordbox_set_password(void* element, const char* password);
// ── ResourceDictionary, Style, templates (TODO §7) ──────────────────────────
//
// ResourceDictionary create/own + key→component add + borrowed lookup + merged
// dictionaries + parse-from-XAML; application resources install/query; Style
// from code (target type + setters + based-on) with element assign/read-back;
// ControlTemplate / DataTemplate parse + assign + FrameworkTemplate::FindName.
//
// OWNERSHIP: *_create / *_parse return a +1-owned object (release via the
// matching *_destroy or the generic dm_noesis_base_component_release). The
// *_get_resources / *_get_style / *_get_template getters AddRef before handing
// out, so the caller owns a +1 too. *_find / *_find_resource /
// *_find_name / *_get_application_resources hand out BORROWED pointers (no +1) —
// do NOT release; valid only transiently.

// Box a float as a BoxedValue<float> (+1 ref). Companion to the bool/int32/
// double boxers in the binding section — float DPs (FontSize, Opacity, …) need
// a float box for a Style Setter / resource value to apply.
void* dm_noesis_box_float(float value);

// Create an empty ResourceDictionary (+1 ref for the caller).
void* dm_noesis_resource_dictionary_create(void);
void dm_noesis_resource_dictionary_destroy(void* dict);
// Parse a bare <ResourceDictionary> from an in-memory XAML string. +1 ref for
// the caller; NULL if malformed or the root is not a ResourceDictionary.
void* dm_noesis_resource_dictionary_parse(const char* xaml);
// Number of base-dictionary entries (excludes merged dictionaries).
uint32_t dm_noesis_resource_dictionary_count(void* dict);
// Add a borrowed `value` under `key`; the dictionary stores its own reference.
// false on a NULL/non-dictionary handle or NULL key/value.
bool dm_noesis_resource_dictionary_add(void* dict, const char* key, void* value);
// Whether the dictionary (or a merged one) contains `key`.
bool dm_noesis_resource_dictionary_contains(void* dict, const char* key);
// Borrowed (no +1) lookup by key; NULL if absent (non-throwing Find).
void* dm_noesis_resource_dictionary_find(void* dict, const char* key);
// Add `merged` to `dict`'s MergedDictionaries collection (takes its own ref).
bool dm_noesis_resource_dictionary_add_merged(void* dict, void* merged);

// Install `dict` as the process-global application resources (Noesis takes its
// own reference). NULL clears them.
void dm_noesis_gui_set_application_resources(void* dict);
// Borrowed (no +1) application ResourceDictionary*, or NULL if none installed.
void* dm_noesis_gui_get_application_resources(void);
// Register `uri`'s dictionary in the internal theme (default styles). false on
// a NULL/empty uri.
bool dm_noesis_gui_register_default_styles(const char* uri);

// +1-owned ResourceDictionary* for `element`'s local Resources (AddRef'd), or
// NULL if none / not a FrameworkElement.
void* dm_noesis_framework_element_get_resources(void* element);
// Replace `element`'s local Resources with `dict`. false if `element` is not a
// FrameworkElement or `dict` not a ResourceDictionary.
bool dm_noesis_framework_element_set_resources(void* element, void* dict);
// Non-throwing FindResource walking the logical parent chain + app resources.
// Borrowed (no +1); NULL if not found / not a FrameworkElement.
void* dm_noesis_framework_element_find_resource(void* element, const char* key);

// Create an empty Style (+1 ref for the caller).
void* dm_noesis_style_create(void);
void dm_noesis_style_destroy(void* style);
// Resolve `type_name` via reflection and set it as the style's TargetType.
// false on a NULL/non-Style handle or an unknown type name.
bool dm_noesis_style_set_target_type(void* style, const char* type_name);
// Append a Setter: resolve `dp_name` on the style's TargetType, store the boxed
// `value` (the setter takes its own ref). false if no TargetType, unknown DP,
// NULL value, or non-Style handle.
bool dm_noesis_style_add_setter(void* style, const char* dp_name, void* value);
// Set the BasedOn style (NULL clears). No-op on a NULL/non-Style handle.
void dm_noesis_style_set_based_on(void* style, void* base);

// Assign `style` to `element` (FrameworkElement::SetStyle). false if `element`
// is not a FrameworkElement or `style` not a Style.
bool dm_noesis_framework_element_set_style(void* element, void* style);
// +1-owned Style* for `element`'s assigned Style (AddRef'd), or NULL.
void* dm_noesis_framework_element_get_style(void* element);

// Parse a bare <ControlTemplate> / <DataTemplate> from a string. +1 ref for the
// caller; NULL if malformed or the root is the wrong type.
void* dm_noesis_control_template_parse(const char* xaml);
void* dm_noesis_data_template_parse(const char* xaml);
// Assign a ControlTemplate to a Control (Control::SetTemplate). false if
// `control` is not a Control or `tmpl` not a ControlTemplate.
bool dm_noesis_control_set_template(void* control, void* tmpl);
// +1-owned ControlTemplate* for `control`'s assigned Template, or NULL.
void* dm_noesis_control_get_template(void* control);
// FrameworkTemplate::FindName within `tmpl` applied to `templated_parent`.
// Borrowed (no +1); NULL if not found / wrong types.
void* dm_noesis_framework_template_find_name(
    void* tmpl, const char* name, void* templated_parent);

// ── Animation & timing (TODO §6 / Phase C) ──────────────────────────────────
//
// Code-built Storyboards, animation classes, key frames, and easing functions.
// Each `*_create` returns a +1-owned BaseComponent* (release via
// dm_noesis_base_component_release, mirrored by the owning Rust handle's Drop).
// Adding a timeline to a Storyboard, or a key frame / easing function to its
// parent, makes Noesis take its own reference. Animations advance off the View
// clock — pump view.update(t). See cpp/noesis_animation.cpp.

// Storyboard. `fe` arguments below are nullable FrameworkElement*; pass the
// element tree root (also the namescope for TargetName resolution).
void* dm_noesis_storyboard_create(void);
bool dm_noesis_storyboard_add_child(void* sb, void* timeline);
int32_t dm_noesis_storyboard_child_count(void* sb);
// Attached-property setters; `timeline` is a child animation (DependencyObject).
bool dm_noesis_storyboard_set_target_name(void* timeline, const char* name);
bool dm_noesis_storyboard_set_target_property(void* timeline, const char* path);
bool dm_noesis_storyboard_set_target(void* timeline, void* target);
// `controllable` must be true for Pause/Resume/Stop/Seek to take effect.
bool dm_noesis_storyboard_begin(void* sb, void* fe, bool controllable);
bool dm_noesis_storyboard_pause(void* sb, void* fe);
bool dm_noesis_storyboard_resume(void* sb, void* fe);
bool dm_noesis_storyboard_stop(void* sb, void* fe);
bool dm_noesis_storyboard_seek(void* sb, void* fe, double seconds);
bool dm_noesis_storyboard_is_playing(void* sb, void* fe);
bool dm_noesis_storyboard_is_paused(void* sb, void* fe);

// Timeline common knobs (apply to any Timeline / animation handle).
bool dm_noesis_timeline_set_duration_seconds(void* tl, double seconds);
bool dm_noesis_timeline_set_duration_auto(void* tl);
bool dm_noesis_timeline_set_duration_forever(void* tl);
double dm_noesis_timeline_get_duration_seconds(void* tl);
bool dm_noesis_timeline_set_begin_time_seconds(void* tl, double seconds);
bool dm_noesis_timeline_set_auto_reverse(void* tl, bool value);
bool dm_noesis_timeline_set_speed_ratio(void* tl, float value);
bool dm_noesis_timeline_set_fill_behavior(void* tl, int32_t behavior);  // 0 HoldEnd, 1 Stop
bool dm_noesis_timeline_set_repeat_count(void* tl, float count);
bool dm_noesis_timeline_set_repeat_duration(void* tl, double seconds);
bool dm_noesis_timeline_set_repeat_forever(void* tl);

// From/To/By animations. Each setter takes a `has` flag (false => clear to
// null, so the animation infers the endpoint from the base property value).
void* dm_noesis_double_animation_create(void);
bool dm_noesis_double_animation_set_from(void* anim, bool has, float v);
bool dm_noesis_double_animation_set_to(void* anim, bool has, float v);
bool dm_noesis_double_animation_set_by(void* anim, bool has, float v);

void* dm_noesis_color_animation_create(void);
bool dm_noesis_color_animation_set_from(void* anim, bool has, const float color[4]);
bool dm_noesis_color_animation_set_to(void* anim, bool has, const float color[4]);
bool dm_noesis_color_animation_set_by(void* anim, bool has, const float color[4]);

void* dm_noesis_thickness_animation_create(void);
bool dm_noesis_thickness_animation_set_from(void* anim, bool has, const float t[4]);
bool dm_noesis_thickness_animation_set_to(void* anim, bool has, const float t[4]);
bool dm_noesis_thickness_animation_set_by(void* anim, bool has, const float t[4]);

void* dm_noesis_point_animation_create(void);
bool dm_noesis_point_animation_set_from(void* anim, bool has, float x, float y);
bool dm_noesis_point_animation_set_to(void* anim, bool has, float x, float y);
bool dm_noesis_point_animation_set_by(void* anim, bool has, float x, float y);

// Assign an easing function to a Double/Color/Thickness/Point From-To animation.
bool dm_noesis_animation_set_easing_function(void* anim, void* easing);

// Easing functions. kind: 0 Quadratic, 1 Cubic, 2 Quartic, 3 Quintic, 4 Sine,
// 5 Circle, 6 Back, 7 Bounce, 8 Elastic, 9 Exponential, 10 Power. mode matches
// Noesis::EasingMode (0 EaseOut, 1 EaseIn, 2 EaseInOut).
void* dm_noesis_easing_function_create(int32_t kind, int32_t mode);
bool dm_noesis_easing_function_set_amplitude(void* easing, float value);      // Back
bool dm_noesis_easing_function_set_power(void* easing, float value);          // Power
bool dm_noesis_easing_function_set_exponent(void* easing, float value);       // Exponential
bool dm_noesis_easing_function_set_oscillations(void* easing, int32_t value); // Elastic/Bounce
bool dm_noesis_easing_function_set_springiness(void* easing, float value);    // Elastic/Bounce

// Key-frame animations. add_keyframe kind: 0 Discrete, 1 Linear, 2 Easing
// (uses `easing` if non-null). key_time is in seconds.
void* dm_noesis_double_animation_keyframes_create(void);
bool dm_noesis_double_animation_add_keyframe(void* anim, int32_t kind, double key_time_seconds,
                                             float value, void* easing);
void* dm_noesis_color_animation_keyframes_create(void);
bool dm_noesis_color_animation_add_keyframe(void* anim, int32_t kind, double key_time_seconds,
                                            const float color[4], void* easing);

// Storyboard-less direct animation: start `anim` on `target`'s `dp_name`
// property using the target's view TimeManager. `target` must be a
// FrameworkElement connected to a live View. handoff matches
// Noesis::HandoffBehavior (0 SnapshotAndReplace, 1 Compose).
bool dm_noesis_animation_begin_on(void* anim, void* target, const char* dp_name, int32_t handoff);
// ── Plain (non-DependencyObject) view models + MultiBinding (TODO §9 + §3) ──
//
// The bevy-bridge unblocker: a binding source that is NOT a DependencyObject.
// A `RustPlainVm` is a plain `Noesis::BaseComponent` that (a) implements
// `INotifyPropertyChanged` so a bound UI target refreshes when Rust raises
// PropertyChanged, and (b) carries a per-registration synthetic `TypeClass`
// whose properties resolve through reflection (custom `TypeProperty`
// accessors) — so `{Binding Title}` against this object as a DataContext reads
// a value Rust pushed in. Each property's current value is stored per-instance
// as a boxed `BaseComponent*` (use the dm_noesis_box_* helpers to produce one);
// reflection reads it back through `TypeProperty::GetComponent`.
//
// Lifetime mirrors the synthetic-class registry in noesis_classes.cpp: the
// registration token (`PlainClassData*`) is refcounted — the Rust caller owns
// the initial +1 (released by dm_noesis_plain_vm_unregister), every live
// instance holds its own share, and the donated Rust free handler runs exactly
// once when the last reference drops. A shutdown sweep
// (dm_noesis_plain_vm_force_free_at_shutdown) defensively frees any handler box
// whose instances bypassed normal teardown.

// Content-type tag for a plain-VM reflected property. Determines the property's
// reflected `Type*` (so the binding engine can convert to the target DP type)
// and which Boxing the stored value is expected to carry.
typedef enum dm_noesis_plain_type {
    DM_NOESIS_PLAIN_INT32          = 0,
    DM_NOESIS_PLAIN_DOUBLE         = 1,
    DM_NOESIS_PLAIN_BOOL           = 2,
    DM_NOESIS_PLAIN_STRING         = 3,
    DM_NOESIS_PLAIN_BASE_COMPONENT = 4
} dm_noesis_plain_type;

// Invoked when a TwoWay / OneWayToSource binding writes a value BACK to a
// plain-VM property (the UI mutated the source). `instance` is the borrowed
// RustPlainVm*, `prop_index` the dense index returned by register_property, and
// `boxed_value` a borrowed boxed `BaseComponent*` (may be NULL) — copy it
// immediately (unbox with the dm_noesis_unbox_* helpers). The value is also
// stored in the instance so a subsequent reflection read returns it. Optional.
typedef void (*dm_noesis_plain_set_fn)(
    void* userdata, void* instance, uint32_t prop_index, void* boxed_value);

// Free callback for the donated registration userdata; runs exactly once when
// the PlainClassData refcount hits zero. Optional (may be NULL).
typedef void (*dm_noesis_plain_free_fn)(void* userdata);

// Register a plain-VM type named `type_name`. Returns an opaque registration
// token (owned via the refcount described above) or NULL on a NULL name or a
// name already registered with Noesis Reflection. `on_set` / `free_handler`
// may be NULL; `userdata` ownership transfers to C++.
void* dm_noesis_plain_vm_register(
    const char* type_name,
    dm_noesis_plain_set_fn on_set,
    void* userdata,
    dm_noesis_plain_free_fn free_handler);

// Add a reflected property `prop_name` of content type `content_type`
// (dm_noesis_plain_type). Returns the dense property index, or UINT32_MAX on
// failure (NULL args / bad tag / called after an instance exists). Call before
// creating instances.
uint32_t dm_noesis_plain_vm_register_property(
    void* token, const char* prop_name, uint32_t content_type);

// Create an instance of a registered plain-VM type. Returns a BaseComponent*
// with +1 ref for the caller (release via dm_noesis_base_component_release).
// Set it as an element's DataContext (dm_noesis_framework_element_set_data_context)
// and author `{Binding PropName}` in XAML. NULL on a NULL token.
void* dm_noesis_plain_vm_create_instance(void* token);

// Store `boxed_value` (a boxed BaseComponent*, e.g. from dm_noesis_box_string;
// may be NULL to clear) as the current value of property `prop_index`. The
// instance takes its OWN reference — the caller still owns / must release its
// boxed value. Does NOT raise PropertyChanged (call dm_noesis_plain_vm_notify).
// false on a NULL instance or out-of-range index.
bool dm_noesis_plain_vm_set_value(void* instance, uint32_t prop_index, void* boxed_value);

// +1-owned boxed value currently stored for `prop_index` (AddRef'd; release via
// dm_noesis_base_component_release), or NULL if unset / out of range. Reads the
// reflection-visible value back without going through the binding.
void* dm_noesis_plain_vm_get_value(void* instance, uint32_t prop_index);

// Raise INotifyPropertyChanged.PropertyChanged for `prop_name` on `instance`,
// so every binding sourced from this property re-reads. false on NULL args.
bool dm_noesis_plain_vm_notify(void* instance, const char* prop_name);

// Stop new instances being created and release the Rust caller's +1 on the
// registration token. Live instances keep the registration alive until they die.
void dm_noesis_plain_vm_unregister(void* token);

// Shutdown sweep — see dm_noesis_classes_force_free_at_shutdown. Called from
// dm_noesis_shutdown after Noesis::Shutdown.
void dm_noesis_plain_vm_force_free_at_shutdown(void);

// ── IMultiValueConverter + MultiBinding (TODO §3) ──────────────────────────
//
// MultiBinding combines N child Bindings through an IMultiValueConverter into a
// single target value. RustMultiValueConverter forwards TryConvert into a Rust
// vtable over an ARRAY of boxed values (one per child binding); the converter
// boxes its combined result. Lifetime is modelled on RustValueConverter.

// `values` points at `count` borrowed boxed `BaseComponent*` (each may be NULL),
// one per child Binding in source order. `target_type` is an opaque
// `const Noesis::Type*` (ignore it for simple converters). Write a +1-owned
// `BaseComponent*` into `*out_result` (ownership transfers to Noesis) and return
// true; return false to signal UnsetValue (fallback / default). Same threading
// contract as the single-value converter — fires from Noesis's binding pump.
typedef struct dm_noesis_multi_value_converter_vtable {
    bool (*convert)(
        void* userdata, void* const* values, uint32_t count,
        const void* target_type, void* parameter, void** out_result);
} dm_noesis_multi_value_converter_vtable;

typedef void (*dm_noesis_multi_value_converter_free_fn)(void* userdata);

// Create a Rust-backed IMultiValueConverter. +1 ref for the caller (release via
// dm_noesis_multi_value_converter_destroy). NULL if `vt` is NULL.
void* dm_noesis_multi_value_converter_create(
    const dm_noesis_multi_value_converter_vtable* vt,
    void* userdata,
    dm_noesis_multi_value_converter_free_fn free_handler);
void dm_noesis_multi_value_converter_destroy(void* converter);

// Create an empty MultiBinding (+1 ref for the caller; release via
// dm_noesis_multi_binding_destroy). SetBinding takes its own reference.
void* dm_noesis_multi_binding_create(void);
void dm_noesis_multi_binding_destroy(void* multi_binding);
// Append a child Binding (a dm_noesis_binding_create result). The MultiBinding
// takes its own reference. false on NULL/non-MultiBinding/non-Binding args.
bool dm_noesis_multi_binding_add_binding(void* multi_binding, void* binding);
// Attach the IMultiValueConverter (NULL clears). No-op on a bad handle.
void dm_noesis_multi_binding_set_converter(void* multi_binding, void* converter);
// Borrowed converter parameter (NULL clears).
void dm_noesis_multi_binding_set_converter_parameter(void* multi_binding, void* parameter);
// BindingMode ordinal (see dm_noesis_binding_set_mode).
void dm_noesis_multi_binding_set_mode(void* multi_binding, int32_t mode);
// Wire the MultiBinding onto `element`'s `dp_name` property. false if `element`
// is not a DependencyObject, the DP name is unknown, or args are NULL.
bool dm_noesis_set_multi_binding(void* element, const char* dp_name, void* multi_binding);
// ── Reflection meta: enums / routed events / factory / type converters (TODO §9) ──
//
// Runtime registration of "other reflected entities" against Noesis's
// reflection database, so XAML / bindings / the parser can resolve them the
// same way they resolve compile-time NS_REGISTER_* declarations. These reuse
// the synthetic-type machinery from noesis_classes.cpp (RustContentControl) for
// the per-type owner; everything here is keyed by the reflected type *name* so
// it does not need the opaque ClassData token.

// (A) Custom enums ----------------------------------------------------------

// One (string name -> integer value) pair of a runtime enum.
typedef struct dm_noesis_enum_value {
    const char* name;
    int32_t     value;
} dm_noesis_enum_value;

// Register a named runtime enum (a Noesis::TypeEnum) with `count` string<->int
// pairs, so it is reachable by reflection name (XAML enum-typed values, Style
// setters, the EnumConverter path). Returns a borrowed `const Noesis::Type*`
// (owned by the reflection registry; do NOT release) or NULL on a NULL/empty
// name or if the name is already registered. Idempotent-unsafe: a duplicate
// name returns NULL rather than shadowing.
void* dm_noesis_register_enum(
    const char* name, const dm_noesis_enum_value* values, uint32_t count);

// Resolve `enum_type` (reflected name) and look up the integer value of
// `value_name`. Returns false if the type is unknown / not an enum / the name
// is not a member. This reads straight through Noesis::TypeEnum::HasName, so it
// is the ground truth of what was registered.
bool dm_noesis_enum_value_from_name(
    const char* enum_type, const char* value_name, int32_t* out_value);

// Inverse of the above: the member name for an integer value (borrowed string,
// valid while Noesis lives — it is an interned Symbol). false if unknown.
bool dm_noesis_enum_name_from_value(
    const char* enum_type, int32_t value, const char** out_name);

// Resolve the TypeConverter registered for `type_name` (TypeConverter::Get) and
// convert `str` to a boxed value via TryConvertFromString. Writes a +1-owned
// boxed `BaseComponent*` to *out_boxed (release via base_component_release).
// This is the exact string->value path the XAML parser drives for a typed
// property. Returns false if the type / converter is unknown or the string
// does not convert.
bool dm_noesis_type_converter_from_string(
    const char* type_name, const char* str, void** out_boxed);

// (B) Custom routed events --------------------------------------------------

// Register a routed event named `event_name` on the registered type
// `type_name` (must own a UIElementData meta — i.e. a Rust-backed
// ContentControl from dm_noesis_class_register). `strategy`: 0 Tunnel,
// 1 Bubble, 2 Direct. Returns false if the type is unknown, has no
// UIElementData, or the name is already registered on it.
bool dm_noesis_register_routed_event(
    const char* type_name, const char* event_name, int32_t strategy);

// Raise the routed event `event_name` from `element` (a UIElement), resolving
// it through the element's class hierarchy (FindRoutedEvent) and dispatching
// via UIElement::RaiseEvent. Returns false if `element` is not a UIElement or
// the event is not found. Subscribers wired with dm_noesis_subscribe_event
// observe it.
bool dm_noesis_raise_routed_event(void* element, const char* event_name);

// (C) Factory / component metadata ------------------------------------------

// Whether a component named `name` is registered in Noesis::Factory (so
// `<ns:name/>` can be instantiated by the XAML parser). Rust-backed classes
// register their factory creator in dm_noesis_class_register.
bool dm_noesis_factory_is_registered(const char* name);

// Attach ContentPropertyMetaData(prop_name) to the registered type `type_name`,
// so XAML child content (`<ns:Thing><Child/></ns:Thing>`) is routed into the
// `prop_name` property instead of the inherited content property. Returns false
// if the type is unknown.
bool dm_noesis_type_set_content_property(
    const char* type_name, const char* prop_name);

// (D) Custom reflection TypeConverter registration is DEFERRED — not exposed in
// 3.2.13. TypeConverter::Get resolves converters via an internal registry that
// TypeConverterMetaData + Factory::RegisterComponent do not drive at runtime.
// The consumption side (dm_noesis_type_converter_from_string above) works for
// any built-in / reflected type. See TODO.md "Known SDK limitations".

// ── Geometry object model (TODO §10) ────────────────────────────────────────
//
// Code-built Geometry objects (NsGui/Geometry.h and derivatives). Every
// *_create hands out one owned BaseComponent reference (release via
// dm_noesis_base_component_release, mirrored by the owning Rust handle's Drop).
// Assign a finished geometry to a Path's Data via the generic component DP path
// (FrameworkElement::set_component("Data", ...)); Noesis takes its own
// reference, so the Rust handle may drop afterwards. `cast`-style type checks
// make every accessor a no-op (false / -1) on a wrong-type pointer. Rects are
// float[4] = {x, y, width, height}; points are passed as (x, y) pairs. FillRule
// ordinals match Noesis::FillRule (0 = EvenOdd, 1 = Nonzero); GeometryCombineMode
// matches Noesis::GeometryCombineMode (0 Union, 1 Intersect, 2 Xor, 3 Exclude);
// SweepDirection matches Noesis::SweepDirection (0 Counterclockwise, 1 Clockwise).

// Geometry base — works on any Geometry* (StreamGeometry, PathGeometry, …).
// out = {x, y, width, height}.
bool dm_noesis_geometry_get_bounds(void* geometry, float out[4]);
// Render bounds with a null Pen (no stroke widening). out = {x, y, w, h}.
bool dm_noesis_geometry_get_render_bounds(void* geometry, float out[4]);
// 1 = empty, 0 = non-empty, -1 = not a Geometry.
int32_t dm_noesis_geometry_is_empty(void* geometry);
// Assign / read the Transform applied to the geometry. set takes a borrowed
// Transform* (or null to clear); Noesis takes its own reference. get returns a
// borrowed Transform* (no +1) or null.
bool dm_noesis_geometry_set_transform(void* geometry, void* transform);
void* dm_noesis_geometry_get_transform(void* geometry);

// StreamGeometry + StreamGeometryContext.
void* dm_noesis_stream_geometry_create(void);
// Build from an SVG path-data string (e.g. "M 0,0 L 10,0 10,10 Z"). NULL data
// yields an empty geometry.
void* dm_noesis_stream_geometry_create_from_data(const char* data);
bool dm_noesis_stream_geometry_set_data(void* geometry, const char* data);
bool dm_noesis_stream_geometry_set_fill_rule(void* geometry, int32_t rule);
int32_t dm_noesis_stream_geometry_get_fill_rule(void* geometry);
// Open a drawing context. Returns an opaque heap StreamGeometryContext* that
// keeps the geometry alive; finish with dm_noesis_stream_geometry_context_close
// (flush + free) or dm_noesis_stream_geometry_context_destroy (free, no flush).
void* dm_noesis_stream_geometry_open(void* geometry);
bool dm_noesis_stream_geometry_context_begin_figure(void* ctx, float x, float y, bool is_closed);
bool dm_noesis_stream_geometry_context_line_to(void* ctx, float x, float y);
bool dm_noesis_stream_geometry_context_cubic_to(void* ctx, float x1, float y1, float x2, float y2,
                                                float x3, float y3);
bool dm_noesis_stream_geometry_context_quadratic_to(void* ctx, float x1, float y1, float x2,
                                                    float y2);
bool dm_noesis_stream_geometry_context_arc_to(void* ctx, float x, float y, float width,
                                              float height, float rotation_deg, bool is_large_arc,
                                              int32_t sweep_direction);
bool dm_noesis_stream_geometry_context_set_is_closed(void* ctx, bool is_closed);
// Close the context: flush its commands into the geometry, then free it.
bool dm_noesis_stream_geometry_context_close(void* ctx);
// Free the context WITHOUT flushing (the geometry is left unaltered).
void dm_noesis_stream_geometry_context_destroy(void* ctx);

// PathGeometry + PathFigureCollection of PathFigure.
void* dm_noesis_path_geometry_create(void);
bool dm_noesis_path_geometry_set_fill_rule(void* geometry, int32_t rule);
int32_t dm_noesis_path_geometry_get_fill_rule(void* geometry);
// Append a borrowed PathFigure*; the collection takes its own reference.
// Returns the new index, or -1 on failure.
int32_t dm_noesis_path_geometry_add_figure(void* geometry, void* figure);
int32_t dm_noesis_path_geometry_figure_count(void* geometry);

// PathFigure + PathSegmentCollection of PathSegment.
void* dm_noesis_path_figure_create(void);
bool dm_noesis_path_figure_set_start_point(void* figure, float x, float y);
bool dm_noesis_path_figure_get_start_point(void* figure, float out[2]);
bool dm_noesis_path_figure_set_is_closed(void* figure, bool is_closed);
bool dm_noesis_path_figure_set_is_filled(void* figure, bool is_filled);
// 1 = true, 0 = false, -1 = not a PathFigure.
int32_t dm_noesis_path_figure_get_is_closed(void* figure);
int32_t dm_noesis_path_figure_get_is_filled(void* figure);
// Append a borrowed PathSegment*; the collection takes its own reference.
int32_t dm_noesis_path_figure_add_segment(void* figure, void* segment);
int32_t dm_noesis_path_figure_segment_count(void* figure);

// Path segments. Each *_create hands out one owned reference.
void* dm_noesis_line_segment_create(float x, float y);
bool dm_noesis_line_segment_get_point(void* segment, float out[2]);

void* dm_noesis_bezier_segment_create(float x1, float y1, float x2, float y2, float x3, float y3);
// out = {x1, y1, x2, y2, x3, y3}
bool dm_noesis_bezier_segment_get(void* segment, float out[6]);

void* dm_noesis_quadratic_bezier_segment_create(float x1, float y1, float x2, float y2);
// out = {x1, y1, x2, y2}
bool dm_noesis_quadratic_bezier_segment_get(void* segment, float out[4]);

void* dm_noesis_arc_segment_create(float x, float y, float width, float height, float rotation_deg,
                                   bool is_large_arc, int32_t sweep_direction);
// out_point = {x, y}; out_size = {width, height}; any out pointer may be null.
bool dm_noesis_arc_segment_get(void* segment, float out_point[2], float out_size[2],
                               float* out_rotation_deg, bool* out_is_large_arc,
                               int32_t* out_sweep_direction);

// Poly* segments: `points` is `num_points` (x, y) pairs (2 * num_points floats).
void* dm_noesis_poly_line_segment_create(const float* points, uint32_t num_points);
void* dm_noesis_poly_bezier_segment_create(const float* points, uint32_t num_points);
void* dm_noesis_poly_quadratic_bezier_segment_create(const float* points, uint32_t num_points);
// Read-back over any of the three poly segment types. count returns -1 on a
// non-poly pointer; get_point fills out = {x, y}.
int32_t dm_noesis_poly_segment_point_count(void* segment);
bool dm_noesis_poly_segment_get_point(void* segment, uint32_t index, float out[2]);

// EllipseGeometry.
void* dm_noesis_ellipse_geometry_create(float cx, float cy, float rx, float ry);
// out = {centerX, centerY, radiusX, radiusY}
bool dm_noesis_ellipse_geometry_get(void* geometry, float out[4]);

// RectangleGeometry. rx / ry round the corners.
void* dm_noesis_drawing_rect_geometry_create(float x, float y, float width, float height, float rx,
                                             float ry);
// out_rect = {x, y, width, height}; out_radii = {radiusX, radiusY}; either may
// be null.
bool dm_noesis_rectangle_geometry_get(void* geometry, float out_rect[4], float out_radii[2]);

// LineGeometry.
void* dm_noesis_line_geometry_create(float x1, float y1, float x2, float y2);
// out = {startX, startY, endX, endY}
bool dm_noesis_line_geometry_get(void* geometry, float out[4]);

// CombinedGeometry. `mode` is a GeometryCombineMode ordinal; geometry1/2 are
// borrowed Geometry* (or null), Noesis takes its own references.
void* dm_noesis_combined_geometry_create(int32_t mode, void* geometry1, void* geometry2);
bool dm_noesis_combined_geometry_set_geometry1(void* geometry, void* g1);
bool dm_noesis_combined_geometry_set_geometry2(void* geometry, void* g2);
// Borrowed Geometry* (no +1) or null.
void* dm_noesis_combined_geometry_get_geometry1(void* geometry);
void* dm_noesis_combined_geometry_get_geometry2(void* geometry);
bool dm_noesis_combined_geometry_set_mode(void* geometry, int32_t mode);
int32_t dm_noesis_combined_geometry_get_mode(void* geometry);

// GeometryGroup + GeometryCollection of Geometry.
void* dm_noesis_geometry_group_create(void);
bool dm_noesis_geometry_group_set_fill_rule(void* geometry, int32_t rule);
int32_t dm_noesis_geometry_group_get_fill_rule(void* geometry);
// Append a borrowed child Geometry*; the collection takes its own reference.
int32_t dm_noesis_geometry_group_add_child(void* geometry, void* child);
int32_t dm_noesis_geometry_group_child_count(void* geometry);
// ── SVG / SVGPath parsing (TODO §12) ────────────────────────────────────────
//
// Implemented in cpp/noesis_svg.cpp. Both surfaces are CPU/headless — no GPU
// RenderDevice or render pass needed. The handles are plain heap objects (NOT
// BaseComponents); release SVGPath* with dm_noesis_svg_path_destroy and
// SVG::Image* with dm_noesis_svg_image_destroy.

// Parse an SVG path string (e.g. "M0 0 L100 0 L100 50 Z") into an owned
// SVGPath. Returns null on parse failure.
void* dm_noesis_svg_path_parse(const char* str);

// Create an empty SVGPath to populate with the builder entrypoints below.
void* dm_noesis_svg_path_create(void);

// Release an SVGPath created by parse/create.
void dm_noesis_svg_path_destroy(void* path);

// Number of uint32 entries in the path's command buffer (0 for null/empty).
uint32_t dm_noesis_svg_path_command_count(void* path);

// Path-builder statics appending to the owned command buffer.
void dm_noesis_svg_path_move_to(void* path, float x, float y);
void dm_noesis_svg_path_line_to(void* path, float x, float y);
void dm_noesis_svg_path_close(void* path);
void dm_noesis_svg_path_add_rect(void* path, float x, float y, float width, float height);
void dm_noesis_svg_path_add_ellipse(void* path, float x, float y, float rx, float ry);

// AABB of the path geometry. out = [x, y, width, height]. Returns false if null.
bool dm_noesis_svg_path_calculate_bounds(void* path, float out[4]);

// True if (x, y) is inside the filled region. fill_rule: 0 EvenOdd, 1 NonZero.
bool dm_noesis_svg_path_fill_contains(void* path, float x, float y, int32_t fill_rule);

// True if (x, y) falls within the stroked outline for the given pen. `join` is a
// StrokeJoinStyle ordinal (0 Miter, 1 Bevel, 2 Round); `start_cap`/`end_cap` are
// StrokeCapStyle ordinals (0 Butt, 1 Square, 2 Round, 3 Triangle).
bool dm_noesis_svg_path_stroke_contains(void* path, float x, float y, float width, int32_t join,
                                        int32_t start_cap, int32_t end_cap, float miter_limit);

// Parse a full <svg> document string into an owned Noesis::SVG::Image. Returns
// null only if `svg` is null; a malformed document yields a zero-shape image.
void* dm_noesis_svg_image_parse(const char* svg);

// Release an SVG::Image created by dm_noesis_svg_image_parse.
void dm_noesis_svg_image_destroy(void* image);

// Parsed document size (the <svg> width/height). Returns false if `image` null.
bool dm_noesis_svg_image_get_size(void* image, float* width, float* height);

// Number of parsed shapes (paths) in the document.
uint32_t dm_noesis_svg_image_shape_count(void* image);

// Fill-brush type ordinal of shape `index` (0 None, 1 Solid, 2 Linear,
// 3 Radial), or -1 if the index is out of range.
int32_t dm_noesis_svg_image_shape_fill_type(void* image, uint32_t index);
// ── TextBlock inline content model (TODO §13) ──────────────────────────────
//
// The Inline element family shipped in 3.2.13 — Run, Span, Bold, Italic,
// Underline, Hyperlink, LineBreak, InlineUIContainer — plus the InlineCollection
// (UICollection<Inline>) that TextBlock and Span expose. Inlines are assembled
// in Rust and added to a TextBlock's (or Span's) Inlines collection; read-back
// getters re-read from the live Noesis object so a stub fails the round-trip.
//
// Every *_create returns a BaseComponent* with +1 ref for the caller (release
// via dm_noesis_base_component_release). Adding an inline to a collection makes
// the collection take its own reference, so the builder handle may then be
// dropped.

// Construct an inline. `run_create` seeds the text (NULL == empty Run). Each
// returns a +1 BaseComponent*, or NULL on allocation failure.
void* dm_noesis_text_inlines_run_create(const char* text);
void* dm_noesis_text_inlines_span_create(void);
void* dm_noesis_text_inlines_bold_create(void);
void* dm_noesis_text_inlines_italic_create(void);
void* dm_noesis_text_inlines_underline_create(void);
void* dm_noesis_text_inlines_hyperlink_create(void);
void* dm_noesis_text_inlines_line_break_create(void);
void* dm_noesis_text_inlines_ui_container_create(void);

// Run text. `set` copies into the Run's storage (NULL clears to empty) and
// returns false if `run` is not a Run. `get` returns a borrowed (no +1) pointer
// into the Run's UTF-8 storage, or NULL if `run` is not a Run.
bool dm_noesis_text_inlines_run_set_text(void* run, const char* text);
const char* dm_noesis_text_inlines_run_get_text(void* run);

// Hyperlink NavigateUri. `set` copies the URI (NULL clears) and returns false
// if `link` is not a Hyperlink. `get` returns a borrowed (no +1) pointer, or
// NULL if `link` is not a Hyperlink.
bool dm_noesis_text_inlines_hyperlink_set_navigate_uri(void* link, const char* uri);
const char* dm_noesis_text_inlines_hyperlink_get_navigate_uri(void* link);

// Inline base TextDecorations (0 None, 1 OverLine, 2 Baseline, 3 Underline,
// 4 Strikethrough). `set` returns false if `inl` is not an Inline; `get`
// returns -1 if `inl` is not an Inline.
bool dm_noesis_text_inlines_inline_set_text_decorations(void* inl, int32_t decorations);
int32_t dm_noesis_text_inlines_inline_get_text_decorations(void* inl);

// InlineUIContainer Child (hosts a UIElement, e.g. a Button). `set` makes the
// container take its own reference (NULL clears) and returns false if
// `container` is not an InlineUIContainer or `child` is non-null but not a
// UIElement. `get` returns a borrowed (no +1) BaseComponent* whose address
// matches the BaseComponent subobject of the element set (so it can be compared
// for identity), or NULL.
bool dm_noesis_text_inlines_ui_container_set_child(void* container, void* child);
void* dm_noesis_text_inlines_ui_container_get_child(void* container);

// Live InlineCollection (UICollection<Inline>) of a TextBlock's / Span's
// top-level inlines, handed out at +1 (release via
// dm_noesis_base_component_release). The collection is also owned by its host
// element; the +1 keeps it alive for the handle's lifetime. NULL if the object
// is not a TextBlock / Span.
void* dm_noesis_text_inlines_text_block_get_inlines(void* text_block);
void* dm_noesis_text_inlines_span_get_inlines(void* span);

// InlineCollection mutation/inspection. `add` appends a borrowed Inline* (the
// collection takes its own ref) and returns the insertion index, or -1 if
// `collection` is not an InlineCollection or `inl` is not an Inline. `count`
// returns the item count, or -1 for a non-collection. `get` returns a borrowed
// (no +1) Inline* at `index`, or NULL on null/non-collection/out-of-range.
int32_t dm_noesis_text_inlines_collection_add(void* collection, void* inl);
int32_t dm_noesis_text_inlines_collection_count(void* collection);
void* dm_noesis_text_inlines_collection_get(void* collection, uint32_t index);
// ── FormattedText measurement / layout (TODO §13) ───────────────────────────
//
// FormattedText (NsGui/FormattedText.h) computes glyph metrics + a text layout
// for a string and font properties at construction time. This unit owns no
// FontFamily entrypoint: _create takes the font family as a NAME and builds the
// Noesis::FontFamily internally. The returned handle is a +1 BaseComponent*
// (release with dm_noesis_base_component_release). Metrics getters re-read from
// the live object so a stub fails the round-trip. None of these call
// VerifyAccess(); FormattedText is not view-bound, so they are safe off-thread.

// Build a FormattedText. `weight`/`stretch`/`style` are NsGui/FontProperties.h
// ordinals; `flow_direction`/`text_alignment`/`text_trimming` are the
// TextProperties.h / FlowDirection ordinals. Negative `max_width`/`max_height`
// mean unconstrained (FLT_MAX); `line_height` 0 means natural. `foreground` is
// an optional [r,g,b,a] (null ⇒ opaque black). Returns a +1 FormattedText*.
void* dm_noesis_formatted_text_create(
    const char* text, const char* font_family, int32_t weight, int32_t stretch, int32_t style,
    float font_size, int32_t flow_direction, float max_width, float max_height, float line_height,
    int32_t text_alignment, int32_t text_trimming, const float foreground[4]);

// Layout bounds: out = {x, y, width, height} in DIPs. False if not a FormattedText.
bool dm_noesis_formatted_text_get_bounds(void* ft, float out[4]);

// Number of laid-out lines, or -1 if `ft` is not a FormattedText.
int32_t dm_noesis_formatted_text_get_num_lines(void* ft);

// Per-line metrics for `index`: glyph count, height, baseline (any out may be
// null). False on not-a-FormattedText or out-of-range index.
bool dm_noesis_formatted_text_get_line_info(void* ft, uint32_t index, uint32_t* out_num_glyphs,
    float* out_height, float* out_baseline);

// Write IsEmpty() / HasVisualBrush() to `out`. False (out untouched) if `ft` is
// not a FormattedText.
bool dm_noesis_formatted_text_is_empty(void* ft, bool* out);
bool dm_noesis_formatted_text_has_visual_brush(void* ft, bool* out);

// Re-measure the stored runs under fresh constraints; writes the Size to
// out_w/out_h. Enum args are the matching ordinals; negative max_* ⇒ FLT_MAX.
bool dm_noesis_formatted_text_measure(void* ft, int32_t alignment, int32_t wrapping,
    int32_t trimming, float max_width, float max_height, float line_height, int32_t line_stacking,
    int32_t flow_direction, float* out_w, float* out_h);

// Glyph x/y for character `ch_index` (after the char when `after_char`); Noesis
// writes -10/-10 when the index is outside layout limits.
bool dm_noesis_formatted_text_get_glyph_position(void* ft, uint32_t ch_index, bool after_char,
    float* out_x, float* out_y);

// Glyph index under (x, y) in layout DIPs, plus inside / trailing flags (any
// out may be null). False if `ft` is not a FormattedText.
bool dm_noesis_formatted_text_hit_test(void* ft, float x, float y, uint32_t* out_index,
    bool* out_is_inside, bool* out_is_trailing);

#ifdef __cplusplus
}
#endif

#endif  // DM_NOESIS_SHIM_H
