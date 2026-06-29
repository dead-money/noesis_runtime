//! Safe wrappers around the Noesis `FrameworkElement`, `IView`, and
//! `IRenderer` opaque pointers.
//!
//! ```text
//!   load_xaml(uri) -> FrameworkElement
//!   FrameworkElement + View::create -> View
//!   View::renderer() -> Renderer (borrowed from View)
//!   Renderer: init(device), update_render_tree, render_offscreen, render, shutdown
//! ```
//!
//! Every owning wrapper releases its +1 reference on drop via the Noesis
//! intrusive refcount, which means the Noesis runtime must still be alive
//! (i.e. [`crate::shutdown`] not yet called) at drop time. Otherwise the
//! `Release()` path would touch freed state. Keep these wrappers on the
//! stack for the scope of a single frame, dropped before `shutdown`.

use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::brushes::{Brush, Effect};
use crate::ffi::{
    PropType, noesis_base_component_add_reference, noesis_base_component_get_num_references,
    noesis_base_component_release, noesis_binding_expression_update_source,
    noesis_binding_expression_update_target, noesis_control_get_template,
    noesis_control_set_template, noesis_controls_contextmenu_get_is_open,
    noesis_controls_contextmenu_set_is_open, noesis_controls_contextmenuservice_get_context_menu,
    noesis_controls_contextmenuservice_set_context_menu, noesis_controls_fe_get_context_menu,
    noesis_controls_fe_get_tooltip, noesis_controls_fe_set_context_menu,
    noesis_controls_fe_set_tooltip, noesis_controls_fe_set_tooltip_string,
    noesis_controls_generator_container_from_index, noesis_controls_generator_container_from_item,
    noesis_controls_generator_index_from_container, noesis_controls_generator_item_from_container,
    noesis_controls_gridview_column_count, noesis_controls_gridview_column_get_actual_width,
    noesis_controls_gridview_column_get_header, noesis_controls_gridview_column_get_width,
    noesis_controls_gridview_column_set_width, noesis_controls_image_get_source,
    noesis_controls_image_set_source, noesis_controls_listview_get_view,
    noesis_controls_scrollviewer_edge, noesis_controls_scrollviewer_line,
    noesis_controls_scrollviewer_metric, noesis_controls_scrollviewer_page,
    noesis_controls_selector_get_selected_value, noesis_controls_selector_get_selected_value_path,
    noesis_controls_selector_set_selected_value, noesis_controls_selector_set_selected_value_path,
    noesis_controls_tooltip_get_is_open, noesis_controls_tooltip_set_is_open,
    noesis_controls_tooltipservice_get_tooltip, noesis_controls_tooltipservice_set_tooltip,
    noesis_controls_treeview_get_selected_item, noesis_controls_treeviewitem_get_is_expanded,
    noesis_controls_treeviewitem_get_is_selected, noesis_controls_treeviewitem_set_is_expanded,
    noesis_controls_treeviewitem_set_is_selected, noesis_decorator_get_child,
    noesis_decorator_set_child, noesis_dependency_object_check_access,
    noesis_dependency_object_clear_value, noesis_dependency_object_get_attached,
    noesis_dependency_object_get_base_value, noesis_dependency_object_get_property,
    noesis_dependency_object_property_tag, noesis_dependency_object_set_attached,
    noesis_dependency_object_set_current_value, noesis_dependency_object_set_property,
    noesis_dependency_object_thread_id, noesis_element_get_transform3d,
    noesis_element_set_transform3d, noesis_expander_get_is_expanded,
    noesis_expander_set_is_expanded, noesis_focus_element, noesis_framework_element_find_name,
    noesis_framework_element_find_resource, noesis_framework_element_get_data_context,
    noesis_framework_element_get_halign, noesis_framework_element_get_name,
    noesis_framework_element_get_resources, noesis_framework_element_get_style,
    noesis_framework_element_get_valign, noesis_framework_element_logical_parent,
    noesis_framework_element_register_name, noesis_framework_element_set_data_context,
    noesis_framework_element_set_halign, noesis_framework_element_set_margin,
    noesis_framework_element_set_resources, noesis_framework_element_set_style,
    noesis_framework_element_set_valign, noesis_framework_element_set_visibility,
    noesis_framework_element_template_child, noesis_framework_element_unregister_name,
    noesis_get_binding_expression, noesis_gui_load_xaml, noesis_gui_parse_xaml,
    noesis_items_control_items_add, noesis_items_control_items_clear,
    noesis_items_control_items_count, noesis_items_control_items_insert,
    noesis_items_control_items_remove_at, noesis_items_control_realized_count,
    noesis_items_control_set_items_source, noesis_logical_child, noesis_logical_children_count,
    noesis_passwordbox_get_password, noesis_passwordbox_set_password, noesis_path_set_points,
    noesis_popup_get_is_open, noesis_popup_set_is_open, noesis_rangebase_get, noesis_rangebase_set,
    noesis_render_options_get_bitmap_scaling_mode, noesis_render_options_set_bitmap_scaling_mode,
    noesis_renderer_init, noesis_renderer_render, noesis_renderer_render_offscreen,
    noesis_renderer_render_stereo, noesis_renderer_render_stereo_both, noesis_renderer_shutdown,
    noesis_renderer_update_render_tree, noesis_scrollviewer_get, noesis_scrollviewer_scroll_to_end,
    noesis_scrollviewer_scroll_to_home, noesis_scrollviewer_scroll_to_horizontal,
    noesis_scrollviewer_scroll_to_vertical, noesis_selector_get_selected_index,
    noesis_selector_get_selected_item, noesis_selector_set_selected_index,
    noesis_selector_set_selected_item, noesis_solid_color_brush_get_color,
    noesis_text_caret_to_end, noesis_text_get, noesis_text_set, noesis_textbox_get_int,
    noesis_textbox_get_selected_text, noesis_textbox_select, noesis_textbox_select_all,
    noesis_textbox_set_int, noesis_toggle_get_is_checked, noesis_toggle_set_is_checked,
    noesis_ui_element_capture_mouse, noesis_ui_element_capture_mouse_mode,
    noesis_ui_element_capture_touch, noesis_ui_element_focus_engage,
    noesis_ui_element_get_is_focused, noesis_ui_element_get_is_keyboard_focus_within,
    noesis_ui_element_get_is_keyboard_focused, noesis_ui_element_get_is_mouse_captured,
    noesis_ui_element_get_key_states, noesis_ui_element_get_keyboard_focused,
    noesis_ui_element_get_modifiers, noesis_ui_element_get_mouse_captured,
    noesis_ui_element_get_mouse_position, noesis_ui_element_get_render_transform_origin,
    noesis_ui_element_is_key_down, noesis_ui_element_is_key_toggled, noesis_ui_element_is_key_up,
    noesis_ui_element_move_focus, noesis_ui_element_predict_focus,
    noesis_ui_element_predict_focus_name, noesis_ui_element_release_mouse_capture,
    noesis_ui_element_set_render_transform_origin, noesis_view_activate, noesis_view_add_reference,
    noesis_view_add_rendering_handler, noesis_view_cancel_timer, noesis_view_char,
    noesis_view_create, noesis_view_create_timer, noesis_view_deactivate, noesis_view_destroy,
    noesis_view_get_content, noesis_view_get_flags, noesis_view_get_renderer,
    noesis_view_get_stats, noesis_view_get_tessellation_max_pixel_error, noesis_view_hscroll,
    noesis_view_key_down, noesis_view_key_up, noesis_view_mouse_button_down,
    noesis_view_mouse_button_up, noesis_view_mouse_double_click, noesis_view_mouse_hwheel,
    noesis_view_mouse_move, noesis_view_mouse_wheel, noesis_view_remove_rendering_handler,
    noesis_view_restart_timer, noesis_view_scroll, noesis_view_set_double_tap_distance_threshold,
    noesis_view_set_double_tap_time_threshold, noesis_view_set_emulate_touch,
    noesis_view_set_flags, noesis_view_set_holding_distance_threshold,
    noesis_view_set_holding_time_threshold, noesis_view_set_manipulation_distance_threshold,
    noesis_view_set_projection_matrix, noesis_view_set_scale, noesis_view_set_size,
    noesis_view_set_stereo_offscreen_scale_factor, noesis_view_set_tessellation_max_pixel_error,
    noesis_view_touch_down, noesis_view_touch_move, noesis_view_touch_up, noesis_view_update,
    noesis_visual_child, noesis_visual_children_count, noesis_visual_hit_test,
    noesis_visual_hit_test_filtered, noesis_visual_parent, noesis_visual_state_go_to_state,
};
use crate::render_device::Registered as RegisteredDevice;
use crate::transforms::{Transform, Transform3D};

/// A loaded XAML root. Holds a +1 refcount on the underlying
/// `Noesis::FrameworkElement`; [`View::create`] consumes it and forwards the
/// ownership to the View.
pub struct FrameworkElement {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for FrameworkElement {}

/// A **borrowed** handle to a `Noesis::BindingExpression`: the live binding
/// instance on a target element's dependency property, obtained from
/// [`FrameworkElement::binding_expression`].
///
/// The expression is owned by the target element, NOT by this handle: it holds
/// no `+1` reference and runs no `Drop`. The `'a` lifetime ties it to the
/// `&FrameworkElement` it was borrowed from, so it cannot outlive that borrow.
/// It also becomes stale if the binding is cleared from the property while the
/// handle is held. Only call its methods while the binding is known live.
///
/// # Threading
///
/// These run on the view-driving thread, like the other accessors here
/// (no `VerifyAccess`).
pub struct BindingExpressionRef<'a> {
    ptr: NonNull<c_void>,
    _marker: PhantomData<&'a FrameworkElement>,
}

impl BindingExpressionRef<'_> {
    /// Force a source → target data transfer (re-pull the source value onto the
    /// target property), via `BaseBindingExpression::UpdateTarget`.
    pub fn update_target(&self) {
        // SAFETY: self.ptr is the borrowed BindingExpression* owned by the
        // target element, valid for the `'a` borrow this handle carries.
        unsafe { noesis_binding_expression_update_target(self.ptr.as_ptr()) }
    }

    /// Push the current target value back to the source, via
    /// `BaseBindingExpression::UpdateSource`. This is what commits a `TwoWay` /
    /// `OneWayToSource` binding whose
    /// [`UpdateSourceTrigger`](crate::binding::UpdateSourceTrigger) is
    /// `Explicit`; Noesis no-ops it for other binding modes.
    pub fn update_source(&self) {
        // SAFETY: as above; borrowed BindingExpression* valid for `'a`.
        unsafe { noesis_binding_expression_update_source(self.ptr.as_ptr()) }
    }
}

impl FrameworkElement {
    /// Load XAML by URI. Returns `None` when the URI is unknown to the
    /// installed `XamlProvider` or when the loaded root is not a
    /// `FrameworkElement`. Requires a provider installed via
    /// [`crate::xaml_provider::set_xaml_provider`].
    ///
    /// # Panics
    ///
    /// Panics if `uri` contains an interior NUL byte.
    #[must_use]
    pub fn load(uri: &str) -> Option<Self> {
        let c = CString::new(uri).expect("uri contained interior NUL");
        // SAFETY: c.as_ptr() is valid for the duration of the call; the
        // C ABI just copies into Noesis::Uri.
        let ptr = unsafe { noesis_gui_load_xaml(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Parse XAML directly from an in-memory string, without needing a
    /// [`XamlProvider`] or a URI. Returns `None` when the XAML is malformed
    /// or when the parsed root is not a `FrameworkElement` (e.g. a bare
    /// `ResourceDictionary`; use the application-resources helpers for those).
    ///
    /// This is the in-memory sibling of [`FrameworkElement::load`]: it backs
    /// `GUI::ParseXaml`. The returned element holds an independent `+1`
    /// reference, released on drop like any other `FrameworkElement` wrapper.
    ///
    /// [`XamlProvider`]: crate::xaml_provider::XamlProvider
    ///
    /// # Panics
    ///
    /// Panics if `xaml` contains an interior NUL byte.
    #[must_use]
    pub fn parse(xaml: &str) -> Option<Self> {
        let c = CString::new(xaml).expect("xaml contained interior NUL");
        // SAFETY: c.as_ptr() is valid for the duration of the call; the C ABI
        // only reads the bytes while parsing (synchronously). The result is a
        // freshly-created FrameworkElement* at +1, which `Self`'s Drop releases.
        let ptr = unsafe { noesis_gui_parse_xaml(c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    fn into_raw(self) -> *mut c_void {
        let ptr = self.ptr.as_ptr();
        core::mem::forget(self);
        ptr
    }

    /// Raw `Noesis::FrameworkElement*` for handing to other Noesis APIs that
    /// take one (e.g. event subscription). Borrowed for the lifetime of
    /// `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// Wrap a raw `BaseComponent*` that already carries a `+1` reference this
    /// handle takes ownership of (released on drop). Crate-internal: used by
    /// accessors that receive an owning pointer from the C side (e.g.
    /// [`crate::name_scope::NameScope::find_name`]).
    pub(crate) unsafe fn from_owned(ptr: NonNull<c_void>) -> Self {
        Self { ptr }
    }

    /// Current strong reference count of the underlying `BaseComponent`
    /// (`BaseRefCounted::GetNumReferences`). The absolute value is an internal
    /// detail. Use it for **deltas**: [`clone_ref`](Self::clone_ref) (and any
    /// Noesis-side retain) bumps it `+1`, dropping that handle (or a Noesis-side
    /// release) drops it `-1`. A live, owned handle always reports `>= 1`.
    #[must_use]
    pub fn num_references(&self) -> i32 {
        // SAFETY: self.ptr is a live BaseComponent* for the lifetime of self.
        unsafe { noesis_base_component_get_num_references(self.ptr.as_ptr()) }
    }

    /// Take a new owning handle to the same underlying component, bumping its
    /// reference count (`AddReference`). Useful for keeping a handle whose
    /// pointer was only borrowed, e.g. a hit-test visual handed to a callback.
    #[must_use]
    pub fn clone_ref(&self) -> Self {
        // SAFETY: self.ptr is a live BaseComponent*; the C side AddRef's and
        // returns it, and `Self`'s Drop releases the new reference.
        let p = unsafe { noesis_base_component_add_reference(self.ptr.as_ptr()) };
        Self {
            ptr: NonNull::new(p).expect("add_reference returned null on a live component"),
        }
    }

    /// Look up a descendant by `x:Name`. Returns `None` if no element with
    /// that name exists in this element's namescope, or if the named object
    /// is not itself a `FrameworkElement` (e.g. it's a `Brush` registered in
    /// a `ResourceDictionary`).
    ///
    /// The returned element holds an independent `+1` reference. Dropping
    /// it does not affect `self`.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn find_name(&self, name: &str) -> Option<Self> {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr is a live FrameworkElement*; c lives for the call.
        let ptr = unsafe { noesis_framework_element_find_name(self.ptr.as_ptr(), c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Borrow the [`BindingExpressionRef`] for the binding on this element's
    /// dependency property named `dp_name`, via
    /// `BindingOperations::GetBindingExpression`. Returns `None` if `dp_name`
    /// is unknown on this element's type, or if no binding is currently set on
    /// that property.
    ///
    /// The returned handle is **borrowed**: it is owned by this element and
    /// stays valid only while the binding is live and `self` is alive (the
    /// `'_` lifetime ties it to `&self`). Use it to drive an explicit
    /// `UpdateSource` / `UpdateTarget`, notably to commit a `TwoWay` binding
    /// whose [`UpdateSourceTrigger`](crate::binding::UpdateSourceTrigger) is
    /// `Explicit`.
    ///
    /// # Panics
    ///
    /// Panics if `dp_name` contains an interior NUL byte.
    #[must_use]
    pub fn binding_expression(&self, dp_name: &str) -> Option<BindingExpressionRef<'_>> {
        let c = CString::new(dp_name).expect("dp name contained interior NUL");
        // SAFETY: self.ptr is a live FrameworkElement*; c lives for the call.
        // The returned pointer is borrowed (owned by the target); never
        // released; its validity is bounded by the `'_` borrow of `self`.
        let ptr = unsafe { noesis_get_binding_expression(self.ptr.as_ptr(), c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| BindingExpressionRef {
            ptr,
            _marker: PhantomData,
        })
    }

    /// The element's `x:Name`, or `None` if it has no name. The returned
    /// string is a borrowed copy; Noesis owns the underlying storage.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C entrypoint
        // returns either NULL or a Noesis-owned static-ish string we copy
        // immediately.
        let p = unsafe { noesis_framework_element_get_name(self.ptr.as_ptr()) };
        if p.is_null() {
            None
        } else {
            // SAFETY: p is a NUL-terminated UTF-8 / ASCII string while we
            // hold our element reference; copy out before yielding control.
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }

    /// Set `Visibility` to `Visible` (`visible = true`) or `Collapsed`
    /// (`visible = false`). The third Noesis Visibility state (`Hidden`,
    /// where the element reserves layout space but doesn't paint)
    /// isn't surfaced; modal-overlay and panel-toggle patterns
    /// (the use cases driving this API) want full Collapsed behaviour.
    /// Add a separate setter if a consumer needs Hidden later.
    pub fn set_visibility(&mut self, visible: bool) {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side does a
        // null check + a typed `SetValue` on the `Visibility` DP. No
        // userdata or callbacks pass through.
        unsafe { noesis_framework_element_set_visibility(self.ptr.as_ptr(), visible) }
    }

    /// Set this element's `Margin` (layout offsets in DIPs: left, top, right,
    /// bottom). Paired with `HorizontalAlignment="Left"` /
    /// `VerticalAlignment="Top"`, a margin of `(x, y, 0, 0)` lands the element's
    /// top-left corner at `(x, y)`, the positioning primitive a floating
    /// menu / popup needs, since Noesis's `Canvas.Left`/`Top` attached property
    /// isn't surfaced through this shim.
    pub fn set_margin(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side null-checks
        // and does a typed `SetMargin(Thickness)`. No userdata or callbacks pass
        // through.
        unsafe {
            noesis_framework_element_set_margin(self.ptr.as_ptr(), left, top, right, bottom);
        }
    }

    /// Read the `Text` property of a `TextBox` or `TextBlock`, copying it
    /// into an owned [`String`]. Returns `None` if this element is neither
    /// a `TextBox` nor a `TextBlock`, or if the underlying text is null
    /// (Noesis returns null for an unset / never-touched Text DP).
    ///
    /// The pointer Noesis returns is borrowed; we copy immediately so the
    /// owned String stays valid past the next layout pass (which may
    /// reallocate the underlying storage).
    #[must_use]
    pub fn text(&self) -> Option<String> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side
        // DynamicCasts to TextBox/TextBlock and reads `GetText()`. The
        // returned pointer is null on type mismatch, otherwise a borrowed
        // NUL-terminated UTF-8 string from Noesis-owned storage.
        let p = unsafe { noesis_text_get(self.ptr.as_ptr()) };
        if p.is_null() {
            None
        } else {
            // SAFETY: p is a live NUL-terminated UTF-8 string while we
            // hold our element reference; copy out before yielding control.
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }

    /// Write the `Text` property of a `TextBox` or `TextBlock`. Returns
    /// `true` on success, `false` if this element is neither a `TextBox` nor
    /// a `TextBlock`.
    ///
    /// # Panics
    ///
    /// Panics if `text` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_text(&mut self, text: &str) -> bool {
        let c = CString::new(text).expect("text contained interior NUL");
        // SAFETY: self.ptr is a live FrameworkElement*; c.as_ptr() lives
        // for the call duration; the C side either copies into Noesis-
        // owned storage (TextBox::SetText / TextBlock::SetText) or returns
        // false on a type mismatch.
        unsafe { noesis_text_set(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Set the caret of a `TextBox` to the end of its current text. No-op
    /// (returns `false`) if the element is not a `TextBox`. Useful after
    /// replacing the text programmatically (e.g. a history-nav substitution).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_caret_to_end(&mut self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side does a
        // null check + DynamicCast + SetCaretIndex.
        unsafe { noesis_text_caret_to_end(self.ptr.as_ptr()) }
    }

    /// Move keyboard focus to this element. Returns the value Noesis
    /// reports for `UIElement::Focus()`: `true` if the element accepted
    /// focus, `false` if it's not a `UIElement` or is non-focusable.
    pub fn focus(&mut self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side does a
        // DynamicCast<UIElement*> + Focus().
        unsafe { noesis_focus_element(self.ptr.as_ptr()) }
    }

    // ── Input: finer control ────────────────────────────────────────────────
    //
    // Element-level mouse/touch capture, keyboard-state queries, focus-state
    // DPs, focus engagement, and focus traversal. All narrow this element to a
    // `UIElement` on the C side (returning `false` / `None` on a mismatch).
    // Capture and keyboard-state queries require the element to be connected to
    // a live `View` (Noesis's `GetMouse()` / `GetKeyboard()` are null until
    // then). Drive them after [`View::create`] + [`View::update`]. The value
    // types ([`ModifierKeys`], [`KeyStates`], [`FocusNavigationDirection`],
    // [`CaptureMode`]) live in [`crate::input`].

    /// Capture the mouse to this element (`UIElement::CaptureMouse`). Returns
    /// `true` if capture was taken. Requires a live `View`; returns `false` if
    /// this is not a `UIElement` or capture is refused.
    pub fn capture_mouse(&mut self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; C narrows to UIElement.
        unsafe { noesis_ui_element_capture_mouse(self.ptr.as_ptr()) }
    }

    /// Capture the mouse to this element with an explicit
    /// [`CaptureMode`](crate::input::CaptureMode) (`Element` vs `SubTree`), via
    /// the element's `Mouse::Capture`. `false` if not a `UIElement` or there is
    /// no live `View`.
    pub fn capture_mouse_mode(&mut self, mode: crate::input::CaptureMode) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; mode is a valid ordinal.
        unsafe { noesis_ui_element_capture_mouse_mode(self.ptr.as_ptr(), mode as i32) }
    }

    /// Release any mouse capture held by this element
    /// (`UIElement::ReleaseMouseCapture`). No-op if not captured.
    pub fn release_mouse_capture(&mut self) {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_release_mouse_capture(self.ptr.as_ptr()) }
    }

    /// Whether this element currently holds mouse capture
    /// (`UIElement::GetIsMouseCaptured`).
    #[must_use]
    pub fn is_mouse_captured(&self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_get_is_mouse_captured(self.ptr.as_ptr()) }
    }

    /// Capture the touch device `touch_device` to this element
    /// (`UIElement::CaptureTouch`). `false` if not a `UIElement` or refused.
    pub fn capture_touch(&mut self, touch_device: u64) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_capture_touch(self.ptr.as_ptr(), touch_device) }
    }

    /// The element currently holding mouse capture in this element's `View`
    /// (`Mouse::GetCaptured`), as a **borrowed** pointer (no `+1`). `None` if
    /// nothing is captured. Compare against another element's
    /// [`raw`](Self::raw) to identify it.
    #[must_use]
    pub fn mouse_captured(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live FrameworkElement*; borrowed or null.
        let p = unsafe { noesis_ui_element_get_mouse_captured(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// The pointer position relative to this element, in element-local DIPs
    /// (`Mouse::GetPosition(UIElement*)`), the WPF idiom for reading the cursor
    /// location *outside* a mouse event handler. `None` if this is not a
    /// `UIElement`.
    ///
    /// The value is the last position the `View`'s mouse recorded (updated by
    /// [`View::mouse_move`]), so it is meaningful only once the element is
    /// connected to a live, input-pumped `View`; before any pointer event it
    /// reads back as the element origin `(0.0, 0.0)`.
    #[must_use]
    pub fn mouse_position(&self) -> Option<(f32, f32)> {
        let (mut x, mut y) = (0.0f32, 0.0f32);
        // SAFETY: self.ptr is a live FrameworkElement*; both out params valid.
        unsafe { noesis_ui_element_get_mouse_position(self.ptr.as_ptr(), &mut x, &mut y) }
            .then_some((x, y))
    }

    /// The chord [`ModifierKeys`](crate::input::ModifierKeys) currently held, via
    /// this element's `Keyboard::GetModifiers`. `None` if not a `UIElement` or
    /// not attached to a `View`.
    #[must_use]
    pub fn modifiers(&self) -> Option<crate::input::ModifierKeys> {
        let mut out = 0;
        // SAFETY: self.ptr is a live FrameworkElement*; out is a valid i32.
        unsafe { noesis_ui_element_get_modifiers(self.ptr.as_ptr(), &mut out) }
            .then(|| crate::input::ModifierKeys::from_bits(out))
    }

    /// The [`KeyStates`](crate::input::KeyStates) of `key` via this element's
    /// `Keyboard::GetKeyStates`. `None` if not a `UIElement` / not attached.
    #[must_use]
    pub fn key_states(&self, key: Key) -> Option<crate::input::KeyStates> {
        let mut out = 0;
        // SAFETY: self.ptr is a live FrameworkElement*; out is a valid i32.
        unsafe { noesis_ui_element_get_key_states(self.ptr.as_ptr(), key as i32, &mut out) }
            .then(|| crate::input::KeyStates::from_bits(out))
    }

    /// Whether `key` is currently down (`Keyboard::IsKeyDown`). `false` if not a
    /// `UIElement` / not attached / the key is up.
    #[must_use]
    pub fn is_key_down(&self, key: Key) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_is_key_down(self.ptr.as_ptr(), key as i32) }
    }

    /// Whether `key` is currently up (`Keyboard::IsKeyUp`). An un-attached
    /// element reports `true` (no key held).
    #[must_use]
    pub fn is_key_up(&self, key: Key) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_is_key_up(self.ptr.as_ptr(), key as i32) }
    }

    /// Whether `key`'s toggle is on (`Keyboard::IsKeyToggled`, e.g. `CapsLock`).
    #[must_use]
    pub fn is_key_toggled(&self, key: Key) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_is_key_toggled(self.ptr.as_ptr(), key as i32) }
    }

    /// The element with keyboard focus in this element's `View`
    /// (`Keyboard::GetFocused`), as a **borrowed** pointer (no `+1`). `None` if
    /// nothing is focused. Compare against [`raw`](Self::raw).
    #[must_use]
    pub fn keyboard_focused(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live FrameworkElement*; borrowed or null.
        let p = unsafe { noesis_ui_element_get_keyboard_focused(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Whether this element has logical focus (`UIElement::GetIsFocused`).
    #[must_use]
    pub fn is_focused(&self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_get_is_focused(self.ptr.as_ptr()) }
    }

    /// Whether this element has keyboard focus
    /// (`UIElement::GetIsKeyboardFocused`).
    #[must_use]
    pub fn is_keyboard_focused(&self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_get_is_keyboard_focused(self.ptr.as_ptr()) }
    }

    /// Whether keyboard focus is on this element or a descendant
    /// (`UIElement::GetIsKeyboardFocusWithin`).
    #[must_use]
    pub fn is_keyboard_focus_within(&self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*.
        unsafe { noesis_ui_element_get_is_keyboard_focus_within(self.ptr.as_ptr()) }
    }

    /// Move keyboard focus to this element, optionally **engaging** it
    /// (`UIElement::Focus(bool engage)`). Engagement is the gamepad/console
    /// focus-engagement model: `engage = true` enters the element so directional
    /// input drives it rather than moving focus. Returns the focusable result.
    pub fn focus_engage(&mut self, engage: bool) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; C narrows to UIElement.
        unsafe { noesis_ui_element_focus_engage(self.ptr.as_ptr(), engage) }
    }

    /// Move focus away from this element in the given
    /// [`FocusNavigationDirection`](crate::input::FocusNavigationDirection)
    /// (`UIElement::MoveFocus`). `wrapped` lets traversal wrap around the ends.
    /// Returns `true` if focus moved.
    pub fn move_focus(
        &mut self,
        direction: crate::input::FocusNavigationDirection,
        wrapped: bool,
    ) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; direction a valid ordinal.
        unsafe { noesis_ui_element_move_focus(self.ptr.as_ptr(), direction as i32, wrapped) }
    }

    /// Predict the element focus would land on in `direction` without moving it
    /// (`UIElement::PredictFocus`), as a **borrowed** `DependencyObject*` (no
    /// `+1`). `None` if no candidate (or for the tab-order directions `Next` /
    /// `Previous` / `First` / `Last`, which `PredictFocus` does not support).
    #[must_use]
    pub fn predict_focus(
        &self,
        direction: crate::input::FocusNavigationDirection,
    ) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live FrameworkElement*; borrowed or null.
        let p = unsafe { noesis_ui_element_predict_focus(self.ptr.as_ptr(), direction as i32) };
        NonNull::new(p)
    }

    /// The `x:Name` of the element [`predict_focus`](Self::predict_focus) would
    /// land on in `direction`, or `None` if there is no candidate (or the target
    /// is unnamed / not a `FrameworkElement`). A convenience over `predict_focus`
    /// when you only need to identify the predicted element by name.
    #[must_use]
    pub fn predict_focus_name(
        &self,
        direction: crate::input::FocusNavigationDirection,
    ) -> Option<String> {
        // SAFETY: self.ptr is a live FrameworkElement*; borrowed const char* or
        // null. The string is Noesis-owned and borrowed only for this call, so
        // copy it out before yielding control.
        let p =
            unsafe { noesis_ui_element_predict_focus_name(self.ptr.as_ptr(), direction as i32) };
        if p.is_null() {
            return None;
        }
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// Assign this element's geometry (as a `Path`) to an open polyline through
    /// `points` (`[x, y]` pairs in the Path's local coordinate space). Returns
    /// `false` if the element is not a `Path` or there are fewer than two points.
    /// A real vector trace (built via a Noesis `StreamGeometry`), the geometry
    /// counterpart of [`set_text`](Self::set_text).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_path_points(&mut self, points: &[[f32; 2]]) -> bool {
        if points.len() < 2 {
            return false;
        }
        // `[[f32; 2]]` is contiguous x,y pairs, so it reinterprets as `2*len`
        // floats with no copy.
        let count = u32::try_from(points.len()).unwrap_or(u32::MAX);
        // SAFETY: self.ptr is a live FrameworkElement*; `points` lives for the
        // call and is exactly `2*count` contiguous f32s; the C side null-checks,
        // DynamicCasts to Path, and copies the points into a Noesis-owned
        // StreamGeometry before returning.
        unsafe { noesis_path_set_points(self.ptr.as_ptr(), points.as_ptr().cast::<f32>(), count) }
    }

    /// Transition this control to the visual state named `state`, via
    /// `VisualStateManager::GoToState`. Pass `use_transitions = true` to run
    /// the state's `VisualTransition` (animated change), or `false` to snap
    /// straight to the new state.
    ///
    /// This targets a templated control: `GoToState` resolves `state` against
    /// the `VisualStateGroup`s declared in the element's `ControlTemplate`
    /// (e.g. a `Button`'s `CommonStates`: `Normal` / `MouseOver` / `Pressed`
    /// / `Disabled`). Returns `false` if this element is not such a control,
    /// or if `state` names no group/state the control knows about.
    ///
    /// Like the other accessors here this has `View`-thread affinity (no
    /// `VerifyAccess()`); call it on the thread driving the `View`.
    ///
    /// # Panics
    ///
    /// Panics if `state` contains an interior NUL byte.
    pub fn go_to_state(&self, state: &str, use_transitions: bool) -> bool {
        let c = CString::new(state).expect("state contained interior NUL");
        // SAFETY: self.ptr is a live FrameworkElement*; c lives for the call;
        // the C side DynamicCasts to FrameworkElement*, interns the Symbol, and
        // calls VisualStateManager::GoToState, returning false on null / wrong
        // type / unknown state.
        unsafe { noesis_visual_state_go_to_state(self.ptr.as_ptr(), c.as_ptr(), use_transitions) }
    }

    // ── Generic dependency-property access ──────────────────────────────────
    //
    // Set / get any `DependencyProperty` on this element by name, mirroring the
    // index-keyed [`crate::classes::Instance`] accessors but resolving the
    // property from a name string (`FindDependencyProperty`) rather than a
    // dense registration index. The `PropType` tag the wrapper passes is
    // validated against the property's real reflected type on the C++ side, so
    // calling the wrong-typed accessor for a property fails gracefully
    // (returns `false` / `None`) instead of corrupting memory.
    //
    // Thread affinity: like every other accessor here (`text`, `set_margin`),
    // these do not call `VerifyAccess()` and must be used on the thread driving
    // the `View`. Getter results that borrow Noesis-owned storage (strings,
    // components) are copied / wrapped immediately before returning.

    /// Internal: resolve `name` to a C string and forward a typed set. Returns
    /// `false` if the property is unknown, the tag mismatches the real type, or
    /// the property is read-only.
    fn set_prop(&self, name: &str, kind: PropType, value_ptr: *const c_void) -> bool {
        let c = CString::new(name).expect("property name contained interior NUL");
        // SAFETY: self.ptr is a live DependencyObject*; c lives for the call;
        // `value_ptr` points at a stack value in the per-type FFI layout that
        // the C++ side reads synchronously (or null for "type default").
        unsafe {
            noesis_dependency_object_set_property(self.ptr.as_ptr(), c.as_ptr(), kind, value_ptr)
        }
    }

    /// Internal: resolve `name` to a C string and forward a typed get into
    /// `out`. Returns `false` on unknown name / tag mismatch / not-a-DO.
    fn get_prop(&self, name: &str, kind: PropType, out: *mut c_void) -> bool {
        let c = CString::new(name).expect("property name contained interior NUL");
        // SAFETY: self.ptr is a live DependencyObject*; c lives for the call;
        // `out` points at a buffer matching the per-type FFI layout.
        unsafe { noesis_dependency_object_get_property(self.ptr.as_ptr(), c.as_ptr(), kind, out) }
    }

    /// Set an `Int32` dependency property by name.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_i32(&mut self, name: &str, value: i32) -> bool {
        self.set_prop(name, PropType::Int32, (&value as *const i32).cast())
    }

    /// Set a `UInt32` dependency property by name. Noesis declares several
    /// integer DPs as `uint32_t` (notably the `Grid.Row` / `Grid.Column`
    /// family). The `Int32` tag rejects those, so reach for this instead.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_u32(&mut self, name: &str, value: u32) -> bool {
        self.set_prop(name, PropType::UInt32, (&value as *const u32).cast())
    }

    /// Set a `Float` (single-precision) dependency property by name. Most
    /// `FrameworkElement` scalars Noesis exposes (`Width`, `Height`,
    /// `Opacity`) are `float`, so this is the common case.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_f32(&mut self, name: &str, value: f32) -> bool {
        self.set_prop(name, PropType::Float, (&value as *const f32).cast())
    }

    /// Set a `Double` (double-precision) dependency property by name.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_f64(&mut self, name: &str, value: f64) -> bool {
        self.set_prop(name, PropType::Double, (&value as *const f64).cast())
    }

    /// Set a `Bool` dependency property by name.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_bool(&mut self, name: &str, value: bool) -> bool {
        self.set_prop(name, PropType::Bool, (&value as *const bool).cast())
    }

    /// Set a `String` dependency property by name. Noesis copies the bytes
    /// into its own storage.
    ///
    /// # Panics
    ///
    /// Panics if `name` or `value` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_string(&mut self, name: &str, value: &str) -> bool {
        let v = CString::new(value).expect("string value contained interior NUL");
        let ptr: *const i8 = v.as_ptr();
        self.set_prop(name, PropType::String, (&ptr as *const *const i8).cast())
    }

    /// Set a `Thickness` dependency property (`left, top, right, bottom`).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_thickness(&mut self, name: &str, value: [f32; 4]) -> bool {
        self.set_prop(name, PropType::Thickness, value.as_ptr().cast())
    }

    /// Set a `Color` dependency property (`r, g, b, a`, each in `0..=1`).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_color(&mut self, name: &str, rgba: [f32; 4]) -> bool {
        self.set_prop(name, PropType::Color, rgba.as_ptr().cast())
    }

    /// Set a `Rect` dependency property (`x, y, width, height`).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_rect(&mut self, name: &str, value: [f32; 4]) -> bool {
        self.set_prop(name, PropType::Rect, value.as_ptr().cast())
    }

    /// Read an `Int32` dependency property by name. `None` on unknown name or
    /// type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_i32(&self, name: &str) -> Option<i32> {
        let mut out: i32 = 0;
        self.get_prop(name, PropType::Int32, (&mut out as *mut i32).cast())
            .then_some(out)
    }

    /// Read a `UInt32` dependency property by name. `None` on unknown name or
    /// type mismatch. The `uint32_t` counterpart to [`get_i32`](Self::get_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_u32(&self, name: &str) -> Option<u32> {
        let mut out: u32 = 0;
        self.get_prop(name, PropType::UInt32, (&mut out as *mut u32).cast())
            .then_some(out)
    }

    /// Read a `Float` dependency property by name. `None` on unknown name or
    /// type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_f32(&self, name: &str) -> Option<f32> {
        let mut out: f32 = 0.0;
        self.get_prop(name, PropType::Float, (&mut out as *mut f32).cast())
            .then_some(out)
    }

    /// Read a `Double` dependency property by name. `None` on unknown name or
    /// type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_f64(&self, name: &str) -> Option<f64> {
        let mut out: f64 = 0.0;
        self.get_prop(name, PropType::Double, (&mut out as *mut f64).cast())
            .then_some(out)
    }

    /// Read a `Bool` dependency property by name. `None` on unknown name or
    /// type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_bool(&self, name: &str) -> Option<bool> {
        let mut out: bool = false;
        self.get_prop(name, PropType::Bool, (&mut out as *mut bool).cast())
            .then_some(out)
    }

    /// Read a `String` dependency property by name, copying it into an owned
    /// [`String`]. `None` on unknown name or type mismatch. The pointer Noesis
    /// returns is borrowed; we copy immediately so the result stays valid past
    /// the next layout pass.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_string(&self, name: &str) -> Option<String> {
        let mut p: *const i8 = core::ptr::null();
        if !self.get_prop(name, PropType::String, (&mut p as *mut *const i8).cast()) {
            return None;
        }
        if p.is_null() {
            return None;
        }
        // SAFETY: p is a live NUL-terminated UTF-8 string borrowed from
        // Noesis-owned storage while we hold our element reference; copy out
        // before yielding control.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// Read a `Thickness` dependency property as `[left, top, right, bottom]`.
    /// `None` on unknown name or type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_thickness(&self, name: &str) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        self.get_prop(name, PropType::Thickness, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Read a `Color` dependency property as `[r, g, b, a]` (each in `0..=1`).
    /// `None` on unknown name or type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_color(&self, name: &str) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        self.get_prop(name, PropType::Color, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Read a `Rect` dependency property as `[x, y, width, height]`. `None` on
    /// unknown name or type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_rect(&self, name: &str) -> Option<[f32; 4]> {
        let mut out = [0.0f32; 4];
        self.get_prop(name, PropType::Rect, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Set a `Point` dependency property (`[x, y]`).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_point(&mut self, name: &str, value: [f32; 2]) -> bool {
        self.set_prop(name, PropType::Point, value.as_ptr().cast())
    }

    /// Read a `Point` dependency property as `[x, y]`. `None` on unknown name or
    /// type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_point(&self, name: &str) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        self.get_prop(name, PropType::Point, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Set a `Size` dependency property (`[width, height]`).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_size(&mut self, name: &str, value: [f32; 2]) -> bool {
        self.set_prop(name, PropType::Size, value.as_ptr().cast())
    }

    /// Read a `Size` dependency property as `[width, height]`. `None` on unknown
    /// name or type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_size(&self, name: &str) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        self.get_prop(name, PropType::Size, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Set a `Vector` dependency property (`Noesis::Vector2`, `[x, y]`).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_vector(&mut self, name: &str, value: [f32; 2]) -> bool {
        self.set_prop(name, PropType::Vector, value.as_ptr().cast())
    }

    /// Read a `Vector` (`Noesis::Vector2`) dependency property as `[x, y]`.
    /// `None` on unknown name or type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_vector(&self, name: &str) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        self.get_prop(name, PropType::Vector, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Set an enum-typed dependency property by its underlying `int32` member
    /// value. The DP's reflected type must be a runtime enum.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_enum(&mut self, name: &str, value: i32) -> bool {
        self.set_prop(name, PropType::Enum, (&value as *const i32).cast())
    }

    /// Read an enum-typed dependency property as its underlying `int32` member
    /// value. `None` on unknown name or type mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_enum(&self, name: &str) -> Option<i32> {
        let mut out: i32 = 0;
        self.get_prop(name, PropType::Enum, (&mut out as *mut i32).cast())
            .then_some(out)
    }

    /// Read a reference-typed dependency property (any `BaseComponent`
    /// subclass: `Brush`, `ImageSource`, `Style`, ...) as a borrowed opaque
    /// pointer. `None` on unknown name, type mismatch, or a null value.
    ///
    /// The returned pointer is borrowed: it has no `+1` reference and must not
    /// be released. Treat it as valid only while this element is alive and the
    /// property is unchanged. Useful for checking whether a `Background` /
    /// `Source` is set, or feeding the pointer to another Noesis accessor
    /// (e.g. [`crate::classes::image_source_size`]).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_component(&self, name: &str) -> Option<NonNull<c_void>> {
        let mut p: *mut c_void = core::ptr::null_mut();
        if !self.get_prop(
            name,
            PropType::BaseComponent,
            (&mut p as *mut *mut c_void).cast(),
        ) {
            return None;
        }
        NonNull::new(p)
    }

    /// Read the color of the `SolidColorBrush` currently assigned to the
    /// Brush-typed dependency property `name` (e.g. `"Background"`,
    /// `"Foreground"`, `"Fill"`, `"Stroke"`) as `[r, g, b, a]` (each `0..=1`).
    ///
    /// `None` if the property is unset, the value is not a brush, or the brush
    /// is not a `SolidColorBrush` (e.g. a gradient). This is the read-back
    /// counterpart to the brush-assignment sugar
    /// ([`set_background`](Self::set_background) etc.): it lets a caller observe
    /// that a code-built [`SolidColorBrush`](crate::brushes::SolidColorBrush)
    /// actually landed on the element.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn solid_brush_color(&self, name: &str) -> Option<[f32; 4]> {
        let brush = self.get_component(name)?;
        let mut out = [0.0f32; 4];
        // SAFETY: `brush` is a live, borrowed `BaseComponent*` valid for this
        // call; the getter `DynamicCast`s to `SolidColorBrush` and returns
        // `false` (leaving `out` untouched) when the value is a different brush.
        let ok = unsafe { noesis_solid_color_brush_get_color(brush.as_ptr(), out.as_mut_ptr()) };
        ok.then_some(out)
    }

    // ── Data binding ────────────────────────────────────────────────────────
    //
    // Point this element's `DataContext` at a Rust view model, or an
    // ItemsControl's `ItemsSource` at an [`crate::binding::ObservableCollection`].
    // Bindings authored in XAML (`{Binding Path}`) then resolve against that
    // Rust-owned data. Same View-thread affinity as the other accessors here.

    /// Set this element's `DataContext` to a Rust-backed view model. Returns
    /// `false` if this element is not a `FrameworkElement`. Noesis stores its
    /// own reference to the instance, so it stays valid even after `instance`
    /// is dropped on the Rust side, though by convention the
    /// [`ClassInstance`](crate::classes::ClassInstance) is kept alive for as
    /// long as the binding is live.
    ///
    /// This is the safe entry point preferred by `unsafe`-free consumers (e.g.
    /// `noesis_bevy`, which is `unsafe_code = forbid`): the `&ClassInstance`
    /// borrow encodes the "live `BaseComponent`" invariant the raw setter
    /// demands. For an arbitrary `BaseComponent*` use [`Self::set_data_context_raw`].
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_data_context(&mut self, instance: &crate::classes::ClassInstance) -> bool {
        // SAFETY: `instance.raw()` is a live BaseComponent* for the duration of
        // the borrow, which fully covers this synchronous call.
        unsafe { self.set_data_context_raw(instance.raw()) }
    }

    /// Set this element's `DataContext` to an arbitrary `Noesis::BaseComponent*`.
    /// Returns `false` if this element is not a `FrameworkElement`. Noesis stores
    /// its own reference to `context`. Prefer the safe [`Self::set_data_context`]
    /// when the context is a [`ClassInstance`](crate::classes::ClassInstance).
    ///
    /// # Safety
    ///
    /// `context` must be a valid live `Noesis::BaseComponent*` (e.g. from
    /// [`crate::classes::ClassInstance::raw`]) or null to clear.
    pub unsafe fn set_data_context_raw(&mut self, context: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; `context` is a live
        // BaseComponent* (or null) per the contract above; the C side
        // DynamicCasts and null-checks.
        unsafe { noesis_framework_element_set_data_context(self.ptr.as_ptr(), context) }
    }

    /// Clear this element's `DataContext`.
    pub fn clear_data_context(&mut self) -> bool {
        // SAFETY: clearing with null is always sound.
        unsafe {
            noesis_framework_element_set_data_context(self.ptr.as_ptr(), core::ptr::null_mut())
        }
    }

    /// Borrowed (no `+1`) pointer to this element's current `DataContext`, or
    /// `None` if unset / not a `FrameworkElement`.
    #[must_use]
    pub fn data_context(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side returns a
        // borrowed pointer or null.
        let p = unsafe { noesis_framework_element_get_data_context(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Point this element's `ItemsSource` at a Rust-owned
    /// [`ObservableCollection`](crate::binding::ObservableCollection). The
    /// element must be an `ItemsControl` (e.g. `ItemsControl` / `ListBox` /
    /// `ListView` / `ComboBox`); returns `false` otherwise. Noesis stores its
    /// own reference to the collection.
    ///
    /// Safe entry point for `unsafe`-free consumers: the `&ObservableCollection`
    /// borrow encodes the live-`BaseComponent` invariant. Use
    /// [`Self::clear_items_source`] to detach, or [`Self::set_items_source_raw`]
    /// for an arbitrary list-implementing `BaseComponent*`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_items_source(&mut self, items: &crate::binding::ObservableCollection) -> bool {
        // SAFETY: `items.raw()` is a live ObservableCollection* (a BaseComponent
        // implementing a list interface) for the duration of the borrow.
        unsafe { self.set_items_source_raw(items.raw()) }
    }

    /// Detach this element's `ItemsSource`. Returns `false` if this element is
    /// not an `ItemsControl`. Clearing with null is always sound.
    pub fn clear_items_source(&mut self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; null is always valid.
        unsafe { noesis_items_control_set_items_source(self.ptr.as_ptr(), core::ptr::null_mut()) }
    }

    /// Set this element's `ItemsSource` to an arbitrary `Noesis::BaseComponent*`
    /// (it must be an `ItemsControl`). Returns `false` if this element is not an
    /// `ItemsControl`. Prefer the safe [`Self::set_items_source`] /
    /// [`Self::clear_items_source`].
    ///
    /// # Safety
    ///
    /// `items` must be a valid live `Noesis::BaseComponent*` implementing a
    /// list interface (e.g. an `ObservableCollection`) or null to clear.
    pub unsafe fn set_items_source_raw(&mut self, items: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; `items` is a live
        // BaseComponent* (or null) per the contract above.
        unsafe { noesis_items_control_set_items_source(self.ptr.as_ptr(), items) }
    }

    /// Number of items this `ItemsControl` sees through its bound source (a live
    /// passthrough to the `ItemsSource`). `None` if this element is not an
    /// `ItemsControl`.
    #[must_use]
    pub fn items_count(&self) -> Option<usize> {
        // SAFETY: self.ptr is a live FrameworkElement*.
        let n = unsafe { noesis_items_control_items_count(self.ptr.as_ptr()) };
        (n >= 0).then_some(n as usize)
    }

    /// Number of *realized* item containers the generator has materialized.
    /// Unlike [`items_count`](Self::items_count), this only grows when the
    /// generator regenerates, which for a source mutated after the first
    /// layout pass requires `INotifyCollectionChanged` to have fired. `None` if
    /// this element is not an `ItemsControl`.
    #[must_use]
    pub fn realized_item_count(&self) -> Option<usize> {
        // SAFETY: self.ptr is a live FrameworkElement*.
        let n = unsafe { noesis_items_control_realized_count(self.ptr.as_ptr()) };
        (n >= 0).then_some(n as usize)
    }

    // ── Tree traversal ──────────────────────────────────────────────────────
    //
    // Walk the visual and logical trees from this element. Returned elements
    // hold an independent `+1` reference (dropping them does not affect
    // `self`). Visual-tree children may be plain `Visual`s rather than
    // `FrameworkElement`s, but the wrapper is just an owned `BaseComponent*`
    // whose `FrameworkElement` methods `DynamicCast` internally, so a `Visual`
    // round-trips fine; its FE-specific accessors return `None` / no-op.

    /// Number of visual children. `0` if this element is not a `Visual`.
    #[must_use]
    pub fn visual_children_count(&self) -> u32 {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts
        // to Visual* and returns 0 on mismatch.
        unsafe { noesis_visual_children_count(self.ptr.as_ptr()) }
    }

    /// Visual child at `index`, or `None` if out of bounds / not a `Visual`.
    #[must_use]
    pub fn visual_child(&self, index: u32) -> Option<Self> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side bounds-checks
        // and hands back a +1 child or NULL.
        let ptr = unsafe { noesis_visual_child(self.ptr.as_ptr(), index) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Visual parent, or `None` at the visual-tree root / not a `Visual`.
    #[must_use]
    pub fn visual_parent(&self) -> Option<Self> {
        // SAFETY: self.ptr is a live BaseComponent*; +1 parent or NULL.
        let ptr = unsafe { noesis_visual_parent(self.ptr.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Hit-test a single point in this element's local coordinate space (DIPs).
    /// Returns the topmost hit element (`+1`), or `None` when nothing is hit /
    /// this element is not a `Visual`.
    #[must_use]
    pub fn hit_test(&self, x: f32, y: f32) -> Option<Self> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side runs
        // VisualTreeHelper::HitTest and hands back a +1 hit or NULL.
        let ptr = unsafe { noesis_visual_hit_test(self.ptr.as_ptr(), x, y) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Filtered hit test at a point in this element's local space (DIPs), the
    /// callback overload of `VisualTreeHelper::HitTest`. As the tree is walked
    /// top-down, `filter` is called for each visual to steer the descent (skip
    /// children/self, continue, or stop), and `result` is called for each hit
    /// to keep collecting or stop. Both receive a **borrowed**
    /// [`FrameworkElement`] valid only for that call; use
    /// [`clone_ref`](Self::clone_ref) to keep one.
    ///
    /// Runs synchronously on the view-driving thread. See [`Self::hit_test_all`]
    /// for the common "collect every hit" case.
    pub fn hit_test_filtered<F, R>(&self, x: f32, y: f32, mut filter: F, mut result: R)
    where
        F: FnMut(&FrameworkElement) -> HitTestFilterBehavior,
        R: FnMut(&FrameworkElement) -> HitTestResultBehavior,
    {
        // The closures are borrowed for the synchronous call only; bundle thin
        // trait-object refs behind one userdata pointer.
        struct Ctx<'a> {
            filter: &'a mut dyn FnMut(&FrameworkElement) -> HitTestFilterBehavior,
            result: &'a mut dyn FnMut(&FrameworkElement) -> HitTestResultBehavior,
        }

        // Wrap a borrowed visual pointer as a non-owning FrameworkElement (never
        // dropped, so no spurious Release) and hand it to `f`.
        unsafe fn with_borrowed<T>(
            visual: *mut c_void,
            f: impl FnOnce(&FrameworkElement) -> T,
            default: T,
        ) -> T {
            match NonNull::new(visual) {
                // SAFETY: from_owned just stores the ptr; ManuallyDrop prevents
                // the Release, preserving borrowed semantics for the callback.
                Some(p) => {
                    let elem =
                        core::mem::ManuallyDrop::new(unsafe { FrameworkElement::from_owned(p) });
                    f(&elem)
                }
                None => default,
            }
        }

        unsafe extern "C" fn filter_tramp(ud: *mut c_void, visual: *mut c_void) -> i32 {
            // A panicking user filter is contained and treated as "Continue"
            // (descend normally) rather than unwinding across the C ABI.
            crate::panic_guard::guard_or(HitTestFilterBehavior::Continue as i32, || {
                // SAFETY: ud is the &mut Ctx passed below, live for the call.
                let ctx = unsafe { &mut *ud.cast::<Ctx>() };
                let behavior = unsafe {
                    with_borrowed(visual, |e| (ctx.filter)(e), HitTestFilterBehavior::Continue)
                };
                behavior as i32
            })
        }
        unsafe extern "C" fn result_tramp(ud: *mut c_void, visual: *mut c_void) -> i32 {
            // A panicking user callback is contained and treated as "Continue".
            crate::panic_guard::guard_or(HitTestResultBehavior::Continue as i32, || {
                // SAFETY: ud is the &mut Ctx passed below, live for the call.
                let ctx = unsafe { &mut *ud.cast::<Ctx>() };
                let behavior = unsafe {
                    with_borrowed(visual, |e| (ctx.result)(e), HitTestResultBehavior::Continue)
                };
                behavior as i32
            })
        }

        let mut ctx = Ctx {
            filter: &mut filter,
            result: &mut result,
        };
        // SAFETY: trampolines match the C ABI; ctx outlives the synchronous
        // call; self.ptr is a live Visual* (or the C side no-ops on mismatch).
        unsafe {
            noesis_visual_hit_test_filtered(
                self.ptr.as_ptr(),
                x,
                y,
                filter_tramp,
                result_tramp,
                (&raw mut ctx).cast(),
            );
        }
    }

    /// Collect **every** visual hit at a point in this element's local space
    /// (DIPs), topmost-first, descending the whole subtree. Built on
    /// [`Self::hit_test_filtered`]; each returned element owns its own `+1`.
    #[must_use]
    pub fn hit_test_all(&self, x: f32, y: f32) -> Vec<FrameworkElement> {
        let mut hits = Vec::new();
        self.hit_test_filtered(
            x,
            y,
            |_| HitTestFilterBehavior::Continue,
            |hit| {
                hits.push(hit.clone_ref());
                HitTestResultBehavior::Continue
            },
        );
        hits
    }

    /// Logical parent (`FrameworkElement::GetParent`), or `None` if this is not
    /// a `FrameworkElement` or has no logical parent.
    #[must_use]
    pub fn logical_parent(&self) -> Option<Self> {
        // SAFETY: self.ptr is a live BaseComponent*; +1 parent or NULL.
        let ptr = unsafe { noesis_framework_element_logical_parent(self.ptr.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Number of logical children. `0` if this is not a `FrameworkElement`.
    #[must_use]
    pub fn logical_children_count(&self) -> u32 {
        // SAFETY: self.ptr is a live BaseComponent*; 0 on mismatch.
        unsafe { noesis_logical_children_count(self.ptr.as_ptr()) }
    }

    /// Logical child at `index`, or `None` if out of bounds / not a
    /// `FrameworkElement`.
    #[must_use]
    pub fn logical_child(&self, index: u32) -> Option<Self> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side bounds-checks
        // and hands back a +1 child or NULL.
        let ptr = unsafe { noesis_logical_child(self.ptr.as_ptr(), index) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Named part from this control's applied template
    /// (`FrameworkElement::GetTemplateChild`). `None` if this is not a
    /// `FrameworkElement` or no such named part exists.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn template_child(&self, name: &str) -> Option<Self> {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr is a live BaseComponent*; c lives for the call; the
        // C side AddRefs the result so Rust owns the +1.
        let ptr = unsafe { noesis_framework_element_template_child(self.ptr.as_ptr(), c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    // ── Attached properties ─────────────────────────────────────────────────
    //
    // Resolve a DependencyProperty registered on `owner` (e.g. `Grid` / `Row`,
    // `Canvas` / `Left`) and set / get it on this object. The owner type must
    // already be registered with Noesis Reflection (referencing it from XAML
    // forces registration). Same per-tag validation as the generic accessors.

    /// Internal: forward a typed attached-property set.
    fn set_attached(
        &self,
        owner: &str,
        prop: &str,
        kind: PropType,
        value_ptr: *const c_void,
    ) -> bool {
        let o = CString::new(owner).expect("owner type contained interior NUL");
        let p = CString::new(prop).expect("attached property name contained interior NUL");
        // SAFETY: self.ptr is a live DependencyObject*; both C strings live for
        // the call; `value_ptr` matches the per-tag FFI layout.
        unsafe {
            noesis_dependency_object_set_attached(
                self.ptr.as_ptr(),
                o.as_ptr(),
                p.as_ptr(),
                kind,
                value_ptr,
            )
        }
    }

    /// Internal: forward a typed attached-property get into `out`.
    fn get_attached(&self, owner: &str, prop: &str, kind: PropType, out: *mut c_void) -> bool {
        let o = CString::new(owner).expect("owner type contained interior NUL");
        let p = CString::new(prop).expect("attached property name contained interior NUL");
        // SAFETY: self.ptr is a live DependencyObject*; both C strings live for
        // the call; `out` matches the per-tag FFI layout.
        unsafe {
            noesis_dependency_object_get_attached(
                self.ptr.as_ptr(),
                o.as_ptr(),
                p.as_ptr(),
                kind,
                out,
            )
        }
    }

    /// Set an `Int32` attached property (e.g. `Grid` / `Row`). `false` on
    /// unknown owner / property, tag mismatch, or read-only.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `prop` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_attached_i32(&mut self, owner: &str, prop: &str, v: i32) -> bool {
        self.set_attached(owner, prop, PropType::Int32, (&v as *const i32).cast())
    }

    /// Read an `Int32` attached property. `None` on unknown owner / property or
    /// tag mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `prop` contains an interior NUL byte.
    #[must_use]
    pub fn get_attached_i32(&self, owner: &str, prop: &str) -> Option<i32> {
        let mut out: i32 = 0;
        self.get_attached(owner, prop, PropType::Int32, (&mut out as *mut i32).cast())
            .then_some(out)
    }

    /// Set a `UInt32` attached property. The `uint32_t` counterpart to
    /// [`set_attached_i32`](Self::set_attached_i32), needed for the
    /// `Grid.Row` / `Grid.Column` / `Grid.RowSpan` / `Grid.ColumnSpan` family,
    /// which Noesis declares as `uint32_t`. `false` on unknown owner /
    /// property, tag mismatch, or read-only.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `prop` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_attached_u32(&mut self, owner: &str, prop: &str, v: u32) -> bool {
        self.set_attached(owner, prop, PropType::UInt32, (&v as *const u32).cast())
    }

    /// Read a `UInt32` attached property (e.g. `Grid` / `Row`). `None` on
    /// unknown owner / property or tag mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `prop` contains an interior NUL byte.
    #[must_use]
    pub fn get_attached_u32(&self, owner: &str, prop: &str) -> Option<u32> {
        let mut out: u32 = 0;
        self.get_attached(owner, prop, PropType::UInt32, (&mut out as *mut u32).cast())
            .then_some(out)
    }

    /// Set a `Float` attached property (e.g. `Canvas` / `Left`).
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `prop` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_attached_f32(&mut self, owner: &str, prop: &str, v: f32) -> bool {
        self.set_attached(owner, prop, PropType::Float, (&v as *const f32).cast())
    }

    /// Read a `Float` attached property. `None` on unknown owner / property or
    /// tag mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `prop` contains an interior NUL byte.
    #[must_use]
    pub fn get_attached_f32(&self, owner: &str, prop: &str) -> Option<f32> {
        let mut out: f32 = 0.0;
        self.get_attached(owner, prop, PropType::Float, (&mut out as *mut f32).cast())
            .then_some(out)
    }

    /// Set a `Bool` attached property.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `prop` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_attached_bool(&mut self, owner: &str, prop: &str, v: bool) -> bool {
        self.set_attached(owner, prop, PropType::Bool, (&v as *const bool).cast())
    }

    /// Read a `Bool` attached property. `None` on unknown owner / property or
    /// tag mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `owner` or `prop` contains an interior NUL byte.
    #[must_use]
    pub fn get_attached_bool(&self, owner: &str, prop: &str) -> Option<bool> {
        let mut out: bool = false;
        self.get_attached(owner, prop, PropType::Bool, (&mut out as *mut bool).cast())
            .then_some(out)
    }

    // ── ClearValue / SetCurrentValue / GetBaseValue ─────────────────────────

    /// Clear the local value of the named dependency property
    /// (`ClearLocalValue`), reverting it to its default / inherited / styled
    /// value. `false` if the property is unknown or read-only.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn clear_value(&mut self, name: &str) -> bool {
        let c = CString::new(name).expect("property name contained interior NUL");
        // SAFETY: self.ptr is a live DependencyObject*; c lives for the call.
        unsafe { noesis_dependency_object_clear_value(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Internal: forward a typed `SetCurrentValue`.
    fn set_current(&self, name: &str, kind: PropType, value_ptr: *const c_void) -> bool {
        let c = CString::new(name).expect("property name contained interior NUL");
        // SAFETY: self.ptr is a live DependencyObject*; c lives for the call;
        // `value_ptr` matches the per-tag FFI layout.
        unsafe {
            noesis_dependency_object_set_current_value(
                self.ptr.as_ptr(),
                c.as_ptr(),
                kind,
                value_ptr,
            )
        }
    }

    /// Internal: forward a typed `GetBaseValue` into `out`.
    fn get_base(&self, name: &str, kind: PropType, out: *mut c_void) -> bool {
        let c = CString::new(name).expect("property name contained interior NUL");
        // SAFETY: self.ptr is a live DependencyObject*; c lives for the call;
        // `out` matches the per-tag FFI layout.
        unsafe { noesis_dependency_object_get_base_value(self.ptr.as_ptr(), c.as_ptr(), kind, out) }
    }

    /// Set the current value of an `Int32` dependency property
    /// (`SetCurrentValue`: sets the coerced value without overwriting the
    /// local / source value).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_i32(&mut self, name: &str, value: i32) -> bool {
        self.set_current(name, PropType::Int32, (&value as *const i32).cast())
    }

    /// Set the current value of a `UInt32` dependency property. See
    /// [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_u32(&mut self, name: &str, value: u32) -> bool {
        self.set_current(name, PropType::UInt32, (&value as *const u32).cast())
    }

    /// Set the current value of a `Float` dependency property. See
    /// [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_f32(&mut self, name: &str, value: f32) -> bool {
        self.set_current(name, PropType::Float, (&value as *const f32).cast())
    }

    /// Set the current value of a `Double` dependency property. See
    /// [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_f64(&mut self, name: &str, value: f64) -> bool {
        self.set_current(name, PropType::Double, (&value as *const f64).cast())
    }

    /// Set the current value of a `Bool` dependency property. See
    /// [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_bool(&mut self, name: &str, value: bool) -> bool {
        self.set_current(name, PropType::Bool, (&value as *const bool).cast())
    }

    /// Set the current value of a `String` dependency property. See
    /// [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` or `value` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_string(&mut self, name: &str, value: &str) -> bool {
        let v = CString::new(value).expect("string value contained interior NUL");
        let ptr: *const i8 = v.as_ptr();
        self.set_current(name, PropType::String, (&ptr as *const *const i8).cast())
    }

    /// Set the current value of a `Point` dependency property. See
    /// [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_point(&mut self, name: &str, value: [f32; 2]) -> bool {
        self.set_current(name, PropType::Point, value.as_ptr().cast())
    }

    /// Set the current value of a `Size` dependency property. See
    /// [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_size(&mut self, name: &str, value: [f32; 2]) -> bool {
        self.set_current(name, PropType::Size, value.as_ptr().cast())
    }

    /// Set the current value of a `Vector` (`Noesis::Vector2`) dependency
    /// property. See [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_vector(&mut self, name: &str, value: [f32; 2]) -> bool {
        self.set_current(name, PropType::Vector, value.as_ptr().cast())
    }

    /// Set the current value of a runtime-enum-typed dependency property (the
    /// underlying `int32` member value). See
    /// [`set_current_i32`](Self::set_current_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_current_enum(&mut self, name: &str, value: i32) -> bool {
        self.set_current(name, PropType::Enum, (&value as *const i32).cast())
    }

    /// Read the base value (pre-animation / pre-coerce) of an `Int32`
    /// dependency property (`GetBaseValue`). `None` on unknown name or tag
    /// mismatch.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_i32(&self, name: &str) -> Option<i32> {
        let mut out: i32 = 0;
        self.get_base(name, PropType::Int32, (&mut out as *mut i32).cast())
            .then_some(out)
    }

    /// Read the base value of a `UInt32` dependency property. See
    /// [`get_base_i32`](Self::get_base_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_u32(&self, name: &str) -> Option<u32> {
        let mut out: u32 = 0;
        self.get_base(name, PropType::UInt32, (&mut out as *mut u32).cast())
            .then_some(out)
    }

    /// Read the base value of a `Float` dependency property. See
    /// [`get_base_i32`](Self::get_base_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_f32(&self, name: &str) -> Option<f32> {
        let mut out: f32 = 0.0;
        self.get_base(name, PropType::Float, (&mut out as *mut f32).cast())
            .then_some(out)
    }

    /// Read the base value of a `Double` dependency property. See
    /// [`get_base_i32`](Self::get_base_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_f64(&self, name: &str) -> Option<f64> {
        let mut out: f64 = 0.0;
        self.get_base(name, PropType::Double, (&mut out as *mut f64).cast())
            .then_some(out)
    }

    /// Read the base value of a `Bool` dependency property. See
    /// [`get_base_i32`](Self::get_base_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_bool(&self, name: &str) -> Option<bool> {
        let mut out: bool = false;
        self.get_base(name, PropType::Bool, (&mut out as *mut bool).cast())
            .then_some(out)
    }

    /// Read the base value of a `String` dependency property, copying it into
    /// an owned [`String`]. See [`get_base_i32`](Self::get_base_i32). The
    /// pointer Noesis returns is borrowed; we copy immediately.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_string(&self, name: &str) -> Option<String> {
        let mut p: *const i8 = core::ptr::null();
        if !self.get_base(name, PropType::String, (&mut p as *mut *const i8).cast()) {
            return None;
        }
        if p.is_null() {
            return None;
        }
        // SAFETY: p is a live NUL-terminated UTF-8 string borrowed from
        // Noesis-owned storage while we hold our element reference; copy out
        // before yielding control.
        Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
    }

    /// Read the base value of a `Point` dependency property as `[x, y]`. See
    /// [`get_base_i32`](Self::get_base_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_point(&self, name: &str) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        self.get_base(name, PropType::Point, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Read the base value of a `Size` dependency property as `[width, height]`.
    /// See [`get_base_i32`](Self::get_base_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_size(&self, name: &str) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        self.get_base(name, PropType::Size, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Read the base value of a `Vector` (`Noesis::Vector2`) dependency property
    /// as `[x, y]`. See [`get_base_i32`](Self::get_base_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_vector(&self, name: &str) -> Option<[f32; 2]> {
        let mut out = [0.0f32; 2];
        self.get_base(name, PropType::Vector, out.as_mut_ptr().cast())
            .then_some(out)
    }

    /// Read the base value of a runtime-enum-typed dependency property as its
    /// underlying `int32` member value. See [`get_base_i32`](Self::get_base_i32).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_base_enum(&self, name: &str) -> Option<i32> {
        let mut out: i32 = 0;
        self.get_base(name, PropType::Enum, (&mut out as *mut i32).cast())
            .then_some(out)
    }

    // ── Dynamic tag inference ───────────────────────────────────────────────

    /// The [`PropType`] tag of the named dependency property, or `None` if this
    /// is not a `DependencyObject`, the property is unknown, or its reflected
    /// type maps to no tag. The inverse of the validation the typed setters
    /// apply.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn property_tag(&self, name: &str) -> Option<PropType> {
        let c = CString::new(name).expect("property name contained interior NUL");
        // SAFETY: self.ptr is a live BaseComponent*; c lives for the call; the
        // C side returns -1 or a valid tag ordinal.
        let tag = unsafe { noesis_dependency_object_property_tag(self.ptr.as_ptr(), c.as_ptr()) };
        match tag {
            0 => Some(PropType::Int32),
            1 => Some(PropType::Float),
            2 => Some(PropType::Double),
            3 => Some(PropType::Bool),
            4 => Some(PropType::String),
            5 => Some(PropType::Thickness),
            6 => Some(PropType::Color),
            7 => Some(PropType::Rect),
            8 => Some(PropType::ImageSource),
            9 => Some(PropType::BaseComponent),
            10 => Some(PropType::UInt32),
            11 => Some(PropType::Point),
            12 => Some(PropType::Size),
            13 => Some(PropType::Vector),
            14 => Some(PropType::Enum),
            _ => None,
        }
    }

    /// Read the named dependency property as a [`DynValue`], inferring the type
    /// via [`property_tag`](Self::property_tag) and dispatching to the matching
    /// typed getter. `None` if the property is unknown / untyped, or the
    /// resolved value is null (for component types).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn get_dynamic(&self, name: &str) -> Option<DynValue> {
        match self.property_tag(name)? {
            PropType::Int32 => self.get_i32(name).map(DynValue::I32),
            PropType::UInt32 => self.get_u32(name).map(DynValue::U32),
            PropType::Float => self.get_f32(name).map(DynValue::F32),
            PropType::Double => self.get_f64(name).map(DynValue::F64),
            PropType::Bool => self.get_bool(name).map(DynValue::Bool),
            PropType::String => self.get_string(name).map(DynValue::Str),
            PropType::Thickness => self.get_thickness(name).map(DynValue::Thickness),
            PropType::Color => self.get_color(name).map(DynValue::Color),
            PropType::Rect => self.get_rect(name).map(DynValue::Rect),
            PropType::Point => self.get_point(name).map(DynValue::Point),
            PropType::Size => self.get_size(name).map(DynValue::Size),
            PropType::Vector => self.get_vector(name).map(DynValue::Vector),
            PropType::Enum => self.get_enum(name).map(DynValue::Enum),
            PropType::ImageSource | PropType::BaseComponent => {
                self.get_component(name).map(DynValue::Component)
            }
        }
    }

    // ── Typed FrameworkElement sugar ────────────────────────────────────────
    //
    // Thin wrappers over the generic name-keyed accessors for the common
    // `FrameworkElement` scalars, plus a bespoke alignment path (the alignment
    // enums don't match the generic INT32 tag's reflected type).

    /// Rendered width after the last layout pass (`ActualWidth`, read-only).
    #[must_use]
    pub fn actual_width(&self) -> Option<f32> {
        self.get_f32("ActualWidth")
    }

    /// Rendered height after the last layout pass (`ActualHeight`, read-only).
    #[must_use]
    pub fn actual_height(&self) -> Option<f32> {
        self.get_f32("ActualHeight")
    }

    /// Requested `Width` (may be `NaN` for "auto").
    #[must_use]
    pub fn width(&self) -> Option<f32> {
        self.get_f32("Width")
    }

    /// Set the requested `Width`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_width(&mut self, value: f32) -> bool {
        self.set_f32("Width", value)
    }

    /// Requested `Height` (may be `NaN` for "auto").
    #[must_use]
    pub fn height(&self) -> Option<f32> {
        self.get_f32("Height")
    }

    /// Set the requested `Height`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_height(&mut self, value: f32) -> bool {
        self.set_f32("Height", value)
    }

    /// `Opacity` in `0.0..=1.0`.
    #[must_use]
    pub fn opacity(&self) -> Option<f32> {
        self.get_f32("Opacity")
    }

    /// Set `Opacity`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_opacity(&mut self, value: f32) -> bool {
        self.set_f32("Opacity", value)
    }

    /// `IsEnabled`: whether the element accepts input.
    #[must_use]
    pub fn is_enabled(&self) -> Option<bool> {
        self.get_bool("IsEnabled")
    }

    /// Set `IsEnabled`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_enabled(&mut self, value: bool) -> bool {
        self.set_bool("IsEnabled", value)
    }

    /// `Focusable`: whether the element can receive keyboard focus.
    #[must_use]
    pub fn focusable(&self) -> Option<bool> {
        self.get_bool("Focusable")
    }

    /// Set `Focusable`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_focusable(&mut self, value: bool) -> bool {
        self.set_bool("Focusable", value)
    }

    /// The element's `Tag` (an arbitrary `BaseComponent`), as a borrowed opaque
    /// pointer with no `+1` reference. `None` if unset. See
    /// [`get_component`](Self::get_component) for the borrow contract.
    #[must_use]
    pub fn tag(&self) -> Option<NonNull<c_void>> {
        self.get_component("Tag")
    }

    /// Set the element's `Tag` to another live element (stored as a
    /// `BaseComponent`). Noesis stores its own reference. Returns `false` on a
    /// tag mismatch (should not happen for `Tag`) or if this is not a
    /// `DependencyObject`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_tag(&mut self, value: &Self) -> bool {
        // SAFETY: `value` is a live FrameworkElement we borrow for the call.
        unsafe { self.set_component("Tag", value.ptr.as_ptr()) }
    }

    /// Set a reference-typed (`BaseComponent`) dependency property by name to a
    /// raw `Noesis::BaseComponent*`. Noesis stores its own reference; pass null
    /// to clear. `false` on unknown name, tag mismatch, or read-only.
    ///
    /// # Safety
    ///
    /// `value` must be a valid live `Noesis::BaseComponent*` or null.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub unsafe fn set_component(&mut self, name: &str, value: *mut c_void) -> bool {
        // The BASE_COMPONENT layout is a pointer-to-pointer (`BaseComponent**`).
        self.set_prop(
            name,
            PropType::BaseComponent,
            (&value as *const *mut c_void).cast(),
        )
    }

    /// Assign a command (any [`AsCommand`](crate::commands::AsCommand): a
    /// [`Command`](crate::commands::Command),
    /// [`RoutedCommand`](crate::commands::RoutedCommand),
    /// [`RoutedUICommand`](crate::commands::RoutedUICommand), or built-in
    /// [`BorrowedCommand`](crate::commands::BorrowedCommand)) to the
    /// `BaseComponent`-typed dependency property `name`. Noesis stores its own
    /// reference, so `command` may be dropped afterwards (the bound object keeps
    /// it alive). Returns `false` if `name` is unknown on this element's type or
    /// is not a `BaseComponent`-typed DP.
    ///
    /// This is the safe, `unsafe`-free element counterpart of
    /// [`Instance::set_command`](crate::classes::Instance::set_command): the
    /// `&impl AsCommand` borrow encodes the live-`BaseComponent` invariant the
    /// raw [`set_component`](Self::set_component) demands.
    ///
    /// This reaches a built-in control's `Command` directly, e.g.
    /// `button.set_command("Command", &cmd)` wires a Rust command to a `Button`
    /// from code without a `DataContext` binding. To drive it from a view model
    /// instead, register a `BaseComponent` property, point it with
    /// [`Instance::set_command`](crate::classes::Instance::set_command), and bind
    /// `Command="{Binding ...}"`, the route the [`crate::commands`] module
    /// documents.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use = "a false return means the command was not set (unknown name / not a BaseComponent DP)"]
    pub fn set_command(&mut self, name: &str, command: &impl crate::commands::AsCommand) -> bool {
        // SAFETY: `command.command_ptr()` is a live ICommand* (a BaseComponent*
        // at runtime) borrowed for the duration of the call; the DP stores its
        // own reference.
        unsafe { self.set_component(name, command.command_ptr()) }
    }

    /// `HorizontalAlignment`, or `None` if this is not a `FrameworkElement`.
    #[must_use]
    pub fn horizontal_alignment(&self) -> Option<HAlign> {
        // SAFETY: self.ptr is a live BaseComponent*; -1 on non-FE.
        let v = unsafe { noesis_framework_element_get_halign(self.ptr.as_ptr()) };
        HAlign::from_ordinal(v)
    }

    /// Set `HorizontalAlignment`. No-op if this is not a `FrameworkElement`.
    pub fn set_horizontal_alignment(&mut self, a: HAlign) {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts
        // and no-ops on mismatch.
        unsafe { noesis_framework_element_set_halign(self.ptr.as_ptr(), a as i32) }
    }

    /// `VerticalAlignment`, or `None` if this is not a `FrameworkElement`.
    #[must_use]
    pub fn vertical_alignment(&self) -> Option<VAlign> {
        // SAFETY: self.ptr is a live BaseComponent*; -1 on non-FE.
        let v = unsafe { noesis_framework_element_get_valign(self.ptr.as_ptr()) };
        VAlign::from_ordinal(v)
    }

    /// Set `VerticalAlignment`. No-op if this is not a `FrameworkElement`.
    pub fn set_vertical_alignment(&mut self, a: VAlign) {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts
        // and no-ops on mismatch.
        unsafe { noesis_framework_element_set_valign(self.ptr.as_ptr(), a as i32) }
    }

    // ── Namescope register / unregister ─────────────────────────────────────

    /// Register `name` for `object` in the namescope hosting this element, so
    /// that subsequent [`find_name`](Self::find_name) lookups resolve it. The
    /// scope takes its own reference to `object`. Returns `false` if this is
    /// not a `FrameworkElement`. Registering a name already present updates it.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn register_name(&mut self, name: &str, object: &Self) -> bool {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr is a live BaseComponent*; c lives for the call;
        // `object` is a live element we borrow; the scope stores its own ref.
        unsafe {
            noesis_framework_element_register_name(
                self.ptr.as_ptr(),
                c.as_ptr(),
                object.ptr.as_ptr(),
            )
        }
    }

    /// Remove `name` from the namescope hosting this element. Returns `false`
    /// if this is not a `FrameworkElement`.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn unregister_name(&mut self, name: &str) -> bool {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr is a live BaseComponent*; c lives for the call.
        unsafe { noesis_framework_element_unregister_name(self.ptr.as_ptr(), c.as_ptr()) }
    }

    // ── Thread affinity ─────────────────────────────────────────────────────

    /// Whether the calling thread owns this object
    /// (`DispatcherObject::CheckAccess`). `false` if this is not a
    /// `DispatcherObject`.
    #[must_use]
    pub fn check_access(&self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; false on non-DO.
        unsafe { noesis_dependency_object_check_access(self.ptr.as_ptr()) }
    }

    /// The id of the thread this object is attached to
    /// (`DispatcherObject::GetThreadId`). `u32::MAX` when unattached or not a
    /// `DispatcherObject`.
    #[must_use]
    pub fn thread_id(&self) -> u32 {
        // SAFETY: self.ptr is a live BaseComponent*; UINT32_MAX on non-DO.
        unsafe { noesis_dependency_object_thread_id(self.ptr.as_ptr()) }
    }

    // ── Brushes / transforms / effects / RenderOptions ───────────────────────
    //
    // Thin typed sugar over the generic `set_component` DP path for the
    // code-built objects in `crate::brushes` / `crate::transforms`. Each routes
    // the object's borrowed `BaseComponent*` into the named DP; Noesis takes its
    // own reference, so the builder handle may be dropped right after the call.
    // These all return `false` if the property is absent on this element type
    // (e.g. `set_fill` on a `Border`, which has no `Fill`).

    /// Paint this element's `Background` with `brush` (a `Border`, `Panel`,
    /// `Control`, ...). Returns `false` if the element has no `Background` DP.
    pub fn set_background<B: Brush>(&mut self, brush: &B) -> bool {
        // SAFETY: brush.brush_raw() is a live Brush* borrowed for the call;
        // Noesis stores its own reference.
        unsafe { self.set_component("Background", brush.brush_raw()) }
    }

    /// Paint this element's `Foreground` with `brush` (text controls). Returns
    /// `false` if the element has no `Foreground` DP.
    pub fn set_foreground<B: Brush>(&mut self, brush: &B) -> bool {
        // SAFETY: brush.brush_raw() is a live Brush* borrowed for the call.
        unsafe { self.set_component("Foreground", brush.brush_raw()) }
    }

    /// Paint this `Shape`'s `Fill` with `brush` (e.g. a `Rectangle`,
    /// `Ellipse`). Returns `false` if the element has no `Fill` DP.
    pub fn set_fill<B: Brush>(&mut self, brush: &B) -> bool {
        // SAFETY: brush.brush_raw() is a live Brush* borrowed for the call.
        unsafe { self.set_component("Fill", brush.brush_raw()) }
    }

    /// Paint this `Shape`'s `Stroke` with `brush`. Returns `false` if the
    /// element has no `Stroke` DP.
    pub fn set_stroke<B: Brush>(&mut self, brush: &B) -> bool {
        // SAFETY: brush.brush_raw() is a live Brush* borrowed for the call.
        unsafe { self.set_component("Stroke", brush.brush_raw()) }
    }

    /// Set this element's `RenderTransform` to `transform`. Returns `false` if
    /// the element has no `RenderTransform` DP (i.e. is not a `UIElement`).
    pub fn set_render_transform<T: Transform>(&mut self, transform: &T) -> bool {
        // SAFETY: transform.transform_raw() is a live Transform* borrowed for
        // the call; Noesis stores its own reference.
        unsafe { self.set_component("RenderTransform", transform.transform_raw()) }
    }

    /// This element's current `RenderTransform` (`UIElement::GetRenderTransform`)
    /// as an owning, type-erased [`AnyTransform`](crate::transforms::AnyTransform),
    /// or `None` if no transform is set / this is not a `UIElement`. The handle
    /// can be re-applied to another element via [`Self::set_render_transform`].
    #[must_use]
    pub fn render_transform(&self) -> Option<crate::transforms::AnyTransform> {
        // get_component hands back a borrowed pointer; AddRef so the returned
        // handle owns its reference.
        let borrowed = self.get_component("RenderTransform")?;
        // SAFETY: borrowed is a live Transform* (BaseComponent*); AddRef and
        // wrap as an owning handle whose Drop releases the new reference.
        let owned = unsafe { noesis_base_component_add_reference(borrowed.as_ptr()) };
        NonNull::new(owned).map(|p| unsafe { crate::transforms::AnyTransform::from_owned(p) })
    }

    /// This element's `RenderTransformOrigin` as `(x, y)` in the relative
    /// `0.0..=1.0` space (`UIElement::GetRenderTransformOrigin`). Returns
    /// `(0.0, 0.0)` when this is not a `UIElement`.
    #[must_use]
    pub fn render_transform_origin(&self) -> (f32, f32) {
        let mut x = 0.0_f32;
        let mut y = 0.0_f32;
        // SAFETY: self.ptr is a live BaseComponent*; both out-pointers are valid
        // for the call and always written by the C side.
        unsafe {
            noesis_ui_element_get_render_transform_origin(
                self.ptr.as_ptr(),
                &raw mut x,
                &raw mut y,
            );
        }
        (x, y)
    }

    /// Set this element's `RenderTransformOrigin` (the relative `0.0..=1.0`
    /// pivot the render transform rotates/scales around). Returns `false` if
    /// this is not a `UIElement`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_render_transform_origin(&mut self, x: f32, y: f32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; thin pass-through.
        unsafe { noesis_ui_element_set_render_transform_origin(self.ptr.as_ptr(), x, y) }
    }

    /// Set this element's 3D transform (`UIElement::SetTransform3D`, the
    /// `Transform3DProperty`) to `transform`. This is the WinUI/Noesis
    /// `Element.Transform3D` attached behaviour, distinct from `RenderTransform`.
    /// Returns `false` if this element is not a `UIElement`.
    pub fn set_transform3d<T: Transform3D>(&mut self, transform: &T) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; transform.transform3d_raw()
        // is a live Transform3D* borrowed for the call; Noesis stores its own ref.
        unsafe { noesis_element_set_transform3d(self.ptr.as_ptr(), transform.transform3d_raw()) }
    }

    /// Clear this element's 3D transform. Returns `false` if this is not a
    /// `UIElement`.
    pub fn clear_transform3d(&mut self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; null clears the property.
        unsafe { noesis_element_set_transform3d(self.ptr.as_ptr(), core::ptr::null_mut()) }
    }

    /// This element's current 3D transform (`UIElement::GetTransform3D`) as an
    /// owning, type-erased [`AnyTransform3D`](crate::transforms::AnyTransform3D),
    /// or `None` if none is set / this is not a `UIElement`. The handle can be
    /// re-applied to another element via [`Self::set_transform3d`].
    #[must_use]
    pub fn transform3d(&self) -> Option<crate::transforms::AnyTransform3D> {
        // SAFETY: self.ptr is a live BaseComponent*; returns a borrowed pointer.
        let borrowed = unsafe { noesis_element_get_transform3d(self.ptr.as_ptr()) };
        let borrowed = NonNull::new(borrowed)?;
        // AddRef so the returned handle owns its reference (released on Drop).
        // SAFETY: borrowed is a live Transform3D* (BaseComponent*).
        let owned = unsafe { noesis_base_component_add_reference(borrowed.as_ptr()) };
        NonNull::new(owned).map(|p| unsafe { crate::transforms::AnyTransform3D::from_owned(p) })
    }

    /// Set this element's `Effect` to `effect` (e.g. a blur or drop shadow).
    /// Returns `false` if the element has no `Effect` DP.
    pub fn set_effect<E: Effect>(&mut self, effect: &E) -> bool {
        // SAFETY: effect.effect_raw() is a live Effect* borrowed for the call.
        unsafe { self.set_component("Effect", effect.effect_raw()) }
    }

    /// Set the `RenderOptions.BitmapScalingMode` attached property on this
    /// element (ordinals match Noesis `BitmapScalingMode`: `0` `Unspecified`,
    /// `1` `LowQuality`, `2` `HighQuality`). Returns `false` if this is not a
    /// `DependencyObject`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_bitmap_scaling_mode(&mut self, mode: i32) -> bool {
        // SAFETY: self.ptr is a live DependencyObject*; the C side DynamicCasts.
        unsafe { noesis_render_options_set_bitmap_scaling_mode(self.ptr.as_ptr(), mode) }
    }

    /// Read the `RenderOptions.BitmapScalingMode` attached property back as an
    /// ordinal. `None` if this is not a `DependencyObject`.
    #[must_use]
    pub fn bitmap_scaling_mode(&self) -> Option<i32> {
        // SAFETY: self.ptr is a live BaseComponent*; -1 on non-DO.
        let v = unsafe { noesis_render_options_get_bitmap_scaling_mode(self.ptr.as_ptr()) };
        (v >= 0).then_some(v)
    }

    // ── Controls ──────────────────────────────────────────────────────────────
    //
    // Typed sugar + genuinely-new entrypoints over the standard Noesis controls.
    // Each method DynamicCasts (C++ side) to the right control type and degrades
    // gracefully (None / false / no-op) on a type mismatch, mirroring
    // [`text`](Self::text) / [`go_to_state`](Self::go_to_state). Several of these
    // (e.g. selection index, range value) are reachable through the generic DP
    // accessors too; they exist as typed sugar that also validates the control
    // type, and (for ranges) routes through the proper setter so Noesis coercion
    // runs. View-thread affinity is the caller's, like the rest of this impl.

    // -- Selector: ListBox / ComboBox / TabControl / ListView --

    /// The index of the first selected item, or `-1` when nothing is selected.
    /// `None` if this element is not a `Selector`.
    #[must_use]
    pub fn selected_index(&self) -> Option<i32> {
        let mut out: i32 = 0;
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts to
        // Selector and writes `out` only on success.
        unsafe { noesis_selector_get_selected_index(self.ptr.as_ptr(), &mut out) }.then_some(out)
    }

    /// Set the selected index. Pass `-1` to clear the selection; an out-of-range
    /// index is coerced by Noesis to `-1`. Returns `false` if this element is not
    /// a `Selector`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_selected_index(&mut self, index: i32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_selector_set_selected_index(self.ptr.as_ptr(), index) }
    }

    /// Borrowed (no `+1`) pointer to the selected item, or `None` when nothing is
    /// selected / this element is not a `Selector`. For an `ItemsSource`-bound
    /// control this is the data item; for direct items it is the container.
    ///
    /// The pointer is borrowed exactly like [`get_component`](Self::get_component):
    /// it must not be released and is valid only transiently.
    #[must_use]
    pub fn selected_item(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side returns a borrowed
        // pointer or null.
        let p = unsafe { noesis_selector_get_selected_item(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set the selected item to `item` (which should already be present in the
    /// control's items). Pass null to clear. Returns `false` if this element is
    /// not a `Selector`. Noesis takes its own reference to `item`.
    ///
    /// # Safety
    ///
    /// `item` must be a valid live `Noesis::BaseComponent*` (e.g. a
    /// [`crate::binding::Boxed::raw`] or a pointer from [`Self::selected_item`] /
    /// [`crate::binding::ObservableCollection::get`]) or null.
    pub unsafe fn set_selected_item(&mut self, item: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; `item` is a live
        // BaseComponent* (or null) per the contract above.
        unsafe { noesis_selector_set_selected_item(self.ptr.as_ptr(), item) }
    }

    // -- ItemsControl: direct Items mutation --

    /// Append `item` to this `ItemsControl`'s own `Items` collection, returning
    /// its new index. `None` if this element is not an `ItemsControl` or the add
    /// was rejected (e.g. an external `ItemsSource` is set, making `Items`
    /// read-only; use [`set_items_source`](Self::set_items_source) +
    /// [`crate::binding::ObservableCollection`] for that case). The collection
    /// takes its own reference to `item`.
    ///
    /// # Safety
    ///
    /// `item` must be a valid live `Noesis::BaseComponent*` (typically a
    /// [`crate::binding::Boxed::raw`]).
    pub unsafe fn items_add(&mut self, item: *mut c_void) -> Option<usize> {
        // SAFETY: self.ptr is a live BaseComponent*; `item` is live per contract.
        let idx = unsafe { noesis_items_control_items_add(self.ptr.as_ptr(), item) };
        (idx >= 0).then_some(idx as usize)
    }

    /// Append a boxed string to this `ItemsControl`'s `Items`, returning its
    /// index. Sugar over [`items_add`](Self::items_add) +
    /// [`crate::binding::box_string`]. `None` on a non-`ItemsControl` /
    /// read-only `Items`.
    ///
    /// # Panics
    ///
    /// Panics if `value` contains an interior NUL byte.
    pub fn items_add_string(&mut self, value: &str) -> Option<usize> {
        let boxed = crate::binding::box_string(value);
        // SAFETY: `boxed` is a live BaseComponent* for the call; the collection
        // takes its own ref, so dropping `boxed` afterwards is sound.
        unsafe { self.items_add(boxed.raw()) }
    }

    /// Insert `item` at `index` (allows `index == count`). Returns `false` if
    /// this element is not an `ItemsControl` or `index` is out of range.
    ///
    /// # Safety
    ///
    /// `item` must be a valid live `Noesis::BaseComponent*`.
    pub unsafe fn items_insert(&mut self, index: usize, item: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; `item` is live per contract.
        unsafe { noesis_items_control_items_insert(self.ptr.as_ptr(), index as u32, item) }
    }

    /// Remove the item at `index` from `Items`. Returns `false` on a
    /// non-`ItemsControl` or out-of-range `index`.
    pub fn items_remove_at(&mut self, index: usize) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*.
        unsafe { noesis_items_control_items_remove_at(self.ptr.as_ptr(), index as u32) }
    }

    /// Remove every item from `Items`. Returns `false` if this element is not an
    /// `ItemsControl`.
    pub fn items_clear(&mut self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*.
        unsafe { noesis_items_control_items_clear(self.ptr.as_ptr()) }
    }

    // ── Decorator / Border Child ────────────────────────────────────────────
    //
    // `Decorator::Child` is NOT a DependencyProperty, so it cannot be reached by
    // the by-name DP setters; these wrap the typed `Decorator::SetChild` /
    // `GetChild` (Border derives from Decorator). Other panel-tree building
    // blocks (`Panel::Children`, `Grid` definitions) live in
    // [`crate::element_tree`].

    /// Set this `Decorator`'s (e.g. `Border`'s) single `Child`. The decorator
    /// takes its own reference, so `child` may be dropped afterwards. Returns
    /// `false` if this element is not a `Decorator` or `child` is not a
    /// `UIElement`.
    #[must_use = "a false return means the child was not set (not a Decorator / not a UIElement)"]
    pub fn set_decorator_child(&mut self, child: &FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live BaseComponent* (DynamicCast to Decorator
        // C-side); child.raw() is a live UIElement*.
        unsafe { noesis_decorator_set_child(self.ptr.as_ptr(), child.raw()) }
    }

    /// Clear this `Decorator`'s `Child`. Returns `false` if this element is not a
    /// `Decorator`.
    #[must_use = "a false return means this element is not a Decorator"]
    pub fn clear_decorator_child(&mut self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; null clears the child.
        unsafe { noesis_decorator_set_child(self.ptr.as_ptr(), core::ptr::null_mut()) }
    }

    /// This `Decorator`'s current `Child` as an owning [`FrameworkElement`]
    /// (an independent `+1`, so dropping it does not affect the tree), or `None`
    /// if there is no child or this element is not a `Decorator`.
    #[must_use]
    pub fn decorator_child(&self) -> Option<FrameworkElement> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side returns a
        // borrowed Child* (no +1) or null.
        let borrowed = unsafe { noesis_decorator_get_child(self.ptr.as_ptr()) };
        let borrowed = NonNull::new(borrowed)?;
        // AddRef so the returned handle owns its reference, released on drop.
        // SAFETY: `borrowed` is a live UIElement* (BaseComponent*).
        let owned = unsafe { noesis_base_component_add_reference(borrowed.as_ptr()) };
        NonNull::new(owned).map(|ptr| Self { ptr })
    }

    // ── ContentControl Content ──────────────────────────────────────────────
    //
    // Unlike `Decorator::Child`, `ContentControl::Content` *is* a
    // `DependencyProperty` (of type `Object` / `BaseComponent`), so these are
    // thin, safe sugar over the by-name component-DP path
    // ([`set_component`](Self::set_component) / [`get_component`](Self::get_component)):
    // the `&FrameworkElement` borrow encodes the live-`BaseComponent` invariant
    // the raw setter demands, so callers need no `unsafe`.

    /// Set this `ContentControl`'s (e.g. `Button` / `ContentControl`) `Content`
    /// to another live element. Noesis stores its own reference, so `content`
    /// may be dropped afterwards. Returns `false` if this element has no
    /// `Content` dependency property (i.e. it is not a `ContentControl`).
    ///
    /// Safe wrapper over the `Content` component-DP path: the `&FrameworkElement`
    /// borrow guarantees the value is a live `BaseComponent` for the call, so no
    /// `unsafe` is needed at the call site (unlike the generic
    /// [`set_component`](Self::set_component)).
    #[must_use = "a false return means the content was not set (no Content DP / not a ContentControl)"]
    pub fn set_content(&mut self, content: &FrameworkElement) -> bool {
        // SAFETY: content.raw() is a live UIElement* (BaseComponent*) borrowed
        // for the call; `Content` is a BaseComponent-typed DP, so the C side's
        // tag check accepts it and stores its own reference.
        unsafe { self.set_component("Content", content.raw()) }
    }

    /// Clear this `ContentControl`'s `Content`. Returns `false` if this element
    /// has no `Content` dependency property.
    #[must_use = "a false return means this element has no Content DP"]
    pub fn clear_content(&mut self) -> bool {
        // SAFETY: clearing a BaseComponent DP with null is always sound.
        unsafe { self.set_component("Content", core::ptr::null_mut()) }
    }

    /// This `ContentControl`'s current `Content` as an owning [`FrameworkElement`]
    /// (an independent `+1`, so dropping it does not affect the tree), or `None`
    /// if the `Content` is unset, this element is not a `ContentControl`, or the
    /// content is a non-element value (e.g. a bare string). The returned handle
    /// is intended for element content set via [`set_content`](Self::set_content).
    #[must_use]
    pub fn content(&self) -> Option<FrameworkElement> {
        let borrowed = self.get_component("Content")?;
        // get_component returns a borrowed pointer; AddRef so the handle owns its
        // reference, released on drop.
        // SAFETY: `borrowed` is a live BaseComponent* held by this element.
        let owned = unsafe { noesis_base_component_add_reference(borrowed.as_ptr()) };
        NonNull::new(owned).map(|ptr| Self { ptr })
    }

    // -- RangeBase: Slider / ProgressBar / ScrollBar --

    /// The current `Value`. `None` if this element is not a `RangeBase`.
    #[must_use]
    pub fn range_value(&self) -> Option<f32> {
        self.rangebase_get(0)
    }

    /// The `Minimum`. `None` if not a `RangeBase`.
    #[must_use]
    pub fn range_minimum(&self) -> Option<f32> {
        self.rangebase_get(1)
    }

    /// The `Maximum`. `None` if not a `RangeBase`.
    #[must_use]
    pub fn range_maximum(&self) -> Option<f32> {
        self.rangebase_get(2)
    }

    fn rangebase_get(&self, which: i32) -> Option<f32> {
        let mut out: f32 = 0.0;
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts to
        // RangeBase and writes `out` only on success.
        unsafe { noesis_rangebase_get(self.ptr.as_ptr(), which, &mut out) }.then_some(out)
    }

    /// Set the `Value`, going through `RangeBase::SetValue` so Noesis coerces it
    /// into `[Minimum, Maximum]`. Returns `false` if not a `RangeBase`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_range_value(&mut self, value: f32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_rangebase_set(self.ptr.as_ptr(), 0, value) }
    }

    /// Set the `Minimum`. Returns `false` if not a `RangeBase`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_range_minimum(&mut self, value: f32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_rangebase_set(self.ptr.as_ptr(), 1, value) }
    }

    /// Set the `Maximum`. Returns `false` if not a `RangeBase`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_range_maximum(&mut self, value: f32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_rangebase_set(self.ptr.as_ptr(), 2, value) }
    }

    // -- ToggleButton: CheckBox / RadioButton (tri-state) --

    /// The tri-state `IsChecked`: outer `None` means this element is not a
    /// `ToggleButton`; inner `Some(true)`/`Some(false)` are checked/unchecked,
    /// and inner `None` is the indeterminate state. The indeterminate state is
    /// preserved (never collapsed to `false`).
    #[must_use]
    pub fn is_checked(&self) -> Option<Option<bool>> {
        let mut state: i8 = 0;
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts to
        // ToggleButton and writes `state` (0/1/2) only on success.
        if !unsafe { noesis_toggle_get_is_checked(self.ptr.as_ptr(), &mut state) } {
            return None;
        }
        Some(match state {
            1 => Some(true),
            0 => Some(false),
            _ => None, // 2 == indeterminate
        })
    }

    /// Set the tri-state `IsChecked` (`None` = indeterminate). Returns `false`
    /// if this element is not a `ToggleButton`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_is_checked(&mut self, state: Option<bool>) -> bool {
        let code: i8 = match state {
            Some(true) => 1,
            Some(false) => 0,
            None => 2,
        };
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_toggle_set_is_checked(self.ptr.as_ptr(), code) }
    }

    // -- Popup / Expander --

    /// `Popup.IsOpen`. `None` if this element is not a `Popup`.
    #[must_use]
    pub fn is_open(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts to
        // Popup and writes `out` only on success.
        unsafe { noesis_popup_get_is_open(self.ptr.as_ptr(), &mut out) }.then_some(out)
    }

    /// Set `Popup.IsOpen`. Returns `false` if this element is not a `Popup`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_is_open(&mut self, open: bool) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_popup_set_is_open(self.ptr.as_ptr(), open) }
    }

    /// `Expander.IsExpanded`. `None` if this element is not an `Expander`.
    #[must_use]
    pub fn is_expanded(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts to
        // Expander and writes `out` only on success.
        unsafe { noesis_expander_get_is_expanded(self.ptr.as_ptr(), &mut out) }.then_some(out)
    }

    /// Set `Expander.IsExpanded`. Returns `false` if not an `Expander`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_is_expanded(&mut self, expanded: bool) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_expander_set_is_expanded(self.ptr.as_ptr(), expanded) }
    }

    // -- ScrollViewer --

    /// `HorizontalOffset` (current scroll position). `None` if not a
    /// `ScrollViewer`.
    #[must_use]
    pub fn horizontal_offset(&self) -> Option<f32> {
        self.scrollviewer_get(0)
    }

    /// `VerticalOffset` (current scroll position). `None` if not a
    /// `ScrollViewer`.
    #[must_use]
    pub fn vertical_offset(&self) -> Option<f32> {
        self.scrollviewer_get(1)
    }

    /// `ScrollableWidth` (extent minus viewport width). `None` if not a
    /// `ScrollViewer`.
    #[must_use]
    pub fn scrollable_width(&self) -> Option<f32> {
        self.scrollviewer_get(2)
    }

    /// `ScrollableHeight` (extent minus viewport height). `None` if not a
    /// `ScrollViewer`.
    #[must_use]
    pub fn scrollable_height(&self) -> Option<f32> {
        self.scrollviewer_get(3)
    }

    fn scrollviewer_get(&self, which: i32) -> Option<f32> {
        let mut out: f32 = 0.0;
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts to
        // ScrollViewer and writes `out` only on success.
        unsafe { noesis_scrollviewer_get(self.ptr.as_ptr(), which, &mut out) }.then_some(out)
    }

    /// Scroll horizontally to `offset` (clamped by Noesis to the scrollable
    /// range, applied at the next layout pass). Returns `false` if not a
    /// `ScrollViewer`.
    pub fn scroll_to_horizontal_offset(&mut self, offset: f32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_scrollviewer_scroll_to_horizontal(self.ptr.as_ptr(), offset) }
    }

    /// Scroll vertically to `offset` (clamped by Noesis to the scrollable range,
    /// applied at the next layout pass). Returns `false` if not a `ScrollViewer`.
    pub fn scroll_to_vertical_offset(&mut self, offset: f32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_scrollviewer_scroll_to_vertical(self.ptr.as_ptr(), offset) }
    }

    /// Scroll to the top-left origin (`ScrollToHome`). Returns `false` if not a
    /// `ScrollViewer`.
    pub fn scroll_to_home(&mut self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_scrollviewer_scroll_to_home(self.ptr.as_ptr()) }
    }

    /// Scroll to the bottom (`ScrollToEnd`). Returns `false` if not a
    /// `ScrollViewer`.
    pub fn scroll_to_end(&mut self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_scrollviewer_scroll_to_end(self.ptr.as_ptr()) }
    }

    // -- TextBox selection / caret --

    /// Select `length` characters starting at `start`. Returns `false` if this
    /// element is not a `TextBox`.
    pub fn select(&mut self, start: i32, length: i32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_textbox_select(self.ptr.as_ptr(), start, length) }
    }

    /// Select all text. Returns `false` if this element is not a `TextBox`.
    pub fn select_all(&mut self) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_textbox_select_all(self.ptr.as_ptr()) }
    }

    /// `SelectionStart` (caret-anchor offset of the current selection). `None`
    /// if this element is not a `TextBox`.
    #[must_use]
    pub fn selection_start(&self) -> Option<i32> {
        self.textbox_get_int(0)
    }

    /// `SelectionLength`. `None` if not a `TextBox`.
    #[must_use]
    pub fn selection_length(&self) -> Option<i32> {
        self.textbox_get_int(1)
    }

    /// `CaretIndex`. `None` if not a `TextBox`. See also
    /// [`set_caret_to_end`](Self::set_caret_to_end).
    #[must_use]
    pub fn caret_index(&self) -> Option<i32> {
        self.textbox_get_int(2)
    }

    fn textbox_get_int(&self, which: i32) -> Option<i32> {
        let mut out: i32 = 0;
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts to
        // TextBox and writes `out` only on success.
        unsafe { noesis_textbox_get_int(self.ptr.as_ptr(), which, &mut out) }.then_some(out)
    }

    /// Set `SelectionStart`. Returns `false` if not a `TextBox`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_selection_start(&mut self, value: i32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_textbox_set_int(self.ptr.as_ptr(), 0, value) }
    }

    /// Set `SelectionLength`. Returns `false` if not a `TextBox`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_selection_length(&mut self, value: i32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_textbox_set_int(self.ptr.as_ptr(), 1, value) }
    }

    /// Set `CaretIndex`. Returns `false` if not a `TextBox`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_caret_index(&mut self, value: i32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_textbox_set_int(self.ptr.as_ptr(), 2, value) }
    }

    /// The currently-selected text, copied into an owned [`String`]. `None` if
    /// this element is not a `TextBox`. An empty selection yields `Some("")`.
    #[must_use]
    pub fn selected_text(&self) -> Option<String> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side returns a borrowed
        // NUL-terminated UTF-8 string (or null on a non-TextBox); we copy out
        // immediately before yielding control.
        let p = unsafe { noesis_textbox_get_selected_text(self.ptr.as_ptr()) };
        if p.is_null() {
            None
        } else {
            // SAFETY: p is a live NUL-terminated UTF-8 string while we hold our
            // element reference.
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }

    // -- PasswordBox --

    /// The `PasswordBox` plaintext, copied into an owned [`String`]. `None` if
    /// this element is not a `PasswordBox`.
    #[must_use]
    pub fn password(&self) -> Option<String> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side returns a borrowed
        // NUL-terminated UTF-8 string (or null on a non-PasswordBox); copy out now.
        let p = unsafe { noesis_passwordbox_get_password(self.ptr.as_ptr()) };
        if p.is_null() {
            None
        } else {
            // SAFETY: p is a live NUL-terminated UTF-8 string while we hold our
            // element reference.
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }

    /// Set the `PasswordBox` plaintext. Returns `false` if this element is not a
    /// `PasswordBox`.
    ///
    /// # Panics
    ///
    /// Panics if `password` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_password(&mut self, password: &str) -> bool {
        let c = CString::new(password).expect("password contained interior NUL");
        // SAFETY: self.ptr is a live BaseComponent*; c.as_ptr() lives for the call.
        unsafe { noesis_passwordbox_set_password(self.ptr.as_ptr(), c.as_ptr()) }
    }

    // -- Selector: SelectedValue / SelectedValuePath --

    /// Borrowed (no `+1`) pointer to the current `SelectedValue` (the
    /// [`selected_item`](Self::selected_item) projected through
    /// [`selected_value_path`](Self::selected_value_path); the whole item when
    /// the path is empty). `None` when nothing is selected / not a `Selector`.
    /// Borrowed exactly like [`get_component`](Self::get_component).
    #[must_use]
    pub fn selected_value(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts to
        // Selector and returns a borrowed pointer or null.
        let p = unsafe { noesis_controls_selector_get_selected_value(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Select the item whose [`selected_value_path`](Self::selected_value_path)
    /// projection equals `value`. Pass null to clear. Returns `false` if not a
    /// `Selector`. Noesis takes its own reference to `value`.
    ///
    /// # Safety
    ///
    /// `value` must be a valid live `Noesis::BaseComponent*` (e.g. a
    /// [`crate::binding::Boxed::raw`]) or null.
    pub unsafe fn set_selected_value(&mut self, value: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; `value` is live per contract.
        unsafe { noesis_controls_selector_set_selected_value(self.ptr.as_ptr(), value) }
    }

    /// The `SelectedValuePath` (the property path projected from the selected
    /// item to produce `SelectedValue`). `None` if not a `Selector`; an unset
    /// path yields `Some("")`.
    #[must_use]
    pub fn selected_value_path(&self) -> Option<String> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side returns a borrowed
        // NUL-terminated UTF-8 string (or null on a non-Selector); copy out now.
        let p = unsafe { noesis_controls_selector_get_selected_value_path(self.ptr.as_ptr()) };
        if p.is_null() {
            None
        } else {
            // SAFETY: p is a live NUL-terminated UTF-8 string while we hold our ref.
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }

    /// Set the `SelectedValuePath`. Returns `false` if not a `Selector`.
    ///
    /// # Panics
    ///
    /// Panics if `path` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_selected_value_path(&mut self, path: &str) -> bool {
        let c = CString::new(path).expect("path contained interior NUL");
        // SAFETY: self.ptr is a live BaseComponent*; c.as_ptr() lives for the call.
        unsafe { noesis_controls_selector_set_selected_value_path(self.ptr.as_ptr(), c.as_ptr()) }
    }

    // -- TreeView selection / TreeViewItem state --

    /// Borrowed pointer to the `TreeView`'s currently-selected item (the data
    /// item, or the `TreeViewItem` container for directly-authored items).
    /// `None` when nothing is selected / not a `TreeView`. Borrowed like
    /// [`get_component`](Self::get_component).
    #[must_use]
    pub fn tree_selected_item(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; borrowed pointer or null.
        let p = unsafe { noesis_controls_treeview_get_selected_item(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// `TreeViewItem.IsSelected`. `None` if not a `TreeViewItem`. (A `TreeView`
    /// has no public `SetSelectedItem`; selection is driven per item: set this
    /// on the item, then read it back via [`tree_selected_item`](Self::tree_selected_item).)
    #[must_use]
    pub fn tree_item_is_selected(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live BaseComponent*; writes `out` only on success.
        unsafe { noesis_controls_treeviewitem_get_is_selected(self.ptr.as_ptr(), &mut out) }
            .then_some(out)
    }

    /// Set `TreeViewItem.IsSelected`. Returns `false` if not a `TreeViewItem`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_tree_item_is_selected(&mut self, selected: bool) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_controls_treeviewitem_set_is_selected(self.ptr.as_ptr(), selected) }
    }

    /// `TreeViewItem.IsExpanded`. `None` if not a `TreeViewItem`.
    #[must_use]
    pub fn tree_item_is_expanded(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live BaseComponent*; writes `out` only on success.
        unsafe { noesis_controls_treeviewitem_get_is_expanded(self.ptr.as_ptr(), &mut out) }
            .then_some(out)
    }

    /// Set `TreeViewItem.IsExpanded`. Returns `false` if not a `TreeViewItem`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_tree_item_is_expanded(&mut self, expanded: bool) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_controls_treeviewitem_set_is_expanded(self.ptr.as_ptr(), expanded) }
    }

    // -- ItemContainerGenerator (container <-> item <-> index) --
    //
    // These route through this `ItemsControl`'s `ItemContainerGenerator`.
    // Containers exist only after the control has been laid out in a live
    // [`View`]; call before a layout pass and the lookups return `None` / `-1`.

    /// Borrowed pointer to the realized container for item `index`, or `None`
    /// when the index has no realized container / this is not an `ItemsControl`.
    #[must_use]
    pub fn container_from_index(&self, index: i32) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; borrowed pointer or null.
        let p = unsafe { noesis_controls_generator_container_from_index(self.ptr.as_ptr(), index) };
        NonNull::new(p)
    }

    /// Borrowed pointer to the realized container for `item`, or `None`.
    ///
    /// # Safety
    ///
    /// `item` must be a valid live `Noesis::BaseComponent*` or null.
    #[must_use]
    pub unsafe fn container_from_item(&self, item: *mut c_void) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is live; `item` is live per contract.
        let p = unsafe { noesis_controls_generator_container_from_item(self.ptr.as_ptr(), item) };
        NonNull::new(p)
    }

    /// Index of `container` in the items, or `None` when it is not a realized
    /// container / this is not an `ItemsControl`.
    ///
    /// # Safety
    ///
    /// `container` must be a valid live `Noesis::DependencyObject*` (e.g. a
    /// pointer from [`container_from_index`](Self::container_from_index)).
    #[must_use]
    pub unsafe fn index_from_container(&self, container: *mut c_void) -> Option<i32> {
        // SAFETY: self.ptr is live; `container` is live per contract.
        let idx =
            unsafe { noesis_controls_generator_index_from_container(self.ptr.as_ptr(), container) };
        (idx >= 0).then_some(idx)
    }

    /// Borrowed pointer to the data item backing `container`, or `None`.
    ///
    /// # Safety
    ///
    /// `container` must be a valid live `Noesis::DependencyObject*` or null.
    #[must_use]
    pub unsafe fn item_from_container(&self, container: *mut c_void) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is live; `container` is live per contract.
        let p =
            unsafe { noesis_controls_generator_item_from_container(self.ptr.as_ptr(), container) };
        NonNull::new(p)
    }

    // -- ListView / GridView columns --

    /// Borrowed pointer to the `GridView` set as this `ListView`'s `View`, for
    /// passing to the `gridview_column_*` accessors. `None` if not a `ListView`
    /// or its `View` is not a `GridView`. Borrowed like
    /// [`get_component`](Self::get_component).
    #[must_use]
    pub fn listview_gridview(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; borrowed pointer or null.
        let p = unsafe { noesis_controls_listview_get_view(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Number of columns in a `GridView` (the pointer from
    /// [`listview_gridview`](Self::listview_gridview)). `None` if not a
    /// `GridView`.
    ///
    /// # Safety
    ///
    /// `gridview` must be a live `Noesis::GridView*`.
    #[must_use]
    pub unsafe fn gridview_column_count(gridview: NonNull<c_void>) -> Option<i32> {
        // SAFETY: `gridview` is a live GridView* per contract.
        let n = unsafe { noesis_controls_gridview_column_count(gridview.as_ptr()) };
        (n >= 0).then_some(n)
    }

    /// A column's `Width` (NaN means `Auto`). `None` on a bad index / non-GridView.
    ///
    /// # Safety
    ///
    /// `gridview` must be a live `Noesis::GridView*`.
    #[must_use]
    pub unsafe fn gridview_column_width(gridview: NonNull<c_void>, index: u32) -> Option<f32> {
        let mut out = 0.0f32;
        // SAFETY: `gridview` is a live GridView* per contract; writes on success.
        unsafe { noesis_controls_gridview_column_get_width(gridview.as_ptr(), index, &mut out) }
            .then_some(out)
    }

    /// Set a column's `Width`. Returns `false` on a bad index / non-GridView.
    ///
    /// # Safety
    ///
    /// `gridview` must be a live `Noesis::GridView*`.
    pub unsafe fn set_gridview_column_width(
        gridview: NonNull<c_void>,
        index: u32,
        width: f32,
    ) -> bool {
        // SAFETY: `gridview` is a live GridView* per contract.
        unsafe { noesis_controls_gridview_column_set_width(gridview.as_ptr(), index, width) }
    }

    /// A column's computed `ActualWidth`. `None` on a bad index / non-GridView.
    ///
    /// # Safety
    ///
    /// `gridview` must be a live `Noesis::GridView*`.
    #[must_use]
    pub unsafe fn gridview_column_actual_width(
        gridview: NonNull<c_void>,
        index: u32,
    ) -> Option<f32> {
        let mut out = 0.0f32;
        // SAFETY: `gridview` is a live GridView* per contract; writes on success.
        unsafe {
            noesis_controls_gridview_column_get_actual_width(gridview.as_ptr(), index, &mut out)
        }
        .then_some(out)
    }

    /// Borrowed pointer to a column's `Header` (typically a boxed string), or
    /// `None`.
    ///
    /// # Safety
    ///
    /// `gridview` must be a live `Noesis::GridView*`.
    #[must_use]
    pub unsafe fn gridview_column_header(
        gridview: NonNull<c_void>,
        index: u32,
    ) -> Option<NonNull<c_void>> {
        // SAFETY: `gridview` is a live GridView* per contract.
        let p = unsafe { noesis_controls_gridview_column_get_header(gridview.as_ptr(), index) };
        NonNull::new(p)
    }

    // -- ToolTip / ToolTipService --

    /// Borrowed pointer to this element's `ToolTip` content, or `None`. Borrowed
    /// like [`get_component`](Self::get_component).
    #[must_use]
    pub fn tooltip(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; borrowed pointer or null.
        let p = unsafe { noesis_controls_fe_get_tooltip(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set this element's `ToolTip` to a borrowed content component (e.g. a
    /// boxed string, or a `ToolTip`/element). Pass null to clear. Returns
    /// `false` if not a `FrameworkElement`. Noesis takes its own reference.
    ///
    /// # Safety
    ///
    /// `tooltip` must be a valid live `Noesis::BaseComponent*` or null.
    pub unsafe fn set_tooltip(&mut self, tooltip: *mut c_void) -> bool {
        // SAFETY: self.ptr is live; `tooltip` is live per contract.
        unsafe { noesis_controls_fe_set_tooltip(self.ptr.as_ptr(), tooltip) }
    }

    /// Set this element's `ToolTip` to a plain string. Returns `false` if not a
    /// `FrameworkElement`.
    ///
    /// # Panics
    ///
    /// Panics if `text` contains an interior NUL byte.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_tooltip_string(&mut self, text: &str) -> bool {
        let c = CString::new(text).expect("tooltip text contained interior NUL");
        // SAFETY: self.ptr is live; c.as_ptr() lives for the call.
        unsafe { noesis_controls_fe_set_tooltip_string(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Borrowed pointer to the `ToolTipService.ToolTip` attached value on this
    /// object (what [`set_tooltip`](Self::set_tooltip) ultimately writes), or
    /// `None`. Readable on any `DependencyObject`.
    #[must_use]
    pub fn tooltip_service_tooltip(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; borrowed pointer or null.
        let p = unsafe { noesis_controls_tooltipservice_get_tooltip(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set the `ToolTipService.ToolTip` attached value. Returns `false` if not a
    /// `DependencyObject`.
    ///
    /// # Safety
    ///
    /// `tooltip` must be a valid live `Noesis::BaseComponent*` or null.
    pub unsafe fn set_tooltip_service_tooltip(&mut self, tooltip: *mut c_void) -> bool {
        // SAFETY: self.ptr is live; `tooltip` is live per contract.
        unsafe { noesis_controls_tooltipservice_set_tooltip(self.ptr.as_ptr(), tooltip) }
    }

    /// `ToolTip.IsOpen`. `None` if not a `ToolTip` control.
    #[must_use]
    pub fn tooltip_is_open(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live BaseComponent*; writes `out` only on success.
        unsafe { noesis_controls_tooltip_get_is_open(self.ptr.as_ptr(), &mut out) }.then_some(out)
    }

    /// Set `ToolTip.IsOpen`. Returns `false` if not a `ToolTip` control.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_tooltip_is_open(&mut self, open: bool) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_controls_tooltip_set_is_open(self.ptr.as_ptr(), open) }
    }

    // -- ContextMenu / ContextMenuService --

    /// Borrowed pointer to this element's `ContextMenu`, or `None`. Borrowed
    /// like [`get_component`](Self::get_component).
    #[must_use]
    pub fn context_menu(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; borrowed pointer or null.
        let p = unsafe { noesis_controls_fe_get_context_menu(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set this element's `ContextMenu`. `menu` must be a `ContextMenu*` (e.g.
    /// from [`context_menu`](Self::context_menu) or a parsed element) or null to
    /// clear. Returns `false` if not a `FrameworkElement`, or `menu` is not a
    /// `ContextMenu`. Noesis takes its own reference.
    ///
    /// # Safety
    ///
    /// `menu` must be a valid live `Noesis::ContextMenu*` or null.
    pub unsafe fn set_context_menu(&mut self, menu: *mut c_void) -> bool {
        // SAFETY: self.ptr is live; `menu` is live per contract.
        unsafe { noesis_controls_fe_set_context_menu(self.ptr.as_ptr(), menu) }
    }

    /// Borrowed pointer to the `ContextMenuService.ContextMenu` attached value,
    /// or `None`. Readable on any `DependencyObject`.
    #[must_use]
    pub fn context_menu_service_menu(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; borrowed pointer or null.
        let p = unsafe { noesis_controls_contextmenuservice_get_context_menu(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set the `ContextMenuService.ContextMenu` attached value. Returns `false`
    /// if not a `DependencyObject`, or `menu` is not a `ContextMenu`.
    ///
    /// # Safety
    ///
    /// `menu` must be a valid live `Noesis::ContextMenu*` or null.
    pub unsafe fn set_context_menu_service_menu(&mut self, menu: *mut c_void) -> bool {
        // SAFETY: self.ptr is live; `menu` is live per contract.
        unsafe { noesis_controls_contextmenuservice_set_context_menu(self.ptr.as_ptr(), menu) }
    }

    /// `ContextMenu.IsOpen`. `None` if not a `ContextMenu` control.
    #[must_use]
    pub fn context_menu_is_open(&self) -> Option<bool> {
        let mut out = false;
        // SAFETY: self.ptr is a live BaseComponent*; writes `out` only on success.
        unsafe { noesis_controls_contextmenu_get_is_open(self.ptr.as_ptr(), &mut out) }
            .then_some(out)
    }

    /// Set `ContextMenu.IsOpen`. Returns `false` if not a `ContextMenu` control.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_context_menu_is_open(&mut self, open: bool) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_controls_contextmenu_set_is_open(self.ptr.as_ptr(), open) }
    }

    // -- ScrollViewer line / page / edge scrolling + IScrollInfo --

    /// Scroll up one line (`LineUp`). Returns `false` if not a `ScrollViewer`.
    pub fn line_up(&mut self) -> bool {
        self.scrollviewer_line(0)
    }

    /// Scroll down one line (`LineDown`). `false` if not a `ScrollViewer`.
    pub fn line_down(&mut self) -> bool {
        self.scrollviewer_line(1)
    }

    /// Scroll left one line (`LineLeft`). `false` if not a `ScrollViewer`.
    pub fn line_left(&mut self) -> bool {
        self.scrollviewer_line(2)
    }

    /// Scroll right one line (`LineRight`). `false` if not a `ScrollViewer`.
    pub fn line_right(&mut self) -> bool {
        self.scrollviewer_line(3)
    }

    fn scrollviewer_line(&mut self, which: i32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_controls_scrollviewer_line(self.ptr.as_ptr(), which) }
    }

    /// Scroll up one page (`PageUp`). `false` if not a `ScrollViewer`.
    pub fn page_up(&mut self) -> bool {
        self.scrollviewer_page(0)
    }

    /// Scroll down one page (`PageDown`). `false` if not a `ScrollViewer`.
    pub fn page_down(&mut self) -> bool {
        self.scrollviewer_page(1)
    }

    /// Scroll left one page (`PageLeft`). `false` if not a `ScrollViewer`.
    pub fn page_left(&mut self) -> bool {
        self.scrollviewer_page(2)
    }

    /// Scroll right one page (`PageRight`). `false` if not a `ScrollViewer`.
    pub fn page_right(&mut self) -> bool {
        self.scrollviewer_page(3)
    }

    fn scrollviewer_page(&mut self, which: i32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_controls_scrollviewer_page(self.ptr.as_ptr(), which) }
    }

    /// Scroll to the top edge (`ScrollToTop`). `false` if not a `ScrollViewer`.
    pub fn scroll_to_top(&mut self) -> bool {
        self.scrollviewer_edge(0)
    }

    /// Scroll to the bottom edge (`ScrollToBottom`). `false` if not a
    /// `ScrollViewer`.
    pub fn scroll_to_bottom(&mut self) -> bool {
        self.scrollviewer_edge(1)
    }

    /// Scroll to the left edge (`ScrollToLeftEnd`). `false` if not a
    /// `ScrollViewer`.
    pub fn scroll_to_left_end(&mut self) -> bool {
        self.scrollviewer_edge(2)
    }

    /// Scroll to the right edge (`ScrollToRightEnd`). `false` if not a
    /// `ScrollViewer`.
    pub fn scroll_to_right_end(&mut self) -> bool {
        self.scrollviewer_edge(3)
    }

    fn scrollviewer_edge(&mut self, which: i32) -> bool {
        // SAFETY: self.ptr is a live BaseComponent*; the C side DynamicCasts.
        unsafe { noesis_controls_scrollviewer_edge(self.ptr.as_ptr(), which) }
    }

    /// `ExtentWidth` (the full content width). `None` if not a `ScrollViewer`.
    #[must_use]
    pub fn extent_width(&self) -> Option<f32> {
        self.scrollviewer_metric(6)
    }

    /// `ViewportWidth` (the visible content width). `None` if not a
    /// `ScrollViewer`.
    #[must_use]
    pub fn viewport_width(&self) -> Option<f32> {
        self.scrollviewer_metric(7)
    }

    fn scrollviewer_metric(&self, which: i32) -> Option<f32> {
        let mut out = 0.0f32;
        // SAFETY: self.ptr is a live BaseComponent*; writes `out` only on success.
        unsafe { noesis_controls_scrollviewer_metric(self.ptr.as_ptr(), which, &mut out) }
            .then_some(out)
    }

    // -- Image source --

    /// Borrowed pointer to this `Image`'s `Source` (an `ImageSource`), or
    /// `None` when unset / not an `Image`. Borrowed like
    /// [`get_component`](Self::get_component).
    #[must_use]
    pub fn image_source(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live BaseComponent*; borrowed pointer or null.
        let p = unsafe { noesis_controls_image_get_source(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set this `Image`'s `Source`. `source` must be an `ImageSource*` (e.g. a
    /// [`crate::imaging::BitmapImage`] / [`crate::imaging::TextureSource`]
    /// handle's [`raw`](crate::imaging::TextureSource::raw)) or null to clear.
    /// Returns `false` if not an `Image`, or `source` is not an `ImageSource`.
    /// Noesis takes its own reference.
    ///
    /// # Safety
    ///
    /// `source` must be a valid live `Noesis::ImageSource*` or null.
    pub unsafe fn set_image_source(&mut self, source: *mut c_void) -> bool {
        // SAFETY: self.ptr is live; `source` is live per contract.
        unsafe { noesis_controls_image_set_source(self.ptr.as_ptr(), source) }
    }

    // ── Resources / styles / templates ──────────────────────────────────────
    //
    // Per-element Resources get/set + non-throwing FindResource, Style
    // assign/read-back, and ControlTemplate assign/read-back. The owned
    // wrappers ([`ResourceDictionary`], [`Style`], [`ControlTemplate`]) and the
    // free application-resource / parse helpers live in [`crate::resources`] /
    // [`crate::styles`]; these methods are the element-facing entrypoints.

    /// This element's local resource dictionary (`FrameworkElement::GetResources`),
    /// or `None` if it has none. The returned
    /// [`ResourceDictionary`](crate::resources::ResourceDictionary) owns its own
    /// `+1` reference (the accessor `AddRef`'d it), so it stays valid past this
    /// borrow and mutating it mutates the live dictionary on the element.
    #[must_use]
    pub fn resources(&self) -> Option<crate::resources::ResourceDictionary> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side AddRefs the
        // result so the wrapper owns a +1.
        let ptr = unsafe { noesis_framework_element_get_resources(self.ptr.as_ptr()) };
        NonNull::new(ptr)
            .map(|ptr| unsafe { crate::resources::ResourceDictionary::from_owned(ptr) })
    }

    /// Replace this element's local resource dictionary
    /// (`FrameworkElement::SetResources`). Noesis takes its own reference, so
    /// `dict` may be dropped afterwards. Returns `false` if this is not a
    /// `FrameworkElement`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_resources(&mut self, dict: &crate::resources::ResourceDictionary) -> bool {
        // SAFETY: both pointers are live; Noesis AddRefs the dictionary.
        unsafe { noesis_framework_element_set_resources(self.ptr.as_ptr(), dict.raw()) }
    }

    /// Look up a resource by `key`, walking the logical parent chain and the
    /// application resources (`FrameworkElement::FindResource`). Borrowed (no
    /// `+1`). Valid only transiently; copy / re-root if you need it longer.
    /// Returns `None` on a miss (the non-throwing variant) or if this is not a
    /// `FrameworkElement`.
    ///
    /// # Panics
    ///
    /// Panics if `key` contains an interior NUL byte.
    #[must_use]
    pub fn find_resource(&self, key: &str) -> Option<NonNull<c_void>> {
        let c = CString::new(key).expect("resource key contained interior NUL");
        // SAFETY: self.ptr live; c lives for the call. The returned pointer is
        // borrowed (owned by whichever dictionary holds the entry).
        let p = unsafe { noesis_framework_element_find_resource(self.ptr.as_ptr(), c.as_ptr()) };
        NonNull::new(p)
    }

    /// Assign a [`Style`](crate::styles::Style) to this element
    /// (`FrameworkElement::SetStyle`). Applying the style seals it. Noesis takes
    /// its own reference, so `style` may be dropped afterwards. Returns `false`
    /// if this is not a `FrameworkElement`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_style(&mut self, style: &crate::styles::Style) -> bool {
        // SAFETY: both pointers are live; Noesis AddRefs the style.
        unsafe { noesis_framework_element_set_style(self.ptr.as_ptr(), style.raw()) }
    }

    /// This element's assigned [`Style`](crate::styles::Style)
    /// (`FrameworkElement::GetStyle`), or `None` if none. The returned wrapper
    /// owns its own `+1` reference (the accessor `AddRef`'d it).
    #[must_use]
    pub fn style(&self) -> Option<crate::styles::Style> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side AddRefs.
        let ptr = unsafe { noesis_framework_element_get_style(self.ptr.as_ptr()) };
        NonNull::new(ptr).map(|ptr| unsafe { crate::styles::Style::from_owned(ptr) })
    }

    /// Assign a [`ControlTemplate`](crate::styles::ControlTemplate) to this
    /// element (`Control::SetTemplate`). Noesis takes its own reference, so the
    /// template may be dropped afterwards. Returns `false` if this is not a
    /// `Control`. After the next layout pass the template parts become
    /// resolvable via [`template_child`](Self::template_child) or
    /// [`ControlTemplate::find_name`](crate::styles::ControlTemplate::find_name).
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_control_template(&mut self, template: &crate::styles::ControlTemplate) -> bool {
        // SAFETY: both pointers are live; Noesis AddRefs the template.
        unsafe { noesis_control_set_template(self.ptr.as_ptr(), template.raw()) }
    }

    /// This control's assigned [`ControlTemplate`](crate::styles::ControlTemplate)
    /// (`Control::GetTemplate`), or `None` if none / not a `Control`. The
    /// returned wrapper owns its own `+1` reference (the accessor `AddRef`'d it).
    #[must_use]
    pub fn control_template(&self) -> Option<crate::styles::ControlTemplate> {
        // SAFETY: self.ptr is a live BaseComponent*; the C side AddRefs.
        let ptr = unsafe { noesis_control_get_template(self.ptr.as_ptr()) };
        NonNull::new(ptr).map(|ptr| unsafe { crate::styles::ControlTemplate::from_owned(ptr) })
    }
}

/// A dependency-property value whose type was inferred at runtime via
/// [`FrameworkElement::property_tag`]. Returned by
/// [`FrameworkElement::get_dynamic`]. Each variant mirrors a [`PropType`] tag
/// and its FFI value layout.
#[derive(Debug)]
pub enum DynValue {
    /// `Int32`.
    I32(i32),
    /// `UInt32`.
    U32(u32),
    /// `Float` (single-precision).
    F32(f32),
    /// `Double` (double-precision).
    F64(f64),
    /// `Bool`.
    Bool(bool),
    /// `String` (copied into an owned [`String`]).
    Str(String),
    /// `Thickness` as `[left, top, right, bottom]`.
    Thickness([f32; 4]),
    /// `Color` as `[r, g, b, a]` (each in `0..=1`).
    Color([f32; 4]),
    /// `Rect` as `[x, y, width, height]`.
    Rect([f32; 4]),
    /// `Point` as `[x, y]`.
    Point([f32; 2]),
    /// `Size` as `[width, height]`.
    Size([f32; 2]),
    /// `Vector` (`Noesis::Vector2`) as `[x, y]`.
    Vector([f32; 2]),
    /// Runtime-enum-typed DP value (the underlying `int32` member value).
    Enum(i32),
    /// A reference-typed value (`ImageSource` / `BaseComponent` subclass) as a
    /// borrowed opaque pointer (no `+1` ref; see
    /// [`FrameworkElement::get_component`]).
    Component(NonNull<c_void>),
}

/// `Noesis::HorizontalAlignment` (`NsGui/Enums.h`). Ordinals match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HAlign {
    Left = 0,
    Center = 1,
    Right = 2,
    Stretch = 3,
}

impl HAlign {
    /// Map a raw C++ ordinal (or `-1` for "not a `FrameworkElement`") to a
    /// variant, returning `None` outside `0..=3`.
    #[must_use]
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Left),
            1 => Some(Self::Center),
            2 => Some(Self::Right),
            3 => Some(Self::Stretch),
            _ => None,
        }
    }
}

/// `Noesis::VerticalAlignment` (`NsGui/Enums.h`). Ordinals match the C++ enum.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum VAlign {
    Top = 0,
    Center = 1,
    Bottom = 2,
    Stretch = 3,
}

impl VAlign {
    /// Map a raw C++ ordinal (or `-1` for "not a `FrameworkElement`") to a
    /// variant, returning `None` outside `0..=3`.
    #[must_use]
    fn from_ordinal(v: i32) -> Option<Self> {
        match v {
            0 => Some(Self::Top),
            1 => Some(Self::Center),
            2 => Some(Self::Bottom),
            3 => Some(Self::Stretch),
            _ => None,
        }
    }
}

impl Drop for FrameworkElement {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_gui_load_xaml which returns a +1 ref.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A Noesis view wrapping a loaded XAML root. Owns a +1 refcount on the
/// underlying `Noesis::IView`; its internal `Ptr<FrameworkElement>` keeps
/// the root alive too.
pub struct View {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for View {}

impl View {
    /// Create a View whose root is `content`. Consumes the
    /// [`FrameworkElement`] wrapper; its refcount transfers into the view.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis factory returns null (only possible on internal
    /// logic errors once `content` is non-null).
    #[must_use]
    pub fn create(content: FrameworkElement) -> Self {
        let raw = content.into_raw();
        // SAFETY: raw is a live FrameworkElement* with +1 ref.
        let ptr = unsafe { noesis_view_create(raw) };
        // View took its own ref internally; release our +1 on the element so
        // refcount stays balanced (its total is still the original 1).
        unsafe { noesis_base_component_release(raw) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_view_create returned null"),
        }
    }

    /// Surface size the view lays out against.
    pub fn set_size(&mut self, width: u32, height: u32) {
        unsafe { noesis_view_set_size(self.ptr.as_ptr(), width, height) }
    }

    /// DPI scale for the view's content (1.0 == 96 ppi). Scales layout + hit
    /// testing without resizing the surface, keeping the UI crisp at any density.
    pub fn set_scale(&mut self, scale: f32) {
        unsafe { noesis_view_set_scale(self.ptr.as_ptr(), scale) }
    }

    /// Set the projection matrix. 16 floats, row-major: the native
    /// `Matrix4::GetData()` layout. Typical Noesis-facing projection is an
    /// ortho that maps UI pixel coords into Noesis's clip space (0..width,
    /// 0..height).
    pub fn set_projection_matrix(&mut self, matrix: &[f32; 16]) {
        unsafe { noesis_view_set_projection_matrix(self.ptr.as_ptr(), matrix.as_ptr()) }
    }

    /// Set the view's render flags from a raw `Noesis::RenderFlags` bitmask
    /// (an OR of [`RenderFlag`] values). For a typed set that avoids hand-ORing
    /// `u32`s, prefer [`Self::set_render_flags`].
    pub fn set_flags(&mut self, flags: u32) {
        unsafe { noesis_view_set_flags(self.ptr.as_ptr(), flags) }
    }

    /// Set the view's render flags from a typed [`RenderFlags`] set, so callers
    /// don't hand-OR raw `u32`s.
    pub fn set_render_flags(&mut self, flags: RenderFlags) {
        self.set_flags(flags.bits());
    }

    /// Current render flags as a raw bitmask (`IView::GetFlags`). The raw
    /// counterpart of [`Self::flags`].
    #[must_use]
    pub fn get_flags(&self) -> u32 {
        // SAFETY: self.ptr is a live IView*; GetFlags is a const accessor.
        unsafe { noesis_view_get_flags(self.ptr.as_ptr()) }
    }

    /// Current render flags as a typed [`RenderFlags`] set.
    #[must_use]
    pub fn flags(&self) -> RenderFlags {
        RenderFlags(self.get_flags())
    }

    /// Set the tessellation curve tolerance in screen-space pixels, the raw
    /// `IView::SetTessellationMaxPixelError` knob. Smaller values mean finer
    /// curve subdivision (higher quality, more triangles). Prefer
    /// [`Self::set_quality`] for the named presets.
    pub fn set_tessellation_max_pixel_error(&mut self, error: f32) {
        // SAFETY: self.ptr is a live IView*; thin pass-through.
        unsafe { noesis_view_set_tessellation_max_pixel_error(self.ptr.as_ptr(), error) }
    }

    /// Set the antialiasing / curve quality to one of the named presets
    /// (`Low` 0.7, `Medium` 0.4, `High` 0.2 pixel error).
    pub fn set_quality(&mut self, quality: Quality) {
        self.set_tessellation_max_pixel_error(quality.pixel_error());
    }

    /// Current tessellation curve tolerance in screen-space pixels
    /// (`IView::GetTessellationMaxPixelError`).
    #[must_use]
    pub fn tessellation_max_pixel_error(&self) -> f32 {
        // SAFETY: self.ptr is a live IView*; const accessor.
        unsafe { noesis_view_get_tessellation_max_pixel_error(self.ptr.as_ptr()) }
    }

    /// Time, in milliseconds, an interaction must be held before it promotes to
    /// a `Holding` (long-press) event rather than a `Tapped` (`IView::
    /// SetHoldingTimeThreshold`). Default 500ms.
    pub fn set_holding_time_threshold(&mut self, ms: u32) {
        // SAFETY: self.ptr is a live IView*; thin pass-through.
        unsafe { noesis_view_set_holding_time_threshold(self.ptr.as_ptr(), ms) }
    }

    /// Maximum distance, in pixels, between first and last contact for an
    /// interaction to still count as a `Tapped` / `Holding` event
    /// (`IView::SetHoldingDistanceThreshold`). Default 10px.
    pub fn set_holding_distance_threshold(&mut self, pixels: u32) {
        // SAFETY: self.ptr is a live IView*; thin pass-through.
        unsafe { noesis_view_set_holding_distance_threshold(self.ptr.as_ptr(), pixels) }
    }

    /// Minimum distance, in pixels, from first contact before a manipulation
    /// starts (raising `ManipulationStarted`). `IView::
    /// SetManipulationDistanceThreshold`. Default 10px.
    pub fn set_manipulation_distance_threshold(&mut self, pixels: u32) {
        // SAFETY: self.ptr is a live IView*; thin pass-through.
        unsafe { noesis_view_set_manipulation_distance_threshold(self.ptr.as_ptr(), pixels) }
    }

    /// Maximum delay, in milliseconds, between two `Tapped` events for them to
    /// be interpreted as a `DoubleTapped` (`IView::SetDoubleTapTimeThreshold`).
    /// Default 500ms.
    pub fn set_double_tap_time_threshold(&mut self, ms: u32) {
        // SAFETY: self.ptr is a live IView*; thin pass-through.
        unsafe { noesis_view_set_double_tap_time_threshold(self.ptr.as_ptr(), ms) }
    }

    /// Maximum distance, in pixels, between two taps for them to be interpreted
    /// as a `DoubleTapped` (`IView::SetDoubleTapDistanceThreshold`). Default
    /// 10px.
    pub fn set_double_tap_distance_threshold(&mut self, pixels: u32) {
        // SAFETY: self.ptr is a live IView*; thin pass-through.
        unsafe { noesis_view_set_double_tap_distance_threshold(self.ptr.as_ptr(), pixels) }
    }

    /// Whether mouse input is emulated as touch input
    /// (`IView::SetEmulateTouch`). Off by default.
    pub fn set_emulate_touch(&mut self, emulate: bool) {
        // SAFETY: self.ptr is a live IView*; thin pass-through.
        unsafe { noesis_view_set_emulate_touch(self.ptr.as_ptr(), emulate) }
    }

    /// Scale applied to the offscreen render phase to account for stereo (VR)
    /// eye matrices differing from the view projection
    /// (`IView::SetStereoOffscreenScaleFactor`). Must be `1.0` (the default)
    /// for non-VR rendering; `2.0`-`3.0` is recommended for VR.
    pub fn set_stereo_offscreen_scale_factor(&mut self, factor: f32) {
        // SAFETY: self.ptr is a live IView*; thin pass-through.
        unsafe { noesis_view_set_stereo_offscreen_scale_factor(self.ptr.as_ptr(), factor) }
    }

    /// Performance counters for the last rendered frame (`IView::GetStats`).
    /// Most counters (triangle / draw / batch / glyph counts) are populated by
    /// the render pass; timing fields are tracked across update + render. See
    /// [`ViewStats`].
    #[must_use]
    pub fn stats(&self) -> ViewStats {
        let mut out = ViewStats::default();
        // SAFETY: self.ptr is a live IView*; `out` is a live, correctly-sized
        // ViewStats whose repr(C) layout matches the C ABI struct (guarded by
        // a static_assert in noesis_view.cpp); the C side writes all fields.
        unsafe { noesis_view_get_stats(self.ptr.as_ptr(), &raw mut out) };
        out
    }

    /// Create a view-driven timer firing roughly every `interval_ms`
    /// milliseconds. Timers are serviced from inside [`Self::update`] (off the
    /// view clock advanced by the time passed to `update`), so the cadence
    /// follows your update loop rather than wall-clock time.
    ///
    /// `handler` returns the next interval in milliseconds (`0` stops the
    /// timer). The returned [`TimerSubscription`] is RAII: drop it to cancel
    /// the timer and free the handler. Returns `None` only if the underlying
    /// C entrypoint fails (e.g. a null view).
    ///
    /// # Panics
    ///
    /// Panics only on internal logic errors: if `Box::into_raw` returns null
    /// (it cannot; the wrapper is `NonNull` to keep the invariant explicit).
    pub fn create_timer<H: TimerHandler>(
        &mut self,
        interval_ms: u32,
        handler: H,
    ) -> Option<TimerSubscription> {
        // Double-Box: stable thin pointer for the C ABI userdata.
        let outer: Box<Box<dyn TimerHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(outer);

        // SAFETY: trampolines are `extern "C"`; userdata is freshly leaked and
        // donated to the C++ RustTimer (freed via `timer_free` on cancel); the
        // view pointer is borrowed for the call only.
        let token = unsafe {
            noesis_view_create_timer(
                self.ptr.as_ptr(),
                interval_ms,
                timer_trampoline,
                userdata.cast(),
                timer_free,
            )
        };

        if let Some(token) = NonNull::new(token) {
            Some(TimerSubscription { token })
        } else {
            // Creation failed before donation took effect; free the box we
            // leaked above so the handler isn't leaked.
            // SAFETY: userdata came from Box::into_raw moments ago; nothing
            // else ever saw the pointer.
            unsafe { drop(Box::from_raw(userdata)) };
            None
        }
    }

    /// Subscribe to the view's `Rendering` event (`IView::Rendering`), raised
    /// once per frame after animation and layout are applied to the composition
    /// tree, just before it is rendered. Fired from inside [`Self::update`] on
    /// the view-driving thread. Use it for per-frame work that must observe the
    /// final, laid-out tree (e.g. syncing an external overlay).
    ///
    /// The returned [`RenderingSubscription`] is RAII: drop it to detach the
    /// handler and free it. Returns `None` only if the underlying C entrypoint
    /// fails (e.g. a null view).
    pub fn add_rendering_handler<H: RenderingHandler>(
        &mut self,
        handler: H,
    ) -> Option<RenderingSubscription> {
        // Double-Box: stable thin pointer for the C ABI userdata.
        let outer: Box<Box<dyn RenderingHandler>> = Box::new(Box::new(handler));
        let userdata = Box::into_raw(outer);

        // SAFETY: trampolines are `extern "C"`; userdata is freshly leaked and
        // donated to the C++ handler (freed via `rendering_free` on removal);
        // the view pointer is borrowed for the call only.
        let token = unsafe {
            noesis_view_add_rendering_handler(
                self.ptr.as_ptr(),
                rendering_trampoline,
                userdata.cast(),
                rendering_free,
            )
        };

        if let Some(token) = NonNull::new(token) {
            Some(RenderingSubscription { token })
        } else {
            // Creation failed before donation took effect; free the box we
            // leaked above so the handler isn't leaked.
            // SAFETY: userdata came from Box::into_raw moments ago; nothing
            // else ever saw the pointer.
            unsafe { drop(Box::from_raw(userdata)) };
            None
        }
    }

    /// Recover keyboard focus for this view. Noesis ignores keyboard input
    /// until a view is activated.
    pub fn activate(&mut self) {
        unsafe { noesis_view_activate(self.ptr.as_ptr()) }
    }

    /// Release keyboard focus.
    pub fn deactivate(&mut self) {
        unsafe { noesis_view_deactivate(self.ptr.as_ptr()) }
    }

    /// Pointer position, in physical pixels, origin top-left. Noesis
    /// requires a `mouse_move` at the press coordinate before a
    /// [`Self::mouse_button_down`] or [`Self::touch_down`] will hit-test
    /// correctly; callers must ensure the ordering.
    pub fn mouse_move(&mut self, x: i32, y: i32) -> bool {
        unsafe { noesis_view_mouse_move(self.ptr.as_ptr(), x, y) }
    }

    /// Press `button` at `(x, y)` (physical pixels, origin top-left). Issue a
    /// [`Self::mouse_move`] to the same point first so the hit-test resolves.
    /// Returns whether Noesis handled the event.
    pub fn mouse_button_down(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { noesis_view_mouse_button_down(self.ptr.as_ptr(), x, y, button as i32) }
    }

    /// Release `button` at `(x, y)` (physical pixels). Returns whether Noesis
    /// handled the event.
    pub fn mouse_button_up(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { noesis_view_mouse_button_up(self.ptr.as_ptr(), x, y, button as i32) }
    }

    /// Deliver a double-click of `button` at `(x, y)` (physical pixels).
    /// Returns whether Noesis handled the event.
    pub fn mouse_double_click(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { noesis_view_mouse_double_click(self.ptr.as_ptr(), x, y, button as i32) }
    }

    /// `delta` is signed; Noesis uses Windows-style 120 units per notch.
    pub fn mouse_wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        unsafe { noesis_view_mouse_wheel(self.ptr.as_ptr(), x, y, delta) }
    }

    /// Horizontal mouse wheel (e.g. a tilt-wheel or trackpad swipe). `delta`
    /// is signed with the same 120-units-per-notch convention as
    /// [`Self::mouse_wheel`]; positive scrolls right. Returns whether Noesis
    /// handled the event.
    pub fn mouse_hwheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        // SAFETY: self.ptr is a live IView*; thin pass-through to MouseHWheel.
        unsafe { noesis_view_mouse_hwheel(self.ptr.as_ptr(), x, y, delta) }
    }

    /// Vertical scroll with the cursor at `(x, y)`. `value` is in lines
    /// (per WPF convention: integer lines, fractional allowed).
    pub fn scroll(&mut self, x: i32, y: i32, value: f32) -> bool {
        unsafe { noesis_view_scroll(self.ptr.as_ptr(), x, y, value) }
    }

    /// Horizontal scroll. See [`Self::scroll`].
    pub fn hscroll(&mut self, x: i32, y: i32, value: f32) -> bool {
        unsafe { noesis_view_hscroll(self.ptr.as_ptr(), x, y, value) }
    }

    /// Begin touch contact `id` at `(x, y)` (physical pixels). Returns whether
    /// Noesis handled the event.
    pub fn touch_down(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { noesis_view_touch_down(self.ptr.as_ptr(), x, y, id) }
    }

    /// Move touch contact `id` to `(x, y)` (physical pixels). Returns whether
    /// Noesis handled the event.
    pub fn touch_move(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { noesis_view_touch_move(self.ptr.as_ptr(), x, y, id) }
    }

    /// End touch contact `id` at `(x, y)` (physical pixels). Returns whether
    /// Noesis handled the event.
    pub fn touch_up(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { noesis_view_touch_up(self.ptr.as_ptr(), x, y, id) }
    }

    /// Press [`Key`] `key`. Returns whether Noesis handled the event. The view
    /// must be activated ([`Self::activate`]) to receive keyboard input.
    pub fn key_down(&mut self, key: Key) -> bool {
        unsafe { noesis_view_key_down(self.ptr.as_ptr(), key as i32) }
    }

    /// Release [`Key`] `key`. Returns whether Noesis handled the event.
    pub fn key_up(&mut self, key: Key) -> bool {
        unsafe { noesis_view_key_up(self.ptr.as_ptr(), key as i32) }
    }

    /// Text-input codepoint. Send between the matching
    /// [`Self::key_down`]/[`Self::key_up`] pair for the key that produced
    /// the character.
    pub fn char_input(&mut self, codepoint: u32) -> bool {
        unsafe { noesis_view_char(self.ptr.as_ptr(), codepoint) }
    }

    /// Run layout + record a snapshot for the renderer. Returns `false` when
    /// nothing changed and skipping the render pair is safe.
    pub fn update(&mut self, time_seconds: f64) -> bool {
        unsafe { noesis_view_update(self.ptr.as_ptr(), time_seconds) }
    }

    /// Borrow the renderer owned by this view. The `Renderer` can't outlive
    /// the `View`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns a null renderer; impossible on a
    /// successfully-constructed `View`.
    pub fn renderer(&mut self) -> Renderer<'_> {
        let ptr = unsafe { noesis_view_get_renderer(self.ptr.as_ptr()) };
        Renderer {
            ptr: NonNull::new(ptr).expect("GetRenderer returned null"),
            _view: PhantomData,
        }
    }

    /// Take an **owned, thread-movable** handle to this view's renderer, for the
    /// render-thread / UI-thread split: keep driving [`Self::update`] on the UI
    /// thread through this `View`, and move the returned [`RendererHandle`] to a
    /// render thread to call `update_render_tree` / `render` there.
    ///
    /// Unlike [`Self::renderer`] (a borrow that cannot outlive or coexist with
    /// other use of the `View`), the handle holds its own `+1` reference on the
    /// underlying `IView`, so it keeps the view (and the `IRenderer` the view
    /// owns) alive independently. The `View` and the `RendererHandle` may then
    /// live on different threads.
    ///
    /// # Threading contract
    ///
    /// Noesis decouples the two halves through the snapshot taken by
    /// `update`/`update_render_tree`, but it does **not** lock them for you: you
    /// must serialize the hand-off yourself. The supported pattern per frame is
    /// `View::update` (UI thread) → a sync point → `RendererHandle::
    /// update_render_tree` (grabs the snapshot; must not overlap `update`) →
    /// `RendererHandle::render` (may overlap the next `update`). Driving both
    /// halves from one thread is always fine.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns a null renderer; impossible on a
    /// successfully-constructed `View`.
    #[must_use]
    pub fn renderer_handle(&self) -> RendererHandle {
        // AddReference the IView so the handle keeps it alive independently of
        // this View wrapper; balanced by noesis_view_destroy in Drop.
        // SAFETY: self.ptr is a live IView*.
        let view = unsafe { noesis_view_add_reference(self.ptr.as_ptr()) };
        let view = NonNull::new(view).expect("noesis_view_add_reference returned null");
        // SAFETY: view is the live IView* we just took a ref on.
        let renderer = unsafe { noesis_view_get_renderer(view.as_ptr()) };
        RendererHandle {
            view,
            renderer: NonNull::new(renderer).expect("GetRenderer returned null"),
        }
    }

    /// Raw `Noesis::IView*` for handing to other Noesis APIs that take one.
    /// Borrowed for the lifetime of this `View`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }

    /// The view's content root, as an owning [`FrameworkElement`]. Returns
    /// `None` only if the view has no content (which shouldn't happen on a
    /// successfully-constructed `View`, but guard the contract anyway).
    ///
    /// The returned element is independently refcounted; dropping it does
    /// not affect the view's own internal reference. Useful for `find_name`
    /// lookups against the live tree (e.g. wiring [`crate::events::subscribe_click`]
    /// to a named button after the view is up).
    #[must_use]
    pub fn content(&self) -> Option<FrameworkElement> {
        // SAFETY: self.ptr is a live IView*; the C entrypoint AddRefs the
        // returned content pointer so Rust owns the +1.
        let ptr = unsafe { noesis_view_get_content(self.ptr.as_ptr()) };
        NonNull::new(ptr).map(|ptr| FrameworkElement { ptr })
    }
}

impl Drop for View {
    fn drop(&mut self) {
        // SAFETY: produced by noesis_view_create which returns +1 ref.
        unsafe { noesis_view_destroy(self.ptr.as_ptr()) }
    }
}

/// Mirror of `Noesis::HitTestFilterBehavior` (`NsGui/Enums.h`): the value the
/// filter callback of [`FrameworkElement::hit_test_filtered`] returns to steer
/// the tree walk.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HitTestFilterBehavior {
    /// Skip this visual and its subtree, continue elsewhere.
    ContinueSkipSelfAndChildren = 0,
    /// Test this visual but not its children.
    ContinueSkipChildren = 1,
    /// Test this visual's children but not the visual itself.
    ContinueSkipSelf = 2,
    /// Test this visual and descend into its children.
    Continue = 3,
    /// Stop the hit test entirely.
    Stop = 4,
}

/// Mirror of `Noesis::HitTestResultBehavior` (`NsGui/Enums.h`): the value the
/// result callback of [`FrameworkElement::hit_test_filtered`] returns to keep
/// collecting hits or stop.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum HitTestResultBehavior {
    /// Stop the hit test after this hit.
    Stop = 0,
    /// Continue walking for more hits.
    Continue = 1,
}

/// Mirror of `Noesis::MouseButton` from `NsGui/InputEnums.h`. Ordinals
/// validated at C++ compile time via `static_assert` in `noesis_view.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
    XButton1 = 3,
    XButton2 = 4,
}

/// Common subset of `Noesis::Key` from `NsGui/InputEnums.h`. Values are the
/// C++ enum ordinals, validated by `static_assert` in `noesis_view.cpp`.
/// Anything outside this subset can still be sent via [`View::key_down`] with
/// a raw cast; prefer adding a variant here (and a matching assert in C++) to
/// centralize the mapping.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Key {
    None = 0,
    Back = 2,
    Tab = 3,
    Return = 6,
    Pause = 7,
    CapsLock = 8,
    Escape = 13,
    Space = 18,
    PageUp = 19,
    PageDown = 20,
    End = 21,
    Home = 22,
    Left = 23,
    Up = 24,
    Right = 25,
    Down = 26,
    PrintScreen = 30,
    Insert = 31,
    Delete = 32,
    Help = 33,
    D0 = 34,
    D1 = 35,
    D2 = 36,
    D3 = 37,
    D4 = 38,
    D5 = 39,
    D6 = 40,
    D7 = 41,
    D8 = 42,
    D9 = 43,
    A = 44,
    B = 45,
    C = 46,
    D = 47,
    E = 48,
    F = 49,
    G = 50,
    H = 51,
    I = 52,
    J = 53,
    K = 54,
    L = 55,
    M = 56,
    N = 57,
    O = 58,
    P = 59,
    Q = 60,
    R = 61,
    S = 62,
    T = 63,
    U = 64,
    V = 65,
    W = 66,
    X = 67,
    Y = 68,
    Z = 69,
    LWin = 70,
    RWin = 71,
    Apps = 72,
    NumPad0 = 74,
    NumPad1 = 75,
    NumPad2 = 76,
    NumPad3 = 77,
    NumPad4 = 78,
    NumPad5 = 79,
    NumPad6 = 80,
    NumPad7 = 81,
    NumPad8 = 82,
    NumPad9 = 83,
    Multiply = 84,
    Add = 85,
    Subtract = 87,
    Decimal = 88,
    Divide = 89,
    F1 = 90,
    F2 = 91,
    F3 = 92,
    F4 = 93,
    F5 = 94,
    F6 = 95,
    F7 = 96,
    F8 = 97,
    F9 = 98,
    F10 = 99,
    F11 = 100,
    F12 = 101,
    F13 = 102,
    F14 = 103,
    F15 = 104,
    F16 = 105,
    F17 = 106,
    F18 = 107,
    F19 = 108,
    F20 = 109,
    F21 = 110,
    F22 = 111,
    F23 = 112,
    F24 = 113,
    NumLock = 114,
    ScrollLock = 115,
    LeftShift = 116,
    RightShift = 117,
    LeftCtrl = 118,
    RightCtrl = 119,
    LeftAlt = 120,
    RightAlt = 121,
    /// Semicolon / colon on US layouts (`Key_Oem1` / `Key_OemSemicolon`).
    OemSemicolon = 140,
    /// `=` / `+` (`Key_OemPlus`).
    OemPlus = 141,
    OemComma = 142,
    OemMinus = 143,
    OemPeriod = 144,
    /// `/` / `?` (`Key_Oem2` / `Key_OemQuestion`).
    OemSlash = 145,
    /// Backtick / tilde (`Key_Oem3` / `Key_OemTilde`).
    OemTilde = 146,
    /// `[` / `{` (`Key_Oem4` / `Key_OemOpenBrackets`).
    OemOpenBrackets = 149,
    /// `\` / `|` (`Key_Oem5` / `Key_OemPipe`).
    OemPipe = 150,
    /// `]` / `}` (`Key_Oem6` / `Key_OemCloseBrackets`).
    OemCloseBrackets = 151,
    /// `'` / `"` (`Key_Oem7` / `Key_OemQuotes`).
    OemQuotes = 152,
}

/// `Noesis::RenderFlags` bit values mirrored for convenience. See
/// `NsGui/IView.h` for the authoritative list.
#[repr(u32)]
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum RenderFlag {
    Wireframe = 1,
    ColorBatches = 2,
    Overdraw = 4,
    FlipY = 8,
    Ppaa = 16,
    Lcd = 32,
    ShowGlyphs = 64,
    ShowRamps = 128,
    DepthTesting = 256,
}

/// A typed bitset of [`RenderFlag`]s, so callers compose render flags without
/// hand-ORing raw `u32`s. Convert to/from the raw bitmask Noesis uses with
/// [`Self::bits`] / [`Self::from_bits`]; pass it to
/// [`View::set_render_flags`] and read it back via [`View::flags`].
///
/// ```
/// use noesis_runtime::view::{RenderFlag, RenderFlags};
/// let flags = RenderFlags::from_iter([RenderFlag::Ppaa, RenderFlag::Wireframe]);
/// assert!(flags.contains(RenderFlag::Ppaa));
/// assert!(!flags.contains(RenderFlag::Overdraw));
/// assert_eq!(flags.bits(), RenderFlag::Ppaa as u32 | RenderFlag::Wireframe as u32);
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct RenderFlags(pub u32);

impl RenderFlags {
    /// An empty set (no flags).
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    /// Wrap a raw `Noesis::RenderFlags` bitmask.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The raw bitmask Noesis expects.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// Add `flag` to the set (in place).
    pub const fn insert(&mut self, flag: RenderFlag) {
        self.0 |= flag as u32;
    }

    /// Return a copy of this set with `flag` added, handy for `const`-ish
    /// builder chains (`RenderFlags::empty().with(..).with(..)`).
    #[must_use]
    pub const fn with(mut self, flag: RenderFlag) -> Self {
        self.0 |= flag as u32;
        self
    }

    /// Whether `flag` is present in the set.
    #[must_use]
    pub const fn contains(self, flag: RenderFlag) -> bool {
        self.0 & (flag as u32) != 0
    }
}

impl FromIterator<RenderFlag> for RenderFlags {
    fn from_iter<I: IntoIterator<Item = RenderFlag>>(iter: I) -> Self {
        let mut flags = Self(0);
        for flag in iter {
            flags.insert(flag);
        }
        flags
    }
}

impl Extend<RenderFlag> for RenderFlags {
    fn extend<I: IntoIterator<Item = RenderFlag>>(&mut self, iter: I) {
        for flag in iter {
            self.insert(flag);
        }
    }
}

/// Named antialiasing / curve-quality presets for
/// [`View::set_quality`], mapping onto `Noesis::TessellationMaxPixelError`'s
/// screen-space pixel-error thresholds. `Medium` is the Noesis default; `High`
/// is recommended only when rendering to a multisampled surface.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum Quality {
    /// `LowQuality`: 0.7 px error (coarsest curves).
    Low,
    /// `MediumQuality`: 0.4 px error (the Noesis default).
    Medium,
    /// `HighQuality`: 0.2 px error (finest curves).
    High,
}

impl Quality {
    /// The screen-space pixel-error threshold this preset maps to. Mirrors the
    /// `TessellationMaxPixelError::{Low,Medium,High}Quality()` constants.
    #[must_use]
    pub const fn pixel_error(self) -> f32 {
        match self {
            Self::Low => 0.7,
            Self::Medium => 0.4,
            Self::High => 0.2,
        }
    }
}

/// Per-frame performance counters returned by [`View::stats`], mirroring
/// `Noesis::ViewStats` field-for-field (3 `f32` timings then 12 `u32` counts).
/// The `#[repr(C)]` layout is the FFI contract; a `static_assert` in
/// `noesis_view.cpp` guards the size against SDK drift.
///
/// Timing fields are in milliseconds. Counters reflect the work of the last
/// rendered frame, so the geometry / draw / glyph counts are only meaningful
/// after a render pass (`Renderer::render`); a pure `update()` populates the
/// timing-related fields.
#[repr(C)]
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct ViewStats {
    /// Total frame time (ms).
    pub frame_time: f32,
    /// Time spent in `Update` (ms).
    pub update_time: f32,
    /// Time spent rendering (ms).
    pub render_time: f32,
    /// Triangles submitted.
    pub triangles: u32,
    /// Draw calls.
    pub draws: u32,
    /// Batches submitted to the GPU.
    pub batches: u32,
    /// Geometry tessellations performed.
    pub tessellations: u32,
    /// Vertex/index buffer flushes.
    pub flushes: u32,
    /// Geometry buffer size (bytes).
    pub geometry_size: u32,
    /// Clipping masks rendered.
    pub masks: u32,
    /// Opacity (transparency layer) groups rendered.
    pub opacities: u32,
    /// Render-target switches.
    pub render_target_switches: u32,
    /// Gradient ramps uploaded this frame.
    pub uploaded_ramps: u32,
    /// Glyphs rasterized this frame.
    pub rasterized_glyphs: u32,
    /// Glyph atlas tiles discarded this frame.
    pub discarded_glyph_tiles: u32,
}

// ── View-driven timers ───────────────────────────────────────────────────────

/// Rust-side handler for a view timer (see [`View::create_timer`]). Called once
/// per tick from inside [`View::update`]; returns the next interval in
/// milliseconds, or `0` to stop the timer.
///
/// The `Send + 'static` bounds let the handler live inside a Bevy `Resource`
/// or be moved onto the render thread, same rationale as the event handlers
/// in [`crate::events`]. Timers fire on the view-driving thread.
///
/// Takes `&self` (re-entrant: a tick may call [`View::update`] on the same
/// view, re-entering this same box; use interior mutability for handler
/// state).
pub trait TimerHandler: Send + 'static {
    /// Run one tick; return the next interval in ms (`0` stops the timer).
    fn on_tick(&self) -> u32;
}

impl<F: Fn() -> u32 + Send + 'static> TimerHandler for F {
    fn on_tick(&self) -> u32 {
        self()
    }
}

/// SAFETY: `userdata` must be a pointer produced by [`View::create_timer`] and
/// still alive (its [`TimerSubscription`] hasn't been dropped).
unsafe extern "C" fn timer_trampoline(userdata: *mut c_void) -> u32 {
    // A panicking tick is contained and reported as `0` (stop the timer) rather
    // than unwinding across the C ABI.
    crate::panic_guard::guard(|| {
        // SAFETY: userdata is the double-boxed handler leaked in create_timer; the
        // RustTimer keeps it alive until cancel, so the deref is valid here.
        // Shared `&`: re-entrant handler box (see `TimerHandler`).
        let handler = unsafe { &*userdata.cast::<Box<dyn TimerHandler>>() };
        handler.on_tick()
    })
}

/// SAFETY: `userdata` must be the pointer donated to the C++ `RustTimer` by
/// [`View::create_timer`]; the C side invokes this exactly once on cancel.
unsafe extern "C" fn timer_free(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        // SAFETY: reconstitute and drop the double box we leaked in create_timer.
        unsafe { drop(Box::from_raw(userdata.cast::<Box<dyn TimerHandler>>())) };
    })
}

/// RAII handle for a view timer created by [`View::create_timer`]. While alive,
/// the timer stays scheduled; dropping it cancels the timer and frees the
/// boxed handler (the C++ teardown runs `CancelTimer` then the donated free
/// handler exactly once). Drop it before [`crate::shutdown`], like every other
/// owning handle in this crate.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct TimerSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for TimerSubscription {}

impl TimerSubscription {
    /// Restart this timer with a new interval (ms). Equivalent to
    /// `IView::RestartTimer`; takes effect on the next [`View::update`].
    pub fn restart(&self, interval_ms: u32) {
        // SAFETY: token is a live RustTimer* until this handle drops.
        unsafe { noesis_view_restart_timer(self.token.as_ptr(), interval_ms) };
    }
}

impl Drop for TimerSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by create_timer; cancel deletes the RustTimer
        // (running the donated free handler), freed exactly once here.
        unsafe { noesis_view_cancel_timer(self.token.as_ptr()) };
    }
}

// ── Rendering event ──────────────────────────────────────────────────────────

/// Rust-side handler for a view's `Rendering` event (see
/// [`View::add_rendering_handler`]). Called once per frame from inside
/// [`View::update`], after animation + layout and before the composition tree
/// is rendered.
///
/// The `Send + 'static` bounds let the handler live inside a Bevy `Resource` or
/// be moved onto the render thread, same rationale as [`TimerHandler`]. The
/// event fires on the view-driving thread.
///
/// Takes `&self` (re-entrant: the callback may call [`View::update`] on the
/// same view, re-entering this same box; use interior mutability for handler
/// state).
pub trait RenderingHandler: Send + 'static {
    /// Run one frame's rendering callback.
    fn on_rendering(&self);
}

impl<F: Fn() + Send + 'static> RenderingHandler for F {
    fn on_rendering(&self) {
        self();
    }
}

/// SAFETY: `userdata` must be a pointer produced by
/// [`View::add_rendering_handler`] and still alive (its
/// [`RenderingSubscription`] hasn't been dropped).
unsafe extern "C" fn rendering_trampoline(userdata: *mut c_void, _view: *mut c_void) {
    crate::panic_guard::guard(|| {
        // SAFETY: userdata is the double-boxed handler leaked in
        // add_rendering_handler; the C++ handler keeps it alive until removal, so
        // the deref is valid here.
        // Shared `&`: re-entrant handler box (see `RenderingHandler`).
        let handler = unsafe { &*userdata.cast::<Box<dyn RenderingHandler>>() };
        handler.on_rendering();
    })
}

/// SAFETY: `userdata` must be the pointer donated to the C++ handler by
/// [`View::add_rendering_handler`]; the C side invokes this exactly once on
/// removal.
unsafe extern "C" fn rendering_free(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        // SAFETY: reconstitute and drop the double box we leaked in
        // add_rendering_handler.
        unsafe { drop(Box::from_raw(userdata.cast::<Box<dyn RenderingHandler>>())) };
    })
}

/// RAII handle for a `Rendering` subscription created by
/// [`View::add_rendering_handler`]. While alive, the handler stays attached;
/// dropping it detaches the delegate and frees the boxed handler (the C++
/// teardown runs `-=` then the donated free handler exactly once). Drop it
/// before [`crate::shutdown`], like every other owning handle in this crate.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct RenderingSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for RenderingSubscription {}

impl Drop for RenderingSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by add_rendering_handler; removal detaches the
        // delegate and runs the donated free handler, exactly once here.
        unsafe { noesis_view_remove_rendering_handler(self.token.as_ptr()) };
    }
}

/// Borrowed handle to the view's renderer. Methods map 1:1 onto
/// `Noesis::IRenderer`; the renderer is owned by the view and must not
/// outlive it.
pub struct Renderer<'a> {
    ptr: NonNull<c_void>,
    _view: PhantomData<&'a mut View>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for Renderer<'_> {}

impl Renderer<'_> {
    /// Bind the Noesis renderer to `render_device`. Must be called once
    /// before any of the render methods. Pair with [`Self::shutdown`] before
    /// the device is dropped.
    pub fn init(&mut self, render_device: &RegisteredDevice) {
        // SAFETY: RegisteredDevice owns a live Noesis::RenderDevice* and
        // outlives this call (borrow checker enforces).
        unsafe { noesis_renderer_init(self.ptr.as_ptr(), render_device.raw()) }
    }

    /// Release the renderer's device-bound resources.
    pub fn shutdown(&mut self) {
        unsafe { noesis_renderer_shutdown(self.ptr.as_ptr()) }
    }

    /// Grab the most recent snapshot captured by [`View::update`]. Returns
    /// `false` when no new snapshot was available.
    pub fn update_render_tree(&mut self) -> bool {
        unsafe { noesis_renderer_update_render_tree(self.ptr.as_ptr()) }
    }

    /// Populate offscreen textures the next [`Self::render`] may sample.
    /// Returns `false` when nothing was rendered (safe to skip GPU state
    /// restore in that case).
    pub fn render_offscreen(&mut self) -> bool {
        unsafe { noesis_renderer_render_offscreen(self.ptr.as_ptr()) }
    }

    /// Render the UI into the currently-bound "onscreen" target (from the
    /// render device's perspective).
    pub fn render(&mut self, flip_y: bool, clear: bool) {
        unsafe { noesis_renderer_render(self.ptr.as_ptr(), flip_y, clear) }
    }

    /// Multi-pass stereo (VR) render of a single eye
    /// (`IRenderer::RenderStereo`). Call once per eye, binding that eye's
    /// render target first. `eye_matrix` is a row-major 4×4 (16 floats); since
    /// culling uses the view's projection (see
    /// [`View::set_projection_matrix`]), the eye matrix must be enclosed by it.
    /// Pair with [`View::set_stereo_offscreen_scale_factor`] (2-3 for VR).
    pub fn render_stereo(&mut self, eye_matrix: &[f32; 16], flip_y: bool, clear: bool) {
        // SAFETY: self.ptr is a live IRenderer*; eye_matrix is exactly 16 floats
        // as the C side reads (Matrix4(const float*)).
        unsafe {
            noesis_renderer_render_stereo(self.ptr.as_ptr(), eye_matrix.as_ptr(), flip_y, clear)
        }
    }

    /// Single-pass stereo (VR) render of both eyes in one call
    /// (`IRenderer::RenderStereo`), for multiview / instanced VR pipelines.
    /// Each eye matrix is a row-major 4×4 (16 floats), and both must be
    /// enclosed by the view's projection matrix.
    pub fn render_stereo_both(
        &mut self,
        left_eye_matrix: &[f32; 16],
        right_eye_matrix: &[f32; 16],
        flip_y: bool,
        clear: bool,
    ) {
        // SAFETY: self.ptr is a live IRenderer*; each matrix is exactly 16
        // floats as the C side reads.
        unsafe {
            noesis_renderer_render_stereo_both(
                self.ptr.as_ptr(),
                left_eye_matrix.as_ptr(),
                right_eye_matrix.as_ptr(),
                flip_y,
                clear,
            )
        }
    }
}

/// An **owned**, thread-movable handle to a view's renderer, from
/// [`View::renderer_handle`]. Holds its own `+1` reference on the underlying
/// `IView` (which owns the `IRenderer`), so it keeps both alive independently of
/// the [`View`] wrapper and can be moved to a render thread for the
/// render-thread / UI-thread split. See [`View::renderer_handle`] for the
/// threading contract.
///
/// Call [`Self::renderer`] each frame to get the borrowed [`Renderer`] the
/// actual render calls live on. Drop the handle (before [`crate::shutdown`])
/// to release its view reference.
pub struct RendererHandle {
    view: NonNull<c_void>,     // owns a +1 ref on the IView.
    renderer: NonNull<c_void>, // borrowed from `view`; valid while the ref is held.
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for RendererHandle {}

impl RendererHandle {
    /// Borrow the [`Renderer`] for this frame's render calls (`init` /
    /// `update_render_tree` / `render` / `render_stereo` / ...). The borrow is
    /// tied to `&mut self`, so it cannot escape the handle.
    pub fn renderer(&mut self) -> Renderer<'_> {
        Renderer {
            ptr: self.renderer,
            _view: PhantomData,
        }
    }

    /// Raw `Noesis::IView*` this handle keeps alive (borrowed for the handle's
    /// lifetime). Useful for APIs that take the view rather than the renderer.
    #[must_use]
    pub fn view_raw(&self) -> *mut c_void {
        self.view.as_ptr()
    }
}

impl Drop for RendererHandle {
    fn drop(&mut self) {
        // SAFETY: `view` carries the +1 ref taken in View::renderer_handle;
        // release it exactly once. noesis_view_destroy is just IView::Release.
        unsafe { noesis_view_destroy(self.view.as_ptr()) };
    }
}
