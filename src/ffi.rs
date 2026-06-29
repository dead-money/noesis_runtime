//! Raw `extern "C"` declarations for the `noesis_shim` C ABI.
//!
//! This is the unsafe boundary between Rust and the native Noesis SDK. Every
//! entrypoint here is hand-mirrored from `cpp/noesis_shim.h`, which is the
//! source of truth for the per-function pointer-ownership, refcount, and
//! value-layout contracts. Prefer the safe wrappers in the sibling modules
//! (`view`, `brushes`, `commands`, ...) over calling these directly.

use std::os::raw::{c_char, c_void};

/// Severity of a log record delivered to a [`LogFn`] handler.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warning = 3,
    Error = 4,
}

/// Log handler installed via [`noesis_set_log_handler`]. `file`, `channel`, and
/// `message` are borrowed NUL-terminated strings valid only for the call.
pub type LogFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    level: LogLevel,
    channel: *const c_char,
    message: *const c_char,
);

unsafe extern "C" {
    pub fn noesis_set_license(name: *const c_char, key: *const c_char);
    pub fn noesis_set_log_handler(cb: Option<LogFn>, userdata: *mut c_void);
    pub fn noesis_init();
    pub fn noesis_shutdown();
    pub fn noesis_version() -> *const c_char;

    // Inspector / hot-reload toggles + queries. The Disable* trio must be
    // called before noesis_init.
    pub fn noesis_disable_hot_reload();
    pub fn noesis_disable_socket_init();
    pub fn noesis_disable_inspector();
    pub fn noesis_is_inspector_connected() -> bool;
    pub fn noesis_update_inspector();
}

// ────────────────────────────────────────────────────────────────────────────
// XamlProvider + View / Renderer FFI. See cpp/noesis_shim.h for
// pointer-ownership contracts.
// ────────────────────────────────────────────────────────────────────────────

/// Resolves XAML `uri`s to in-memory buffers. Install a Rust implementation via
/// [`noesis_xaml_provider_create`] + [`noesis_set_xaml_provider`].
#[repr(C)]
pub struct XamlProviderVTable {
    pub load_xaml: unsafe extern "C" fn(
        userdata: *mut c_void,
        uri: *const c_char,
        out_data: *mut *const u8,
        out_len: *mut u32,
    ) -> bool,
}

/// Callback signature the C++ side passes into `scan_folder` so Rust can
/// register each font filename synchronously. `register_cx` is opaque to
/// Rust; pass it back verbatim.
pub type RegisterFontFn = unsafe extern "C" fn(register_cx: *mut c_void, filename: *const c_char);

// System integration callback function pointers. Each matches a
// `noesis_*_cb` typedef in `cpp/noesis_shim.h`. `view` / `focused` are
// borrowed opaque `Noesis::IView*` / `Noesis::UIElement*` pointers.
pub type CursorCb = unsafe extern "C" fn(user: *mut c_void, view: *mut c_void, cursor_type: i32);
pub type SoftwareKeyboardCb =
    unsafe extern "C" fn(user: *mut c_void, focused: *mut c_void, open: bool);
pub type OpenUrlCb = unsafe extern "C" fn(user: *mut c_void, url: *const c_char);
pub type PlayAudioCb = unsafe extern "C" fn(user: *mut c_void, uri: *const c_char, volume: f32);

/// Enumerates and opens font files for the text stack. Install a Rust
/// implementation via [`noesis_font_provider_create`] + [`noesis_set_font_provider`].
#[repr(C)]
pub struct FontProviderVTable {
    pub scan_folder: unsafe extern "C" fn(
        userdata: *mut c_void,
        folder_uri: *const c_char,
        register_fn: RegisterFontFn,
        register_cx: *mut c_void,
    ),
    pub open_font: unsafe extern "C" fn(
        userdata: *mut c_void,
        folder_uri: *const c_char,
        filename: *const c_char,
        out_data: *mut *const u8,
        out_len: *mut u32,
    ) -> bool,
}

/// Mirror of `noesis_texture_info` in `noesis_shim.h`: texture metadata
/// returned by the provider's `get_info` callback.
#[repr(C)]
pub struct TextureInfoFfi {
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
    pub dpi_scale: f32,
}

/// Supplies texture metadata and pixel data for image `uri`s. Install a Rust
/// implementation via [`noesis_texture_provider_create`] + [`noesis_set_texture_provider`].
#[repr(C)]
pub struct TextureProviderVTable {
    pub get_info: unsafe extern "C" fn(
        userdata: *mut c_void,
        uri: *const c_char,
        out: *mut TextureInfoFfi,
    ) -> bool,
    pub load_texture: unsafe extern "C" fn(
        userdata: *mut c_void,
        uri: *const c_char,
        out_width: *mut u32,
        out_height: *mut u32,
        out_data: *mut *const u8,
        out_len: *mut u32,
    ) -> bool,
}

/// Callback signature for [`noesis_get_xaml_dependencies`]. The C++ trampoline
/// invokes it once per dependency found in the XAML buffer. `uri` is a borrowed
/// NUL-terminated string; `kind` is a `Noesis::XamlDependencyType` ordinal
/// (0 Filename, 1 Font, 2 `UserControl`, 3 Root).
pub type XamlDependencyFn = unsafe extern "C" fn(user: *mut c_void, uri: *const c_char, kind: i32);

unsafe extern "C" {
    pub fn noesis_xaml_provider_create(
        vtable: *const XamlProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_xaml_provider_destroy(provider: *mut c_void);
    pub fn noesis_set_xaml_provider(provider: *mut c_void);

    pub fn noesis_font_provider_create(
        vtable: *const FontProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_font_provider_destroy(provider: *mut c_void);
    pub fn noesis_set_font_provider(provider: *mut c_void);
    pub fn noesis_set_font_fallbacks(families: *const *const c_char, count: u32);
    pub fn noesis_set_font_default_properties(size: f32, weight: i32, stretch: i32, style: i32);
    pub fn noesis_font_provider_register_font(
        provider: *mut c_void,
        folder_uri: *const c_char,
        filename: *const c_char,
    );

    pub fn noesis_texture_provider_create(
        vtable: *const TextureProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_texture_provider_destroy(provider: *mut c_void);
    pub fn noesis_set_texture_provider(provider: *mut c_void);

    // ── XAML loading variants ────────────────────────────────────────────────
    pub fn noesis_get_xaml_dependencies(
        xaml: *const u8,
        len: u32,
        base_uri: *const c_char,
        user: *mut c_void,
        cb: XamlDependencyFn,
    );
    pub fn noesis_gui_load_xaml_component(uri: *const c_char) -> *mut c_void;
    pub fn noesis_base_component_type_name(obj: *mut c_void) -> *const c_char;

    // Scheme- / assembly-scoped provider setters. Each takes a provider handle
    // produced by the matching `noesis_*_provider_create`.
    pub fn noesis_set_xaml_provider_scheme(scheme: *const c_char, provider: *mut c_void);
    pub fn noesis_set_xaml_provider_assembly(assembly: *const c_char, provider: *mut c_void);
    pub fn noesis_set_xaml_provider_scheme_assembly(
        scheme: *const c_char,
        assembly: *const c_char,
        provider: *mut c_void,
    );
    pub fn noesis_set_texture_provider_scheme(scheme: *const c_char, provider: *mut c_void);
    pub fn noesis_set_texture_provider_assembly(assembly: *const c_char, provider: *mut c_void);
    pub fn noesis_set_texture_provider_scheme_assembly(
        scheme: *const c_char,
        assembly: *const c_char,
        provider: *mut c_void,
    );
    pub fn noesis_set_font_provider_scheme(scheme: *const c_char, provider: *mut c_void);
    pub fn noesis_set_font_provider_assembly(assembly: *const c_char, provider: *mut c_void);
    pub fn noesis_set_font_provider_scheme_assembly(
        scheme: *const c_char,
        assembly: *const c_char,
        provider: *mut c_void,
    );

    // System integration callbacks.
    pub fn noesis_set_cursor_callback(user: *mut c_void, cb: Option<CursorCb>);
    pub fn noesis_set_software_keyboard_callback(user: *mut c_void, cb: Option<SoftwareKeyboardCb>);
    pub fn noesis_set_open_url_callback(user: *mut c_void, cb: Option<OpenUrlCb>);
    pub fn noesis_open_url(url: *const c_char);
    pub fn noesis_set_play_audio_callback(user: *mut c_void, cb: Option<PlayAudioCb>);
    pub fn noesis_play_audio(uri: *const c_char, volume: f32);
    pub fn noesis_set_culture(name: *const c_char);
    pub fn noesis_get_culture() -> *const c_char;

    pub fn noesis_gui_load_xaml(uri: *const c_char) -> *mut c_void;
    pub fn noesis_gui_parse_xaml(text: *const c_char) -> *mut c_void;
    pub fn noesis_gui_load_component(component: *mut c_void, uri: *const c_char) -> bool;
    pub fn noesis_gui_load_application_resources(uri: *const c_char) -> bool;
    pub fn noesis_gui_install_app_resources_chain(uris: *const *const c_char, count: u32) -> bool;
    pub fn noesis_base_component_release(obj: *mut c_void);
    pub fn noesis_base_component_add_reference(obj: *mut c_void) -> *mut c_void;
    pub fn noesis_base_component_get_num_references(obj: *mut c_void) -> i32;

    pub fn noesis_view_create(framework_element: *mut c_void) -> *mut c_void;
    pub fn noesis_view_destroy(view: *mut c_void);
    pub fn noesis_view_add_reference(view: *mut c_void) -> *mut c_void;
    pub fn noesis_view_set_size(view: *mut c_void, width: u32, height: u32);
    pub fn noesis_view_set_scale(view: *mut c_void, scale: f32);
    pub fn noesis_view_set_projection_matrix(view: *mut c_void, matrix: *const f32);
    pub fn noesis_view_update(view: *mut c_void, time_seconds: f64) -> bool;
    pub fn noesis_view_set_flags(view: *mut c_void, flags: u32);
    pub fn noesis_view_get_renderer(view: *mut c_void) -> *mut c_void;
    pub fn noesis_view_get_content(view: *mut c_void) -> *mut c_void;

    pub fn noesis_renderer_init(renderer: *mut c_void, render_device: *mut c_void);
    pub fn noesis_renderer_shutdown(renderer: *mut c_void);
    pub fn noesis_renderer_update_render_tree(renderer: *mut c_void) -> bool;
    pub fn noesis_renderer_render_offscreen(renderer: *mut c_void) -> bool;
    pub fn noesis_renderer_render(renderer: *mut c_void, flip_y: bool, clear: bool);

    // ── Stereo / VR rendering ─────────────────────────────────────────────────
    pub fn noesis_renderer_render_stereo(
        renderer: *mut c_void,
        eye_matrix: *const f32,
        flip_y: bool,
        clear: bool,
    );
    pub fn noesis_renderer_render_stereo_both(
        renderer: *mut c_void,
        left_eye_matrix: *const f32,
        right_eye_matrix: *const f32,
        flip_y: bool,
        clear: bool,
    );

    pub fn noesis_view_mouse_move(view: *mut c_void, x: i32, y: i32) -> bool;
    pub fn noesis_view_mouse_button_down(view: *mut c_void, x: i32, y: i32, button: i32) -> bool;
    pub fn noesis_view_mouse_button_up(view: *mut c_void, x: i32, y: i32, button: i32) -> bool;
    pub fn noesis_view_mouse_double_click(view: *mut c_void, x: i32, y: i32, button: i32) -> bool;
    pub fn noesis_view_mouse_wheel(view: *mut c_void, x: i32, y: i32, delta: i32) -> bool;
    pub fn noesis_view_scroll(view: *mut c_void, x: i32, y: i32, value: f32) -> bool;
    pub fn noesis_view_hscroll(view: *mut c_void, x: i32, y: i32, value: f32) -> bool;

    pub fn noesis_view_touch_down(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;
    pub fn noesis_view_touch_move(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;
    pub fn noesis_view_touch_up(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;

    pub fn noesis_view_key_down(view: *mut c_void, key: i32) -> bool;
    pub fn noesis_view_key_up(view: *mut c_void, key: i32) -> bool;
    pub fn noesis_view_char(view: *mut c_void, codepoint: u32) -> bool;

    pub fn noesis_view_activate(view: *mut c_void);
    pub fn noesis_view_deactivate(view: *mut c_void);
    pub fn noesis_view_mouse_hwheel(view: *mut c_void, x: i32, y: i32, delta: i32) -> bool;

    // ── View flags / quality / stats ──────────────────────────────────────────
    pub fn noesis_view_get_flags(view: *mut c_void) -> u32;
    pub fn noesis_view_set_tessellation_max_pixel_error(view: *mut c_void, error: f32);
    pub fn noesis_view_get_tessellation_max_pixel_error(view: *mut c_void) -> f32;
    pub fn noesis_view_get_stats(view: *mut c_void, out: *mut crate::view::ViewStats);

    // ── Gesture / touch thresholds ────────────────────────────────────────────
    pub fn noesis_view_set_holding_time_threshold(view: *mut c_void, ms: u32);
    pub fn noesis_view_set_holding_distance_threshold(view: *mut c_void, pixels: u32);
    pub fn noesis_view_set_manipulation_distance_threshold(view: *mut c_void, pixels: u32);
    pub fn noesis_view_set_double_tap_time_threshold(view: *mut c_void, ms: u32);
    pub fn noesis_view_set_double_tap_distance_threshold(view: *mut c_void, pixels: u32);
    pub fn noesis_view_set_emulate_touch(view: *mut c_void, emulate: bool);

    // ── Stereo / VR ────────────────────────────────────────────────────────────
    pub fn noesis_view_set_stereo_offscreen_scale_factor(view: *mut c_void, factor: f32);

    // ── View-driven timers ─────────────────────────────────────────────────────
    pub fn noesis_view_create_timer(
        view: *mut c_void,
        interval_ms: u32,
        cb: TimerFn,
        userdata: *mut c_void,
        free_handler: TimerFreeFn,
    ) -> *mut c_void;
    pub fn noesis_view_restart_timer(token: *mut c_void, interval_ms: u32);
    pub fn noesis_view_cancel_timer(token: *mut c_void);

    // ── Rendering event ──────────────────────────────────────────────────────
    pub fn noesis_view_add_rendering_handler(
        view: *mut c_void,
        cb: RenderingFn,
        userdata: *mut c_void,
        free_handler: RenderingFreeFn,
    ) -> *mut c_void;
    pub fn noesis_view_remove_rendering_handler(token: *mut c_void);

    pub fn noesis_framework_element_find_name(
        element: *mut c_void,
        name: *const c_char,
    ) -> *mut c_void;
    pub fn noesis_framework_element_get_name(element: *mut c_void) -> *const c_char;
    pub fn noesis_framework_element_set_visibility(element: *mut c_void, visible: bool);
    pub fn noesis_framework_element_set_margin(
        element: *mut c_void,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
    );

    pub fn noesis_subscribe_click(
        element: *mut c_void,
        cb: ClickFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_unsubscribe_click(token: *mut c_void);

    pub fn noesis_subscribe_keydown(
        element: *mut c_void,
        cb: KeyDownFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_unsubscribe_keydown(token: *mut c_void);

    pub fn noesis_subscribe_event(
        element: *mut c_void,
        event_name: *const c_char,
        handled_too: bool,
        cb: RoutedEventFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_unsubscribe_event(token: *mut c_void);

    // ── Non-routed lifecycle events ───────────────────────────────────────────
    // The callback is the same shape as `ClickFn` (a bare `void(userdata)`), so
    // it is reused here.
    pub fn noesis_subscribe_lifecycle(
        element: *mut c_void,
        event_name: *const c_char,
        cb: ClickFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_unsubscribe_lifecycle(token: *mut c_void);

    pub fn noesis_mouse_args_position(args: *const c_void, x: *mut f32, y: *mut f32) -> bool;
    pub fn noesis_mouse_button_args_button(args: *const c_void) -> i32;
    pub fn noesis_mouse_wheel_args_delta(args: *const c_void) -> i32;
    pub fn noesis_key_args_key(args: *const c_void) -> i32;
    pub fn noesis_text_args_ch(args: *const c_void) -> i32;
    pub fn noesis_size_changed_args_new_size(
        args: *const c_void,
        width: *mut f32,
        height: *mut f32,
    ) -> bool;
    pub fn noesis_routed_args_source(args: *const c_void) -> *mut c_void;

    // ── Typed arg accessors: focus / drag / manipulation ──────────────────────
    pub fn noesis_routed_events_focus_old(args: *const c_void) -> *mut c_void;
    pub fn noesis_routed_events_focus_new(args: *const c_void) -> *mut c_void;
    pub fn noesis_routed_events_drag_effects(
        args: *const c_void,
        effects: *mut u32,
        allowed: *mut u32,
        key_states: *mut u32,
    ) -> bool;
    pub fn noesis_routed_events_drag_set_effects(args: *const c_void, effects: u32) -> bool;
    pub fn noesis_routed_events_drag_data(args: *const c_void) -> *mut c_void;
    pub fn noesis_routed_events_drag_position(
        args: *const c_void,
        relative_to: *mut c_void,
        x: *mut f32,
        y: *mut f32,
    ) -> bool;
    pub fn noesis_routed_events_manip_origin(args: *const c_void, x: *mut f32, y: *mut f32)
    -> bool;
    pub fn noesis_routed_events_manip_delta(
        args: *const c_void,
        tx: *mut f32,
        ty: *mut f32,
        scale: *mut f32,
        rotation: *mut f32,
        ex: *mut f32,
        ey: *mut f32,
    ) -> bool;
    pub fn noesis_routed_events_manip_cumulative(
        args: *const c_void,
        tx: *mut f32,
        ty: *mut f32,
        scale: *mut f32,
        rotation: *mut f32,
        ex: *mut f32,
        ey: *mut f32,
    ) -> bool;
    pub fn noesis_routed_events_manip_velocities(
        args: *const c_void,
        angular: *mut f32,
        lx: *mut f32,
        ly: *mut f32,
        ex: *mut f32,
        ey: *mut f32,
    ) -> bool;
    pub fn noesis_routed_events_manip_is_inertial(args: *const c_void) -> i32;

    // ── DragDrop source side + DataObject copy/paste handlers ──────────────────
    pub fn noesis_routed_events_do_drag_drop(
        source: *mut c_void,
        data: *mut c_void,
        allowed_effects: u32,
    ) -> bool;
    pub fn noesis_routed_events_add_copying_handler(
        element: *mut c_void,
        cb: DataObjectFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_routed_events_add_pasting_handler(
        element: *mut c_void,
        cb: DataObjectFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_routed_events_remove_data_object_handler(token: *mut c_void);

    pub fn noesis_text_get(element: *mut c_void) -> *const c_char;
    pub fn noesis_text_set(element: *mut c_void, text: *const c_char) -> bool;
    pub fn noesis_text_caret_to_end(element: *mut c_void) -> bool;
    pub fn noesis_focus_element(element: *mut c_void) -> bool;
    pub fn noesis_path_set_points(element: *mut c_void, xy: *const f32, count: u32) -> bool;
    pub fn noesis_visual_state_go_to_state(
        element: *mut c_void,
        state: *const c_char,
        use_transitions: bool,
    ) -> bool;

    pub fn noesis_class_register(
        name: *const c_char,
        base: ClassBase,
        cb: PropChangedFn,
        userdata: *mut c_void,
        free_handler: ClassFreeFn,
    ) -> *mut c_void;
    pub fn noesis_class_register_property(
        class_token: *mut c_void,
        prop_name: *const c_char,
        prop_type: PropType,
        default_ptr: *const c_void,
    ) -> u32;
    // ── Custom base classes + richer DP metadata + layout ─────────────────────
    pub fn noesis_class_register_property_ex(
        class_token: *mut c_void,
        prop_name: *const c_char,
        prop_type: PropType,
        default_ptr: *const c_void,
        fpm_options: u32,
        read_only: bool,
        coerce: bool,
    ) -> u32;
    pub fn noesis_class_register_enum_property(
        class_token: *mut c_void,
        prop_name: *const c_char,
        enum_type_name: *const c_char,
        default_value: i32,
        fpm_options: u32,
        read_only: bool,
    ) -> u32;
    pub fn noesis_instance_set_readonly_property(
        instance: *mut c_void,
        prop_index: u32,
        value_ptr: *const c_void,
    ) -> bool;
    pub fn noesis_freezable_freeze(freezable: *mut c_void) -> bool;
    pub fn noesis_freezable_is_frozen(freezable: *mut c_void) -> bool;
    pub fn noesis_freezable_can_freeze(freezable: *mut c_void) -> bool;
    pub fn noesis_class_set_coerce(
        class_token: *mut c_void,
        cb: CoerceFn,
        userdata: *mut c_void,
        free_handler: ClassFreeFn,
    );
    pub fn noesis_class_set_layout(
        class_token: *mut c_void,
        vtable: *const LayoutVtable,
        userdata: *mut c_void,
        free_handler: LayoutFreeFn,
    );
    pub fn noesis_class_set_render(
        class_token: *mut c_void,
        cb: RenderFn,
        userdata: *mut c_void,
        free_handler: RenderFreeFn,
    );
    pub fn noesis_uielement_measure(element: *mut c_void, avail_w: f32, avail_h: f32) -> bool;
    pub fn noesis_uielement_arrange(element: *mut c_void, x: f32, y: f32, w: f32, h: f32) -> bool;
    pub fn noesis_uielement_desired_size(
        element: *mut c_void,
        out_w: *mut f32,
        out_h: *mut f32,
    ) -> bool;
    pub fn noesis_class_unregister(class_token: *mut c_void);
    pub fn noesis_instance_set_property(
        instance: *mut c_void,
        prop_index: u32,
        value_ptr: *const c_void,
    );
    pub fn noesis_instance_get_property(
        instance: *mut c_void,
        prop_index: u32,
        out_value: *mut c_void,
    ) -> bool;
    pub fn noesis_image_source_get_size(
        image_source: *mut c_void,
        out_width: *mut f32,
        out_height: *mut f32,
    ) -> bool;

    pub fn noesis_dependency_object_set_property(
        obj: *mut c_void,
        name: *const c_char,
        prop_type: PropType,
        value_ptr: *const c_void,
    ) -> bool;
    pub fn noesis_dependency_object_get_property(
        obj: *mut c_void,
        name: *const c_char,
        prop_type: PropType,
        out_value: *mut c_void,
    ) -> bool;

    pub fn noesis_markup_extension_register(
        name: *const c_char,
        cb: MarkupProvideFn,
        userdata: *mut c_void,
        free_handler: MarkupFreeFn,
    ) -> *mut c_void;
    pub fn noesis_markup_extension_unregister(token: *mut c_void);

    pub fn noesis_class_create_instance(class_token: *mut c_void) -> *mut c_void;

    pub fn noesis_box_string(text: *const c_char) -> *mut c_void;

    pub fn noesis_observable_collection_create() -> *mut c_void;
    pub fn noesis_observable_collection_add(collection: *mut c_void, item: *mut c_void) -> i32;
    pub fn noesis_observable_collection_insert(
        collection: *mut c_void,
        index: u32,
        item: *mut c_void,
    ) -> bool;
    pub fn noesis_observable_collection_set(
        collection: *mut c_void,
        index: u32,
        item: *mut c_void,
    ) -> bool;
    pub fn noesis_observable_collection_remove_at(collection: *mut c_void, index: u32) -> bool;
    pub fn noesis_observable_collection_clear(collection: *mut c_void);
    pub fn noesis_observable_collection_count(collection: *mut c_void) -> i32;
    pub fn noesis_observable_collection_get(collection: *mut c_void, index: u32) -> *mut c_void;

    // ── ICollectionView current-item navigation ───────────────────────────────
    pub fn noesis_collection_view_source_create() -> *mut c_void;
    pub fn noesis_collection_view_source_set_source(cvs: *mut c_void, source: *mut c_void) -> bool;
    pub fn noesis_collection_view_source_get_view(cvs: *mut c_void) -> *mut c_void;
    pub fn noesis_collection_view_count(view: *mut c_void) -> i32;
    pub fn noesis_collection_view_current_position(view: *mut c_void) -> i32;
    pub fn noesis_collection_view_current_item(view: *mut c_void) -> *mut c_void;
    pub fn noesis_collection_view_is_current_before_first(view: *mut c_void) -> bool;
    pub fn noesis_collection_view_is_current_after_last(view: *mut c_void) -> bool;
    pub fn noesis_collection_view_move_current_to_first(view: *mut c_void) -> bool;
    pub fn noesis_collection_view_move_current_to_last(view: *mut c_void) -> bool;
    pub fn noesis_collection_view_move_current_to_next(view: *mut c_void) -> bool;
    pub fn noesis_collection_view_move_current_to_previous(view: *mut c_void) -> bool;
    pub fn noesis_collection_view_move_current_to_position(
        view: *mut c_void,
        position: i32,
    ) -> bool;
    pub fn noesis_collection_view_refresh(view: *mut c_void);
    pub fn noesis_collection_view_subscribe_current_changed(
        view: *mut c_void,
        cb: ClickFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_collection_view_unsubscribe_current_changed(token: *mut c_void);

    pub fn noesis_framework_element_set_data_context(
        element: *mut c_void,
        context: *mut c_void,
    ) -> bool;
    pub fn noesis_framework_element_get_data_context(element: *mut c_void) -> *mut c_void;

    pub fn noesis_items_control_set_items_source(element: *mut c_void, items: *mut c_void) -> bool;
    pub fn noesis_items_control_items_count(element: *mut c_void) -> i32;
    pub fn noesis_items_control_realized_count(element: *mut c_void) -> i32;

    // ── Element tree access. See cpp/noesis_shim.h for pointer-ownership +
    // tag-validation contracts. ───────────────────────────────────────────────

    // A. Tree traversal.
    pub fn noesis_visual_children_count(element: *mut c_void) -> u32;
    pub fn noesis_visual_child(element: *mut c_void, index: u32) -> *mut c_void;
    pub fn noesis_visual_parent(element: *mut c_void) -> *mut c_void;
    pub fn noesis_visual_hit_test(element: *mut c_void, x: f32, y: f32) -> *mut c_void;
    pub fn noesis_visual_hit_test_filtered(
        element: *mut c_void,
        x: f32,
        y: f32,
        filter: HitFilterFn,
        result: HitResultFn,
        userdata: *mut c_void,
    );
    pub fn noesis_ui_element_get_render_transform_origin(
        element: *mut c_void,
        out_x: *mut f32,
        out_y: *mut f32,
    );
    pub fn noesis_ui_element_set_render_transform_origin(
        element: *mut c_void,
        x: f32,
        y: f32,
    ) -> bool;
    pub fn noesis_framework_element_logical_parent(element: *mut c_void) -> *mut c_void;
    pub fn noesis_logical_children_count(element: *mut c_void) -> u32;
    pub fn noesis_logical_child(element: *mut c_void, index: u32) -> *mut c_void;
    pub fn noesis_framework_element_template_child(
        element: *mut c_void,
        name: *const c_char,
    ) -> *mut c_void;

    // B. Attached properties.
    pub fn noesis_dependency_object_set_attached(
        obj: *mut c_void,
        owner_type: *const c_char,
        prop_name: *const c_char,
        prop_type: PropType,
        value_ptr: *const c_void,
    ) -> bool;
    pub fn noesis_dependency_object_get_attached(
        obj: *mut c_void,
        owner_type: *const c_char,
        prop_name: *const c_char,
        prop_type: PropType,
        out_value: *mut c_void,
    ) -> bool;

    // C. ClearValue / SetCurrentValue / GetBaseValue.
    pub fn noesis_dependency_object_clear_value(obj: *mut c_void, name: *const c_char) -> bool;
    pub fn noesis_dependency_object_set_current_value(
        obj: *mut c_void,
        name: *const c_char,
        prop_type: PropType,
        value_ptr: *const c_void,
    ) -> bool;
    pub fn noesis_dependency_object_get_base_value(
        obj: *mut c_void,
        name: *const c_char,
        prop_type: PropType,
        out_value: *mut c_void,
    ) -> bool;

    // D. Dynamic tag inference.
    pub fn noesis_dependency_object_property_tag(obj: *mut c_void, name: *const c_char) -> i32;

    // E. Alignment.
    pub fn noesis_framework_element_set_halign(element: *mut c_void, value: i32);
    pub fn noesis_framework_element_set_valign(element: *mut c_void, value: i32);
    pub fn noesis_framework_element_get_halign(element: *mut c_void) -> i32;
    pub fn noesis_framework_element_get_valign(element: *mut c_void) -> i32;

    // F. Namescope register / unregister.
    pub fn noesis_framework_element_register_name(
        element: *mut c_void,
        name: *const c_char,
        object: *mut c_void,
    ) -> bool;
    pub fn noesis_framework_element_unregister_name(
        element: *mut c_void,
        name: *const c_char,
    ) -> bool;

    // Standalone NameScope.
    pub fn noesis_name_scope_create() -> *mut c_void;
    pub fn noesis_name_scope_get(element: *mut c_void) -> *mut c_void;
    pub fn noesis_name_scope_set(element: *mut c_void, scope: *mut c_void) -> bool;
    pub fn noesis_name_scope_find_name(scope: *mut c_void, name: *const c_char) -> *mut c_void;
    pub fn noesis_name_scope_register_name(
        scope: *mut c_void,
        name: *const c_char,
        obj: *mut c_void,
    );
    pub fn noesis_name_scope_unregister_name(scope: *mut c_void, name: *const c_char);
    pub fn noesis_name_scope_update_name(scope: *mut c_void, name: *const c_char, obj: *mut c_void);
    pub fn noesis_name_scope_find_object(scope: *mut c_void, obj: *mut c_void) -> *const c_char;
    pub fn noesis_name_scope_enum(scope: *mut c_void, cb: NameScopeEnumFn, userdata: *mut c_void);

    // G. Thread affinity.
    pub fn noesis_dependency_object_check_access(obj: *mut c_void) -> bool;
    pub fn noesis_dependency_object_thread_id(obj: *mut c_void) -> u32;

    // ── Commands: ICommand from Rust ──────────────────────────────────────────
    pub fn noesis_command_create(
        vt: *const CommandVTable,
        userdata: *mut c_void,
        free_handler: CommandFreeFn,
    ) -> *mut c_void;
    pub fn noesis_command_destroy(command: *mut c_void);
    pub fn noesis_command_raise_can_execute_changed(command: *mut c_void);

    // RoutedCommand / RoutedUICommand.
    pub fn noesis_routed_command_create(
        name: *const c_char,
        owner_type_name: *const c_char,
    ) -> *mut c_void;
    pub fn noesis_routed_ui_command_create(
        name: *const c_char,
        text: *const c_char,
        owner_type_name: *const c_char,
    ) -> *mut c_void;
    pub fn noesis_routed_command_execute(
        command: *mut c_void,
        param: *mut c_void,
        target: *mut c_void,
    );
    pub fn noesis_routed_command_can_execute(
        command: *mut c_void,
        param: *mut c_void,
        target: *mut c_void,
    ) -> bool;
    pub fn noesis_routed_command_get_name(command: *mut c_void) -> *const c_char;
    pub fn noesis_routed_ui_command_get_text(command: *mut c_void) -> *const c_char;
    pub fn noesis_routed_ui_command_set_text(command: *mut c_void, text: *const c_char);

    // CommandBinding.
    pub fn noesis_command_binding_create(
        command: *mut c_void,
        executed: CmdExecutedFn,
        can_execute: Option<CmdCanExecuteFn>,
        userdata: *mut c_void,
        free_handler: CommandFreeFn,
    ) -> *mut c_void;
    pub fn noesis_command_binding_attach(token: *mut c_void, element: *mut c_void) -> bool;
    pub fn noesis_command_binding_destroy(token: *mut c_void);

    // Built-in command libraries.
    pub fn noesis_application_command(which: u32) -> *const c_void;
    pub fn noesis_component_command(which: u32) -> *const c_void;

    // ── Value boxing / unboxing primitives ────────────────────────────────────
    pub fn noesis_box_bool(value: bool) -> *mut c_void;
    pub fn noesis_box_int32(value: i32) -> *mut c_void;
    pub fn noesis_box_double(value: f64) -> *mut c_void;
    pub fn noesis_unbox_bool(boxed: *mut c_void, out: *mut bool) -> bool;
    pub fn noesis_unbox_int32(boxed: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_unbox_double(boxed: *mut c_void, out: *mut f64) -> bool;
    pub fn noesis_unbox_string(boxed: *mut c_void) -> *const c_char;

    // ── Value converters: IValueConverter from Rust ───────────────────────────
    pub fn noesis_value_converter_create(
        vt: *const ValueConverterVTable,
        userdata: *mut c_void,
        free_handler: ValueConverterFreeFn,
    ) -> *mut c_void;
    pub fn noesis_value_converter_destroy(converter: *mut c_void);

    // ── Code-built Binding + SetBinding ────────────────────────────────────────
    pub fn noesis_binding_create(path: *const c_char) -> *mut c_void;
    pub fn noesis_binding_destroy(binding: *mut c_void);
    pub fn noesis_binding_set_source(binding: *mut c_void, source: *mut c_void);
    pub fn noesis_binding_set_element_name(binding: *mut c_void, name: *const c_char);
    pub fn noesis_binding_set_mode(binding: *mut c_void, mode: i32);
    pub fn noesis_binding_set_converter(binding: *mut c_void, converter: *mut c_void);
    pub fn noesis_binding_set_converter_parameter(binding: *mut c_void, parameter: *mut c_void);
    pub fn noesis_binding_set_string_format(binding: *mut c_void, format: *const c_char);
    pub fn noesis_binding_set_fallback_value(binding: *mut c_void, value: *mut c_void);
    pub fn noesis_binding_set_update_source_trigger(binding: *mut c_void, trigger: i32);
    pub fn noesis_binding_set_relative_source_self(binding: *mut c_void);
    pub fn noesis_binding_set_relative_source_find_ancestor(
        binding: *mut c_void,
        type_name: *const c_char,
        level: u32,
    ) -> bool;
    pub fn noesis_binding_set_relative_source_previous_data(binding: *mut c_void);
    pub fn noesis_binding_set_relative_source_templated_parent(binding: *mut c_void);
    pub fn noesis_get_binding_expression(
        element: *mut c_void,
        dp_name: *const c_char,
    ) -> *mut c_void;
    pub fn noesis_binding_expression_update_target(expr: *mut c_void);
    pub fn noesis_binding_expression_update_source(expr: *mut c_void);
    pub fn noesis_set_binding(
        element: *mut c_void,
        dp_name: *const c_char,
        binding: *mut c_void,
    ) -> bool;
    pub fn noesis_framework_element_add_resource(
        element: *mut c_void,
        key: *const c_char,
        object: *mut c_void,
    ) -> bool;

    // ── Plain (non-DependencyObject) view models ──────────────────────────────
    pub fn noesis_plain_vm_register(
        type_name: *const c_char,
        on_set: Option<PlainSetFn>,
        userdata: *mut c_void,
        free_handler: Option<PlainFreeFn>,
    ) -> *mut c_void;
    pub fn noesis_plain_vm_register_property(
        token: *mut c_void,
        prop_name: *const c_char,
        content_type: u32,
    ) -> u32;
    pub fn noesis_plain_vm_create_instance(token: *mut c_void) -> *mut c_void;
    pub fn noesis_plain_vm_set_value(
        instance: *mut c_void,
        prop_index: u32,
        boxed_value: *mut c_void,
    ) -> bool;
    pub fn noesis_plain_vm_get_value(instance: *mut c_void, prop_index: u32) -> *mut c_void;
    pub fn noesis_plain_vm_notify(instance: *mut c_void, prop_name: *const c_char) -> bool;
    pub fn noesis_plain_vm_unregister(token: *mut c_void);

    // ── IMultiValueConverter + MultiBinding ────────────────────────────────────
    pub fn noesis_multi_value_converter_create(
        vt: *const MultiValueConverterVTable,
        userdata: *mut c_void,
        free_handler: MultiValueConverterFreeFn,
    ) -> *mut c_void;
    pub fn noesis_multi_value_converter_destroy(converter: *mut c_void);
    pub fn noesis_multi_binding_create() -> *mut c_void;
    pub fn noesis_multi_binding_destroy(multi_binding: *mut c_void);
    pub fn noesis_multi_binding_add_binding(
        multi_binding: *mut c_void,
        binding: *mut c_void,
    ) -> bool;
    pub fn noesis_multi_binding_set_converter(multi_binding: *mut c_void, converter: *mut c_void);
    pub fn noesis_multi_binding_set_converter_parameter(
        multi_binding: *mut c_void,
        parameter: *mut c_void,
    );
    pub fn noesis_multi_binding_set_mode(multi_binding: *mut c_void, mode: i32);
    pub fn noesis_set_multi_binding(
        element: *mut c_void,
        dp_name: *const c_char,
        multi_binding: *mut c_void,
    ) -> bool;

    // ── Controls: programmatic access ─────────────────────────────────────────
    // Mirrors cpp/noesis_controls.cpp; see cpp/noesis_shim.h for the borrow /
    // sentinel contract of each entrypoint.

    // Selector
    pub fn noesis_selector_get_selected_index(element: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_selector_set_selected_index(element: *mut c_void, index: i32) -> bool;
    pub fn noesis_selector_get_selected_item(element: *mut c_void) -> *mut c_void;
    pub fn noesis_selector_set_selected_item(element: *mut c_void, item: *mut c_void) -> bool;

    // ItemsControl.Items
    pub fn noesis_items_control_items_add(element: *mut c_void, item: *mut c_void) -> i32;
    pub fn noesis_items_control_items_insert(
        element: *mut c_void,
        index: u32,
        item: *mut c_void,
    ) -> bool;
    pub fn noesis_items_control_items_remove_at(element: *mut c_void, index: u32) -> bool;
    pub fn noesis_items_control_items_clear(element: *mut c_void) -> bool;

    // RangeBase
    pub fn noesis_rangebase_get(element: *mut c_void, which: i32, out: *mut f32) -> bool;
    pub fn noesis_rangebase_set(element: *mut c_void, which: i32, value: f32) -> bool;

    // ToggleButton
    pub fn noesis_toggle_get_is_checked(element: *mut c_void, out_state: *mut i8) -> bool;
    pub fn noesis_toggle_set_is_checked(element: *mut c_void, state: i8) -> bool;

    // Popup / Expander
    pub fn noesis_popup_get_is_open(element: *mut c_void, out: *mut bool) -> bool;
    pub fn noesis_popup_set_is_open(element: *mut c_void, open: bool) -> bool;
    pub fn noesis_expander_get_is_expanded(element: *mut c_void, out: *mut bool) -> bool;
    pub fn noesis_expander_set_is_expanded(element: *mut c_void, expanded: bool) -> bool;

    // ScrollViewer
    pub fn noesis_scrollviewer_get(element: *mut c_void, which: i32, out: *mut f32) -> bool;
    pub fn noesis_scrollviewer_scroll_to_horizontal(element: *mut c_void, offset: f32) -> bool;
    pub fn noesis_scrollviewer_scroll_to_vertical(element: *mut c_void, offset: f32) -> bool;
    pub fn noesis_scrollviewer_scroll_to_home(element: *mut c_void) -> bool;
    pub fn noesis_scrollviewer_scroll_to_end(element: *mut c_void) -> bool;

    // TextBox / PasswordBox
    pub fn noesis_textbox_get_int(element: *mut c_void, which: i32, out: *mut i32) -> bool;
    pub fn noesis_textbox_set_int(element: *mut c_void, which: i32, value: i32) -> bool;
    pub fn noesis_textbox_select(element: *mut c_void, start: i32, length: i32) -> bool;
    pub fn noesis_textbox_select_all(element: *mut c_void) -> bool;
    pub fn noesis_textbox_get_selected_text(element: *mut c_void) -> *const c_char;
    pub fn noesis_passwordbox_get_password(element: *mut c_void) -> *const c_char;
    pub fn noesis_passwordbox_set_password(element: *mut c_void, password: *const c_char) -> bool;

    // ── Additional control accessors ──────────────────────────────────────────
    // Selector.SelectedValue / SelectedValuePath
    pub fn noesis_controls_selector_get_selected_value(element: *mut c_void) -> *mut c_void;
    pub fn noesis_controls_selector_set_selected_value(
        element: *mut c_void,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_controls_selector_get_selected_value_path(element: *mut c_void) -> *const c_char;
    pub fn noesis_controls_selector_set_selected_value_path(
        element: *mut c_void,
        path: *const c_char,
    ) -> bool;

    // TreeView selection / TreeViewItem state
    pub fn noesis_controls_treeview_get_selected_item(element: *mut c_void) -> *mut c_void;
    pub fn noesis_controls_treeviewitem_get_is_selected(
        element: *mut c_void,
        out: *mut bool,
    ) -> bool;
    pub fn noesis_controls_treeviewitem_set_is_selected(
        element: *mut c_void,
        selected: bool,
    ) -> bool;
    pub fn noesis_controls_treeviewitem_get_is_expanded(
        element: *mut c_void,
        out: *mut bool,
    ) -> bool;
    pub fn noesis_controls_treeviewitem_set_is_expanded(
        element: *mut c_void,
        expanded: bool,
    ) -> bool;

    // ItemContainerGenerator
    pub fn noesis_controls_generator_container_from_index(
        element: *mut c_void,
        index: i32,
    ) -> *mut c_void;
    pub fn noesis_controls_generator_container_from_item(
        element: *mut c_void,
        item: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_controls_generator_index_from_container(
        element: *mut c_void,
        container: *mut c_void,
    ) -> i32;
    pub fn noesis_controls_generator_item_from_container(
        element: *mut c_void,
        container: *mut c_void,
    ) -> *mut c_void;

    // ListView / GridView columns
    pub fn noesis_controls_listview_get_view(element: *mut c_void) -> *mut c_void;
    pub fn noesis_controls_gridview_column_count(gridview: *mut c_void) -> i32;
    pub fn noesis_controls_gridview_column_get_width(
        gridview: *mut c_void,
        index: u32,
        out: *mut f32,
    ) -> bool;
    pub fn noesis_controls_gridview_column_set_width(
        gridview: *mut c_void,
        index: u32,
        width: f32,
    ) -> bool;
    pub fn noesis_controls_gridview_column_get_actual_width(
        gridview: *mut c_void,
        index: u32,
        out: *mut f32,
    ) -> bool;
    pub fn noesis_controls_gridview_column_get_header(
        gridview: *mut c_void,
        index: u32,
    ) -> *mut c_void;

    // ToolTip / ToolTipService
    pub fn noesis_controls_fe_get_tooltip(element: *mut c_void) -> *mut c_void;
    pub fn noesis_controls_fe_set_tooltip(element: *mut c_void, tooltip: *mut c_void) -> bool;
    pub fn noesis_controls_fe_set_tooltip_string(element: *mut c_void, text: *const c_char)
    -> bool;
    pub fn noesis_controls_tooltipservice_get_tooltip(obj: *mut c_void) -> *mut c_void;
    pub fn noesis_controls_tooltipservice_set_tooltip(
        obj: *mut c_void,
        tooltip: *mut c_void,
    ) -> bool;
    pub fn noesis_controls_tooltip_get_is_open(element: *mut c_void, out: *mut bool) -> bool;
    pub fn noesis_controls_tooltip_set_is_open(element: *mut c_void, open: bool) -> bool;

    // ContextMenu / ContextMenuService
    pub fn noesis_controls_fe_get_context_menu(element: *mut c_void) -> *mut c_void;
    pub fn noesis_controls_fe_set_context_menu(element: *mut c_void, menu: *mut c_void) -> bool;
    pub fn noesis_controls_contextmenuservice_get_context_menu(obj: *mut c_void) -> *mut c_void;
    pub fn noesis_controls_contextmenuservice_set_context_menu(
        obj: *mut c_void,
        menu: *mut c_void,
    ) -> bool;
    pub fn noesis_controls_contextmenu_get_is_open(element: *mut c_void, out: *mut bool) -> bool;
    pub fn noesis_controls_contextmenu_set_is_open(element: *mut c_void, open: bool) -> bool;

    // ScrollViewer line/page/edge + IScrollInfo
    pub fn noesis_controls_scrollviewer_line(element: *mut c_void, which: i32) -> bool;
    pub fn noesis_controls_scrollviewer_page(element: *mut c_void, which: i32) -> bool;
    pub fn noesis_controls_scrollviewer_edge(element: *mut c_void, which: i32) -> bool;
    pub fn noesis_controls_scrollviewer_metric(
        element: *mut c_void,
        which: i32,
        out: *mut f32,
    ) -> bool;

    // Image source
    pub fn noesis_controls_image_get_source(element: *mut c_void) -> *mut c_void;
    pub fn noesis_controls_image_set_source(element: *mut c_void, source: *mut c_void) -> bool;
    // ── ResourceDictionary, Style, templates. See cpp/noesis_shim.h
    //    for the per-function ownership contract (create/parse → +1 owned;
    //    get_* → AddRef'd +1 owned; find_* / get_application_resources →
    //    borrowed, do not release). ──────────────────────────────────────────
    pub fn noesis_box_float(value: f32) -> *mut c_void;
    pub fn noesis_resource_dictionary_create() -> *mut c_void;
    pub fn noesis_resource_dictionary_destroy(dict: *mut c_void);
    pub fn noesis_resource_dictionary_parse(xaml: *const c_char) -> *mut c_void;
    pub fn noesis_resource_dictionary_count(dict: *mut c_void) -> u32;
    pub fn noesis_resource_dictionary_add(
        dict: *mut c_void,
        key: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_resource_dictionary_contains(dict: *mut c_void, key: *const c_char) -> bool;
    pub fn noesis_resource_dictionary_find(dict: *mut c_void, key: *const c_char) -> *mut c_void;
    pub fn noesis_resource_dictionary_add_merged(dict: *mut c_void, merged: *mut c_void) -> bool;

    pub fn noesis_gui_set_application_resources(dict: *mut c_void);
    pub fn noesis_gui_get_application_resources() -> *mut c_void;
    pub fn noesis_gui_register_default_styles(uri: *const c_char) -> bool;

    pub fn noesis_framework_element_get_resources(element: *mut c_void) -> *mut c_void;
    pub fn noesis_framework_element_set_resources(element: *mut c_void, dict: *mut c_void) -> bool;
    pub fn noesis_framework_element_find_resource(
        element: *mut c_void,
        key: *const c_char,
    ) -> *mut c_void;

    pub fn noesis_style_create() -> *mut c_void;
    pub fn noesis_style_destroy(style: *mut c_void);
    pub fn noesis_style_set_target_type(style: *mut c_void, type_name: *const c_char) -> bool;
    pub fn noesis_style_add_setter(
        style: *mut c_void,
        dp_name: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_style_set_based_on(style: *mut c_void, base: *mut c_void);

    pub fn noesis_framework_element_set_style(element: *mut c_void, style: *mut c_void) -> bool;
    pub fn noesis_framework_element_get_style(element: *mut c_void) -> *mut c_void;

    pub fn noesis_control_template_parse(xaml: *const c_char) -> *mut c_void;
    pub fn noesis_data_template_parse(xaml: *const c_char) -> *mut c_void;
    pub fn noesis_control_set_template(control: *mut c_void, tmpl: *mut c_void) -> bool;
    pub fn noesis_control_get_template(control: *mut c_void) -> *mut c_void;
    pub fn noesis_framework_template_find_name(
        tmpl: *mut c_void,
        name: *const c_char,
        templated_parent: *mut c_void,
    ) -> *mut c_void;

    // ── Triggers / selector / resource extensions ──────────────────────────────
    //
    // Trigger/DataTrigger/MultiTrigger/EventTrigger are constructed at +1 and
    // attached to a Style's Triggers collection (which takes its own ref).
    // `_get_*` value/binding getters AddRef (+1 owned); `_get_*_name` getters
    // are borrowed C strings valid while the underlying DP/event lives.
    pub fn noesis_templates_trigger_create() -> *mut c_void;
    pub fn noesis_templates_trigger_set_property(
        trigger: *mut c_void,
        type_name: *const c_char,
        dp_name: *const c_char,
    ) -> bool;
    pub fn noesis_templates_trigger_get_property_name(trigger: *mut c_void) -> *const c_char;
    pub fn noesis_templates_trigger_set_value(trigger: *mut c_void, value: *mut c_void) -> bool;
    pub fn noesis_templates_trigger_get_value(trigger: *mut c_void) -> *mut c_void;
    pub fn noesis_templates_trigger_add_setter(
        trigger: *mut c_void,
        type_name: *const c_char,
        dp_name: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_templates_trigger_setter_count(trigger: *mut c_void) -> i32;

    pub fn noesis_templates_data_trigger_create() -> *mut c_void;
    pub fn noesis_templates_data_trigger_set_binding(
        trigger: *mut c_void,
        binding: *mut c_void,
    ) -> bool;
    pub fn noesis_templates_data_trigger_get_binding(trigger: *mut c_void) -> *mut c_void;
    pub fn noesis_templates_data_trigger_set_value(
        trigger: *mut c_void,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_templates_data_trigger_get_value(trigger: *mut c_void) -> *mut c_void;
    pub fn noesis_templates_data_trigger_add_setter(
        trigger: *mut c_void,
        type_name: *const c_char,
        dp_name: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_templates_data_trigger_setter_count(trigger: *mut c_void) -> i32;

    pub fn noesis_templates_multi_trigger_create() -> *mut c_void;
    pub fn noesis_templates_multi_trigger_add_condition(
        trigger: *mut c_void,
        type_name: *const c_char,
        dp_name: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_templates_multi_trigger_condition_count(trigger: *mut c_void) -> i32;
    pub fn noesis_templates_multi_trigger_get_condition_property_name(
        trigger: *mut c_void,
        index: u32,
    ) -> *const c_char;
    pub fn noesis_templates_multi_trigger_get_condition_value(
        trigger: *mut c_void,
        index: u32,
    ) -> *mut c_void;
    pub fn noesis_templates_multi_trigger_add_setter(
        trigger: *mut c_void,
        type_name: *const c_char,
        dp_name: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_templates_multi_trigger_setter_count(trigger: *mut c_void) -> i32;

    pub fn noesis_templates_event_trigger_create() -> *mut c_void;
    pub fn noesis_templates_event_trigger_set_routed_event(
        trigger: *mut c_void,
        owner_type: *const c_char,
        event_name: *const c_char,
    ) -> bool;
    pub fn noesis_templates_event_trigger_get_routed_event_name(
        trigger: *mut c_void,
    ) -> *const c_char;
    pub fn noesis_templates_event_trigger_set_source_name(
        trigger: *mut c_void,
        name: *const c_char,
    ) -> bool;
    pub fn noesis_templates_event_trigger_get_source_name(trigger: *mut c_void) -> *const c_char;
    pub fn noesis_templates_event_trigger_action_count(trigger: *mut c_void) -> i32;
    pub fn noesis_templates_event_trigger_add_action(
        trigger: *mut c_void,
        action: *mut c_void,
    ) -> bool;

    pub fn noesis_templates_multi_data_trigger_create() -> *mut c_void;
    pub fn noesis_templates_multi_data_trigger_add_condition(
        trigger: *mut c_void,
        binding: *mut c_void,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_templates_multi_data_trigger_condition_count(trigger: *mut c_void) -> i32;
    pub fn noesis_templates_multi_data_trigger_condition_has_binding(
        trigger: *mut c_void,
        index: u32,
    ) -> i32;
    pub fn noesis_templates_multi_data_trigger_get_condition_value(
        trigger: *mut c_void,
        index: u32,
    ) -> *mut c_void;
    pub fn noesis_templates_multi_data_trigger_add_setter(
        trigger: *mut c_void,
        type_name: *const c_char,
        dp_name: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_templates_multi_data_trigger_setter_count(trigger: *mut c_void) -> i32;

    pub fn noesis_templates_style_add_trigger(style: *mut c_void, trigger: *mut c_void) -> bool;
    pub fn noesis_templates_style_trigger_count(style: *mut c_void) -> i32;
    pub fn noesis_templates_style_get_trigger(style: *mut c_void, index: u32) -> *mut c_void;

    pub fn noesis_templates_selector_create(
        vtable: *const TemplateSelectorVTable,
        userdata: *mut c_void,
        free_handler: TemplateSelectorFreeFn,
    ) -> *mut c_void;
    pub fn noesis_templates_selector_destroy(selector: *mut c_void);
    pub fn noesis_templates_selector_select(
        selector: *mut c_void,
        item: *mut c_void,
        container: *mut c_void,
    ) -> *mut c_void;
}

// ── Brushes, transforms, effects, RenderOptions ─────────────────────────────
//
// Object construction from Rust. Each `*_create` returns a `+1`-owned
// `BaseComponent*` (the owning wrapper in src/brushes.rs / src/transforms.rs
// releases it on Drop). Colors are `[f32; 4]` = `{r, g, b, a}` in `0..=1`.
unsafe extern "C" {
    // SolidColorBrush
    pub fn noesis_solid_color_brush_create(color: *const f32) -> *mut c_void;
    pub fn noesis_solid_color_brush_set_color(brush: *mut c_void, color: *const f32) -> bool;
    pub fn noesis_solid_color_brush_get_color(brush: *mut c_void, out: *mut f32) -> bool;

    // LinearGradientBrush
    pub fn noesis_linear_gradient_brush_create() -> *mut c_void;
    pub fn noesis_linear_gradient_brush_set_start_point(brush: *mut c_void, x: f32, y: f32)
    -> bool;
    pub fn noesis_linear_gradient_brush_set_end_point(brush: *mut c_void, x: f32, y: f32) -> bool;
    pub fn noesis_linear_gradient_brush_get_points(brush: *mut c_void, out: *mut f32) -> bool;

    // RadialGradientBrush
    pub fn noesis_radial_gradient_brush_create() -> *mut c_void;
    pub fn noesis_radial_gradient_brush_set_center(brush: *mut c_void, x: f32, y: f32) -> bool;
    pub fn noesis_radial_gradient_brush_set_gradient_origin(
        brush: *mut c_void,
        x: f32,
        y: f32,
    ) -> bool;
    pub fn noesis_radial_gradient_brush_set_radius(brush: *mut c_void, rx: f32, ry: f32) -> bool;
    pub fn noesis_radial_gradient_brush_get_radius(
        brush: *mut c_void,
        rx: *mut f32,
        ry: *mut f32,
    ) -> bool;

    // GradientBrush stops
    pub fn noesis_gradient_brush_add_stop(
        brush: *mut c_void,
        offset: f32,
        color: *const f32,
    ) -> i32;
    pub fn noesis_gradient_brush_stop_count(brush: *mut c_void) -> i32;
    pub fn noesis_gradient_brush_get_stop(
        brush: *mut c_void,
        index: u32,
        out_offset: *mut f32,
        out_color: *mut f32,
    ) -> bool;
    pub fn noesis_gradient_brush_set_spread_method(brush: *mut c_void, method: i32) -> bool;
    pub fn noesis_gradient_brush_get_spread_method(brush: *mut c_void) -> i32;
    pub fn noesis_gradient_brush_set_mapping_mode(brush: *mut c_void, mode: i32) -> bool;
    pub fn noesis_gradient_brush_get_mapping_mode(brush: *mut c_void) -> i32;

    // ImageBrush
    pub fn noesis_image_brush_create(image_source: *mut c_void) -> *mut c_void;
    pub fn noesis_image_brush_set_image_source(
        brush: *mut c_void,
        image_source: *mut c_void,
    ) -> bool;
    pub fn noesis_image_brush_get_image_source(brush: *mut c_void) -> *mut c_void;

    // VisualBrush
    pub fn noesis_visual_brush_create(visual: *mut c_void) -> *mut c_void;
    pub fn noesis_visual_brush_set_visual(brush: *mut c_void, visual: *mut c_void) -> bool;
    pub fn noesis_visual_brush_get_visual(brush: *mut c_void) -> *mut c_void;

    // TileBrush tiling knobs (ImageBrush + VisualBrush)
    pub fn noesis_tile_brush_set_alignment_x(brush: *mut c_void, value: i32) -> bool;
    pub fn noesis_tile_brush_get_alignment_x(brush: *mut c_void) -> i32;
    pub fn noesis_tile_brush_set_alignment_y(brush: *mut c_void, value: i32) -> bool;
    pub fn noesis_tile_brush_get_alignment_y(brush: *mut c_void) -> i32;
    pub fn noesis_tile_brush_set_stretch(brush: *mut c_void, value: i32) -> bool;
    pub fn noesis_tile_brush_get_stretch(brush: *mut c_void) -> i32;
    pub fn noesis_tile_brush_set_tile_mode(brush: *mut c_void, value: i32) -> bool;
    pub fn noesis_tile_brush_get_tile_mode(brush: *mut c_void) -> i32;
    pub fn noesis_tile_brush_set_viewport_units(brush: *mut c_void, value: i32) -> bool;
    pub fn noesis_tile_brush_get_viewport_units(brush: *mut c_void) -> i32;
    pub fn noesis_tile_brush_set_viewbox_units(brush: *mut c_void, value: i32) -> bool;
    pub fn noesis_tile_brush_get_viewbox_units(brush: *mut c_void) -> i32;
    pub fn noesis_tile_brush_set_viewport(
        brush: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool;
    pub fn noesis_tile_brush_get_viewport(brush: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_tile_brush_set_viewbox(
        brush: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool;
    pub fn noesis_tile_brush_get_viewbox(brush: *mut c_void, out: *mut f32) -> bool;

    // Transforms
    pub fn noesis_translate_transform_create(x: f32, y: f32) -> *mut c_void;
    pub fn noesis_translate_transform_set(transform: *mut c_void, x: f32, y: f32) -> bool;
    pub fn noesis_translate_transform_get(transform: *mut c_void, x: *mut f32, y: *mut f32)
    -> bool;

    pub fn noesis_scale_transform_create(sx: f32, sy: f32, cx: f32, cy: f32) -> *mut c_void;
    pub fn noesis_scale_transform_set(
        transform: *mut c_void,
        sx: f32,
        sy: f32,
        cx: f32,
        cy: f32,
    ) -> bool;
    pub fn noesis_scale_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_rotate_transform_create(angle: f32, cx: f32, cy: f32) -> *mut c_void;
    pub fn noesis_rotate_transform_set_angle(transform: *mut c_void, angle: f32) -> bool;
    pub fn noesis_rotate_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_skew_transform_create(ax: f32, ay: f32, cx: f32, cy: f32) -> *mut c_void;
    pub fn noesis_skew_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_matrix_transform_create(matrix: *const f32) -> *mut c_void;
    pub fn noesis_matrix_transform_set(transform: *mut c_void, matrix: *const f32) -> bool;
    pub fn noesis_matrix_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_transform_group_create() -> *mut c_void;
    pub fn noesis_transform_group_add_child(group: *mut c_void, child: *mut c_void) -> bool;
    pub fn noesis_transform_group_child_count(group: *mut c_void) -> i32;

    pub fn noesis_composite_transform_create(fields: *const f32) -> *mut c_void;
    pub fn noesis_composite_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    // 3D transforms
    pub fn noesis_composite_transform3d_create(fields: *const f32) -> *mut c_void;
    pub fn noesis_composite_transform3d_set(transform: *mut c_void, fields: *const f32) -> bool;
    pub fn noesis_composite_transform3d_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_matrix_transform3d_create(matrix: *const f32) -> *mut c_void;
    pub fn noesis_matrix_transform3d_set(transform: *mut c_void, matrix: *const f32) -> bool;
    pub fn noesis_matrix_transform3d_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_element_set_transform3d(element: *mut c_void, transform: *mut c_void) -> bool;
    pub fn noesis_element_get_transform3d(element: *mut c_void) -> *mut c_void;

    // Effects
    pub fn noesis_blur_effect_create(radius: f32) -> *mut c_void;
    pub fn noesis_blur_effect_set_radius(effect: *mut c_void, radius: f32) -> bool;
    pub fn noesis_blur_effect_get_radius(effect: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_drop_shadow_effect_create(
        color: *const f32,
        blur_radius: f32,
        direction: f32,
        shadow_depth: f32,
        opacity: f32,
    ) -> *mut c_void;
    pub fn noesis_drop_shadow_effect_get(
        effect: *mut c_void,
        out_color: *mut f32,
        out_blur: *mut f32,
        out_direction: *mut f32,
        out_shadow_depth: *mut f32,
        out_opacity: *mut f32,
    ) -> bool;
    pub fn noesis_drop_shadow_effect_set_color(effect: *mut c_void, color: *const f32) -> bool;
    pub fn noesis_drop_shadow_effect_set_blur_radius(effect: *mut c_void, blur_radius: f32)
    -> bool;
    pub fn noesis_drop_shadow_effect_set_direction(effect: *mut c_void, direction: f32) -> bool;
    pub fn noesis_drop_shadow_effect_set_shadow_depth(
        effect: *mut c_void,
        shadow_depth: f32,
    ) -> bool;
    pub fn noesis_drop_shadow_effect_set_opacity(effect: *mut c_void, opacity: f32) -> bool;

    // RenderOptions
    pub fn noesis_render_options_set_bitmap_scaling_mode(obj: *mut c_void, mode: i32) -> bool;
    pub fn noesis_render_options_get_bitmap_scaling_mode(obj: *mut c_void) -> i32;

    // ── Shape elements. See cpp/noesis_shapes.cpp / src/shapes.rs ──────────────
    pub fn noesis_rectangle_create() -> *mut c_void;
    pub fn noesis_ellipse_create() -> *mut c_void;
    pub fn noesis_line_create() -> *mut c_void;

    pub fn noesis_shape_set_width(shape: *mut c_void, width: f32) -> bool;
    pub fn noesis_shape_get_width(shape: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_shape_set_height(shape: *mut c_void, height: f32) -> bool;
    pub fn noesis_shape_get_height(shape: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_shape_set_fill(shape: *mut c_void, brush: *mut c_void) -> bool;
    pub fn noesis_shape_get_fill(shape: *mut c_void) -> *mut c_void;
    pub fn noesis_shape_set_stroke(shape: *mut c_void, brush: *mut c_void) -> bool;
    pub fn noesis_shape_get_stroke(shape: *mut c_void) -> *mut c_void;

    pub fn noesis_shape_set_stroke_thickness(shape: *mut c_void, value: f32) -> bool;
    pub fn noesis_shape_get_stroke_thickness(shape: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_shape_set_stroke_miter_limit(shape: *mut c_void, value: f32) -> bool;
    pub fn noesis_shape_get_stroke_miter_limit(shape: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_shape_set_stroke_dash_offset(shape: *mut c_void, value: f32) -> bool;
    pub fn noesis_shape_get_stroke_dash_offset(shape: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_shape_set_trim_start(shape: *mut c_void, value: f32) -> bool;
    pub fn noesis_shape_get_trim_start(shape: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_shape_set_trim_end(shape: *mut c_void, value: f32) -> bool;
    pub fn noesis_shape_get_trim_end(shape: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_shape_set_trim_offset(shape: *mut c_void, value: f32) -> bool;
    pub fn noesis_shape_get_trim_offset(shape: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_shape_set_stroke_dash_cap(shape: *mut c_void, value: i32) -> bool;
    pub fn noesis_shape_get_stroke_dash_cap(shape: *mut c_void) -> i32;
    pub fn noesis_shape_set_stroke_start_line_cap(shape: *mut c_void, value: i32) -> bool;
    pub fn noesis_shape_get_stroke_start_line_cap(shape: *mut c_void) -> i32;
    pub fn noesis_shape_set_stroke_end_line_cap(shape: *mut c_void, value: i32) -> bool;
    pub fn noesis_shape_get_stroke_end_line_cap(shape: *mut c_void) -> i32;
    pub fn noesis_shape_set_stroke_line_join(shape: *mut c_void, value: i32) -> bool;
    pub fn noesis_shape_get_stroke_line_join(shape: *mut c_void) -> i32;
    pub fn noesis_shape_set_stretch(shape: *mut c_void, value: i32) -> bool;
    pub fn noesis_shape_get_stretch(shape: *mut c_void) -> i32;

    pub fn noesis_shape_set_stroke_dash_array(
        shape: *mut c_void,
        dashes: *const std::os::raw::c_char,
    ) -> bool;
    pub fn noesis_shape_get_stroke_dash_array(shape: *mut c_void) -> *const std::os::raw::c_char;

    pub fn noesis_rectangle_set_radius_x(shape: *mut c_void, value: f32) -> bool;
    pub fn noesis_rectangle_get_radius_x(shape: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_rectangle_set_radius_y(shape: *mut c_void, value: f32) -> bool;
    pub fn noesis_rectangle_get_radius_y(shape: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_line_set(shape: *mut c_void, x1: f32, y1: f32, x2: f32, y2: f32) -> bool;
    pub fn noesis_line_get(shape: *mut c_void, out: *mut f32) -> bool;
}

// Geometry object model. Declarations mirror cpp/noesis_shim.h by
// hand; see cpp/noesis_geometry.cpp for the ownership contract (each *_create
// hands out one owned reference released by the Rust handle's Drop).
unsafe extern "C" {
    // Geometry base
    pub fn noesis_geometry_get_bounds(geometry: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_geometry_get_render_bounds(geometry: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_geometry_is_empty(geometry: *mut c_void) -> i32;
    pub fn noesis_geometry_set_transform(geometry: *mut c_void, transform: *mut c_void) -> bool;
    pub fn noesis_geometry_get_transform(geometry: *mut c_void) -> *mut c_void;

    // StreamGeometry + StreamGeometryContext
    pub fn noesis_stream_geometry_create() -> *mut c_void;
    pub fn noesis_stream_geometry_create_from_data(data: *const c_char) -> *mut c_void;
    pub fn noesis_stream_geometry_set_data(geometry: *mut c_void, data: *const c_char) -> bool;
    pub fn noesis_stream_geometry_set_fill_rule(geometry: *mut c_void, rule: i32) -> bool;
    pub fn noesis_stream_geometry_get_fill_rule(geometry: *mut c_void) -> i32;
    pub fn noesis_stream_geometry_open(geometry: *mut c_void) -> *mut c_void;
    pub fn noesis_stream_geometry_context_begin_figure(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        is_closed: bool,
    ) -> bool;
    pub fn noesis_stream_geometry_context_line_to(ctx: *mut c_void, x: f32, y: f32) -> bool;
    pub fn noesis_stream_geometry_context_cubic_to(
        ctx: *mut c_void,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
    ) -> bool;
    pub fn noesis_stream_geometry_context_quadratic_to(
        ctx: *mut c_void,
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> bool;
    pub fn noesis_stream_geometry_context_arc_to(
        ctx: *mut c_void,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rotation_deg: f32,
        is_large_arc: bool,
        sweep_direction: i32,
    ) -> bool;
    pub fn noesis_stream_geometry_context_set_is_closed(ctx: *mut c_void, is_closed: bool) -> bool;
    pub fn noesis_stream_geometry_context_close(ctx: *mut c_void) -> bool;
    pub fn noesis_stream_geometry_context_destroy(ctx: *mut c_void);

    // PathGeometry + PathFigure
    pub fn noesis_path_geometry_create() -> *mut c_void;
    pub fn noesis_path_geometry_set_fill_rule(geometry: *mut c_void, rule: i32) -> bool;
    pub fn noesis_path_geometry_get_fill_rule(geometry: *mut c_void) -> i32;
    pub fn noesis_path_geometry_add_figure(geometry: *mut c_void, figure: *mut c_void) -> i32;
    pub fn noesis_path_geometry_figure_count(geometry: *mut c_void) -> i32;

    pub fn noesis_path_figure_create() -> *mut c_void;
    pub fn noesis_path_figure_set_start_point(figure: *mut c_void, x: f32, y: f32) -> bool;
    pub fn noesis_path_figure_get_start_point(figure: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_path_figure_set_is_closed(figure: *mut c_void, is_closed: bool) -> bool;
    pub fn noesis_path_figure_set_is_filled(figure: *mut c_void, is_filled: bool) -> bool;
    pub fn noesis_path_figure_get_is_closed(figure: *mut c_void) -> i32;
    pub fn noesis_path_figure_get_is_filled(figure: *mut c_void) -> i32;
    pub fn noesis_path_figure_add_segment(figure: *mut c_void, segment: *mut c_void) -> i32;
    pub fn noesis_path_figure_segment_count(figure: *mut c_void) -> i32;

    // Path segments
    pub fn noesis_line_segment_create(x: f32, y: f32) -> *mut c_void;
    pub fn noesis_line_segment_get_point(segment: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_bezier_segment_create(
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
        x3: f32,
        y3: f32,
    ) -> *mut c_void;
    pub fn noesis_bezier_segment_get(segment: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_quadratic_bezier_segment_create(
        x1: f32,
        y1: f32,
        x2: f32,
        y2: f32,
    ) -> *mut c_void;
    pub fn noesis_quadratic_bezier_segment_get(segment: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_arc_segment_create(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rotation_deg: f32,
        is_large_arc: bool,
        sweep_direction: i32,
    ) -> *mut c_void;
    pub fn noesis_arc_segment_get(
        segment: *mut c_void,
        out_point: *mut f32,
        out_size: *mut f32,
        out_rotation_deg: *mut f32,
        out_is_large_arc: *mut bool,
        out_sweep_direction: *mut i32,
    ) -> bool;
    pub fn noesis_poly_line_segment_create(points: *const f32, num_points: u32) -> *mut c_void;
    pub fn noesis_poly_bezier_segment_create(points: *const f32, num_points: u32) -> *mut c_void;
    pub fn noesis_poly_quadratic_bezier_segment_create(
        points: *const f32,
        num_points: u32,
    ) -> *mut c_void;
    pub fn noesis_poly_segment_point_count(segment: *mut c_void) -> i32;
    pub fn noesis_poly_segment_get_point(segment: *mut c_void, index: u32, out: *mut f32) -> bool;

    // EllipseGeometry / RectangleGeometry / LineGeometry
    pub fn noesis_ellipse_geometry_create(cx: f32, cy: f32, rx: f32, ry: f32) -> *mut c_void;
    pub fn noesis_ellipse_geometry_get(geometry: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_rectangle_geometry_create(
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        rx: f32,
        ry: f32,
    ) -> *mut c_void;
    pub fn noesis_rectangle_geometry_get(
        geometry: *mut c_void,
        out_rect: *mut f32,
        out_radii: *mut f32,
    ) -> bool;
    pub fn noesis_line_geometry_create(x1: f32, y1: f32, x2: f32, y2: f32) -> *mut c_void;
    pub fn noesis_line_geometry_get(geometry: *mut c_void, out: *mut f32) -> bool;

    // CombinedGeometry
    pub fn noesis_combined_geometry_create(
        mode: i32,
        geometry1: *mut c_void,
        geometry2: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_combined_geometry_set_geometry1(geometry: *mut c_void, g1: *mut c_void) -> bool;
    pub fn noesis_combined_geometry_set_geometry2(geometry: *mut c_void, g2: *mut c_void) -> bool;
    pub fn noesis_combined_geometry_get_geometry1(geometry: *mut c_void) -> *mut c_void;
    pub fn noesis_combined_geometry_get_geometry2(geometry: *mut c_void) -> *mut c_void;
    pub fn noesis_combined_geometry_set_mode(geometry: *mut c_void, mode: i32) -> bool;
    pub fn noesis_combined_geometry_get_mode(geometry: *mut c_void) -> i32;

    // GeometryGroup
    pub fn noesis_geometry_group_create() -> *mut c_void;
    pub fn noesis_geometry_group_set_fill_rule(geometry: *mut c_void, rule: i32) -> bool;
    pub fn noesis_geometry_group_get_fill_rule(geometry: *mut c_void) -> i32;
    pub fn noesis_geometry_group_add_child(geometry: *mut c_void, child: *mut c_void) -> i32;
    pub fn noesis_geometry_group_child_count(geometry: *mut c_void) -> i32;
}

// SVG / SVGPath parsing. See cpp/noesis_svg.cpp. The handles are
// plain heap objects (NOT BaseComponents); release with the matching *_destroy.
unsafe extern "C" {
    pub fn noesis_svg_path_parse(str: *const c_char) -> *mut c_void;
    pub fn noesis_svg_path_create() -> *mut c_void;
    pub fn noesis_svg_path_destroy(path: *mut c_void);
    pub fn noesis_svg_path_command_count(path: *mut c_void) -> u32;

    pub fn noesis_svg_path_move_to(path: *mut c_void, x: f32, y: f32);
    pub fn noesis_svg_path_line_to(path: *mut c_void, x: f32, y: f32);
    pub fn noesis_svg_path_close(path: *mut c_void);
    pub fn noesis_svg_path_add_rect(path: *mut c_void, x: f32, y: f32, width: f32, height: f32);
    pub fn noesis_svg_path_add_ellipse(path: *mut c_void, x: f32, y: f32, rx: f32, ry: f32);

    pub fn noesis_svg_path_calculate_bounds(path: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_svg_path_fill_contains(path: *mut c_void, x: f32, y: f32, fill_rule: i32)
    -> bool;
    pub fn noesis_svg_path_stroke_contains(
        path: *mut c_void,
        x: f32,
        y: f32,
        width: f32,
        join: i32,
        start_cap: i32,
        end_cap: i32,
        miter_limit: f32,
    ) -> bool;

    pub fn noesis_svg_image_parse(svg: *const c_char) -> *mut c_void;
    pub fn noesis_svg_image_destroy(image: *mut c_void);
    pub fn noesis_svg_image_get_size(image: *mut c_void, width: *mut f32, height: *mut f32)
    -> bool;
    pub fn noesis_svg_image_shape_count(image: *mut c_void) -> u32;
    pub fn noesis_svg_image_shape_fill_type(image: *mut c_void, index: u32) -> i32;
}

/// Mirror of `noesis_texture_render_callback` in `cpp/noesis_shim.h`. Pointer-
/// ABI-compatible with `Noesis::DynamicTextureSource::TextureRenderCallback`
/// (`Texture* (*)(RenderDevice*, void*)`). Invoked from the render thread under a
/// live `RenderDevice` render pass; `device` is a borrowed `Noesis::RenderDevice*`,
/// `user` is the pointer passed to [`noesis_dynamic_texture_source_create`].
/// The returned `*mut c_void` is a borrowed `Noesis::Texture*` (or null).
pub type TextureRenderCallback =
    unsafe extern "C" fn(device: *mut c_void, user: *mut c_void) -> *mut c_void;

// ── ImageSource / BitmapSource family ───────────────────────────────────────
//
// Object construction from Rust. Each `*_create` returns a `+1`-owned
// `BaseComponent*` (the owning wrapper in src/imaging.rs releases it on Drop).
unsafe extern "C" {
    // CroppedBitmap
    pub fn noesis_cropped_bitmap_create() -> *mut c_void;
    pub fn noesis_cropped_bitmap_set_source(crop: *mut c_void, source: *mut c_void) -> bool;
    pub fn noesis_cropped_bitmap_get_source(crop: *mut c_void) -> *mut c_void;
    pub fn noesis_cropped_bitmap_set_source_rect(
        crop: *mut c_void,
        x: i32,
        y: i32,
        width: u32,
        height: u32,
    ) -> bool;
    pub fn noesis_cropped_bitmap_get_source_rect(
        crop: *mut c_void,
        x: *mut i32,
        y: *mut i32,
        width: *mut u32,
        height: *mut u32,
    ) -> bool;

    // TextureSource
    pub fn noesis_texture_source_create(texture: *mut c_void) -> *mut c_void;
    pub fn noesis_texture_source_set_texture(source: *mut c_void, texture: *mut c_void) -> bool;
    pub fn noesis_texture_source_get_texture(source: *mut c_void) -> *mut c_void;

    // BitmapImage
    pub fn noesis_bitmap_image_create(uri: *const c_char) -> *mut c_void;
    pub fn noesis_bitmap_image_set_uri_source(image: *mut c_void, uri: *const c_char) -> bool;
    pub fn noesis_bitmap_image_get_uri_source(image: *mut c_void) -> *const c_char;

    // BitmapSource base getters
    pub fn noesis_bitmap_source_get_pixel_size(
        source: *mut c_void,
        width: *mut i32,
        height: *mut i32,
    ) -> bool;
    pub fn noesis_bitmap_source_get_dpi(
        source: *mut c_void,
        dpi_x: *mut f32,
        dpi_y: *mut f32,
    ) -> bool;

    // DynamicTextureSource
    pub fn noesis_dynamic_texture_source_create(
        width: u32,
        height: u32,
        callback: TextureRenderCallback,
        user: *mut c_void,
    ) -> *mut c_void;
    pub fn noesis_dynamic_texture_source_resize(
        source: *mut c_void,
        width: u32,
        height: u32,
    ) -> bool;
    pub fn noesis_dynamic_texture_source_get_pixel_size(
        source: *mut c_void,
        width: *mut u32,
        height: *mut u32,
    ) -> bool;
    // Typography & text properties; see cpp/noesis_typography.cpp.
    pub fn noesis_typography_font_family_create(source: *const c_char) -> *mut c_void;
    pub fn noesis_typography_font_family_get_source(family: *mut c_void) -> *const c_char;
    pub fn noesis_typography_font_family_get_num_fonts(family: *mut c_void) -> u32;
    pub fn noesis_typography_font_family_get_font_name(
        family: *mut c_void,
        index: u32,
    ) -> *const c_char;

    pub fn noesis_typography_text_element_set_font_size(element: *mut c_void, size: f32) -> bool;
    pub fn noesis_typography_text_element_get_font_size(
        element: *mut c_void,
        out: *mut f32,
    ) -> bool;
    pub fn noesis_typography_text_element_set_font_family(
        element: *mut c_void,
        family: *mut c_void,
    ) -> bool;
    pub fn noesis_typography_text_element_get_font_family(element: *mut c_void) -> *mut c_void;
    pub fn noesis_typography_text_element_set_foreground(
        element: *mut c_void,
        brush: *mut c_void,
    ) -> bool;
    pub fn noesis_typography_text_element_get_foreground(element: *mut c_void) -> *mut c_void;
    pub fn noesis_typography_text_element_set_font_weight(
        element: *mut c_void,
        weight: i32,
    ) -> bool;
    pub fn noesis_typography_text_element_get_font_weight(
        element: *mut c_void,
        out: *mut i32,
    ) -> bool;
    pub fn noesis_typography_text_element_set_font_style(element: *mut c_void, style: i32) -> bool;
    pub fn noesis_typography_text_element_get_font_style(
        element: *mut c_void,
        out: *mut i32,
    ) -> bool;
    pub fn noesis_typography_text_element_set_font_stretch(
        element: *mut c_void,
        stretch: i32,
    ) -> bool;
    pub fn noesis_typography_text_element_get_font_stretch(
        element: *mut c_void,
        out: *mut i32,
    ) -> bool;

    pub fn noesis_typography_set_capitals(element: *mut c_void, value: i32) -> bool;
    pub fn noesis_typography_get_capitals(element: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_typography_set_numeral_style(element: *mut c_void, value: i32) -> bool;
    pub fn noesis_typography_get_numeral_style(element: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_typography_set_fraction(element: *mut c_void, value: i32) -> bool;
    pub fn noesis_typography_get_fraction(element: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_typography_set_variants(element: *mut c_void, value: i32) -> bool;
    pub fn noesis_typography_get_variants(element: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_typography_set_standard_ligatures(element: *mut c_void, value: bool) -> bool;
    pub fn noesis_typography_get_standard_ligatures(element: *mut c_void, out: *mut bool) -> bool;
    pub fn noesis_typography_set_kerning(element: *mut c_void, value: bool) -> bool;
    pub fn noesis_typography_get_kerning(element: *mut c_void, out: *mut bool) -> bool;

    pub fn noesis_typography_text_box_add_composition_underline(
        element: *mut c_void,
        start: u32,
        end: u32,
        style: i32,
        bold: bool,
    ) -> bool;
    pub fn noesis_typography_text_box_num_composition_underlines(element: *mut c_void) -> i32;
    pub fn noesis_typography_text_box_get_composition_underline(
        element: *mut c_void,
        index: u32,
        out_start: *mut u32,
        out_end: *mut u32,
        out_style: *mut i32,
        out_bold: *mut bool,
    ) -> bool;
    pub fn noesis_typography_text_box_clear_composition_underlines(element: *mut c_void) -> bool;
}

// ── Immediate-mode drawing: Pen + DrawingContext ─────────────────────────────
//
// `Pen` / `RectangleGeometry` are code-built like the brushes above (each
// `*_create` returns a `+1`-owned `BaseComponent*` released on the owning
// wrapper's Drop). The `noesis_drawing_*` entrypoints take the borrowed
// `DrawingContext*` delivered to a class render callback; all return `false`
// on a null / wrong-type context.
unsafe extern "C" {
    // Pen
    pub fn noesis_pen_create(brush: *mut c_void, thickness: f32) -> *mut c_void;
    pub fn noesis_pen_set_brush(pen: *mut c_void, brush: *mut c_void) -> bool;
    pub fn noesis_pen_get_brush(pen: *mut c_void) -> *mut c_void;
    pub fn noesis_pen_set_thickness(pen: *mut c_void, thickness: f32) -> bool;
    pub fn noesis_pen_get_thickness(pen: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_pen_set_line_caps(
        pen: *mut c_void,
        start_cap: i32,
        end_cap: i32,
        dash_cap: i32,
    ) -> bool;
    pub fn noesis_pen_get_line_caps(pen: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_pen_set_line_join(pen: *mut c_void, join: i32, miter_limit: f32) -> bool;
    pub fn noesis_pen_get_line_join(
        pen: *mut c_void,
        out_join: *mut i32,
        out_miter_limit: *mut f32,
    ) -> bool;
    pub fn noesis_pen_set_dash_style(
        pen: *mut c_void,
        dashes: *const f32,
        count: u32,
        offset: f32,
    ) -> bool;
    pub fn noesis_pen_get_dash_offset(pen: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_pen_get_dashes(pen: *mut c_void) -> *const c_char;

    // RectangleGeometry
    pub fn noesis_drawing_rect_geometry_create(
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_x: f32,
        r_y: f32,
    ) -> *mut c_void;
    pub fn noesis_rectangle_geometry_get_rect(geometry: *mut c_void, out: *mut f32) -> bool;

    // DrawingContext commands (context is the borrowed render-callback pointer).
    pub fn noesis_drawing_draw_line(
        context: *mut c_void,
        pen: *mut c_void,
        x0: f32,
        y0: f32,
        x1: f32,
        y1: f32,
    ) -> bool;
    pub fn noesis_drawing_draw_rectangle(
        context: *mut c_void,
        brush: *mut c_void,
        pen: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool;
    pub fn noesis_drawing_draw_rounded_rectangle(
        context: *mut c_void,
        brush: *mut c_void,
        pen: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        r_x: f32,
        r_y: f32,
    ) -> bool;
    pub fn noesis_drawing_draw_ellipse(
        context: *mut c_void,
        brush: *mut c_void,
        pen: *mut c_void,
        cx: f32,
        cy: f32,
        r_x: f32,
        r_y: f32,
    ) -> bool;
    pub fn noesis_drawing_draw_geometry(
        context: *mut c_void,
        brush: *mut c_void,
        pen: *mut c_void,
        geometry: *mut c_void,
    ) -> bool;
    pub fn noesis_drawing_draw_text(
        context: *mut c_void,
        formatted_text: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool;
    pub fn noesis_drawing_draw_mesh(
        context: *mut c_void,
        brush: *mut c_void,
        mesh: *mut c_void,
    ) -> bool;
    pub fn noesis_drawing_draw_image(
        context: *mut c_void,
        image_source: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool;
    pub fn noesis_drawing_pop(context: *mut c_void) -> bool;
    pub fn noesis_drawing_push_clip(context: *mut c_void, geometry: *mut c_void) -> bool;
    pub fn noesis_drawing_push_transform(context: *mut c_void, transform: *mut c_void) -> bool;
    pub fn noesis_drawing_push_blending_mode(context: *mut c_void, mode: i32) -> bool;
}

// ── MeshData + Mesh element ──────────────────────────────────────────────────
//
// `_create` entrypoints return a `+1`-owned BaseComponent the Rust handle
// releases on Drop (see src/mesh.rs). Vertex / UV buffers are interleaved
// `(x, y)` / `(u, v)` float pairs (`2 * count` floats); the index buffer is
// 16-bit. `mesh_get_data` / `mesh_get_brush` return BORROWED pointers (no +1).
unsafe extern "C" {
    pub fn noesis_mesh_data_create() -> *mut c_void;
    pub fn noesis_mesh_data_set_vertices(mesh: *mut c_void, xy: *const f32, count: u32) -> bool;
    pub fn noesis_mesh_data_get_vertices(mesh: *mut c_void, out_xy: *mut f32, count: u32) -> bool;
    pub fn noesis_mesh_data_set_uvs(mesh: *mut c_void, uv: *const f32, count: u32) -> bool;
    pub fn noesis_mesh_data_get_uvs(mesh: *mut c_void, out_uv: *mut f32, count: u32) -> bool;
    pub fn noesis_mesh_data_set_indices(mesh: *mut c_void, indices: *const u16, count: u32)
    -> bool;
    pub fn noesis_mesh_data_get_indices(
        mesh: *mut c_void,
        out_indices: *mut u16,
        count: u32,
    ) -> bool;
    pub fn noesis_mesh_data_set_bounds(mesh: *mut c_void, x: f32, y: f32, w: f32, h: f32) -> bool;
    pub fn noesis_mesh_data_get_bounds(mesh: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_mesh_create() -> *mut c_void;
    pub fn noesis_mesh_set_data(mesh: *mut c_void, data: *mut c_void) -> bool;
    pub fn noesis_mesh_get_data(mesh: *mut c_void) -> *mut c_void;
    pub fn noesis_mesh_set_brush(mesh: *mut c_void, brush: *mut c_void) -> bool;
    pub fn noesis_mesh_get_brush(mesh: *mut c_void) -> *mut c_void;
}

/// Mirror of `noesis_value_converter_vtable` in `cpp/noesis_shim.h`. Both fn
/// pointers receive the `userdata` passed to [`noesis_value_converter_create`],
/// the borrowed boxed `value` / `parameter` (`BaseComponent*`, may be null), an
/// opaque `target_type` (`const Noesis::Type*`), and an out-slot that takes a
/// `+1`-owned `BaseComponent*` (ownership transfers to Noesis). Return `true`
/// when a value was produced (`*out_result` may be null for a null value),
/// `false` for `UnsetValue`.
#[repr(C)]
pub struct ValueConverterVTable {
    pub convert: unsafe extern "C" fn(
        userdata: *mut c_void,
        value: *mut c_void,
        target_type: *const c_void,
        parameter: *mut c_void,
        out_result: *mut *mut c_void,
    ) -> bool,
    pub convert_back: unsafe extern "C" fn(
        userdata: *mut c_void,
        value: *mut c_void,
        target_type: *const c_void,
        parameter: *mut c_void,
        out_result: *mut *mut c_void,
    ) -> bool,
}

/// Free callback invoked exactly once when the underlying `RustValueConverter`
/// is finally destroyed (last reference released). Drops the boxed handler
/// whose ownership transferred to C++ at
/// [`noesis_value_converter_create`].
pub type ValueConverterFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Mirror of `noesis_template_selector_vtable` in `cpp/noesis_resources.cpp`.
/// `select` receives the `userdata` passed to
/// [`noesis_templates_selector_create`], the borrowed `item`
/// (`BaseComponent*`, may be null), and the borrowed `container`
/// (`DependencyObject*`, may be null). It returns a **borrowed**
/// `Noesis::DataTemplate*` (the selector keeps its candidate templates alive) or
/// null to select no template.
#[repr(C)]
pub struct TemplateSelectorVTable {
    pub select: unsafe extern "C" fn(
        userdata: *mut c_void,
        item: *mut c_void,
        container: *mut c_void,
    ) -> *mut c_void,
}

/// Free callback invoked exactly once when the underlying
/// `RustDataTemplateSelector` is finally destroyed. Drops the boxed handler whose
/// ownership transferred to C++ at [`noesis_templates_selector_create`].
pub type TemplateSelectorFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Callback for a `TwoWay` / `OneWayToSource` write back to a plain-VM reflected
/// property. Mirrors `noesis_plain_set_fn`. `instance` is the borrowed
/// `RustPlainVm*`, `prop_index` the dense index from
/// [`noesis_plain_vm_register_property`], `boxed_value` the borrowed boxed
/// `BaseComponent*` the UI pushed (may be null).
pub type PlainSetFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    instance: *mut c_void,
    prop_index: u32,
    boxed_value: *mut c_void,
);

/// Free callback invoked exactly once when a plain-VM registration's refcount
/// hits zero. Drops the boxed handler whose ownership transferred to C++ at
/// [`noesis_plain_vm_register`].
pub type PlainFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Mirror of `noesis_multi_value_converter_vtable` in `cpp/noesis_shim.h`.
/// `convert` receives the `userdata`, an array of `count` borrowed boxed
/// `BaseComponent*` (`values`, each may be null), an opaque `target_type`, the
/// borrowed `parameter`, and an out-slot taking a `+1`-owned `BaseComponent*`.
#[repr(C)]
pub struct MultiValueConverterVTable {
    pub convert: unsafe extern "C" fn(
        userdata: *mut c_void,
        values: *const *mut c_void,
        count: u32,
        target_type: *const c_void,
        parameter: *mut c_void,
        out_result: *mut *mut c_void,
    ) -> bool,
}

/// Free callback invoked exactly once when the underlying
/// `RustMultiValueConverter` is finally destroyed. Drops the boxed handler whose
/// ownership transferred to C++ at [`noesis_multi_value_converter_create`].
pub type MultiValueConverterFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Mirror of `noesis_command_vtable` in `cpp/noesis_shim.h`. Both fn
/// pointers receive the `userdata` passed to [`noesis_command_create`] and
/// the borrowed command-parameter `BaseComponent*` (`param`, may be null).
#[repr(C)]
pub struct CommandVTable {
    pub can_execute: unsafe extern "C" fn(userdata: *mut c_void, param: *mut c_void) -> bool,
    pub execute: unsafe extern "C" fn(userdata: *mut c_void, param: *mut c_void),
}

/// Free callback invoked exactly once when the underlying `RustCommand` is
/// finally destroyed (last reference released). Drops the boxed handler whose
/// ownership transferred to C++ at [`noesis_command_create`]. Reused for the
/// [`CommandBinding`](crate::commands::CommandBinding) bridge, same shape and
/// contract.
pub type CommandFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// [`CommandBinding`](crate::commands::CommandBinding) `Executed` callback: run
/// the action. `parameter` is the borrowed command parameter (may be NULL).
/// Fires on the view-driving thread inside the input/command route.
pub type CmdExecutedFn = unsafe extern "C" fn(userdata: *mut c_void, parameter: *mut c_void);

/// [`CommandBinding`](crate::commands::CommandBinding) `CanExecute` callback:
/// return whether the command may run now. `parameter` borrowed; may be NULL.
pub type CmdCanExecuteFn =
    unsafe extern "C" fn(userdata: *mut c_void, parameter: *mut c_void) -> bool;

/// Free callback invoked exactly once per registered markup extension
/// when its underlying C++ `MarkupClassData` is finally freed. Same shape
/// and contract as [`ClassFreeFn`]; see that type's docs.
pub type MarkupFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Callback invoked when a registered `MarkupExtension`'s `ProvideValue` runs
/// during XAML parse. `key` is the `ContentProperty` value the parser set on
/// the extension (the bit between `{aor:Localize` and `}`); see
/// `cpp/noesis_shim.h` for the output-slot contract.
pub type MarkupProvideFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    key: *const c_char,
    out_string: *mut *const c_char,
    out_component: *mut *mut c_void,
) -> bool;

/// C callback invoked when a subscribed `BaseButton::Click` fires. See
/// `cpp/noesis_shim.h` for the threading contract: the callback runs on
/// whatever thread is driving the view, so keep work small.
pub type ClickFn = unsafe extern "C" fn(userdata: *mut c_void);

/// C callback invoked when a subscribed `UIElement::KeyDown` fires.
///
/// `key` is the raw `Noesis::Key` ordinal (mirror in `view::Key`).
/// `out_handled` is a borrowed pointer the C++ side pre-clears to `false`;
/// writing `true` through it sets `KeyEventArgs::handled` so the routed
/// event stops propagating. Same threading contract as [`ClickFn`].
pub type KeyDownFn = unsafe extern "C" fn(userdata: *mut c_void, key: i32, out_handled: *mut bool);

/// C callback invoked when a subscribed routed event fires (the generic
/// `noesis_subscribe_event` path).
///
/// `args` is an opaque handle to the live event arguments; pass it to the
/// `noesis_*_args_*` accessors to read typed fields. It is valid only for
/// the duration of the call. `out_handled` is pre-seeded with the event's
/// current handled state; writing `true` marks the routed event handled.
/// Same threading contract as [`ClickFn`].
pub type RoutedEventFn =
    unsafe extern "C" fn(userdata: *mut c_void, args: *const c_void, out_handled: *mut bool);

/// C callback invoked when a subscribed `DataObject.Copying` / `.Pasting`
/// event fires (the `noesis_routed_events_add_*_handler` path).
///
/// `data_object` is a borrowed `BaseComponent*` (the clipboard data object,
/// may be null). `is_drag_drop` is true when the copy/paste originates from a
/// drag-drop rather than the clipboard. `out_cancel` is pre-seeded with the
/// current cancel state; writing `true` cancels the copy/paste. Same threading
/// contract as [`ClickFn`].
pub type DataObjectFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    data_object: *mut c_void,
    is_drag_drop: bool,
    out_cancel: *mut bool,
);

/// C callback fired on each view-timer tick (the `noesis_view_create_timer`
/// path). Returns the next interval in milliseconds, or `0` to stop the timer.
/// Fires from inside `IView::Update` on the view-driving thread, same
/// threading contract as [`ClickFn`].
pub type TimerFn = unsafe extern "C" fn(userdata: *mut c_void) -> u32;

/// C callback invoked exactly once when a view-timer token is cancelled (the
/// C++ `RustTimer` destroyed). Frees the donated `userdata`. Mirrors
/// [`CommandFreeFn`].
pub type TimerFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// C callback fired on each `IView::Rendering` event (the
/// `noesis_view_add_rendering_handler` path). `view` is the borrowed `IView*`
/// raising the event (do not release). Fires on the view-driving thread, same
/// threading contract as [`TimerFn`].
pub type RenderingFn = unsafe extern "C" fn(userdata: *mut c_void, view: *mut c_void);

/// Filter callback for `noesis_visual_hit_test_filtered`: called per visual
/// as the tree is walked. `visual` is a BORROWED `Visual*` (valid only for the
/// call). Returns a `HitTestFilterBehavior` discriminant. Runs synchronously
/// inside the hit-test call on the view-driving thread.
pub type HitFilterFn = unsafe extern "C" fn(userdata: *mut c_void, visual: *mut c_void) -> i32;

/// Result callback for `noesis_visual_hit_test_filtered`: called per hit.
/// `visual` is the BORROWED hit `Visual*`. Returns a `HitTestResultBehavior`
/// discriminant.
pub type HitResultFn = unsafe extern "C" fn(userdata: *mut c_void, visual: *mut c_void) -> i32;

/// Enumeration callback for `noesis_name_scope_enum`: called per registered
/// (name, object) pair. Both pointers are BORROWED (valid only for the call).
pub type NameScopeEnumFn =
    unsafe extern "C" fn(userdata: *mut c_void, name: *const c_char, obj: *mut c_void);

/// C callback invoked exactly once when a Rendering handler token is removed
/// (the C++ handler destroyed). Frees the donated `userdata`. Mirrors
/// [`TimerFreeFn`].
pub type RenderingFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

// ────────────────────────────────────────────────────────────────────────────
// Custom XAML class registration. See cpp/noesis_shim.h for the
// per-type value layout convention each variant of `PropType` enforces.
// ────────────────────────────────────────────────────────────────────────────

/// Base type the trampoline subclass derives from. Each variant maps to a
/// sibling `Rust*` trampoline subclass on the C++ side (all share the synthetic
/// `TypeClass` + DP machinery). All derive transitively from `FrameworkElement`,
/// so all participate in layout (`MeasureOverride`/`ArrangeOverride`).
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClassBase {
    ContentControl = 0,
    Control = 1,
    FrameworkElement = 2,
    UserControl = 3,
    Panel = 4,
    Decorator = 5,
    /// A custom `Noesis::Freezable` (a `DependencyObject` with freeze/clone
    /// semantics, NOT a `UIElement`): custom DPs work, but there is no layout /
    /// render / routed-event surface. The other `Animatable` subtrees
    /// (`Brush`/`Geometry`/`Transform`/`Effect`) are not subclassable this way.
    Freezable = 6,
}

/// FFI value-type tag. The buffer layout for `value_ptr` / `default_ptr` /
/// `out_value` is determined by this tag; see the per-variant comments in
/// `cpp/noesis_shim.h` for exact byte conventions.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum PropType {
    Int32 = 0,
    Float = 1,
    Double = 2,
    Bool = 3,
    String = 4,
    Thickness = 5,
    Color = 6,
    Rect = 7,
    ImageSource = 8,
    BaseComponent = 9,
    UInt32 = 10,
    /// `Noesis::Point` value DP. `value_ptr` is `const float[2]` (x, y).
    Point = 11,
    /// `Noesis::Size` value DP. `value_ptr` is `const float[2]` (width, height).
    Size = 12,
    /// `Noesis::Vector2` value DP. `value_ptr` is `const float[2]` (x, y).
    Vector = 13,
    /// Runtime-enum-typed DP: int32 storage; register via
    /// `noesis_class_register_enum_property` (it needs the enum type name).
    Enum = 14,
}

/// Property-changed callback. Fired from inside Noesis's property pump
/// (typically the main thread during XAML parse + layout + input). `instance`
/// is the C++ object pointer (stable for the instance's lifetime); see
/// `cpp/noesis_shim.h` for the per-`PropType` layout of `value_ptr`.
pub type PropChangedFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    instance: *mut c_void,
    prop_index: u32,
    value_ptr: *const c_void,
);

/// Free callback invoked exactly once per registered class when the
/// underlying C++ `ClassData` is finally freed (either at
/// `noesis_class_unregister` if no instances exist, or deferred to the
/// last live instance's destruction). Receives the `userdata` passed at
/// registration; the Rust trampoline drops the boxed handler. Ownership
/// of `userdata` transfers to the C++ side at register time.
pub type ClassFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

// ────────────────────────────────────────────────────────────────────────────
// Animation & timing. See cpp/noesis_animation.cpp.
// Every `*_create` returns a +1-owned BaseComponent* (released by the owning
// Rust handle's Drop via noesis_base_component_release).
// ────────────────────────────────────────────────────────────────────────────
unsafe extern "C" {
    // Storyboard
    pub fn noesis_storyboard_create() -> *mut c_void;
    pub fn noesis_storyboard_add_child(sb: *mut c_void, timeline: *mut c_void) -> bool;
    pub fn noesis_storyboard_child_count(sb: *mut c_void) -> i32;
    pub fn noesis_storyboard_set_target_name(timeline: *mut c_void, name: *const c_char) -> bool;
    pub fn noesis_storyboard_set_target_property(
        timeline: *mut c_void,
        path: *const c_char,
    ) -> bool;
    pub fn noesis_storyboard_set_target(timeline: *mut c_void, target: *mut c_void) -> bool;
    pub fn noesis_storyboard_begin(sb: *mut c_void, fe: *mut c_void, controllable: bool) -> bool;
    pub fn noesis_storyboard_begin_handoff(
        sb: *mut c_void,
        fe: *mut c_void,
        handoff: i32,
        controllable: bool,
    ) -> bool;
    pub fn noesis_storyboard_pause(sb: *mut c_void, fe: *mut c_void) -> bool;
    pub fn noesis_storyboard_resume(sb: *mut c_void, fe: *mut c_void) -> bool;
    pub fn noesis_storyboard_stop(sb: *mut c_void, fe: *mut c_void) -> bool;
    pub fn noesis_storyboard_seek(sb: *mut c_void, fe: *mut c_void, seconds: f64) -> bool;
    pub fn noesis_storyboard_is_playing(sb: *mut c_void, fe: *mut c_void) -> bool;
    pub fn noesis_storyboard_is_paused(sb: *mut c_void, fe: *mut c_void) -> bool;

    // Timeline common knobs
    pub fn noesis_timeline_set_duration_seconds(tl: *mut c_void, seconds: f64) -> bool;
    pub fn noesis_timeline_set_duration_auto(tl: *mut c_void) -> bool;
    pub fn noesis_timeline_set_duration_forever(tl: *mut c_void) -> bool;
    pub fn noesis_timeline_get_duration_seconds(tl: *mut c_void) -> f64;
    pub fn noesis_timeline_set_begin_time_seconds(tl: *mut c_void, seconds: f64) -> bool;
    pub fn noesis_timeline_set_auto_reverse(tl: *mut c_void, value: bool) -> bool;
    pub fn noesis_timeline_set_speed_ratio(tl: *mut c_void, value: f32) -> bool;
    pub fn noesis_timeline_set_fill_behavior(tl: *mut c_void, behavior: i32) -> bool;
    pub fn noesis_timeline_set_repeat_count(tl: *mut c_void, count: f32) -> bool;
    pub fn noesis_timeline_set_repeat_duration(tl: *mut c_void, seconds: f64) -> bool;
    pub fn noesis_timeline_set_repeat_forever(tl: *mut c_void) -> bool;

    // From/To/By animations
    pub fn noesis_double_animation_create() -> *mut c_void;
    pub fn noesis_double_animation_set_from(anim: *mut c_void, has: bool, v: f32) -> bool;
    pub fn noesis_double_animation_set_to(anim: *mut c_void, has: bool, v: f32) -> bool;
    pub fn noesis_double_animation_set_by(anim: *mut c_void, has: bool, v: f32) -> bool;

    pub fn noesis_color_animation_create() -> *mut c_void;
    pub fn noesis_color_animation_set_from(anim: *mut c_void, has: bool, color: *const f32)
    -> bool;
    pub fn noesis_color_animation_set_to(anim: *mut c_void, has: bool, color: *const f32) -> bool;
    pub fn noesis_color_animation_set_by(anim: *mut c_void, has: bool, color: *const f32) -> bool;

    pub fn noesis_thickness_animation_create() -> *mut c_void;
    pub fn noesis_thickness_animation_set_from(anim: *mut c_void, has: bool, t: *const f32)
    -> bool;
    pub fn noesis_thickness_animation_set_to(anim: *mut c_void, has: bool, t: *const f32) -> bool;
    pub fn noesis_thickness_animation_set_by(anim: *mut c_void, has: bool, t: *const f32) -> bool;

    pub fn noesis_point_animation_create() -> *mut c_void;
    pub fn noesis_point_animation_set_from(anim: *mut c_void, has: bool, x: f32, y: f32) -> bool;
    pub fn noesis_point_animation_set_to(anim: *mut c_void, has: bool, x: f32, y: f32) -> bool;
    pub fn noesis_point_animation_set_by(anim: *mut c_void, has: bool, x: f32, y: f32) -> bool;

    pub fn noesis_animation_set_easing_function(anim: *mut c_void, easing: *mut c_void) -> bool;

    // Easing functions
    pub fn noesis_easing_function_create(kind: i32, mode: i32) -> *mut c_void;
    pub fn noesis_easing_function_set_amplitude(easing: *mut c_void, value: f32) -> bool;
    pub fn noesis_easing_function_set_power(easing: *mut c_void, value: f32) -> bool;
    pub fn noesis_easing_function_set_exponent(easing: *mut c_void, value: f32) -> bool;
    pub fn noesis_easing_function_set_oscillations(easing: *mut c_void, value: i32) -> bool;
    pub fn noesis_easing_function_set_springiness(easing: *mut c_void, value: f32) -> bool;

    // Key-frame animations
    pub fn noesis_double_animation_keyframes_create() -> *mut c_void;
    pub fn noesis_double_animation_add_keyframe(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        value: f32,
        extra: *mut c_void,
    ) -> bool;
    pub fn noesis_color_animation_keyframes_create() -> *mut c_void;
    pub fn noesis_color_animation_add_keyframe(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        color: *const f32,
        extra: *mut c_void,
    ) -> bool;

    // Storyboard-less direct animation
    pub fn noesis_animation_begin_on(
        anim: *mut c_void,
        target: *mut c_void,
        dp_name: *const c_char,
        handoff: i32,
    ) -> bool;

    // Rect / Size From-To animations
    pub fn noesis_animation_rect_animation_create() -> *mut c_void;
    pub fn noesis_animation_rect_animation_set_from(
        anim: *mut c_void,
        has: bool,
        r: *const f32,
    ) -> bool;
    pub fn noesis_animation_rect_animation_set_to(
        anim: *mut c_void,
        has: bool,
        r: *const f32,
    ) -> bool;
    pub fn noesis_animation_rect_animation_set_by(
        anim: *mut c_void,
        has: bool,
        r: *const f32,
    ) -> bool;
    pub fn noesis_animation_rect_animation_get_from(anim: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_animation_rect_animation_get_to(anim: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_animation_rect_animation_get_by(anim: *mut c_void, out: *mut f32) -> bool;

    pub fn noesis_animation_size_animation_create() -> *mut c_void;
    pub fn noesis_animation_size_animation_set_from(
        anim: *mut c_void,
        has: bool,
        s: *const f32,
    ) -> bool;
    pub fn noesis_animation_size_animation_set_to(
        anim: *mut c_void,
        has: bool,
        s: *const f32,
    ) -> bool;
    pub fn noesis_animation_size_animation_set_by(
        anim: *mut c_void,
        has: bool,
        s: *const f32,
    ) -> bool;
    pub fn noesis_animation_size_animation_get_from(anim: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_animation_size_animation_get_to(anim: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_animation_size_animation_get_by(anim: *mut c_void, out: *mut f32) -> bool;

    // Int16 / Int32 From-To animations (value crosses as i32)
    pub fn noesis_animation_int16_animation_create() -> *mut c_void;
    pub fn noesis_animation_int16_animation_set_from(anim: *mut c_void, has: bool, v: i32) -> bool;
    pub fn noesis_animation_int16_animation_set_to(anim: *mut c_void, has: bool, v: i32) -> bool;
    pub fn noesis_animation_int16_animation_set_by(anim: *mut c_void, has: bool, v: i32) -> bool;
    pub fn noesis_animation_int16_animation_get_from(anim: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_animation_int16_animation_get_to(anim: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_animation_int16_animation_get_by(anim: *mut c_void, out: *mut i32) -> bool;

    pub fn noesis_animation_int32_animation_create() -> *mut c_void;
    pub fn noesis_animation_int32_animation_set_from(anim: *mut c_void, has: bool, v: i32) -> bool;
    pub fn noesis_animation_int32_animation_set_to(anim: *mut c_void, has: bool, v: i32) -> bool;
    pub fn noesis_animation_int32_animation_set_by(anim: *mut c_void, has: bool, v: i32) -> bool;
    pub fn noesis_animation_int32_animation_get_from(anim: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_animation_int32_animation_get_to(anim: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_animation_int32_animation_get_by(anim: *mut c_void, out: *mut i32) -> bool;

    // Int64 From-To animation (value crosses as i64)
    pub fn noesis_animation_int64_animation_create() -> *mut c_void;
    pub fn noesis_animation_int64_animation_set_from(anim: *mut c_void, has: bool, v: i64) -> bool;
    pub fn noesis_animation_int64_animation_set_to(anim: *mut c_void, has: bool, v: i64) -> bool;
    pub fn noesis_animation_int64_animation_set_by(anim: *mut c_void, has: bool, v: i64) -> bool;
    pub fn noesis_animation_int64_animation_get_from(anim: *mut c_void, out: *mut i64) -> bool;
    pub fn noesis_animation_int64_animation_get_to(anim: *mut c_void, out: *mut i64) -> bool;
    pub fn noesis_animation_int64_animation_get_by(anim: *mut c_void, out: *mut i64) -> bool;

    // Rect / Size key-frame animations. `kind`: 0 Discrete, 1 Linear, 2 Easing
    // (`extra` = EasingFunctionBase*), 3 Spline (`extra` = KeySpline*).
    pub fn noesis_animation_rect_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_rect_keyframes_add(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        r: *const f32,
        extra: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_rect_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_rect_keyframes_get_value(
        anim: *mut c_void,
        index: i32,
        out: *mut f32,
    ) -> bool;
    pub fn noesis_animation_rect_keyframes_get_key_time(anim: *mut c_void, index: i32) -> f64;

    pub fn noesis_animation_size_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_size_keyframes_add(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        s: *const f32,
        extra: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_size_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_size_keyframes_get_value(
        anim: *mut c_void,
        index: i32,
        out: *mut f32,
    ) -> bool;
    pub fn noesis_animation_size_keyframes_get_key_time(anim: *mut c_void, index: i32) -> f64;

    // Int16 / Int32 key-frame animations (value crosses as i32)
    pub fn noesis_animation_int16_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_int16_keyframes_add(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        value: i32,
        extra: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_int16_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_int16_keyframes_get_value(
        anim: *mut c_void,
        idx: i32,
        out: *mut i32,
    ) -> bool;
    pub fn noesis_animation_int16_keyframes_get_key_time(anim: *mut c_void, idx: i32) -> f64;

    pub fn noesis_animation_int32_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_int32_keyframes_add(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        value: i32,
        extra: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_int32_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_int32_keyframes_get_value(
        anim: *mut c_void,
        idx: i32,
        out: *mut i32,
    ) -> bool;
    pub fn noesis_animation_int32_keyframes_get_key_time(anim: *mut c_void, idx: i32) -> f64;

    // Int64 key-frame animation (value crosses as i64)
    pub fn noesis_animation_int64_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_int64_keyframes_add(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        value: i64,
        extra: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_int64_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_int64_keyframes_get_value(
        anim: *mut c_void,
        idx: i32,
        out: *mut i64,
    ) -> bool;
    pub fn noesis_animation_int64_keyframes_get_key_time(anim: *mut c_void, idx: i32) -> f64;

    // Object key-frame animation (discrete only; value is a borrowed BaseComponent*)
    pub fn noesis_animation_object_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_object_keyframes_add(
        anim: *mut c_void,
        key_time_seconds: f64,
        value: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_object_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_object_keyframes_get_value(
        anim: *mut c_void,
        index: i32,
    ) -> *mut c_void;
    pub fn noesis_animation_object_keyframes_get_key_time(anim: *mut c_void, index: i32) -> f64;

    // Matrix key-frame animation (discrete only; matrix is a [m00,m01,m10,m11,m20,m21] float[6])
    pub fn noesis_animation_matrix_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_matrix_keyframes_add(
        anim: *mut c_void,
        key_time_seconds: f64,
        m: *const f32,
    ) -> bool;
    pub fn noesis_animation_matrix_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_matrix_keyframes_get_value(
        anim: *mut c_void,
        index: i32,
        out: *mut f32,
    ) -> bool;
    pub fn noesis_animation_matrix_keyframes_get_key_time(anim: *mut c_void, index: i32) -> f64;

    // KeySpline
    pub fn noesis_animation_keyspline_create(c1x: f32, c1y: f32, c2x: f32, c2y: f32)
    -> *mut c_void;
    pub fn noesis_animation_keyspline_set_control_point1(ks: *mut c_void, x: f32, y: f32) -> bool;
    pub fn noesis_animation_keyspline_set_control_point2(ks: *mut c_void, x: f32, y: f32) -> bool;
    pub fn noesis_animation_keyspline_get_control_point1(ks: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_animation_keyspline_get_control_point2(ks: *mut c_void, out: *mut f32) -> bool;

    // BeginStoryboard trigger action
    pub fn noesis_animation_begin_storyboard_create() -> *mut c_void;
    pub fn noesis_animation_begin_storyboard_set_storyboard(
        bs: *mut c_void,
        sb: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_begin_storyboard_get_storyboard(bs: *mut c_void) -> *mut c_void;
    pub fn noesis_animation_begin_storyboard_set_handoff(bs: *mut c_void, behavior: i32) -> bool;
    pub fn noesis_animation_begin_storyboard_get_handoff(bs: *mut c_void) -> i32;
    pub fn noesis_animation_begin_storyboard_set_name(bs: *mut c_void, name: *const c_char)
    -> bool;
    pub fn noesis_animation_begin_storyboard_get_name(bs: *mut c_void) -> *const c_char;

    // Point key-frame animation
    pub fn noesis_animation_point_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_point_keyframes_add(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        p: *const f32,
        extra: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_point_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_point_keyframes_get_value(
        anim: *mut c_void,
        index: i32,
        out: *mut f32,
    ) -> bool;
    pub fn noesis_animation_point_keyframes_get_key_time(anim: *mut c_void, index: i32) -> f64;

    // Thickness key-frame animation
    pub fn noesis_animation_thickness_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_thickness_keyframes_add(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        t: *const f32,
        extra: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_thickness_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_thickness_keyframes_get_value(
        anim: *mut c_void,
        index: i32,
        out: *mut f32,
    ) -> bool;
    pub fn noesis_animation_thickness_keyframes_get_key_time(anim: *mut c_void, index: i32) -> f64;

    // Boolean key-frame animation (discrete only)
    pub fn noesis_animation_boolean_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_boolean_keyframes_add(
        anim: *mut c_void,
        key_time_seconds: f64,
        value: bool,
    ) -> bool;
    pub fn noesis_animation_boolean_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_boolean_keyframes_get_value(
        anim: *mut c_void,
        index: i32,
        out: *mut bool,
    ) -> bool;
    pub fn noesis_animation_boolean_keyframes_get_key_time(anim: *mut c_void, index: i32) -> f64;

    // String key-frame animation (discrete only)
    pub fn noesis_animation_string_keyframes_create() -> *mut c_void;
    pub fn noesis_animation_string_keyframes_add(
        anim: *mut c_void,
        key_time_seconds: f64,
        value: *const c_char,
    ) -> bool;
    pub fn noesis_animation_string_keyframes_count(anim: *mut c_void) -> i32;
    pub fn noesis_animation_string_keyframes_get_value(
        anim: *mut c_void,
        index: i32,
    ) -> *const c_char;
    pub fn noesis_animation_string_keyframes_get_key_time(anim: *mut c_void, index: i32) -> f64;

    // ParallelTimeline (timeline group)
    pub fn noesis_animation_parallel_timeline_create() -> *mut c_void;
    pub fn noesis_animation_parallel_timeline_add_child(
        group: *mut c_void,
        timeline: *mut c_void,
    ) -> bool;
    pub fn noesis_animation_parallel_timeline_child_count(group: *mut c_void) -> i32;
}

// ── FormattedText measurement / layout ──────────────────────────────────────
//
// `_create` returns a `+1`-owned `FormattedText*` (src/formatted_text.rs
// releases it on Drop via noesis_base_component_release). Enum args are the
// NsGui/FontProperties.h + NsGui/TextProperties.h ordinals. See cpp/noesis_shim.h
// for the full contracts.
unsafe extern "C" {
    pub fn noesis_formatted_text_create(
        text: *const c_char,
        font_family: *const c_char,
        weight: i32,
        stretch: i32,
        style: i32,
        font_size: f32,
        flow_direction: i32,
        max_width: f32,
        max_height: f32,
        line_height: f32,
        text_alignment: i32,
        text_trimming: i32,
        foreground: *const f32,
    ) -> *mut c_void;
    pub fn noesis_formatted_text_get_bounds(ft: *mut c_void, out: *mut f32) -> bool;
    pub fn noesis_formatted_text_get_num_lines(ft: *mut c_void) -> i32;
    pub fn noesis_formatted_text_get_line_info(
        ft: *mut c_void,
        index: u32,
        out_num_glyphs: *mut u32,
        out_height: *mut f32,
        out_baseline: *mut f32,
    ) -> bool;
    pub fn noesis_formatted_text_is_empty(ft: *mut c_void, out: *mut bool) -> bool;
    pub fn noesis_formatted_text_has_visual_brush(ft: *mut c_void, out: *mut bool) -> bool;
    pub fn noesis_formatted_text_measure(
        ft: *mut c_void,
        alignment: i32,
        wrapping: i32,
        trimming: i32,
        max_width: f32,
        max_height: f32,
        line_height: f32,
        line_stacking: i32,
        flow_direction: i32,
        out_w: *mut f32,
        out_h: *mut f32,
    ) -> bool;
    pub fn noesis_formatted_text_get_glyph_position(
        ft: *mut c_void,
        ch_index: u32,
        after_char: bool,
        out_x: *mut f32,
        out_y: *mut f32,
    ) -> bool;
    pub fn noesis_formatted_text_hit_test(
        ft: *mut c_void,
        x: f32,
        y: f32,
        out_index: *mut u32,
        out_is_inside: *mut bool,
        out_is_trailing: *mut bool,
    ) -> bool;
}

// Reflection meta: custom enums / routed events / factory + string conversion.
// See cpp/noesis_shim.h for the full ownership + threading contracts.
// ────────────────────────────────────────────────────────────────────────────

/// One (name, value) pair of a runtime enum, mirroring
/// `noesis_enum_value` in `cpp/noesis_shim.h`. `name` is a borrowed C string
/// valid for the duration of the `noesis_register_enum` call.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct EnumValue {
    pub name: *const c_char,
    pub value: i32,
}

unsafe extern "C" {
    // (A) Custom enums
    pub fn noesis_register_enum(
        name: *const c_char,
        values: *const EnumValue,
        count: u32,
    ) -> *mut c_void;
    pub fn noesis_enum_value_from_name(
        enum_type: *const c_char,
        value_name: *const c_char,
        out_value: *mut i32,
    ) -> bool;
    pub fn noesis_enum_name_from_value(
        enum_type: *const c_char,
        value: i32,
        out_name: *mut *const c_char,
    ) -> bool;
    pub fn noesis_type_converter_from_string(
        type_name: *const c_char,
        str: *const c_char,
        out_boxed: *mut *mut c_void,
    ) -> bool;

    // (B) Custom routed events
    pub fn noesis_register_routed_event(
        type_name: *const c_char,
        event_name: *const c_char,
        strategy: i32,
    ) -> bool;
    pub fn noesis_raise_routed_event(element: *mut c_void, event_name: *const c_char) -> bool;

    // (C) Factory / component metadata
    pub fn noesis_factory_is_registered(name: *const c_char) -> bool;
    pub fn noesis_type_set_content_property(
        type_name: *const c_char,
        prop_name: *const c_char,
    ) -> bool;
    pub fn noesis_type_get_content_property(
        type_name: *const c_char,
        out_name: *mut *const c_char,
    ) -> bool;
    pub fn noesis_type_add_depends_on(type_name: *const c_char, prop_name: *const c_char) -> bool;
    pub fn noesis_type_get_depends_on(
        type_name: *const c_char,
        out_name: *mut *const c_char,
    ) -> bool;

    // TextBlock inline content model. Constructors hand out a +1
    // BaseComponent* (release via noesis_base_component_release).
    pub fn noesis_text_inlines_run_create(text: *const c_char) -> *mut c_void;
    pub fn noesis_text_inlines_span_create() -> *mut c_void;
    pub fn noesis_text_inlines_bold_create() -> *mut c_void;
    pub fn noesis_text_inlines_italic_create() -> *mut c_void;
    pub fn noesis_text_inlines_underline_create() -> *mut c_void;
    pub fn noesis_text_inlines_hyperlink_create() -> *mut c_void;
    pub fn noesis_text_inlines_line_break_create() -> *mut c_void;
    pub fn noesis_text_inlines_ui_container_create() -> *mut c_void;

    pub fn noesis_text_inlines_run_set_text(run: *mut c_void, text: *const c_char) -> bool;
    pub fn noesis_text_inlines_run_get_text(run: *mut c_void) -> *const c_char;

    pub fn noesis_text_inlines_hyperlink_set_navigate_uri(
        link: *mut c_void,
        uri: *const c_char,
    ) -> bool;
    pub fn noesis_text_inlines_hyperlink_get_navigate_uri(link: *mut c_void) -> *const c_char;

    pub fn noesis_text_inlines_inline_set_text_decorations(
        inl: *mut c_void,
        decorations: i32,
    ) -> bool;
    pub fn noesis_text_inlines_inline_get_text_decorations(inl: *mut c_void) -> i32;

    pub fn noesis_text_inlines_ui_container_set_child(
        container: *mut c_void,
        child: *mut c_void,
    ) -> bool;
    pub fn noesis_text_inlines_ui_container_get_child(container: *mut c_void) -> *mut c_void;

    pub fn noesis_text_inlines_text_block_get_inlines(text_block: *mut c_void) -> *mut c_void;
    pub fn noesis_text_inlines_span_get_inlines(span: *mut c_void) -> *mut c_void;

    pub fn noesis_text_inlines_collection_add(collection: *mut c_void, inl: *mut c_void) -> i32;
    pub fn noesis_text_inlines_collection_count(collection: *mut c_void) -> i32;
    pub fn noesis_text_inlines_collection_get(collection: *mut c_void, index: u32) -> *mut c_void;

    // Code-side element-tree construction: Decorator/Border Child,
    // Panel Children, Grid row/column definitions. Collections hand out a +1
    // BaseComponent* (release via noesis_base_component_release); get/get_at
    // return borrowed (no +1) pointers owned by the host.
    pub fn noesis_decorator_set_child(decorator: *mut c_void, child: *mut c_void) -> bool;
    pub fn noesis_decorator_get_child(decorator: *mut c_void) -> *mut c_void;

    pub fn noesis_panel_children_get(panel: *mut c_void) -> *mut c_void;
    pub fn noesis_panel_children_add(coll: *mut c_void, child: *mut c_void) -> i32;
    pub fn noesis_panel_children_insert(coll: *mut c_void, index: u32, child: *mut c_void) -> bool;
    pub fn noesis_panel_children_remove_at(coll: *mut c_void, index: u32) -> bool;
    pub fn noesis_panel_children_clear(coll: *mut c_void) -> bool;
    pub fn noesis_panel_children_count(coll: *mut c_void) -> i32;
    pub fn noesis_panel_children_get_at(coll: *mut c_void, index: u32) -> *mut c_void;

    pub fn noesis_grid_row_definition_create() -> *mut c_void;
    pub fn noesis_grid_column_definition_create() -> *mut c_void;
    pub fn noesis_grid_row_definition_set_height(def: *mut c_void, value: f32, unit: i32) -> bool;
    pub fn noesis_grid_row_definition_get_height(
        def: *mut c_void,
        out_value: *mut f32,
        out_unit: *mut i32,
    ) -> bool;
    pub fn noesis_grid_column_definition_set_width(def: *mut c_void, value: f32, unit: i32)
    -> bool;
    pub fn noesis_grid_column_definition_get_width(
        def: *mut c_void,
        out_value: *mut f32,
        out_unit: *mut i32,
    ) -> bool;

    pub fn noesis_grid_get_row_definitions(grid: *mut c_void) -> *mut c_void;
    pub fn noesis_grid_get_column_definitions(grid: *mut c_void) -> *mut c_void;
    pub fn noesis_definition_collection_add(coll: *mut c_void, def: *mut c_void) -> i32;
    pub fn noesis_definition_collection_insert(
        coll: *mut c_void,
        index: u32,
        def: *mut c_void,
    ) -> bool;
    pub fn noesis_definition_collection_remove_at(coll: *mut c_void, index: u32) -> bool;
    pub fn noesis_definition_collection_clear(coll: *mut c_void) -> bool;
    pub fn noesis_definition_collection_count(coll: *mut c_void) -> i32;
    pub fn noesis_definition_collection_get(coll: *mut c_void, index: u32) -> *mut c_void;
}
/// Coerce callback. Invoked inside Noesis's value pipeline when a
/// coerced DP's effective value is computed. `in_value` is the pre-coercion
/// value (per the DP's `PropType` layout); `out_value` is pre-initialized to a
/// copy of `in_value` and the implementation overwrites it with the coerced
/// result. Only scalar / Thickness / Color / Rect tags are coercible.
pub type CoerceFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    instance: *mut c_void,
    prop_index: u32,
    in_value: *const c_void,
    out_value: *mut c_void,
);

/// Layout vtable. The trampoline subclass's `MeasureOverride` /
/// `ArrangeOverride` forward into these. Sizes are in DIPs; `instance` is the
/// owning object's `BaseComponent*`. Implementations write the desired (measure)
/// / used (arrange) size to `out_w`/`out_h`. `#[repr(C)]` so the C++ struct
/// layout matches byte-for-byte.
#[repr(C)]
#[derive(Copy, Clone)]
pub struct LayoutVtable {
    pub measure: Option<
        unsafe extern "C" fn(
            userdata: *mut c_void,
            instance: *mut c_void,
            avail_w: f32,
            avail_h: f32,
            out_w: *mut f32,
            out_h: *mut f32,
        ),
    >,
    pub arrange: Option<
        unsafe extern "C" fn(
            userdata: *mut c_void,
            instance: *mut c_void,
            final_w: f32,
            final_h: f32,
            out_w: *mut f32,
            out_h: *mut f32,
        ),
    >,
}

/// Free callback for a donated layout `userdata` box. Mirrors [`ClassFreeFn`].
pub type LayoutFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Render callback. The trampoline subclass's `OnRender` override
/// forwards into this. `instance` is the owning object's `BaseComponent*`;
/// `context` is a borrowed `Noesis::DrawingContext*` valid only for the call.
/// Issue draw commands through the `noesis_drawing_*` entrypoints.
pub type RenderFn =
    unsafe extern "C" fn(userdata: *mut c_void, instance: *mut c_void, context: *mut c_void);

/// Free callback for a donated render `userdata` box. Mirrors [`ClassFreeFn`].
pub type RenderFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

// ── Test-only routed-event raisers ───────────────────────────────────────────
//
// Gated by the `test-utils` Cargo feature. Drag and manipulation events cannot
// be synthesized headlessly (a drag needs an OS pointer/drag loop; manipulation
// is promoted from a multi-frame touch stream under a live render pass). These
// helpers construct the real `DragEventArgs` / `Manipulation*EventArgs` with
// known field values and invoke `cb` exactly as the live dispatcher would, so
// the typed-arg accessors can be round-trip tested. `element` must be a live
// `UIElement*` (used as source/target so `GetPosition` resolves).
#[cfg(feature = "test-utils")]
unsafe extern "C" {
    pub fn noesis_routed_events_test_raise_drag(
        element: *mut c_void,
        cb: RoutedEventFn,
        userdata: *mut c_void,
    );
    pub fn noesis_routed_events_test_raise_manip_delta(
        element: *mut c_void,
        cb: RoutedEventFn,
        userdata: *mut c_void,
    );
    pub fn noesis_routed_events_test_raise_manip_completed(
        element: *mut c_void,
        cb: RoutedEventFn,
        userdata: *mut c_void,
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Input: finer control. Element-level capture, keyboard/focus
// state, focus traversal, FocusManager / KeyboardNavigation statics, and input
// gestures + bindings. See cpp/noesis_shim.h for the full contracts.
// ────────────────────────────────────────────────────────────────────────────
unsafe extern "C" {
    // Mouse / touch capture (element-level)
    pub fn noesis_ui_element_capture_mouse(element: *mut c_void) -> bool;
    pub fn noesis_ui_element_release_mouse_capture(element: *mut c_void);
    pub fn noesis_ui_element_get_is_mouse_captured(element: *mut c_void) -> bool;
    pub fn noesis_ui_element_capture_touch(element: *mut c_void, touch_device: u64) -> bool;
    pub fn noesis_ui_element_capture_mouse_mode(element: *mut c_void, mode: i32) -> bool;
    pub fn noesis_ui_element_get_mouse_captured(element: *mut c_void) -> *mut c_void;
    pub fn noesis_ui_element_get_mouse_position(
        element: *mut c_void,
        out_x: *mut f32,
        out_y: *mut f32,
    ) -> bool;

    // Keyboard state / modifiers
    pub fn noesis_ui_element_get_modifiers(element: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_ui_element_get_key_states(element: *mut c_void, key: i32, out: *mut i32) -> bool;
    pub fn noesis_ui_element_is_key_down(element: *mut c_void, key: i32) -> bool;
    pub fn noesis_ui_element_is_key_up(element: *mut c_void, key: i32) -> bool;
    pub fn noesis_ui_element_is_key_toggled(element: *mut c_void, key: i32) -> bool;
    pub fn noesis_ui_element_get_keyboard_focused(element: *mut c_void) -> *mut c_void;

    // Focus-state DPs
    pub fn noesis_ui_element_get_is_focused(element: *mut c_void) -> bool;
    pub fn noesis_ui_element_get_is_keyboard_focused(element: *mut c_void) -> bool;
    pub fn noesis_ui_element_get_is_keyboard_focus_within(element: *mut c_void) -> bool;

    // Focus engagement + traversal
    pub fn noesis_ui_element_focus_engage(element: *mut c_void, engage: bool) -> bool;
    pub fn noesis_ui_element_move_focus(
        element: *mut c_void,
        direction: i32,
        wrapped: bool,
    ) -> bool;
    pub fn noesis_ui_element_predict_focus(element: *mut c_void, direction: i32) -> *mut c_void;
    pub fn noesis_ui_element_predict_focus_name(
        element: *mut c_void,
        direction: i32,
    ) -> *const c_char;

    // FocusManager statics
    pub fn noesis_focus_manager_get_focused_element(scope: *mut c_void) -> *mut c_void;
    pub fn noesis_focus_manager_set_focused_element(
        scope: *mut c_void,
        element: *mut c_void,
    ) -> bool;
    pub fn noesis_focus_manager_get_is_focus_scope(element: *mut c_void) -> bool;
    pub fn noesis_focus_manager_set_is_focus_scope(element: *mut c_void, value: bool) -> bool;
    pub fn noesis_focus_manager_get_focus_scope(element: *mut c_void) -> *mut c_void;

    // KeyboardNavigation attached properties
    pub fn noesis_keyboard_navigation_get_tab_index(element: *mut c_void, out: *mut i32) -> bool;
    pub fn noesis_keyboard_navigation_set_tab_index(element: *mut c_void, value: i32) -> bool;
    pub fn noesis_keyboard_navigation_get_is_tab_stop(element: *mut c_void, out: *mut bool)
    -> bool;
    pub fn noesis_keyboard_navigation_set_is_tab_stop(element: *mut c_void, value: bool) -> bool;
    pub fn noesis_keyboard_navigation_get_tab_navigation(
        element: *mut c_void,
        out: *mut i32,
    ) -> bool;
    pub fn noesis_keyboard_navigation_set_tab_navigation(element: *mut c_void, mode: i32) -> bool;
    pub fn noesis_keyboard_navigation_get_control_tab_navigation(
        element: *mut c_void,
        out: *mut i32,
    ) -> bool;
    pub fn noesis_keyboard_navigation_set_control_tab_navigation(
        element: *mut c_void,
        mode: i32,
    ) -> bool;
    pub fn noesis_keyboard_navigation_get_directional_navigation(
        element: *mut c_void,
        out: *mut i32,
    ) -> bool;
    pub fn noesis_keyboard_navigation_set_directional_navigation(
        element: *mut c_void,
        mode: i32,
    ) -> bool;
    pub fn noesis_keyboard_navigation_get_accepts_return(
        element: *mut c_void,
        out: *mut bool,
    ) -> bool;
    pub fn noesis_keyboard_navigation_set_accepts_return(element: *mut c_void, value: bool)
    -> bool;

    // Input gestures + bindings
    pub fn noesis_key_gesture_create(key: i32, modifiers: i32) -> *mut c_void;
    pub fn noesis_mouse_gesture_create(action: i32, modifiers: i32) -> *mut c_void;
    pub fn noesis_key_binding_create(command: *mut c_void, key: i32, modifiers: i32)
    -> *mut c_void;
    pub fn noesis_mouse_binding_create(
        command: *mut c_void,
        action: i32,
        modifiers: i32,
    ) -> *mut c_void;
    pub fn noesis_input_binding_create(command: *mut c_void, gesture: *mut c_void) -> *mut c_void;
    pub fn noesis_ui_element_add_input_binding(element: *mut c_void, binding: *mut c_void) -> bool;
}

// ────────────────────────────────────────────────────────────────────────────
// Diagnostics: error / assert handlers + memory queries. See cpp/noesis_shim.h.
// All NsCore kernel functions; kernel must be up.
// ────────────────────────────────────────────────────────────────────────────

/// Binary-compatible with `Noesis::ErrorContext` (and the C
/// `noesis_error_context`). Surfaces the offending uri/line/column for e.g.
/// XAML parse errors carried by the per-thread `ErrorHandler2`.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct ErrorContext {
    pub uri: *const c_char,
    pub line: u32,
    pub column: u32,
}

/// Global error handler trampoline signature (no per-call context).
pub type ErrorFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    message: *const c_char,
    fatal: bool,
);

/// Global assert handler trampoline signature. Return `true` to request a
/// debug break.
pub type AssertFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    expr: *const c_char,
) -> bool;

/// Per-thread error handler (`Noesis::ErrorHandler2`) trampoline signature.
/// `context` is null when none was supplied.
pub type Error2Fn = unsafe extern "C" fn(
    file: *const c_char,
    line: u32,
    message: *const c_char,
    fatal: bool,
    context: *mut ErrorContext,
    userdata: *mut c_void,
);

unsafe extern "C" {
    pub fn noesis_set_error_handler(
        cb: Option<ErrorFn>,
        userdata: *mut c_void,
        out_prev_cb: *mut Option<ErrorFn>,
        out_prev_user: *mut *mut c_void,
    );
    pub fn noesis_set_assert_handler(
        cb: Option<AssertFn>,
        userdata: *mut c_void,
        out_prev_cb: *mut Option<AssertFn>,
        out_prev_user: *mut *mut c_void,
    );
    pub fn noesis_set_thread_error_handler(
        handler: Option<Error2Fn>,
        userdata: *mut c_void,
        out_prev_handler: *mut Option<Error2Fn>,
        out_prev_user: *mut *mut c_void,
    );
    pub fn noesis_invoke_error_handler(
        file: *const c_char,
        line: u32,
        fatal: bool,
        has_context: bool,
        uri: *const c_char,
        ctx_line: u32,
        ctx_col: u32,
        message: *const c_char,
    );
    pub fn noesis_invoke_assert_handler(
        file: *const c_char,
        line: u32,
        expr: *const c_char,
    ) -> bool;

    pub fn noesis_get_allocated_memory() -> u32;
    pub fn noesis_get_allocated_memory_accum() -> u32;
    pub fn noesis_get_allocations_count() -> u32;
}
