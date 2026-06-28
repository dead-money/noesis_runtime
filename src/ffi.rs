//! Hand-written declarations matching `cpp/noesis_shim.h`.
//!
//! When the shim grows past ~30 functions, switch to `bindgen` driven from a
//! `wrapper.h`. For Phase 0 the surface is too small to justify the build dep.

use std::os::raw::{c_char, c_void};

#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Trace = 0,
    Debug = 1,
    Info = 2,
    Warning = 3,
    Error = 4,
}

pub type LogFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    file: *const c_char,
    line: u32,
    level: LogLevel,
    channel: *const c_char,
    message: *const c_char,
);

unsafe extern "C" {
    pub fn dm_noesis_set_license(name: *const c_char, key: *const c_char);
    pub fn dm_noesis_set_log_handler(cb: Option<LogFn>, userdata: *mut c_void);
    pub fn dm_noesis_init();
    pub fn dm_noesis_shutdown();
    pub fn dm_noesis_version() -> *const c_char;

    // Inspector / hot-reload toggles + queries (TODO §17). The Disable* trio
    // must be called before dm_noesis_init.
    pub fn dm_noesis_disable_hot_reload();
    pub fn dm_noesis_disable_socket_init();
    pub fn dm_noesis_disable_inspector();
    pub fn dm_noesis_is_inspector_connected() -> bool;
    pub fn dm_noesis_update_inspector();
}

// ────────────────────────────────────────────────────────────────────────────
// XamlProvider + View / Renderer FFI (Phase 4.C). See cpp/noesis_shim.h for
// pointer-ownership contracts.
// ────────────────────────────────────────────────────────────────────────────

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
/// Rust — pass it back verbatim.
pub type RegisterFontFn = unsafe extern "C" fn(register_cx: *mut c_void, filename: *const c_char);

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

/// Mirror of `dm_noesis_texture_info` in `noesis_shim.h` — texture metadata
/// returned by the provider's `get_info` callback.
#[repr(C)]
pub struct TextureInfoFfi {
    pub width: u32,
    pub height: u32,
    pub x: u32,
    pub y: u32,
    pub dpi_scale: f32,
}

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

unsafe extern "C" {
    pub fn dm_noesis_xaml_provider_create(
        vtable: *const XamlProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_xaml_provider_destroy(provider: *mut c_void);
    pub fn dm_noesis_set_xaml_provider(provider: *mut c_void);

    pub fn dm_noesis_font_provider_create(
        vtable: *const FontProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_font_provider_destroy(provider: *mut c_void);
    pub fn dm_noesis_set_font_provider(provider: *mut c_void);
    pub fn dm_noesis_set_font_fallbacks(families: *const *const c_char, count: u32);
    pub fn dm_noesis_set_font_default_properties(size: f32, weight: i32, stretch: i32, style: i32);
    pub fn dm_noesis_font_provider_register_font(
        provider: *mut c_void,
        folder_uri: *const c_char,
        filename: *const c_char,
    );

    pub fn dm_noesis_texture_provider_create(
        vtable: *const TextureProviderVTable,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_texture_provider_destroy(provider: *mut c_void);
    pub fn dm_noesis_set_texture_provider(provider: *mut c_void);

    pub fn dm_noesis_gui_load_xaml(uri: *const c_char) -> *mut c_void;
    pub fn dm_noesis_gui_parse_xaml(text: *const c_char) -> *mut c_void;
    pub fn dm_noesis_gui_load_component(component: *mut c_void, uri: *const c_char) -> bool;
    pub fn dm_noesis_gui_load_application_resources(uri: *const c_char) -> bool;
    pub fn dm_noesis_gui_install_app_resources_chain(
        uris: *const *const c_char,
        count: u32,
    ) -> bool;
    pub fn dm_noesis_base_component_release(obj: *mut c_void);

    pub fn dm_noesis_view_create(framework_element: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_view_destroy(view: *mut c_void);
    pub fn dm_noesis_view_add_reference(view: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_view_set_size(view: *mut c_void, width: u32, height: u32);
    pub fn dm_noesis_view_set_scale(view: *mut c_void, scale: f32);
    pub fn dm_noesis_view_set_projection_matrix(view: *mut c_void, matrix: *const f32);
    pub fn dm_noesis_view_update(view: *mut c_void, time_seconds: f64) -> bool;
    pub fn dm_noesis_view_set_flags(view: *mut c_void, flags: u32);
    pub fn dm_noesis_view_get_renderer(view: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_view_get_content(view: *mut c_void) -> *mut c_void;

    pub fn dm_noesis_renderer_init(renderer: *mut c_void, render_device: *mut c_void);
    pub fn dm_noesis_renderer_shutdown(renderer: *mut c_void);
    pub fn dm_noesis_renderer_update_render_tree(renderer: *mut c_void) -> bool;
    pub fn dm_noesis_renderer_render_offscreen(renderer: *mut c_void) -> bool;
    pub fn dm_noesis_renderer_render(renderer: *mut c_void, flip_y: bool, clear: bool);

    // ── Stereo / VR rendering (TODO §1) ──────────────────────────────────────
    pub fn dm_noesis_renderer_render_stereo(
        renderer: *mut c_void,
        eye_matrix: *const f32,
        flip_y: bool,
        clear: bool,
    );
    pub fn dm_noesis_renderer_render_stereo_both(
        renderer: *mut c_void,
        left_eye_matrix: *const f32,
        right_eye_matrix: *const f32,
        flip_y: bool,
        clear: bool,
    );

    pub fn dm_noesis_view_mouse_move(view: *mut c_void, x: i32, y: i32) -> bool;
    pub fn dm_noesis_view_mouse_button_down(view: *mut c_void, x: i32, y: i32, button: i32)
    -> bool;
    pub fn dm_noesis_view_mouse_button_up(view: *mut c_void, x: i32, y: i32, button: i32) -> bool;
    pub fn dm_noesis_view_mouse_double_click(
        view: *mut c_void,
        x: i32,
        y: i32,
        button: i32,
    ) -> bool;
    pub fn dm_noesis_view_mouse_wheel(view: *mut c_void, x: i32, y: i32, delta: i32) -> bool;
    pub fn dm_noesis_view_scroll(view: *mut c_void, x: i32, y: i32, value: f32) -> bool;
    pub fn dm_noesis_view_hscroll(view: *mut c_void, x: i32, y: i32, value: f32) -> bool;

    pub fn dm_noesis_view_touch_down(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;
    pub fn dm_noesis_view_touch_move(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;
    pub fn dm_noesis_view_touch_up(view: *mut c_void, x: i32, y: i32, id: u64) -> bool;

    pub fn dm_noesis_view_key_down(view: *mut c_void, key: i32) -> bool;
    pub fn dm_noesis_view_key_up(view: *mut c_void, key: i32) -> bool;
    pub fn dm_noesis_view_char(view: *mut c_void, codepoint: u32) -> bool;

    pub fn dm_noesis_view_activate(view: *mut c_void);
    pub fn dm_noesis_view_deactivate(view: *mut c_void);
    pub fn dm_noesis_view_mouse_hwheel(view: *mut c_void, x: i32, y: i32, delta: i32) -> bool;

    // ── View flags / quality / stats (TODO §1) ───────────────────────────────
    pub fn dm_noesis_view_get_flags(view: *mut c_void) -> u32;
    pub fn dm_noesis_view_set_tessellation_max_pixel_error(view: *mut c_void, error: f32);
    pub fn dm_noesis_view_get_tessellation_max_pixel_error(view: *mut c_void) -> f32;
    pub fn dm_noesis_view_get_stats(view: *mut c_void, out: *mut crate::view::ViewStats);

    // ── Gesture / touch thresholds (TODO §1) ─────────────────────────────────
    pub fn dm_noesis_view_set_holding_time_threshold(view: *mut c_void, ms: u32);
    pub fn dm_noesis_view_set_holding_distance_threshold(view: *mut c_void, pixels: u32);
    pub fn dm_noesis_view_set_manipulation_distance_threshold(view: *mut c_void, pixels: u32);
    pub fn dm_noesis_view_set_double_tap_time_threshold(view: *mut c_void, ms: u32);
    pub fn dm_noesis_view_set_double_tap_distance_threshold(view: *mut c_void, pixels: u32);
    pub fn dm_noesis_view_set_emulate_touch(view: *mut c_void, emulate: bool);

    // ── Stereo / VR (TODO §1) ────────────────────────────────────────────────
    pub fn dm_noesis_view_set_stereo_offscreen_scale_factor(view: *mut c_void, factor: f32);

    // ── View-driven timers (TODO §1) ─────────────────────────────────────────
    pub fn dm_noesis_view_create_timer(
        view: *mut c_void,
        interval_ms: u32,
        cb: TimerFn,
        userdata: *mut c_void,
        free_handler: TimerFreeFn,
    ) -> *mut c_void;
    pub fn dm_noesis_view_restart_timer(token: *mut c_void, interval_ms: u32);
    pub fn dm_noesis_view_cancel_timer(token: *mut c_void);

    // ── Rendering event (TODO §1) ────────────────────────────────────────────
    pub fn dm_noesis_view_add_rendering_handler(
        view: *mut c_void,
        cb: RenderingFn,
        userdata: *mut c_void,
        free_handler: RenderingFreeFn,
    ) -> *mut c_void;
    pub fn dm_noesis_view_remove_rendering_handler(token: *mut c_void);

    pub fn dm_noesis_framework_element_find_name(
        element: *mut c_void,
        name: *const c_char,
    ) -> *mut c_void;
    pub fn dm_noesis_framework_element_get_name(element: *mut c_void) -> *const c_char;
    pub fn dm_noesis_framework_element_set_visibility(element: *mut c_void, visible: bool);
    pub fn dm_noesis_framework_element_set_margin(
        element: *mut c_void,
        left: f32,
        top: f32,
        right: f32,
        bottom: f32,
    );

    pub fn dm_noesis_subscribe_click(
        element: *mut c_void,
        cb: ClickFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_unsubscribe_click(token: *mut c_void);

    pub fn dm_noesis_subscribe_keydown(
        element: *mut c_void,
        cb: KeyDownFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_unsubscribe_keydown(token: *mut c_void);

    pub fn dm_noesis_subscribe_event(
        element: *mut c_void,
        event_name: *const c_char,
        handled_too: bool,
        cb: RoutedEventFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_unsubscribe_event(token: *mut c_void);

    // ── Non-routed lifecycle events (TODO §5) ─────────────────────────────────
    // The callback is the same shape as `ClickFn` (a bare `void(userdata)`), so
    // it is reused here.
    pub fn dm_noesis_subscribe_lifecycle(
        element: *mut c_void,
        event_name: *const c_char,
        cb: ClickFn,
        userdata: *mut c_void,
    ) -> *mut c_void;
    pub fn dm_noesis_unsubscribe_lifecycle(token: *mut c_void);

    pub fn dm_noesis_mouse_args_position(args: *const c_void, x: *mut f32, y: *mut f32) -> bool;
    pub fn dm_noesis_mouse_button_args_button(args: *const c_void) -> i32;
    pub fn dm_noesis_mouse_wheel_args_delta(args: *const c_void) -> i32;
    pub fn dm_noesis_key_args_key(args: *const c_void) -> i32;
    pub fn dm_noesis_text_args_ch(args: *const c_void) -> i32;
    pub fn dm_noesis_size_changed_args_new_size(
        args: *const c_void,
        width: *mut f32,
        height: *mut f32,
    ) -> bool;
    pub fn dm_noesis_routed_args_source(args: *const c_void) -> *mut c_void;

    pub fn dm_noesis_text_get(element: *mut c_void) -> *const c_char;
    pub fn dm_noesis_text_set(element: *mut c_void, text: *const c_char) -> bool;
    pub fn dm_noesis_text_caret_to_end(element: *mut c_void) -> bool;
    pub fn dm_noesis_focus_element(element: *mut c_void) -> bool;
    pub fn dm_noesis_path_set_points(element: *mut c_void, xy: *const f32, count: u32) -> bool;
    pub fn dm_noesis_visual_state_go_to_state(
        element: *mut c_void,
        state: *const c_char,
        use_transitions: bool,
    ) -> bool;

    pub fn dm_noesis_class_register(
        name: *const c_char,
        base: ClassBase,
        cb: PropChangedFn,
        userdata: *mut c_void,
        free_handler: ClassFreeFn,
    ) -> *mut c_void;
    pub fn dm_noesis_class_register_property(
        class_token: *mut c_void,
        prop_name: *const c_char,
        prop_type: PropType,
        default_ptr: *const c_void,
    ) -> u32;
    // ── Custom base classes + richer DP metadata + layout (TODO §9) ──────────
    pub fn dm_noesis_class_register_property_ex(
        class_token: *mut c_void,
        prop_name: *const c_char,
        prop_type: PropType,
        default_ptr: *const c_void,
        fpm_options: u32,
        read_only: bool,
        coerce: bool,
    ) -> u32;
    pub fn dm_noesis_instance_set_readonly_property(
        instance: *mut c_void,
        prop_index: u32,
        value_ptr: *const c_void,
    ) -> bool;
    pub fn dm_noesis_class_set_coerce(
        class_token: *mut c_void,
        cb: CoerceFn,
        userdata: *mut c_void,
        free_handler: ClassFreeFn,
    );
    pub fn dm_noesis_class_set_layout(
        class_token: *mut c_void,
        vtable: *const LayoutVtable,
        userdata: *mut c_void,
        free_handler: LayoutFreeFn,
    );
    pub fn dm_noesis_uielement_measure(element: *mut c_void, avail_w: f32, avail_h: f32) -> bool;
    pub fn dm_noesis_uielement_arrange(
        element: *mut c_void,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
    ) -> bool;
    pub fn dm_noesis_uielement_desired_size(
        element: *mut c_void,
        out_w: *mut f32,
        out_h: *mut f32,
    ) -> bool;
    pub fn dm_noesis_class_unregister(class_token: *mut c_void);
    pub fn dm_noesis_instance_set_property(
        instance: *mut c_void,
        prop_index: u32,
        value_ptr: *const c_void,
    );
    pub fn dm_noesis_instance_get_property(
        instance: *mut c_void,
        prop_index: u32,
        out_value: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_image_source_get_size(
        image_source: *mut c_void,
        out_width: *mut f32,
        out_height: *mut f32,
    ) -> bool;

    pub fn dm_noesis_dependency_object_set_property(
        obj: *mut c_void,
        name: *const c_char,
        prop_type: PropType,
        value_ptr: *const c_void,
    ) -> bool;
    pub fn dm_noesis_dependency_object_get_property(
        obj: *mut c_void,
        name: *const c_char,
        prop_type: PropType,
        out_value: *mut c_void,
    ) -> bool;

    pub fn dm_noesis_markup_extension_register(
        name: *const c_char,
        cb: MarkupProvideFn,
        userdata: *mut c_void,
        free_handler: MarkupFreeFn,
    ) -> *mut c_void;
    pub fn dm_noesis_markup_extension_unregister(token: *mut c_void);

    pub fn dm_noesis_class_create_instance(class_token: *mut c_void) -> *mut c_void;

    pub fn dm_noesis_box_string(text: *const c_char) -> *mut c_void;

    pub fn dm_noesis_observable_collection_create() -> *mut c_void;
    pub fn dm_noesis_observable_collection_add(collection: *mut c_void, item: *mut c_void) -> i32;
    pub fn dm_noesis_observable_collection_insert(
        collection: *mut c_void,
        index: u32,
        item: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_observable_collection_set(
        collection: *mut c_void,
        index: u32,
        item: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_observable_collection_remove_at(collection: *mut c_void, index: u32) -> bool;
    pub fn dm_noesis_observable_collection_clear(collection: *mut c_void);
    pub fn dm_noesis_observable_collection_count(collection: *mut c_void) -> i32;
    pub fn dm_noesis_observable_collection_get(collection: *mut c_void, index: u32) -> *mut c_void;

    pub fn dm_noesis_framework_element_set_data_context(
        element: *mut c_void,
        context: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_framework_element_get_data_context(element: *mut c_void) -> *mut c_void;

    pub fn dm_noesis_items_control_set_items_source(
        element: *mut c_void,
        items: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_items_control_items_count(element: *mut c_void) -> i32;
    pub fn dm_noesis_items_control_realized_count(element: *mut c_void) -> i32;

    // ── Element tree access (TODO §2). See cpp/noesis_shim.h for pointer-
    // ownership + tag-validation contracts. ──────────────────────────────────

    // A. Tree traversal.
    pub fn dm_noesis_visual_children_count(element: *mut c_void) -> u32;
    pub fn dm_noesis_visual_child(element: *mut c_void, index: u32) -> *mut c_void;
    pub fn dm_noesis_visual_parent(element: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_visual_hit_test(element: *mut c_void, x: f32, y: f32) -> *mut c_void;
    pub fn dm_noesis_framework_element_logical_parent(element: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_logical_children_count(element: *mut c_void) -> u32;
    pub fn dm_noesis_logical_child(element: *mut c_void, index: u32) -> *mut c_void;
    pub fn dm_noesis_framework_element_template_child(
        element: *mut c_void,
        name: *const c_char,
    ) -> *mut c_void;

    // B. Attached properties.
    pub fn dm_noesis_dependency_object_set_attached(
        obj: *mut c_void,
        owner_type: *const c_char,
        prop_name: *const c_char,
        prop_type: PropType,
        value_ptr: *const c_void,
    ) -> bool;
    pub fn dm_noesis_dependency_object_get_attached(
        obj: *mut c_void,
        owner_type: *const c_char,
        prop_name: *const c_char,
        prop_type: PropType,
        out_value: *mut c_void,
    ) -> bool;

    // C. ClearValue / SetCurrentValue / GetBaseValue.
    pub fn dm_noesis_dependency_object_clear_value(obj: *mut c_void, name: *const c_char) -> bool;
    pub fn dm_noesis_dependency_object_set_current_value(
        obj: *mut c_void,
        name: *const c_char,
        prop_type: PropType,
        value_ptr: *const c_void,
    ) -> bool;
    pub fn dm_noesis_dependency_object_get_base_value(
        obj: *mut c_void,
        name: *const c_char,
        prop_type: PropType,
        out_value: *mut c_void,
    ) -> bool;

    // D. Dynamic tag inference.
    pub fn dm_noesis_dependency_object_property_tag(obj: *mut c_void, name: *const c_char) -> i32;

    // E. Alignment.
    pub fn dm_noesis_framework_element_set_halign(element: *mut c_void, value: i32);
    pub fn dm_noesis_framework_element_set_valign(element: *mut c_void, value: i32);
    pub fn dm_noesis_framework_element_get_halign(element: *mut c_void) -> i32;
    pub fn dm_noesis_framework_element_get_valign(element: *mut c_void) -> i32;

    // F. Namescope register / unregister.
    pub fn dm_noesis_framework_element_register_name(
        element: *mut c_void,
        name: *const c_char,
        object: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_framework_element_unregister_name(
        element: *mut c_void,
        name: *const c_char,
    ) -> bool;

    // G. Thread affinity.
    pub fn dm_noesis_dependency_object_check_access(obj: *mut c_void) -> bool;
    pub fn dm_noesis_dependency_object_thread_id(obj: *mut c_void) -> u32;

    // ── Commands: ICommand from Rust (TODO §4) ───────────────────────────────
    pub fn dm_noesis_command_create(
        vt: *const CommandVTable,
        userdata: *mut c_void,
        free_handler: CommandFreeFn,
    ) -> *mut c_void;
    pub fn dm_noesis_command_destroy(command: *mut c_void);
    pub fn dm_noesis_command_raise_can_execute_changed(command: *mut c_void);

    // ── Value boxing / unboxing primitives (TODO §3) ──────────────────────────
    pub fn dm_noesis_box_bool(value: bool) -> *mut c_void;
    pub fn dm_noesis_box_int32(value: i32) -> *mut c_void;
    pub fn dm_noesis_box_double(value: f64) -> *mut c_void;
    pub fn dm_noesis_unbox_bool(boxed: *mut c_void, out: *mut bool) -> bool;
    pub fn dm_noesis_unbox_int32(boxed: *mut c_void, out: *mut i32) -> bool;
    pub fn dm_noesis_unbox_double(boxed: *mut c_void, out: *mut f64) -> bool;
    pub fn dm_noesis_unbox_string(boxed: *mut c_void) -> *const c_char;

    // ── Value converters: IValueConverter from Rust (TODO §3) ─────────────────
    pub fn dm_noesis_value_converter_create(
        vt: *const ValueConverterVTable,
        userdata: *mut c_void,
        free_handler: ValueConverterFreeFn,
    ) -> *mut c_void;
    pub fn dm_noesis_value_converter_destroy(converter: *mut c_void);

    // ── Code-built Binding + SetBinding (TODO §3) ─────────────────────────────
    pub fn dm_noesis_binding_create(path: *const c_char) -> *mut c_void;
    pub fn dm_noesis_binding_destroy(binding: *mut c_void);
    pub fn dm_noesis_binding_set_source(binding: *mut c_void, source: *mut c_void);
    pub fn dm_noesis_binding_set_element_name(binding: *mut c_void, name: *const c_char);
    pub fn dm_noesis_binding_set_mode(binding: *mut c_void, mode: i32);
    pub fn dm_noesis_binding_set_converter(binding: *mut c_void, converter: *mut c_void);
    pub fn dm_noesis_binding_set_converter_parameter(binding: *mut c_void, parameter: *mut c_void);
    pub fn dm_noesis_binding_set_string_format(binding: *mut c_void, format: *const c_char);
    pub fn dm_noesis_binding_set_fallback_value(binding: *mut c_void, value: *mut c_void);
    pub fn dm_noesis_binding_set_update_source_trigger(binding: *mut c_void, trigger: i32);
    pub fn dm_noesis_binding_set_relative_source_self(binding: *mut c_void);
    pub fn dm_noesis_binding_set_relative_source_find_ancestor(
        binding: *mut c_void,
        type_name: *const c_char,
        level: u32,
    ) -> bool;
    pub fn dm_noesis_binding_set_relative_source_previous_data(binding: *mut c_void);
    pub fn dm_noesis_binding_set_relative_source_templated_parent(binding: *mut c_void);
    pub fn dm_noesis_get_binding_expression(
        element: *mut c_void,
        dp_name: *const c_char,
    ) -> *mut c_void;
    pub fn dm_noesis_binding_expression_update_target(expr: *mut c_void);
    pub fn dm_noesis_binding_expression_update_source(expr: *mut c_void);
    pub fn dm_noesis_set_binding(
        element: *mut c_void,
        dp_name: *const c_char,
        binding: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_framework_element_add_resource(
        element: *mut c_void,
        key: *const c_char,
        object: *mut c_void,
    ) -> bool;

    // ── Plain (non-DependencyObject) view models (TODO §9 + §3) ───────────────
    pub fn dm_noesis_plain_vm_register(
        type_name: *const c_char,
        on_set: Option<PlainSetFn>,
        userdata: *mut c_void,
        free_handler: Option<PlainFreeFn>,
    ) -> *mut c_void;
    pub fn dm_noesis_plain_vm_register_property(
        token: *mut c_void,
        prop_name: *const c_char,
        content_type: u32,
    ) -> u32;
    pub fn dm_noesis_plain_vm_create_instance(token: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_plain_vm_set_value(
        instance: *mut c_void,
        prop_index: u32,
        boxed_value: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_plain_vm_get_value(instance: *mut c_void, prop_index: u32) -> *mut c_void;
    pub fn dm_noesis_plain_vm_notify(instance: *mut c_void, prop_name: *const c_char) -> bool;
    pub fn dm_noesis_plain_vm_unregister(token: *mut c_void);

    // ── IMultiValueConverter + MultiBinding (TODO §3) ─────────────────────────
    pub fn dm_noesis_multi_value_converter_create(
        vt: *const MultiValueConverterVTable,
        userdata: *mut c_void,
        free_handler: MultiValueConverterFreeFn,
    ) -> *mut c_void;
    pub fn dm_noesis_multi_value_converter_destroy(converter: *mut c_void);
    pub fn dm_noesis_multi_binding_create() -> *mut c_void;
    pub fn dm_noesis_multi_binding_destroy(multi_binding: *mut c_void);
    pub fn dm_noesis_multi_binding_add_binding(
        multi_binding: *mut c_void,
        binding: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_multi_binding_set_converter(
        multi_binding: *mut c_void,
        converter: *mut c_void,
    );
    pub fn dm_noesis_multi_binding_set_converter_parameter(
        multi_binding: *mut c_void,
        parameter: *mut c_void,
    );
    pub fn dm_noesis_multi_binding_set_mode(multi_binding: *mut c_void, mode: i32);
    pub fn dm_noesis_set_multi_binding(
        element: *mut c_void,
        dp_name: *const c_char,
        multi_binding: *mut c_void,
    ) -> bool;

    // ── Controls — programmatic access (TODO §8 / Phase B) ──────────────────
    // Mirrors cpp/noesis_controls.cpp; see cpp/noesis_shim.h for the borrow /
    // sentinel contract of each entrypoint.

    // Selector
    pub fn dm_noesis_selector_get_selected_index(element: *mut c_void, out: *mut i32) -> bool;
    pub fn dm_noesis_selector_set_selected_index(element: *mut c_void, index: i32) -> bool;
    pub fn dm_noesis_selector_get_selected_item(element: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_selector_set_selected_item(element: *mut c_void, item: *mut c_void) -> bool;

    // ItemsControl.Items
    pub fn dm_noesis_items_control_items_add(element: *mut c_void, item: *mut c_void) -> i32;
    pub fn dm_noesis_items_control_items_insert(
        element: *mut c_void,
        index: u32,
        item: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_items_control_items_remove_at(element: *mut c_void, index: u32) -> bool;
    pub fn dm_noesis_items_control_items_clear(element: *mut c_void) -> bool;

    // RangeBase
    pub fn dm_noesis_rangebase_get(element: *mut c_void, which: i32, out: *mut f32) -> bool;
    pub fn dm_noesis_rangebase_set(element: *mut c_void, which: i32, value: f32) -> bool;

    // ToggleButton
    pub fn dm_noesis_toggle_get_is_checked(element: *mut c_void, out_state: *mut i8) -> bool;
    pub fn dm_noesis_toggle_set_is_checked(element: *mut c_void, state: i8) -> bool;

    // Popup / Expander
    pub fn dm_noesis_popup_get_is_open(element: *mut c_void, out: *mut bool) -> bool;
    pub fn dm_noesis_popup_set_is_open(element: *mut c_void, open: bool) -> bool;
    pub fn dm_noesis_expander_get_is_expanded(element: *mut c_void, out: *mut bool) -> bool;
    pub fn dm_noesis_expander_set_is_expanded(element: *mut c_void, expanded: bool) -> bool;

    // ScrollViewer
    pub fn dm_noesis_scrollviewer_get(element: *mut c_void, which: i32, out: *mut f32) -> bool;
    pub fn dm_noesis_scrollviewer_scroll_to_horizontal(element: *mut c_void, offset: f32) -> bool;
    pub fn dm_noesis_scrollviewer_scroll_to_vertical(element: *mut c_void, offset: f32) -> bool;
    pub fn dm_noesis_scrollviewer_scroll_to_home(element: *mut c_void) -> bool;
    pub fn dm_noesis_scrollviewer_scroll_to_end(element: *mut c_void) -> bool;

    // TextBox / PasswordBox
    pub fn dm_noesis_textbox_get_int(element: *mut c_void, which: i32, out: *mut i32) -> bool;
    pub fn dm_noesis_textbox_set_int(element: *mut c_void, which: i32, value: i32) -> bool;
    pub fn dm_noesis_textbox_select(element: *mut c_void, start: i32, length: i32) -> bool;
    pub fn dm_noesis_textbox_select_all(element: *mut c_void) -> bool;
    pub fn dm_noesis_textbox_get_selected_text(element: *mut c_void) -> *const c_char;
    pub fn dm_noesis_passwordbox_get_password(element: *mut c_void) -> *const c_char;
    pub fn dm_noesis_passwordbox_set_password(
        element: *mut c_void,
        password: *const c_char,
    ) -> bool;
    // ── ResourceDictionary, Style, templates (TODO §7). See cpp/noesis_shim.h
    //    for the per-function ownership contract (create/parse → +1 owned;
    //    get_* → AddRef'd +1 owned; find_* / get_application_resources →
    //    borrowed, do not release). ──────────────────────────────────────────
    pub fn dm_noesis_box_float(value: f32) -> *mut c_void;
    pub fn dm_noesis_resource_dictionary_create() -> *mut c_void;
    pub fn dm_noesis_resource_dictionary_destroy(dict: *mut c_void);
    pub fn dm_noesis_resource_dictionary_parse(xaml: *const c_char) -> *mut c_void;
    pub fn dm_noesis_resource_dictionary_count(dict: *mut c_void) -> u32;
    pub fn dm_noesis_resource_dictionary_add(
        dict: *mut c_void,
        key: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_resource_dictionary_contains(dict: *mut c_void, key: *const c_char) -> bool;
    pub fn dm_noesis_resource_dictionary_find(dict: *mut c_void, key: *const c_char)
    -> *mut c_void;
    pub fn dm_noesis_resource_dictionary_add_merged(dict: *mut c_void, merged: *mut c_void)
    -> bool;

    pub fn dm_noesis_gui_set_application_resources(dict: *mut c_void);
    pub fn dm_noesis_gui_get_application_resources() -> *mut c_void;
    pub fn dm_noesis_gui_register_default_styles(uri: *const c_char) -> bool;

    pub fn dm_noesis_framework_element_get_resources(element: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_framework_element_set_resources(
        element: *mut c_void,
        dict: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_framework_element_find_resource(
        element: *mut c_void,
        key: *const c_char,
    ) -> *mut c_void;

    pub fn dm_noesis_style_create() -> *mut c_void;
    pub fn dm_noesis_style_destroy(style: *mut c_void);
    pub fn dm_noesis_style_set_target_type(style: *mut c_void, type_name: *const c_char) -> bool;
    pub fn dm_noesis_style_add_setter(
        style: *mut c_void,
        dp_name: *const c_char,
        value: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_style_set_based_on(style: *mut c_void, base: *mut c_void);

    pub fn dm_noesis_framework_element_set_style(element: *mut c_void, style: *mut c_void) -> bool;
    pub fn dm_noesis_framework_element_get_style(element: *mut c_void) -> *mut c_void;

    pub fn dm_noesis_control_template_parse(xaml: *const c_char) -> *mut c_void;
    pub fn dm_noesis_data_template_parse(xaml: *const c_char) -> *mut c_void;
    pub fn dm_noesis_control_set_template(control: *mut c_void, tmpl: *mut c_void) -> bool;
    pub fn dm_noesis_control_get_template(control: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_framework_template_find_name(
        tmpl: *mut c_void,
        name: *const c_char,
        templated_parent: *mut c_void,
    ) -> *mut c_void;
}

// ── Brushes, transforms, effects, RenderOptions (TODO §11) ──────────────────
//
// Object construction from Rust. Each `*_create` returns a `+1`-owned
// `BaseComponent*` (the owning wrapper in src/brushes.rs / src/transforms.rs
// releases it on Drop). Colors are `[f32; 4]` = `{r, g, b, a}` in `0..=1`.
unsafe extern "C" {
    // SolidColorBrush
    pub fn dm_noesis_solid_color_brush_create(color: *const f32) -> *mut c_void;
    pub fn dm_noesis_solid_color_brush_set_color(brush: *mut c_void, color: *const f32) -> bool;
    pub fn dm_noesis_solid_color_brush_get_color(brush: *mut c_void, out: *mut f32) -> bool;

    // LinearGradientBrush
    pub fn dm_noesis_linear_gradient_brush_create() -> *mut c_void;
    pub fn dm_noesis_linear_gradient_brush_set_start_point(
        brush: *mut c_void,
        x: f32,
        y: f32,
    ) -> bool;
    pub fn dm_noesis_linear_gradient_brush_set_end_point(
        brush: *mut c_void,
        x: f32,
        y: f32,
    ) -> bool;
    pub fn dm_noesis_linear_gradient_brush_get_points(brush: *mut c_void, out: *mut f32) -> bool;

    // RadialGradientBrush
    pub fn dm_noesis_radial_gradient_brush_create() -> *mut c_void;
    pub fn dm_noesis_radial_gradient_brush_set_center(brush: *mut c_void, x: f32, y: f32) -> bool;
    pub fn dm_noesis_radial_gradient_brush_set_gradient_origin(
        brush: *mut c_void,
        x: f32,
        y: f32,
    ) -> bool;
    pub fn dm_noesis_radial_gradient_brush_set_radius(brush: *mut c_void, rx: f32, ry: f32)
    -> bool;
    pub fn dm_noesis_radial_gradient_brush_get_radius(
        brush: *mut c_void,
        rx: *mut f32,
        ry: *mut f32,
    ) -> bool;

    // GradientBrush stops
    pub fn dm_noesis_gradient_brush_add_stop(
        brush: *mut c_void,
        offset: f32,
        color: *const f32,
    ) -> i32;
    pub fn dm_noesis_gradient_brush_stop_count(brush: *mut c_void) -> i32;
    pub fn dm_noesis_gradient_brush_get_stop(
        brush: *mut c_void,
        index: u32,
        out_offset: *mut f32,
        out_color: *mut f32,
    ) -> bool;

    // ImageBrush
    pub fn dm_noesis_image_brush_create(image_source: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_image_brush_set_image_source(
        brush: *mut c_void,
        image_source: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_image_brush_get_image_source(brush: *mut c_void) -> *mut c_void;

    // Transforms
    pub fn dm_noesis_translate_transform_create(x: f32, y: f32) -> *mut c_void;
    pub fn dm_noesis_translate_transform_set(transform: *mut c_void, x: f32, y: f32) -> bool;
    pub fn dm_noesis_translate_transform_get(
        transform: *mut c_void,
        x: *mut f32,
        y: *mut f32,
    ) -> bool;

    pub fn dm_noesis_scale_transform_create(sx: f32, sy: f32, cx: f32, cy: f32) -> *mut c_void;
    pub fn dm_noesis_scale_transform_set(
        transform: *mut c_void,
        sx: f32,
        sy: f32,
        cx: f32,
        cy: f32,
    ) -> bool;
    pub fn dm_noesis_scale_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn dm_noesis_rotate_transform_create(angle: f32, cx: f32, cy: f32) -> *mut c_void;
    pub fn dm_noesis_rotate_transform_set_angle(transform: *mut c_void, angle: f32) -> bool;
    pub fn dm_noesis_rotate_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn dm_noesis_skew_transform_create(ax: f32, ay: f32, cx: f32, cy: f32) -> *mut c_void;
    pub fn dm_noesis_skew_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn dm_noesis_matrix_transform_create(matrix: *const f32) -> *mut c_void;
    pub fn dm_noesis_matrix_transform_set(transform: *mut c_void, matrix: *const f32) -> bool;
    pub fn dm_noesis_matrix_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    pub fn dm_noesis_transform_group_create() -> *mut c_void;
    pub fn dm_noesis_transform_group_add_child(group: *mut c_void, child: *mut c_void) -> bool;
    pub fn dm_noesis_transform_group_child_count(group: *mut c_void) -> i32;

    pub fn dm_noesis_composite_transform_create(fields: *const f32) -> *mut c_void;
    pub fn dm_noesis_composite_transform_get(transform: *mut c_void, out: *mut f32) -> bool;

    // Effects
    pub fn dm_noesis_blur_effect_create(radius: f32) -> *mut c_void;
    pub fn dm_noesis_blur_effect_set_radius(effect: *mut c_void, radius: f32) -> bool;
    pub fn dm_noesis_blur_effect_get_radius(effect: *mut c_void, out: *mut f32) -> bool;

    pub fn dm_noesis_drop_shadow_effect_create(
        color: *const f32,
        blur_radius: f32,
        direction: f32,
        shadow_depth: f32,
        opacity: f32,
    ) -> *mut c_void;
    pub fn dm_noesis_drop_shadow_effect_get(
        effect: *mut c_void,
        out_color: *mut f32,
        out_blur: *mut f32,
        out_direction: *mut f32,
        out_shadow_depth: *mut f32,
        out_opacity: *mut f32,
    ) -> bool;

    // RenderOptions
    pub fn dm_noesis_render_options_set_bitmap_scaling_mode(obj: *mut c_void, mode: i32) -> bool;
    pub fn dm_noesis_render_options_get_bitmap_scaling_mode(obj: *mut c_void) -> i32;
}

/// Mirror of `dm_noesis_value_converter_vtable` in `cpp/noesis_shim.h`. Both fn
/// pointers receive the `userdata` passed to [`dm_noesis_value_converter_create`],
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
/// [`dm_noesis_value_converter_create`].
pub type ValueConverterFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Callback for a `TwoWay` / `OneWayToSource` write back to a plain-VM reflected
/// property. Mirrors `dm_noesis_plain_set_fn`. `instance` is the borrowed
/// `RustPlainVm*`, `prop_index` the dense index from
/// [`dm_noesis_plain_vm_register_property`], `boxed_value` the borrowed boxed
/// `BaseComponent*` the UI pushed (may be null).
pub type PlainSetFn = unsafe extern "C" fn(
    userdata: *mut c_void,
    instance: *mut c_void,
    prop_index: u32,
    boxed_value: *mut c_void,
);

/// Free callback invoked exactly once when a plain-VM registration's refcount
/// hits zero. Drops the boxed handler whose ownership transferred to C++ at
/// [`dm_noesis_plain_vm_register`].
pub type PlainFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Mirror of `dm_noesis_multi_value_converter_vtable` in `cpp/noesis_shim.h`.
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
/// ownership transferred to C++ at [`dm_noesis_multi_value_converter_create`].
pub type MultiValueConverterFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Mirror of `dm_noesis_command_vtable` in `cpp/noesis_shim.h`. Both fn
/// pointers receive the `userdata` passed to [`dm_noesis_command_create`] and
/// the borrowed command-parameter `BaseComponent*` (`param`, may be null).
#[repr(C)]
pub struct CommandVTable {
    pub can_execute: unsafe extern "C" fn(userdata: *mut c_void, param: *mut c_void) -> bool,
    pub execute: unsafe extern "C" fn(userdata: *mut c_void, param: *mut c_void),
}

/// Free callback invoked exactly once when the underlying `RustCommand` is
/// finally destroyed (last reference released). Drops the boxed handler whose
/// ownership transferred to C++ at [`dm_noesis_command_create`].
pub type CommandFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// Free callback invoked exactly once per registered markup extension
/// when its underlying C++ `MarkupClassData` is finally freed. Same shape
/// and contract as [`ClassFreeFn`] — see that type's docs.
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
/// `cpp/noesis_shim.h` for the threading contract — the callback runs on
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
/// `dm_noesis_subscribe_event` path).
///
/// `args` is an opaque handle to the live event arguments — pass it to the
/// `dm_noesis_*_args_*` accessors to read typed fields. It is valid only for
/// the duration of the call. `out_handled` is pre-seeded with the event's
/// current handled state; writing `true` marks the routed event handled.
/// Same threading contract as [`ClickFn`].
pub type RoutedEventFn =
    unsafe extern "C" fn(userdata: *mut c_void, args: *const c_void, out_handled: *mut bool);

/// C callback fired on each view-timer tick (the `dm_noesis_view_create_timer`
/// path). Returns the next interval in milliseconds, or `0` to stop the timer.
/// Fires from inside `IView::Update` on the view-driving thread — same
/// threading contract as [`ClickFn`].
pub type TimerFn = unsafe extern "C" fn(userdata: *mut c_void) -> u32;

/// C callback invoked exactly once when a view-timer token is cancelled (the
/// C++ `RustTimer` destroyed). Frees the donated `userdata`. Mirrors
/// [`CommandFreeFn`].
pub type TimerFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

/// C callback fired on each `IView::Rendering` event (the
/// `dm_noesis_view_add_rendering_handler` path). `view` is the borrowed `IView*`
/// raising the event (do not release). Fires on the view-driving thread — same
/// threading contract as [`TimerFn`].
pub type RenderingFn = unsafe extern "C" fn(userdata: *mut c_void, view: *mut c_void);

/// C callback invoked exactly once when a Rendering handler token is removed
/// (the C++ handler destroyed). Frees the donated `userdata`. Mirrors
/// [`TimerFreeFn`].
pub type RenderingFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

// ────────────────────────────────────────────────────────────────────────────
// Custom XAML class registration (Phase 5.C). See cpp/noesis_shim.h for the
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
}

/// FFI value-type tag. The buffer layout for `value_ptr` / `default_ptr` /
/// `out_value` is determined by this tag — see the per-variant comments in
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
/// `dm_noesis_class_unregister` if no instances exist, or deferred to the
/// last live instance's destruction). Receives the `userdata` passed at
/// registration; the Rust trampoline drops the boxed handler. Ownership
/// of `userdata` transfers to the C++ side at register time.
pub type ClassFreeFn = unsafe extern "C" fn(userdata: *mut c_void);

// ────────────────────────────────────────────────────────────────────────────
// Animation & timing (TODO §6 / Phase C). See cpp/noesis_animation.cpp.
// Every `*_create` returns a +1-owned BaseComponent* (released by the owning
// Rust handle's Drop via dm_noesis_base_component_release).
// ────────────────────────────────────────────────────────────────────────────
unsafe extern "C" {
    // Storyboard
    pub fn dm_noesis_storyboard_create() -> *mut c_void;
    pub fn dm_noesis_storyboard_add_child(sb: *mut c_void, timeline: *mut c_void) -> bool;
    pub fn dm_noesis_storyboard_child_count(sb: *mut c_void) -> i32;
    pub fn dm_noesis_storyboard_set_target_name(timeline: *mut c_void, name: *const c_char)
    -> bool;
    pub fn dm_noesis_storyboard_set_target_property(
        timeline: *mut c_void,
        path: *const c_char,
    ) -> bool;
    pub fn dm_noesis_storyboard_set_target(timeline: *mut c_void, target: *mut c_void) -> bool;
    pub fn dm_noesis_storyboard_begin(sb: *mut c_void, fe: *mut c_void, controllable: bool)
    -> bool;
    pub fn dm_noesis_storyboard_pause(sb: *mut c_void, fe: *mut c_void) -> bool;
    pub fn dm_noesis_storyboard_resume(sb: *mut c_void, fe: *mut c_void) -> bool;
    pub fn dm_noesis_storyboard_stop(sb: *mut c_void, fe: *mut c_void) -> bool;
    pub fn dm_noesis_storyboard_seek(sb: *mut c_void, fe: *mut c_void, seconds: f64) -> bool;
    pub fn dm_noesis_storyboard_is_playing(sb: *mut c_void, fe: *mut c_void) -> bool;
    pub fn dm_noesis_storyboard_is_paused(sb: *mut c_void, fe: *mut c_void) -> bool;

    // Timeline common knobs
    pub fn dm_noesis_timeline_set_duration_seconds(tl: *mut c_void, seconds: f64) -> bool;
    pub fn dm_noesis_timeline_set_duration_auto(tl: *mut c_void) -> bool;
    pub fn dm_noesis_timeline_set_duration_forever(tl: *mut c_void) -> bool;
    pub fn dm_noesis_timeline_get_duration_seconds(tl: *mut c_void) -> f64;
    pub fn dm_noesis_timeline_set_begin_time_seconds(tl: *mut c_void, seconds: f64) -> bool;
    pub fn dm_noesis_timeline_set_auto_reverse(tl: *mut c_void, value: bool) -> bool;
    pub fn dm_noesis_timeline_set_speed_ratio(tl: *mut c_void, value: f32) -> bool;
    pub fn dm_noesis_timeline_set_fill_behavior(tl: *mut c_void, behavior: i32) -> bool;
    pub fn dm_noesis_timeline_set_repeat_count(tl: *mut c_void, count: f32) -> bool;
    pub fn dm_noesis_timeline_set_repeat_duration(tl: *mut c_void, seconds: f64) -> bool;
    pub fn dm_noesis_timeline_set_repeat_forever(tl: *mut c_void) -> bool;

    // From/To/By animations
    pub fn dm_noesis_double_animation_create() -> *mut c_void;
    pub fn dm_noesis_double_animation_set_from(anim: *mut c_void, has: bool, v: f32) -> bool;
    pub fn dm_noesis_double_animation_set_to(anim: *mut c_void, has: bool, v: f32) -> bool;
    pub fn dm_noesis_double_animation_set_by(anim: *mut c_void, has: bool, v: f32) -> bool;

    pub fn dm_noesis_color_animation_create() -> *mut c_void;
    pub fn dm_noesis_color_animation_set_from(
        anim: *mut c_void,
        has: bool,
        color: *const f32,
    ) -> bool;
    pub fn dm_noesis_color_animation_set_to(
        anim: *mut c_void,
        has: bool,
        color: *const f32,
    ) -> bool;
    pub fn dm_noesis_color_animation_set_by(
        anim: *mut c_void,
        has: bool,
        color: *const f32,
    ) -> bool;

    pub fn dm_noesis_thickness_animation_create() -> *mut c_void;
    pub fn dm_noesis_thickness_animation_set_from(
        anim: *mut c_void,
        has: bool,
        t: *const f32,
    ) -> bool;
    pub fn dm_noesis_thickness_animation_set_to(
        anim: *mut c_void,
        has: bool,
        t: *const f32,
    ) -> bool;
    pub fn dm_noesis_thickness_animation_set_by(
        anim: *mut c_void,
        has: bool,
        t: *const f32,
    ) -> bool;

    pub fn dm_noesis_point_animation_create() -> *mut c_void;
    pub fn dm_noesis_point_animation_set_from(anim: *mut c_void, has: bool, x: f32, y: f32)
    -> bool;
    pub fn dm_noesis_point_animation_set_to(anim: *mut c_void, has: bool, x: f32, y: f32) -> bool;
    pub fn dm_noesis_point_animation_set_by(anim: *mut c_void, has: bool, x: f32, y: f32) -> bool;

    pub fn dm_noesis_animation_set_easing_function(anim: *mut c_void, easing: *mut c_void) -> bool;

    // Easing functions
    pub fn dm_noesis_easing_function_create(kind: i32, mode: i32) -> *mut c_void;
    pub fn dm_noesis_easing_function_set_amplitude(easing: *mut c_void, value: f32) -> bool;
    pub fn dm_noesis_easing_function_set_power(easing: *mut c_void, value: f32) -> bool;
    pub fn dm_noesis_easing_function_set_exponent(easing: *mut c_void, value: f32) -> bool;
    pub fn dm_noesis_easing_function_set_oscillations(easing: *mut c_void, value: i32) -> bool;
    pub fn dm_noesis_easing_function_set_springiness(easing: *mut c_void, value: f32) -> bool;

    // Key-frame animations
    pub fn dm_noesis_double_animation_keyframes_create() -> *mut c_void;
    pub fn dm_noesis_double_animation_add_keyframe(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        value: f32,
        easing: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_color_animation_keyframes_create() -> *mut c_void;
    pub fn dm_noesis_color_animation_add_keyframe(
        anim: *mut c_void,
        kind: i32,
        key_time_seconds: f64,
        color: *const f32,
        easing: *mut c_void,
    ) -> bool;

    // Storyboard-less direct animation
    pub fn dm_noesis_animation_begin_on(
        anim: *mut c_void,
        target: *mut c_void,
        dp_name: *const c_char,
        handoff: i32,
    ) -> bool;
}
// Reflection meta: custom enums / routed events / factory + string conversion
// (TODO §9). See cpp/noesis_shim.h for the full ownership + threading contracts.
// ────────────────────────────────────────────────────────────────────────────

/// One (name, value) pair of a runtime enum, mirroring
/// `dm_noesis_enum_value` in `cpp/noesis_shim.h`. `name` is a borrowed C string
/// valid for the duration of the `dm_noesis_register_enum` call.
#[repr(C)]
#[derive(Copy, Clone, Debug)]
pub struct EnumValue {
    pub name: *const c_char,
    pub value: i32,
}

unsafe extern "C" {
    // (A) Custom enums
    pub fn dm_noesis_register_enum(
        name: *const c_char,
        values: *const EnumValue,
        count: u32,
    ) -> *mut c_void;
    pub fn dm_noesis_enum_value_from_name(
        enum_type: *const c_char,
        value_name: *const c_char,
        out_value: *mut i32,
    ) -> bool;
    pub fn dm_noesis_enum_name_from_value(
        enum_type: *const c_char,
        value: i32,
        out_name: *mut *const c_char,
    ) -> bool;
    pub fn dm_noesis_type_converter_from_string(
        type_name: *const c_char,
        str: *const c_char,
        out_boxed: *mut *mut c_void,
    ) -> bool;

    // (B) Custom routed events
    pub fn dm_noesis_register_routed_event(
        type_name: *const c_char,
        event_name: *const c_char,
        strategy: i32,
    ) -> bool;
    pub fn dm_noesis_raise_routed_event(element: *mut c_void, event_name: *const c_char) -> bool;

    // (C) Factory / component metadata
    pub fn dm_noesis_factory_is_registered(name: *const c_char) -> bool;
    pub fn dm_noesis_type_set_content_property(
        type_name: *const c_char,
        prop_name: *const c_char,
    ) -> bool;

    // TextBlock inline content model (TODO §13). Constructors hand out a +1
    // BaseComponent* (release via dm_noesis_base_component_release).
    pub fn dm_noesis_text_inlines_run_create(text: *const c_char) -> *mut c_void;
    pub fn dm_noesis_text_inlines_span_create() -> *mut c_void;
    pub fn dm_noesis_text_inlines_bold_create() -> *mut c_void;
    pub fn dm_noesis_text_inlines_italic_create() -> *mut c_void;
    pub fn dm_noesis_text_inlines_underline_create() -> *mut c_void;
    pub fn dm_noesis_text_inlines_hyperlink_create() -> *mut c_void;
    pub fn dm_noesis_text_inlines_line_break_create() -> *mut c_void;
    pub fn dm_noesis_text_inlines_ui_container_create() -> *mut c_void;

    pub fn dm_noesis_text_inlines_run_set_text(run: *mut c_void, text: *const c_char) -> bool;
    pub fn dm_noesis_text_inlines_run_get_text(run: *mut c_void) -> *const c_char;

    pub fn dm_noesis_text_inlines_hyperlink_set_navigate_uri(
        link: *mut c_void,
        uri: *const c_char,
    ) -> bool;
    pub fn dm_noesis_text_inlines_hyperlink_get_navigate_uri(link: *mut c_void) -> *const c_char;

    pub fn dm_noesis_text_inlines_inline_set_text_decorations(
        inl: *mut c_void,
        decorations: i32,
    ) -> bool;
    pub fn dm_noesis_text_inlines_inline_get_text_decorations(inl: *mut c_void) -> i32;

    pub fn dm_noesis_text_inlines_ui_container_set_child(
        container: *mut c_void,
        child: *mut c_void,
    ) -> bool;
    pub fn dm_noesis_text_inlines_ui_container_get_child(container: *mut c_void) -> *mut c_void;

    pub fn dm_noesis_text_inlines_text_block_get_inlines(text_block: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_text_inlines_span_get_inlines(span: *mut c_void) -> *mut c_void;

    pub fn dm_noesis_text_inlines_collection_add(collection: *mut c_void, inl: *mut c_void) -> i32;
    pub fn dm_noesis_text_inlines_collection_count(collection: *mut c_void) -> i32;
    pub fn dm_noesis_text_inlines_collection_get(
        collection: *mut c_void,
        index: u32,
    ) -> *mut c_void;
}
/// Coerce callback (TODO §9). Invoked inside Noesis's value pipeline when a
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

/// Layout vtable (TODO §9). The trampoline subclass's `MeasureOverride` /
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
