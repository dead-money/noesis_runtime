//! Input — finer control (TODO §16): keyboard/focus enums, input gestures and
//! bindings, and the `FocusManager` / `KeyboardNavigation` static surfaces.
//!
//! The per-element knobs (mouse/touch capture, keyboard-state queries,
//! focus-state DPs, focus engagement, `MoveFocus` / `PredictFocus`) live as
//! methods on [`FrameworkElement`](crate::view::FrameworkElement), mirroring its
//! existing [`focus`](crate::view::FrameworkElement::focus). This module holds
//! the value types those methods speak in, plus the gesture/binding objects and
//! the two attached-property static helpers.
//!
//! Gestures and bindings cross the FFI as opaque Noesis objects, exactly like
//! [`Command`](crate::commands::Command): each owns a `+1` reference released on
//! drop. A [`KeyBinding`] / [`MouseBinding`] / [`InputBinding`] is added to an
//! element's `InputBindings`, after which the element's collection holds its own
//! reference and drives the bound [`AsCommand`](crate::commands::AsCommand) when
//! the gesture is matched.
//!
//! # Threading
//!
//! Same View-thread affinity as the rest of the crate's element accessors: the
//! statics and binding adds run on the thread driving the `View`.

use core::ptr::NonNull;
use std::ffi::c_void;

use crate::commands::AsCommand;
use crate::ffi::{
    noesis_base_component_release, noesis_focus_manager_get_focus_scope,
    noesis_focus_manager_get_focused_element, noesis_focus_manager_get_is_focus_scope,
    noesis_focus_manager_set_focused_element, noesis_focus_manager_set_is_focus_scope,
    noesis_input_binding_create, noesis_key_binding_create, noesis_key_gesture_create,
    noesis_keyboard_navigation_get_accepts_return,
    noesis_keyboard_navigation_get_control_tab_navigation,
    noesis_keyboard_navigation_get_directional_navigation,
    noesis_keyboard_navigation_get_is_tab_stop, noesis_keyboard_navigation_get_tab_index,
    noesis_keyboard_navigation_get_tab_navigation,
    noesis_keyboard_navigation_set_accepts_return,
    noesis_keyboard_navigation_set_control_tab_navigation,
    noesis_keyboard_navigation_set_directional_navigation,
    noesis_keyboard_navigation_set_is_tab_stop, noesis_keyboard_navigation_set_tab_index,
    noesis_keyboard_navigation_set_tab_navigation, noesis_mouse_binding_create,
    noesis_mouse_gesture_create, noesis_ui_element_add_input_binding,
};
use crate::view::{FrameworkElement, Key};

// ── Enums ────────────────────────────────────────────────────────────────────

/// A typed bitset of `Noesis::ModifierKeys` (`NsGui/InputEnums.h`) — the
/// chord modifiers held down (`Alt` / `Control` / `Shift` / `Windows`). Compose
/// with [`Self::with`] / [`FromIterator`] and test with [`Self::contains`].
/// Bit values are validated against the SDK by `static_assert` in
/// `noesis_input.cpp`.
///
/// ```
/// use noesis_runtime::input::ModifierKeys;
/// let m = ModifierKeys::CONTROL.with(ModifierKeys::SHIFT);
/// assert!(m.contains(ModifierKeys::CONTROL));
/// assert!(!m.contains(ModifierKeys::ALT));
/// ```
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct ModifierKeys(pub i32);

impl ModifierKeys {
    /// No modifiers.
    pub const NONE: Self = Self(0);
    /// The Alt key.
    pub const ALT: Self = Self(1);
    /// The Control key.
    pub const CONTROL: Self = Self(2);
    /// The Shift key.
    pub const SHIFT: Self = Self(4);
    /// The Windows ("logo") key.
    pub const WINDOWS: Self = Self(8);

    /// Wrap a raw `Noesis::ModifierKeys` bitmask.
    #[must_use]
    pub const fn from_bits(bits: i32) -> Self {
        Self(bits)
    }

    /// The raw bitmask Noesis uses.
    #[must_use]
    pub const fn bits(self) -> i32 {
        self.0
    }

    /// A copy of this set with `other`'s bits added.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every bit of `other` is present (with [`Self::NONE`], always
    /// `true`).
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no modifiers are held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitOr for ModifierKeys {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl FromIterator<ModifierKeys> for ModifierKeys {
    fn from_iter<I: IntoIterator<Item = ModifierKeys>>(iter: I) -> Self {
        let mut acc = 0;
        for m in iter {
            acc |= m.0;
        }
        Self(acc)
    }
}

/// A typed bitset of `Noesis::KeyStates` (`NsGui/InputEnums.h`) — the state of
/// a key as reported by the keyboard: `Down` (currently pressed) and/or
/// `Toggled` (the toggle is on, e.g. `CapsLock`). Bit values validated by
/// `static_assert` in `noesis_input.cpp`.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub struct KeyStates(pub i32);

impl KeyStates {
    /// The key is up and untoggled.
    pub const NONE: Self = Self(0);
    /// The key is currently pressed.
    pub const DOWN: Self = Self(1);
    /// The key's toggle is on (`CapsLock` / `NumLock` style).
    pub const TOGGLED: Self = Self(2);

    /// Wrap a raw `Noesis::KeyStates` bitmask.
    #[must_use]
    pub const fn from_bits(bits: i32) -> Self {
        Self(bits)
    }

    /// The raw bitmask Noesis uses.
    #[must_use]
    pub const fn bits(self) -> i32 {
        self.0
    }

    /// Whether every bit of `other` is present.
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
}

/// Mirror of `Noesis::MouseAction` (`NsGui/InputEnums.h`) — the pointer gesture
/// a [`MouseGesture`] / [`MouseBinding`] matches. Ordinals validated by
/// `static_assert` in `noesis_input.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum MouseAction {
    None = 0,
    LeftClick = 1,
    RightClick = 2,
    MiddleClick = 3,
    WheelClick = 4,
    LeftDoubleClick = 5,
    RightDoubleClick = 6,
    MiddleDoubleClick = 7,
}

/// Mirror of `Noesis::CaptureMode` (`NsGui/Mouse.h`) — how an element captures
/// the mouse via [`FrameworkElement::capture_mouse_mode`].
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum CaptureMode {
    /// Release any capture (no element captures the mouse).
    None = 0,
    /// Only the element itself receives mouse events.
    Element = 1,
    /// The element and its visual subtree receive mouse events.
    SubTree = 2,
}

/// Mirror of `Noesis::FocusNavigationDirection` (top of `NsGui/UIElement.h`) —
/// the direction for [`FrameworkElement::move_focus`] /
/// [`FrameworkElement::predict_focus`]. Note `Next` / `Previous` / `First` /
/// `Last` are tab-order traversal (not supported by `PredictFocus`), while
/// `Left` / `Right` / `Up` / `Down` are directional.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum FocusNavigationDirection {
    Next = 0,
    Previous = 1,
    First = 2,
    Last = 3,
    Left = 4,
    Right = 5,
    Up = 6,
    Down = 7,
}

/// Mirror of `Noesis::KeyboardNavigationMode` (`NsGui/Enums.h`) — how Tab /
/// directional traversal behaves inside a container, for the
/// [`KeyboardNavigation`] attached properties. Ordinals validated by
/// `static_assert` in `noesis_input.cpp`.
#[repr(i32)]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum KeyboardNavigationMode {
    /// Tab moves out of the container into the rest of the tab order.
    Continue = 0,
    /// Focus returns to the first/active element on re-entry, then leaves.
    Once = 1,
    /// Tab cycles within the container, wrapping at the ends.
    Cycle = 2,
    /// Tab does not navigate within the container.
    None = 3,
    /// Tab stays contained — it never leaves the container.
    Contained = 4,
    /// Tab cycles locally but, unlike `Cycle`, restarts at the container start.
    Local = 5,
}

// ── Input gestures ───────────────────────────────────────────────────────────

/// A `Noesis::KeyGesture` — a [`Key`] plus chord [`ModifierKeys`] that, when
/// matched, fires a bound command. Owns a `+1` reference released on drop;
/// hand it to [`InputBinding::with_gesture`] (or use the all-in-one
/// [`KeyBinding`]).
pub struct KeyGesture {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for KeyGesture {}

impl KeyGesture {
    /// Build a key gesture for `key` + `modifiers`.
    #[must_use]
    pub fn new(key: Key, modifiers: ModifierKeys) -> Self {
        // SAFETY: the C side constructs a KeyGesture at +1; never null.
        let ptr = unsafe { noesis_key_gesture_create(key as i32, modifiers.bits()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_key_gesture_create returned null"),
        }
    }

    /// Raw `Noesis::InputGesture*`, borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for KeyGesture {
    fn drop(&mut self) {
        // SAFETY: +1 from create, released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A `Noesis::MouseGesture` — a [`MouseAction`] plus chord [`ModifierKeys`].
/// Owns a `+1` reference released on drop. See [`KeyGesture`].
pub struct MouseGesture {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for MouseGesture {}

impl MouseGesture {
    /// Build a mouse gesture for `action` + `modifiers`.
    #[must_use]
    pub fn new(action: MouseAction, modifiers: ModifierKeys) -> Self {
        // SAFETY: the C side constructs a MouseGesture at +1; never null.
        let ptr = unsafe { noesis_mouse_gesture_create(action as i32, modifiers.bits()) };
        Self {
            ptr: NonNull::new(ptr).expect("noesis_mouse_gesture_create returned null"),
        }
    }

    /// Raw `Noesis::InputGesture*`, borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for MouseGesture {
    fn drop(&mut self) {
        // SAFETY: +1 from create, released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

// ── Input bindings ───────────────────────────────────────────────────────────

/// A `Noesis::KeyBinding` — a [`Key`] + [`ModifierKeys`] chord bound to a
/// command. Add it to an element with [`Self::add_to`]; when the focused element
/// (or one routing through it) sees that key chord, the bound command's
/// `Execute` runs. Owns a `+1` reference released on drop (the element's
/// `InputBindings` collection holds its own reference once added).
pub struct KeyBinding {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for KeyBinding {}

impl KeyBinding {
    /// Bind `command` (any [`AsCommand`]) to the `key` + `modifiers` chord.
    /// Returns `None` if `command`'s pointer is not an `ICommand`.
    #[must_use]
    pub fn new<C: AsCommand>(command: &C, key: Key, modifiers: ModifierKeys) -> Option<Self> {
        // SAFETY: command_ptr() is a borrowed live ICommand* for the call; the C
        // side builds a KeyBinding at +1 (or null on a non-command pointer).
        let ptr = unsafe {
            noesis_key_binding_create(command.command_ptr(), key as i32, modifiers.bits())
        };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Add this binding to `element`'s `InputBindings`. Returns `false` if
    /// `element` is not a `UIElement`. After this, the chord drives the command
    /// while `element` (or its focus subtree) has focus.
    pub fn add_to(&self, element: &FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live InputBinding*; element.raw() a live element.
        unsafe { noesis_ui_element_add_input_binding(element.raw(), self.ptr.as_ptr()) }
    }

    /// Raw `Noesis::InputBinding*`, borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for KeyBinding {
    fn drop(&mut self) {
        // SAFETY: +1 from create, released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A `Noesis::MouseBinding` — a [`MouseAction`] + [`ModifierKeys`] chord bound
/// to a command. See [`KeyBinding`].
pub struct MouseBinding {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for MouseBinding {}

impl MouseBinding {
    /// Bind `command` (any [`AsCommand`]) to the `action` + `modifiers` chord.
    /// Returns `None` if `command`'s pointer is not an `ICommand`.
    #[must_use]
    pub fn new<C: AsCommand>(
        command: &C,
        action: MouseAction,
        modifiers: ModifierKeys,
    ) -> Option<Self> {
        // SAFETY: command_ptr() is a borrowed live ICommand* for the call.
        let ptr = unsafe {
            noesis_mouse_binding_create(command.command_ptr(), action as i32, modifiers.bits())
        };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Add this binding to `element`'s `InputBindings`. See
    /// [`KeyBinding::add_to`].
    pub fn add_to(&self, element: &FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live InputBinding*; element.raw() a live element.
        unsafe { noesis_ui_element_add_input_binding(element.raw(), self.ptr.as_ptr()) }
    }

    /// Raw `Noesis::InputBinding*`, borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for MouseBinding {
    fn drop(&mut self) {
        // SAFETY: +1 from create, released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

/// A `Noesis::InputBinding` — a command bound to an arbitrary
/// [`InputGesture`](KeyGesture) built separately. The general form of
/// [`KeyBinding`] / [`MouseBinding`]; use it to reuse a single gesture across
/// bindings. Owns a `+1` reference released on drop.
pub struct InputBinding {
    ptr: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for InputBinding {}

impl InputBinding {
    /// Bind `command` (any [`AsCommand`]) to `gesture` (a [`KeyGesture`] —
    /// generalize to other gestures via the raw pointer overloads if needed).
    /// Returns `None` if either pointer fails its `ICommand` / `InputGesture`
    /// cast. The binding adds its own reference to the gesture, so `gesture`
    /// may be dropped afterwards.
    #[must_use]
    pub fn with_gesture<C: AsCommand>(command: &C, gesture: &KeyGesture) -> Option<Self> {
        // SAFETY: command/gesture pointers are borrowed live for the call; the C
        // side builds an InputBinding at +1 (adding its own ref to the gesture).
        let ptr = unsafe { noesis_input_binding_create(command.command_ptr(), gesture.raw()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Like [`Self::with_gesture`] but for a [`MouseGesture`].
    #[must_use]
    pub fn with_mouse_gesture<C: AsCommand>(command: &C, gesture: &MouseGesture) -> Option<Self> {
        // SAFETY: as above.
        let ptr = unsafe { noesis_input_binding_create(command.command_ptr(), gesture.raw()) };
        NonNull::new(ptr).map(|ptr| Self { ptr })
    }

    /// Add this binding to `element`'s `InputBindings`. See
    /// [`KeyBinding::add_to`].
    pub fn add_to(&self, element: &FrameworkElement) -> bool {
        // SAFETY: self.ptr is a live InputBinding*; element.raw() a live element.
        unsafe { noesis_ui_element_add_input_binding(element.raw(), self.ptr.as_ptr()) }
    }

    /// Raw `Noesis::InputBinding*`, borrowed for the lifetime of `self`.
    #[must_use]
    pub fn raw(&self) -> *mut c_void {
        self.ptr.as_ptr()
    }
}

impl Drop for InputBinding {
    fn drop(&mut self) {
        // SAFETY: +1 from create, released exactly once here.
        unsafe { noesis_base_component_release(self.ptr.as_ptr()) }
    }
}

// ── FocusManager statics ─────────────────────────────────────────────────────

/// The `Noesis::FocusManager` static surface — attached-property helpers for
/// querying and steering logical focus within a *focus scope*. A focus scope
/// (e.g. a `Window`, `Menu`, or `ToolBar`) remembers its own focused element
/// independently of keyboard focus, so menus can restore selection on re-entry.
pub struct FocusManager;

impl FocusManager {
    /// The element with logical focus inside the scope `scope` (its
    /// `FocusManager.FocusedElement`), as a **borrowed** pointer (no `+1`).
    /// `None` if nothing is focused or `scope` is not a `DependencyObject`.
    /// Compare against [`FrameworkElement::raw`] to identify it.
    #[must_use]
    pub fn focused_element(scope: &FrameworkElement) -> Option<NonNull<c_void>> {
        // SAFETY: scope.raw() is a live DependencyObject*; the C side returns a
        // borrowed UIElement* or null.
        let p = unsafe { noesis_focus_manager_get_focused_element(scope.raw()) };
        NonNull::new(p)
    }

    /// Set the logically-focused element within `scope`. Pass `Some(element)`
    /// (a `UIElement`) or `None` to clear. Returns `false` if `scope` is not a
    /// `DependencyObject` or `element` is given but is not a `UIElement`.
    pub fn set_focused_element(
        scope: &FrameworkElement,
        element: Option<&FrameworkElement>,
    ) -> bool {
        let e = element.map_or(core::ptr::null_mut(), FrameworkElement::raw);
        // SAFETY: scope.raw() is a live DependencyObject*; `e` is a live
        // UIElement* or null per the contract.
        unsafe { noesis_focus_manager_set_focused_element(scope.raw(), e) }
    }

    /// Whether `element` is itself a focus scope (`FocusManager.IsFocusScope`).
    #[must_use]
    pub fn is_focus_scope(element: &FrameworkElement) -> bool {
        // SAFETY: element.raw() is a live DependencyObject*.
        unsafe { noesis_focus_manager_get_is_focus_scope(element.raw()) }
    }

    /// Mark `element` as a focus scope (or not). Returns `false` if `element`
    /// is not a `DependencyObject`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_is_focus_scope(element: &FrameworkElement, value: bool) -> bool {
        // SAFETY: element.raw() is a live DependencyObject*.
        unsafe { noesis_focus_manager_set_is_focus_scope(element.raw(), value) }
    }

    /// The nearest ancestor of `element` (inclusive) that is a focus scope,
    /// as a **borrowed** `DependencyObject*` (no `+1`). `None` if there is no
    /// enclosing scope.
    #[must_use]
    pub fn focus_scope(element: &FrameworkElement) -> Option<NonNull<c_void>> {
        // SAFETY: element.raw() is a live DependencyObject*; borrowed or null.
        let p = unsafe { noesis_focus_manager_get_focus_scope(element.raw()) };
        NonNull::new(p)
    }
}

// ── KeyboardNavigation attached properties ───────────────────────────────────

/// The `Noesis::KeyboardNavigation` static surface — attached properties that
/// shape Tab / directional focus traversal (`TabIndex`, `IsTabStop`,
/// `TabNavigation`, `ControlTabNavigation`, `DirectionalNavigation`,
/// `AcceptsReturn`). Each is a round-trippable attached property: set it on any
/// element, read it back. Getters return `None` only if `element` is not a
/// `DependencyObject`.
pub struct KeyboardNavigation;

impl KeyboardNavigation {
    /// `KeyboardNavigation.TabIndex` — the element's position in tab order
    /// (lower goes first).
    #[must_use]
    pub fn tab_index(element: &FrameworkElement) -> Option<i32> {
        let mut out = 0;
        // SAFETY: element.raw() is a live DependencyObject*; out is a valid i32.
        unsafe { noesis_keyboard_navigation_get_tab_index(element.raw(), &mut out) }
            .then_some(out)
    }

    /// Set `KeyboardNavigation.TabIndex`. `false` if `element` is not a
    /// `DependencyObject`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_tab_index(element: &FrameworkElement, value: i32) -> bool {
        // SAFETY: element.raw() is a live DependencyObject*.
        unsafe { noesis_keyboard_navigation_set_tab_index(element.raw(), value) }
    }

    /// `KeyboardNavigation.IsTabStop` — whether Tab can land on the element.
    #[must_use]
    pub fn is_tab_stop(element: &FrameworkElement) -> Option<bool> {
        let mut out = false;
        // SAFETY: element.raw() is a live DependencyObject*; out is a valid bool.
        unsafe { noesis_keyboard_navigation_get_is_tab_stop(element.raw(), &mut out) }
            .then_some(out)
    }

    /// Set `KeyboardNavigation.IsTabStop`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_is_tab_stop(element: &FrameworkElement, value: bool) -> bool {
        // SAFETY: element.raw() is a live DependencyObject*.
        unsafe { noesis_keyboard_navigation_set_is_tab_stop(element.raw(), value) }
    }

    /// `KeyboardNavigation.TabNavigation` — how Tab traverses inside this
    /// container.
    #[must_use]
    pub fn tab_navigation(element: &FrameworkElement) -> Option<KeyboardNavigationMode> {
        Self::get_mode(element, noesis_keyboard_navigation_get_tab_navigation)
    }

    /// Set `KeyboardNavigation.TabNavigation`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_tab_navigation(element: &FrameworkElement, mode: KeyboardNavigationMode) -> bool {
        // SAFETY: element.raw() is a live DependencyObject*.
        unsafe { noesis_keyboard_navigation_set_tab_navigation(element.raw(), mode as i32) }
    }

    /// `KeyboardNavigation.ControlTabNavigation` — how Ctrl+Tab traverses.
    #[must_use]
    pub fn control_tab_navigation(element: &FrameworkElement) -> Option<KeyboardNavigationMode> {
        Self::get_mode(
            element,
            noesis_keyboard_navigation_get_control_tab_navigation,
        )
    }

    /// Set `KeyboardNavigation.ControlTabNavigation`.
    pub fn set_control_tab_navigation(
        element: &FrameworkElement,
        mode: KeyboardNavigationMode,
    ) -> bool {
        // SAFETY: element.raw() is a live DependencyObject*.
        unsafe {
            noesis_keyboard_navigation_set_control_tab_navigation(element.raw(), mode as i32)
        }
    }

    /// `KeyboardNavigation.DirectionalNavigation` — how arrow keys traverse.
    #[must_use]
    pub fn directional_navigation(element: &FrameworkElement) -> Option<KeyboardNavigationMode> {
        Self::get_mode(
            element,
            noesis_keyboard_navigation_get_directional_navigation,
        )
    }

    /// Set `KeyboardNavigation.DirectionalNavigation`.
    pub fn set_directional_navigation(
        element: &FrameworkElement,
        mode: KeyboardNavigationMode,
    ) -> bool {
        // SAFETY: element.raw() is a live DependencyObject*.
        unsafe {
            noesis_keyboard_navigation_set_directional_navigation(element.raw(), mode as i32)
        }
    }

    /// `KeyboardNavigation.AcceptsReturn` — whether the element consumes the
    /// Return key instead of letting it activate a default button.
    #[must_use]
    pub fn accepts_return(element: &FrameworkElement) -> Option<bool> {
        let mut out = false;
        // SAFETY: element.raw() is a live DependencyObject*; out is a valid bool.
        unsafe { noesis_keyboard_navigation_get_accepts_return(element.raw(), &mut out) }
            .then_some(out)
    }

    /// Set `KeyboardNavigation.AcceptsReturn`.
    #[must_use = "a false return means the property was not set (unknown name / type mismatch / read-only)"]
    pub fn set_accepts_return(element: &FrameworkElement, value: bool) -> bool {
        // SAFETY: element.raw() is a live DependencyObject*.
        unsafe { noesis_keyboard_navigation_set_accepts_return(element.raw(), value) }
    }

    /// Shared getter for the three `KeyboardNavigationMode`-typed properties:
    /// read the raw ordinal and map it back to the enum.
    fn get_mode(
        element: &FrameworkElement,
        getter: unsafe extern "C" fn(*mut c_void, *mut i32) -> bool,
    ) -> Option<KeyboardNavigationMode> {
        let mut out = 0;
        // SAFETY: element.raw() is a live DependencyObject*; out is a valid i32;
        // `getter` is one of the KeyboardNavigation mode getters.
        if !unsafe { getter(element.raw(), &mut out) } {
            return None;
        }
        Some(match out {
            0 => KeyboardNavigationMode::Continue,
            1 => KeyboardNavigationMode::Once,
            2 => KeyboardNavigationMode::Cycle,
            3 => KeyboardNavigationMode::None,
            4 => KeyboardNavigationMode::Contained,
            5 => KeyboardNavigationMode::Local,
            // Noesis only ever yields 0..=5 for these DPs.
            _ => return None,
        })
    }
}
