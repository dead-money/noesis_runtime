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
    pub fn dm_noesis_gui_load_application_resources(uri: *const c_char) -> bool;
    pub fn dm_noesis_gui_install_app_resources_chain(
        uris: *const *const c_char,
        count: u32,
    ) -> bool;
    pub fn dm_noesis_base_component_release(obj: *mut c_void);

    pub fn dm_noesis_view_create(framework_element: *mut c_void) -> *mut c_void;
    pub fn dm_noesis_view_destroy(view: *mut c_void);
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

// ────────────────────────────────────────────────────────────────────────────
// Custom XAML class registration (Phase 5.C). See cpp/noesis_shim.h for the
// per-type value layout convention each variant of `PropType` enforces.
// ────────────────────────────────────────────────────────────────────────────

/// Base type the trampoline subclass derives from. v1 only exposes
/// `ContentControl`; sibling base types (Control, `UserControl`,
/// `FrameworkElement`, Panel) plug in by adding trampoline subclasses on the
/// C++ side and a new variant here.
#[repr(u32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ClassBase {
    ContentControl = 0,
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
