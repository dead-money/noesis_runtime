//! Safe wrappers around the Noesis `FrameworkElement`, `IView`, and
//! `IRenderer` opaque pointers (Phase 4.C).
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
//! (i.e. [`crate::shutdown`] not yet called) at drop time — otherwise the
//! `Release()` path would touch freed state. Keep these wrappers on the
//! stack for the scope of a single frame, dropped before `shutdown`.

use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::{CStr, CString, c_void};

use crate::ffi::{
    PropType, dm_noesis_base_component_release, dm_noesis_dependency_object_get_property,
    dm_noesis_dependency_object_set_property, dm_noesis_focus_element,
    dm_noesis_framework_element_find_name, dm_noesis_framework_element_get_data_context,
    dm_noesis_framework_element_get_name, dm_noesis_framework_element_set_data_context,
    dm_noesis_framework_element_set_margin, dm_noesis_framework_element_set_visibility,
    dm_noesis_gui_load_xaml, dm_noesis_items_control_items_count,
    dm_noesis_items_control_realized_count, dm_noesis_items_control_set_items_source,
    dm_noesis_path_set_points, dm_noesis_renderer_init, dm_noesis_renderer_render,
    dm_noesis_renderer_render_offscreen, dm_noesis_renderer_shutdown,
    dm_noesis_renderer_update_render_tree, dm_noesis_text_caret_to_end, dm_noesis_text_get,
    dm_noesis_text_set, dm_noesis_view_activate, dm_noesis_view_char, dm_noesis_view_create,
    dm_noesis_view_deactivate, dm_noesis_view_destroy, dm_noesis_view_get_content,
    dm_noesis_view_get_renderer, dm_noesis_view_hscroll, dm_noesis_view_key_down,
    dm_noesis_view_key_up, dm_noesis_view_mouse_button_down, dm_noesis_view_mouse_button_up,
    dm_noesis_view_mouse_double_click, dm_noesis_view_mouse_move, dm_noesis_view_mouse_wheel,
    dm_noesis_view_scroll, dm_noesis_view_set_flags, dm_noesis_view_set_projection_matrix,
    dm_noesis_view_set_scale, dm_noesis_view_set_size, dm_noesis_view_touch_down,
    dm_noesis_view_touch_move, dm_noesis_view_touch_up, dm_noesis_view_update,
    dm_noesis_visual_state_go_to_state,
};
use crate::render_device::Registered as RegisteredDevice;

/// A loaded XAML root. Holds a +1 refcount on the underlying
/// `Noesis::FrameworkElement`; [`View::create`] consumes it and forwards the
/// ownership to the View.
pub struct FrameworkElement {
    ptr: NonNull<c_void>,
}

// SAFETY: `FrameworkElement` wraps a raw pointer to a Noesis-owned
// `Ptr<FrameworkElement>`. Noesis's API contract is "calls on a given object
// are serialized to one thread" — not "the object must stay on one thread
// for its whole lifetime." Moving a FrameworkElement between threads (via
// `Send`) is safe as long as the receiving thread is the only one making
// subsequent calls. Bevy's resource scheduler guarantees that: access to
// a `Resource` is serialized through `ResMut<_>`, and our callers only
// hold the element across a single render-thread borrow.
//
// `Sync` is safe for essentially the same reason: every mutating method
// takes `&mut self`, so `&FrameworkElement` carries no usable calls to
// Noesis — concurrent shared borrows can't race on Noesis state.
unsafe impl Send for FrameworkElement {}
unsafe impl Sync for FrameworkElement {}

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
        let ptr = unsafe { dm_noesis_gui_load_xaml(c.as_ptr()) };
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

    /// Look up a descendant by `x:Name`. Returns `None` if no element with
    /// that name exists in this element's namescope, or if the named object
    /// is not itself a `FrameworkElement` (e.g. it's a `Brush` registered in
    /// a `ResourceDictionary`).
    ///
    /// The returned element holds an independent `+1` reference — dropping
    /// it does not affect `self`.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    #[must_use]
    pub fn find_name(&self, name: &str) -> Option<Self> {
        let c = CString::new(name).expect("name contained interior NUL");
        // SAFETY: self.ptr is a live FrameworkElement*; c lives for the call.
        let ptr = unsafe { dm_noesis_framework_element_find_name(self.ptr.as_ptr(), c.as_ptr()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// The element's `x:Name`, or `None` if it has no name. The returned
    /// string is a borrowed copy — Noesis owns the underlying storage.
    #[must_use]
    pub fn name(&self) -> Option<String> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C entrypoint
        // returns either NULL or a Noesis-owned static-ish string we copy
        // immediately.
        let p = unsafe { dm_noesis_framework_element_get_name(self.ptr.as_ptr()) };
        if p.is_null() {
            None
        } else {
            // SAFETY: p is a NUL-terminated UTF-8 / ASCII string while we
            // hold our element reference; copy out before yielding control.
            Some(unsafe { CStr::from_ptr(p) }.to_string_lossy().into_owned())
        }
    }

    /// Set `Visibility` to `Visible` (`visible = true`) or `Collapsed`
    /// (`visible = false`). The third Noesis Visibility state — `Hidden`,
    /// where the element reserves layout space but doesn't paint —
    /// isn't surfaced; modal-overlay and panel-toggle patterns
    /// (the use cases driving this API) want full Collapsed behaviour.
    /// Add a separate setter if a consumer needs Hidden later.
    pub fn set_visibility(&mut self, visible: bool) {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side does a
        // null check + a typed `SetValue` on the `Visibility` DP. No
        // userdata or callbacks pass through.
        unsafe { dm_noesis_framework_element_set_visibility(self.ptr.as_ptr(), visible) }
    }

    /// Set this element's `Margin` (layout offsets in DIPs: left, top, right,
    /// bottom). Paired with `HorizontalAlignment="Left"` /
    /// `VerticalAlignment="Top"`, a margin of `(x, y, 0, 0)` lands the element's
    /// top-left corner at `(x, y)` — the positioning primitive a floating
    /// menu / popup needs, since Noesis's `Canvas.Left`/`Top` attached property
    /// isn't surfaced through this shim.
    pub fn set_margin(&mut self, left: f32, top: f32, right: f32, bottom: f32) {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side null-checks
        // and does a typed `SetMargin(Thickness)`. No userdata or callbacks pass
        // through.
        unsafe {
            dm_noesis_framework_element_set_margin(self.ptr.as_ptr(), left, top, right, bottom);
        }
    }

    /// Read the `Text` property of a `TextBox` or `TextBlock`, copying it
    /// into an owned [`String`]. Returns `None` if this element is neither
    /// a `TextBox` nor a `TextBlock`, or if the underlying text is null
    /// (Noesis returns null for an unset / never-touched Text DP).
    ///
    /// The pointer Noesis returns is borrowed — we copy immediately so the
    /// owned String stays valid past the next layout pass (which may
    /// reallocate the underlying storage).
    #[must_use]
    pub fn text(&self) -> Option<String> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side
        // DynamicCasts to TextBox/TextBlock and reads `GetText()`. The
        // returned pointer is null on type mismatch, otherwise a borrowed
        // NUL-terminated UTF-8 string from Noesis-owned storage.
        let p = unsafe { dm_noesis_text_get(self.ptr.as_ptr()) };
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
    pub fn set_text(&mut self, text: &str) -> bool {
        let c = CString::new(text).expect("text contained interior NUL");
        // SAFETY: self.ptr is a live FrameworkElement*; c.as_ptr() lives
        // for the call duration; the C side either copies into Noesis-
        // owned storage (TextBox::SetText / TextBlock::SetText) or returns
        // false on a type mismatch.
        unsafe { dm_noesis_text_set(self.ptr.as_ptr(), c.as_ptr()) }
    }

    /// Set the caret of a `TextBox` to the end of its current text. No-op
    /// (returns `false`) if the element is not a `TextBox`. Mirrors `AoR`'s
    /// `_commandInput.CaretIndex = _commandInput.Text.Length` pattern
    /// after a history-nav substitution.
    pub fn set_caret_to_end(&mut self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side does a
        // null check + DynamicCast + SetCaretIndex.
        unsafe { dm_noesis_text_caret_to_end(self.ptr.as_ptr()) }
    }

    /// Move keyboard focus to this element. Returns the value Noesis
    /// reports for `UIElement::Focus()` — `true` if the element accepted
    /// focus, `false` if it's not a `UIElement` or is non-focusable.
    pub fn focus(&mut self) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side does a
        // DynamicCast<UIElement*> + Focus().
        unsafe { dm_noesis_focus_element(self.ptr.as_ptr()) }
    }

    /// Assign this element's geometry — as a `Path` — to an open polyline through
    /// `points` (`[x, y]` pairs in the Path's local coordinate space). Returns
    /// `false` if the element is not a `Path` or there are fewer than two points.
    /// A real vector trace (built via a Noesis `StreamGeometry`), the geometry
    /// counterpart of [`set_text`](Self::set_text).
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
        unsafe {
            dm_noesis_path_set_points(self.ptr.as_ptr(), points.as_ptr().cast::<f32>(), count)
        }
    }

    /// Transition this control to the visual state named `state`, via
    /// `VisualStateManager::GoToState`. Pass `use_transitions = true` to run
    /// the state's `VisualTransition` (animated change), or `false` to snap
    /// straight to the new state.
    ///
    /// This targets a templated control: `GoToState` resolves `state` against
    /// the `VisualStateGroup`s declared in the element's `ControlTemplate`
    /// (e.g. a `Button`'s `CommonStates` — `Normal` / `MouseOver` / `Pressed`
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
        unsafe {
            dm_noesis_visual_state_go_to_state(self.ptr.as_ptr(), c.as_ptr(), use_transitions)
        }
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
            dm_noesis_dependency_object_set_property(self.ptr.as_ptr(), c.as_ptr(), kind, value_ptr)
        }
    }

    /// Internal: resolve `name` to a C string and forward a typed get into
    /// `out`. Returns `false` on unknown name / tag mismatch / not-a-DO.
    fn get_prop(&self, name: &str, kind: PropType, out: *mut c_void) -> bool {
        let c = CString::new(name).expect("property name contained interior NUL");
        // SAFETY: self.ptr is a live DependencyObject*; c lives for the call;
        // `out` points at a buffer matching the per-type FFI layout.
        unsafe {
            dm_noesis_dependency_object_get_property(self.ptr.as_ptr(), c.as_ptr(), kind, out)
        }
    }

    /// Set an `Int32` dependency property by name.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn set_i32(&mut self, name: &str, value: i32) -> bool {
        self.set_prop(name, PropType::Int32, (&value as *const i32).cast())
    }

    /// Set a `Float` (single-precision) dependency property by name. Most
    /// `FrameworkElement` scalars Noesis exposes — `Width`, `Height`,
    /// `Opacity` — are `float`, so this is the common case.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn set_f32(&mut self, name: &str, value: f32) -> bool {
        self.set_prop(name, PropType::Float, (&value as *const f32).cast())
    }

    /// Set a `Double` (double-precision) dependency property by name.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn set_f64(&mut self, name: &str, value: f64) -> bool {
        self.set_prop(name, PropType::Double, (&value as *const f64).cast())
    }

    /// Set a `Bool` dependency property by name.
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn set_bool(&mut self, name: &str, value: bool) -> bool {
        self.set_prop(name, PropType::Bool, (&value as *const bool).cast())
    }

    /// Set a `String` dependency property by name. Noesis copies the bytes
    /// into its own storage.
    ///
    /// # Panics
    ///
    /// Panics if `name` or `value` contains an interior NUL byte.
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
    pub fn set_thickness(&mut self, name: &str, value: [f32; 4]) -> bool {
        self.set_prop(name, PropType::Thickness, value.as_ptr().cast())
    }

    /// Set a `Color` dependency property (`r, g, b, a`, each in `0..=1`).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
    pub fn set_color(&mut self, name: &str, rgba: [f32; 4]) -> bool {
        self.set_prop(name, PropType::Color, rgba.as_ptr().cast())
    }

    /// Set a `Rect` dependency property (`x, y, width, height`).
    ///
    /// # Panics
    ///
    /// Panics if `name` contains an interior NUL byte.
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

    /// Read a reference-typed dependency property (any `BaseComponent`
    /// subclass — `Brush`, `ImageSource`, `Style`, …) as a borrowed opaque
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

    // ── Data binding (TODO §3) ──────────────────────────────────────────────
    //
    // Point this element's `DataContext` at a Rust view model, or an
    // ItemsControl's `ItemsSource` at an [`crate::binding::ObservableCollection`].
    // Bindings authored in XAML (`{Binding Path}`) then resolve against that
    // Rust-owned data. Same View-thread affinity as the other accessors here.

    /// Set this element's `DataContext` to an arbitrary `Noesis::BaseComponent*`
    /// (most usefully a [`crate::classes::ClassInstance`] view model). Returns
    /// `false` if this element is not a `FrameworkElement`. Noesis stores its
    /// own reference to `context`.
    ///
    /// # Safety
    ///
    /// `context` must be a valid live `Noesis::BaseComponent*` (e.g. from
    /// [`crate::classes::ClassInstance::raw`]) or null to clear.
    pub unsafe fn set_data_context(&mut self, context: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; `context` is a live
        // BaseComponent* (or null) per the contract above; the C side
        // DynamicCasts and null-checks.
        unsafe { dm_noesis_framework_element_set_data_context(self.ptr.as_ptr(), context) }
    }

    /// Clear this element's `DataContext`.
    pub fn clear_data_context(&mut self) -> bool {
        // SAFETY: clearing with null is always sound.
        unsafe {
            dm_noesis_framework_element_set_data_context(self.ptr.as_ptr(), core::ptr::null_mut())
        }
    }

    /// Borrowed (no `+1`) pointer to this element's current `DataContext`, or
    /// `None` if unset / not a `FrameworkElement`.
    #[must_use]
    pub fn data_context(&self) -> Option<NonNull<c_void>> {
        // SAFETY: self.ptr is a live FrameworkElement*; the C side returns a
        // borrowed pointer or null.
        let p = unsafe { dm_noesis_framework_element_get_data_context(self.ptr.as_ptr()) };
        NonNull::new(p)
    }

    /// Set this element's `ItemsSource` (it must be an `ItemsControl` — e.g.
    /// `ItemsControl` / `ListBox` / `ListView`). Returns `false` if this element
    /// is not an `ItemsControl`. Pass an
    /// [`crate::binding::ObservableCollection`]'s `raw()` to drive a live list.
    ///
    /// # Safety
    ///
    /// `items` must be a valid live `Noesis::BaseComponent*` implementing a
    /// list interface (e.g. an `ObservableCollection`) or null to clear.
    pub unsafe fn set_items_source(&mut self, items: *mut c_void) -> bool {
        // SAFETY: self.ptr is a live FrameworkElement*; `items` is a live
        // BaseComponent* (or null) per the contract above.
        unsafe { dm_noesis_items_control_set_items_source(self.ptr.as_ptr(), items) }
    }

    /// Number of items this `ItemsControl` sees through its bound source (a live
    /// passthrough to the `ItemsSource`). `None` if this element is not an
    /// `ItemsControl`.
    #[must_use]
    pub fn items_count(&self) -> Option<usize> {
        // SAFETY: self.ptr is a live FrameworkElement*.
        let n = unsafe { dm_noesis_items_control_items_count(self.ptr.as_ptr()) };
        (n >= 0).then_some(n as usize)
    }

    /// Number of *realized* item containers the generator has materialized.
    /// Unlike [`items_count`](Self::items_count), this only grows when the
    /// generator regenerates — which for a source mutated after the first
    /// layout pass requires `INotifyCollectionChanged` to have fired. `None` if
    /// this element is not an `ItemsControl`.
    #[must_use]
    pub fn realized_item_count(&self) -> Option<usize> {
        // SAFETY: self.ptr is a live FrameworkElement*.
        let n = unsafe { dm_noesis_items_control_realized_count(self.ptr.as_ptr()) };
        (n >= 0).then_some(n as usize)
    }
}

impl Drop for FrameworkElement {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_gui_load_xaml which returns a +1 ref.
        unsafe { dm_noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A Noesis view wrapping a loaded XAML root. Owns a +1 refcount on the
/// underlying `Noesis::IView`; its internal `Ptr<FrameworkElement>` keeps
/// the root alive too.
pub struct View {
    ptr: NonNull<c_void>,
}

// SAFETY: same rationale as [`FrameworkElement`] — Noesis serialises
// per-object calls to one thread at a time; every `View` method is `&mut
// self`; Bevy's scheduler prevents concurrent access. Moving a View between
// threads, or holding a `&View` from multiple threads simultaneously (which
// offers no usable mutation), is safe.
unsafe impl Send for View {}
unsafe impl Sync for View {}

impl View {
    /// Create a View whose root is `content`. Consumes the
    /// [`FrameworkElement`] wrapper — its refcount transfers into the view.
    ///
    /// # Panics
    ///
    /// Panics if the Noesis factory returns null (only possible on internal
    /// logic errors once `content` is non-null).
    #[must_use]
    pub fn create(content: FrameworkElement) -> Self {
        let raw = content.into_raw();
        // SAFETY: raw is a live FrameworkElement* with +1 ref.
        let ptr = unsafe { dm_noesis_view_create(raw) };
        // View took its own ref internally; release our +1 on the element so
        // refcount stays balanced (its total is still the original 1).
        unsafe { dm_noesis_base_component_release(raw) };
        Self {
            ptr: NonNull::new(ptr).expect("dm_noesis_view_create returned null"),
        }
    }

    /// Surface size the view lays out against.
    pub fn set_size(&mut self, width: u32, height: u32) {
        unsafe { dm_noesis_view_set_size(self.ptr.as_ptr(), width, height) }
    }

    /// DPI scale for the view's content (1.0 == 96 ppi). Scales layout + hit
    /// testing without resizing the surface, keeping the UI crisp at any density.
    pub fn set_scale(&mut self, scale: f32) {
        unsafe { dm_noesis_view_set_scale(self.ptr.as_ptr(), scale) }
    }

    /// Set the projection matrix. 16 floats, row-major — the native
    /// `Matrix4::GetData()` layout. Typical Noesis-facing projection is an
    /// ortho that maps UI pixel coords into Noesis's clip space (0..width,
    /// 0..height).
    pub fn set_projection_matrix(&mut self, matrix: &[f32; 16]) {
        unsafe { dm_noesis_view_set_projection_matrix(self.ptr.as_ptr(), matrix.as_ptr()) }
    }

    /// Combination of [`RenderFlag`] values — see `NsGui/IView.h` for the
    /// canonical list.
    pub fn set_flags(&mut self, flags: u32) {
        unsafe { dm_noesis_view_set_flags(self.ptr.as_ptr(), flags) }
    }

    /// Recover keyboard focus for this view. Noesis ignores keyboard input
    /// until a view is activated.
    pub fn activate(&mut self) {
        unsafe { dm_noesis_view_activate(self.ptr.as_ptr()) }
    }

    /// Release keyboard focus.
    pub fn deactivate(&mut self) {
        unsafe { dm_noesis_view_deactivate(self.ptr.as_ptr()) }
    }

    /// Pointer position, in physical pixels, origin top-left. Noesis
    /// requires a `mouse_move` at the press coordinate before a
    /// [`Self::mouse_button_down`] or [`Self::touch_down`] will hit-test
    /// correctly; callers must ensure the ordering.
    pub fn mouse_move(&mut self, x: i32, y: i32) -> bool {
        unsafe { dm_noesis_view_mouse_move(self.ptr.as_ptr(), x, y) }
    }

    pub fn mouse_button_down(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { dm_noesis_view_mouse_button_down(self.ptr.as_ptr(), x, y, button as i32) }
    }

    pub fn mouse_button_up(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { dm_noesis_view_mouse_button_up(self.ptr.as_ptr(), x, y, button as i32) }
    }

    pub fn mouse_double_click(&mut self, x: i32, y: i32, button: MouseButton) -> bool {
        unsafe { dm_noesis_view_mouse_double_click(self.ptr.as_ptr(), x, y, button as i32) }
    }

    /// `delta` is signed — Noesis uses Windows-style 120 units per notch.
    pub fn mouse_wheel(&mut self, x: i32, y: i32, delta: i32) -> bool {
        unsafe { dm_noesis_view_mouse_wheel(self.ptr.as_ptr(), x, y, delta) }
    }

    /// Vertical scroll with the cursor at `(x, y)`. `value` is in lines
    /// (per WPF convention — integer lines, fractional allowed).
    pub fn scroll(&mut self, x: i32, y: i32, value: f32) -> bool {
        unsafe { dm_noesis_view_scroll(self.ptr.as_ptr(), x, y, value) }
    }

    /// Horizontal scroll. See [`Self::scroll`].
    pub fn hscroll(&mut self, x: i32, y: i32, value: f32) -> bool {
        unsafe { dm_noesis_view_hscroll(self.ptr.as_ptr(), x, y, value) }
    }

    pub fn touch_down(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { dm_noesis_view_touch_down(self.ptr.as_ptr(), x, y, id) }
    }

    pub fn touch_move(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { dm_noesis_view_touch_move(self.ptr.as_ptr(), x, y, id) }
    }

    pub fn touch_up(&mut self, x: i32, y: i32, id: u64) -> bool {
        unsafe { dm_noesis_view_touch_up(self.ptr.as_ptr(), x, y, id) }
    }

    pub fn key_down(&mut self, key: Key) -> bool {
        unsafe { dm_noesis_view_key_down(self.ptr.as_ptr(), key as i32) }
    }

    pub fn key_up(&mut self, key: Key) -> bool {
        unsafe { dm_noesis_view_key_up(self.ptr.as_ptr(), key as i32) }
    }

    /// Text-input codepoint. Send between the matching
    /// [`Self::key_down`]/[`Self::key_up`] pair for the key that produced
    /// the character.
    pub fn char_input(&mut self, codepoint: u32) -> bool {
        unsafe { dm_noesis_view_char(self.ptr.as_ptr(), codepoint) }
    }

    /// Run layout + record a snapshot for the renderer. Returns `false` when
    /// nothing changed and skipping the render pair is safe.
    pub fn update(&mut self, time_seconds: f64) -> bool {
        unsafe { dm_noesis_view_update(self.ptr.as_ptr(), time_seconds) }
    }

    /// Borrow the renderer owned by this view. The `Renderer` can't outlive
    /// the `View`.
    ///
    /// # Panics
    ///
    /// Panics if Noesis returns a null renderer — impossible on a
    /// successfully-constructed `View`.
    pub fn renderer(&mut self) -> Renderer<'_> {
        let ptr = unsafe { dm_noesis_view_get_renderer(self.ptr.as_ptr()) };
        Renderer {
            ptr: NonNull::new(ptr).expect("GetRenderer returned null"),
            _view: PhantomData,
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
    /// successfully-constructed `View` — but guard the contract anyway).
    ///
    /// The returned element is independently refcounted; dropping it does
    /// not affect the view's own internal reference. Useful for `find_name`
    /// lookups against the live tree (e.g. wiring [`crate::events::subscribe_click`]
    /// to a named button after the view is up).
    #[must_use]
    pub fn content(&self) -> Option<FrameworkElement> {
        // SAFETY: self.ptr is a live IView*; the C entrypoint AddRefs the
        // returned content pointer so Rust owns the +1.
        let ptr = unsafe { dm_noesis_view_get_content(self.ptr.as_ptr()) };
        NonNull::new(ptr).map(|ptr| FrameworkElement { ptr })
    }
}

impl Drop for View {
    fn drop(&mut self) {
        // SAFETY: produced by dm_noesis_view_create which returns +1 ref.
        unsafe { dm_noesis_view_destroy(self.ptr.as_ptr()) }
    }
}

/// Mirror of `Noesis::MouseButton` from `NsGui/InputEnums.h`. Ordinals
/// validated at C++ compile time via `static_assert` in `noesis_view.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum MouseButton {
    Left = 0,
    Right = 1,
    Middle = 2,
    XButton1 = 3,
    XButton2 = 4,
}

/// Subset of `Noesis::Key` from `NsGui/InputEnums.h` — the keys Bevy's
/// `KeyCode` can produce. Values are the C++ enum ordinals, validated by
/// `static_assert` in `noesis_view.cpp`. Anything outside this subset can
/// still be sent via [`View::key_down`] with a raw cast; prefer adding a
/// variant here (and a matching assert in C++) to centralize the mapping.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
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

/// Borrowed handle to the view's renderer. Methods map 1:1 onto
/// `Noesis::IRenderer`; the renderer is owned by the view and must not
/// outlive it.
pub struct Renderer<'a> {
    ptr: NonNull<c_void>,
    _view: PhantomData<&'a mut View>,
}

// SAFETY: mirrors [`View`]. `Renderer` is a transient borrow that shares
// thread-safety properties with the `View` it was produced from.
unsafe impl Send for Renderer<'_> {}
unsafe impl Sync for Renderer<'_> {}

impl Renderer<'_> {
    /// Bind the Noesis renderer to `render_device`. Must be called once
    /// before any of the render methods. Pair with [`Self::shutdown`] before
    /// the device is dropped.
    pub fn init(&mut self, render_device: &RegisteredDevice) {
        // SAFETY: RegisteredDevice owns a live Noesis::RenderDevice* and
        // outlives this call (borrow checker enforces).
        unsafe { dm_noesis_renderer_init(self.ptr.as_ptr(), render_device.raw()) }
    }

    /// Release the renderer's device-bound resources.
    pub fn shutdown(&mut self) {
        unsafe { dm_noesis_renderer_shutdown(self.ptr.as_ptr()) }
    }

    /// Grab the most recent snapshot captured by [`View::update`]. Returns
    /// `false` when no new snapshot was available.
    pub fn update_render_tree(&mut self) -> bool {
        unsafe { dm_noesis_renderer_update_render_tree(self.ptr.as_ptr()) }
    }

    /// Populate offscreen textures the next [`Self::render`] may sample.
    /// Returns `false` when nothing was rendered (safe to skip GPU state
    /// restore in that case).
    pub fn render_offscreen(&mut self) -> bool {
        unsafe { dm_noesis_renderer_render_offscreen(self.ptr.as_ptr()) }
    }

    /// Render the UI into the currently-bound "onscreen" target (from the
    /// render device's perspective).
    pub fn render(&mut self, flip_y: bool, clear: bool) {
        unsafe { dm_noesis_renderer_render(self.ptr.as_ptr(), flip_y, clear) }
    }
}
