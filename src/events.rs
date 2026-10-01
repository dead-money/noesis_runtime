//! Subscribe Rust callbacks to Noesis events.
//!
//! Pick the entry point by event:
//!
//! - [`subscribe_click`] for `BaseButton::Click`.
//! - [`subscribe_keydown`] for `UIElement::KeyDown`, with the option to mark it handled.
//! - [`subscribe_selection_changed`] for `Selector::SelectionChanged`.
//! - [`subscribe_event`] for any routed event listed in [`RoutedEvent`]. The
//!   handler gets an [`EventArgs`] with typed accessors for mouse, key, text,
//!   focus, drag and manipulation data. [`subscribe_event_by_name`] takes the
//!   event name as a string for events the enum doesn't list.
//! - [`subscribe_lifecycle`] for non-routed notifications ([`LifecycleEvent`]):
//!   `Initialized`, `LayoutUpdated`, `DataContextChanged` and the `Is*Changed`
//!   family. `Loaded`, `Unloaded` and `SizeChanged` are routed events and go
//!   through [`subscribe_event`].
//! - [`subscribe_data_object`] for `DataObject.Copying` / `DataObject.Pasting`.
//! - [`do_drag_drop`] starts a drag from an element.
//!
//! Every handler can be a closure. Each `subscribe_*` function returns `None`
//! when the element is the wrong type for the event or the event name is
//! unknown.
//!
//! ```no_run
//! use noesis_runtime::events::{subscribe_click, subscribe_event, EventArgs, RoutedEvent};
//! use noesis_runtime::view::FrameworkElement;
//!
//! let root = FrameworkElement::parse(
//!     r#"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation">
//!          <Button x:Name="Ok" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"/>
//!        </Grid>"#,
//! )
//! .unwrap();
//! let ok = root.find_name("Ok").unwrap();
//!
//! let _click = subscribe_click(&ok, || println!("clicked")).unwrap();
//! let _wheel = subscribe_event(&root, RoutedEvent::MouseWheel, false, |args: &EventArgs| {
//!     if let Some(delta) = args.wheel_delta() {
//!         println!("wheel {delta}");
//!     }
//!     false
//! })
//! .unwrap();
//! // Keep the subscriptions alive for as long as you want the callbacks.
//! ```
//!
//! # Threading
//!
//! Callbacks run synchronously inside Noesis, on the thread that drives the
//! view, from whichever `View` call raised the event (input injection or
//! `update`). Keep callbacks short: set a flag or push to a queue and do the
//! real work from your own update loop. Handlers must be `Send + 'static` so
//! the subscription can move to the view's thread.
//!
//! Handlers take `&self` and may be re-entered: a handler that raises its own
//! event again (for example through [`crate::reflection::raise_event`]) runs a
//! second time before the first call returns. Keep mutable handler state in a
//! `Cell` or an atomic.
//!
//! # Lifetime
//!
//! Each subscription is an RAII token. While it lives, the handler stays
//! installed and the token holds a `+1` ref on the element, so the element
//! survives even if you drop every [`FrameworkElement`] handle to it. Drop the
//! token to unsubscribe; drop it before [`crate::shutdown`].
//!
//! A token may be dropped from inside its own callback. Destruction of the
//! handler and its closure is deferred until the callback returns.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface; explicit blocks add noise

use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::ffi::{
    noesis_element_datacontext_get_u64, noesis_event_args_kind, noesis_key_args_key,
    noesis_mouse_args_position, noesis_mouse_button_args_button, noesis_mouse_wheel_args_delta,
    noesis_routed_args_source, noesis_routed_events_add_copying_handler,
    noesis_routed_events_add_pasting_handler, noesis_routed_events_do_drag_drop,
    noesis_routed_events_drag_data, noesis_routed_events_drag_effects,
    noesis_routed_events_drag_position, noesis_routed_events_drag_set_effects,
    noesis_routed_events_focus_new, noesis_routed_events_focus_old,
    noesis_routed_events_manip_cumulative, noesis_routed_events_manip_delta,
    noesis_routed_events_manip_is_inertial, noesis_routed_events_manip_origin,
    noesis_routed_events_manip_velocities, noesis_routed_events_remove_data_object_handler,
    noesis_size_changed_args_new_size, noesis_subscribe_click, noesis_subscribe_event,
    noesis_subscribe_keydown, noesis_subscribe_lifecycle, noesis_subscribe_selection_changed,
    noesis_text_args_ch, noesis_unsubscribe_click, noesis_unsubscribe_event,
    noesis_unsubscribe_keydown, noesis_unsubscribe_lifecycle, noesis_unsubscribe_selection_changed,
};
use crate::view::{FrameworkElement, Key, MouseButton};

/// Frees a handler box donated to a C++ subscription (`T` is the inner
/// `Box<dyn Handler>`). C++ calls it once, when the handler is destroyed, which
/// is deferred past any in-flight callback.
///
/// SAFETY: `userdata` is a `Box<T>` from `Box::into_raw` in the matching
/// subscribe, and C++ invokes this at most once.
unsafe extern "C" fn free_donated<T>(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        if userdata.is_null() {
            return;
        }
        // SAFETY: reclaim the exact box leaked in subscribe; runs once.
        drop(unsafe { Box::from_raw(userdata.cast::<T>()) });
    })
}

/// Values of the C++ `DmArgKind` enum in `noesis_events.cpp`; keep in sync.
/// Classify events by this discriminant, not by probing accessors: the
/// accessors share sentinels (a `MouseMove` and a zero-delta `MouseWheel` both
/// report a position and no button).
mod arg_kind {
    pub const MOUSE_WHEEL: i32 = 3;
}

/// Handler for [`subscribe_click`], called once per click with no arguments.
/// Any `Fn() + Send + 'static` closure implements it. For the event args, use
/// [`subscribe_event`] instead.
///
/// May be re-entered (see the module docs on threading).
pub trait ClickHandler: Send + 'static {
    fn on_click(&self);
}

impl<F: Fn() + Send + 'static> ClickHandler for F {
    fn on_click(&self) {
        self();
    }
}

/// SAFETY: `userdata` must be a pointer produced by [`subscribe_click`] and
/// still alive (the [`ClickSubscription`] hasn't been dropped).
unsafe extern "C" fn click_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        // Shared `&`: re-entrant handler box (see `ClickHandler`).
        let handler = &*userdata.cast::<Box<dyn ClickHandler>>();
        handler.on_click();
    })
}

/// Keeps a [`subscribe_click`] handler installed. Drop it to unsubscribe.
///
/// Holds a `+1` ref on the button, released on drop. Drop before
/// [`crate::shutdown`].
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct ClickSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ClickSubscription {}

impl Drop for ClickSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by subscribe_click. Unsubscribe frees the
        // donated handler box exactly once (deferred if we are dropping from
        // inside the callback). Safe to drop re-entrantly.
        unsafe { noesis_unsubscribe_click(self.token.as_ptr()) }
    }
}

/// Subscribes `handler` to `BaseButton::Click` on `element`.
///
/// Returns `None` if `element` is not a `BaseButton` (`Button`, `ToggleButton`,
/// `CheckBox`, ...). A `UserControl` wrapping a button is not one; look up the
/// inner button by name. The handler stays installed while the returned
/// [`ClickSubscription`] lives, and it may be dropped from inside the callback.
pub fn subscribe_click<H: ClickHandler>(
    element: &FrameworkElement,
    handler: H,
) -> Option<ClickSubscription> {
    // Double box: the C ABI userdata needs a thin pointer.
    let outer: Box<Box<dyn ClickHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked and donated
    // to the C++ handler (freed via the free trampoline when it is destroyed);
    // the element pointer is borrowed for the call duration only.
    let token = unsafe {
        noesis_subscribe_click(
            element.raw(),
            click_trampoline,
            userdata.cast(),
            free_donated::<Box<dyn ClickHandler>>,
        )
    };

    if let Some(token) = NonNull::new(token) {
        Some(ClickSubscription { token })
    } else {
        // C++ took no ownership on failure.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

/// Handler for [`subscribe_selection_changed`], called with no arguments each
/// time the selection changes. Read the new selection from the control, its
/// collection view, or the bound model. Any `Fn() + Send + 'static` closure
/// implements it.
///
/// May be re-entered: a handler that changes the selection runs again before
/// it returns.
pub trait SelectionChangedHandler: Send + 'static {
    fn on_selection_changed(&self);
}

impl<F: Fn() + Send + 'static> SelectionChangedHandler for F {
    fn on_selection_changed(&self) {
        self();
    }
}

/// SAFETY: `userdata` must be a pointer produced by
/// [`subscribe_selection_changed`] and still alive (the
/// [`SelectionChangedSubscription`] hasn't been dropped).
unsafe extern "C" fn selection_changed_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        // Shared `&`: re-entrant handler box (see `SelectionChangedHandler`).
        let handler = &*userdata.cast::<Box<dyn SelectionChangedHandler>>();
        handler.on_selection_changed();
    })
}

/// Keeps a [`subscribe_selection_changed`] handler installed. Drop it to
/// unsubscribe. Holds a `+1` ref on the element.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct SelectionChangedSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for SelectionChangedSubscription {}

impl Drop for SelectionChangedSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by subscribe_selection_changed; unsubscribe
        // frees the donated box exactly once (deferred if dropping from inside
        // the callback).
        unsafe { noesis_unsubscribe_selection_changed(self.token.as_ptr()) }
    }
}

/// Subscribes `handler` to `Selector::SelectionChanged` on `element`.
///
/// Returns `None` if `element` is not a `Selector` (`ListBox`, `ListView`,
/// `ComboBox`, `TabControl`, ...). The handler stays installed while the
/// returned [`SelectionChangedSubscription`] lives. Use it instead of polling
/// the selection every frame.
pub fn subscribe_selection_changed<H: SelectionChangedHandler>(
    element: &FrameworkElement,
    handler: H,
) -> Option<SelectionChangedSubscription> {
    let outer: Box<Box<dyn SelectionChangedHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked and donated
    // to the C++ handler; the element pointer is borrowed for the call only.
    let token = unsafe {
        noesis_subscribe_selection_changed(
            element.raw(),
            selection_changed_trampoline,
            userdata.cast(),
            free_donated::<Box<dyn SelectionChangedHandler>>,
        )
    };

    if let Some(token) = NonNull::new(token) {
        Some(SelectionChangedSubscription { token })
    } else {
        // C++ took no ownership on failure.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

/// Handler for [`subscribe_keydown`]. Any `Fn(Key) -> bool + Send + 'static`
/// closure implements it.
pub trait KeyDownHandler: Send + 'static {
    /// Called once per `KeyDown` on the element. Keys outside the [`Key`] enum
    /// arrive as [`Key::None`]. Return `true` to mark the event handled and
    /// stop it from routing further, for example to keep a console hotkey from
    /// also being typed into a focused `TextBox`.
    fn on_keydown(&self, key: Key) -> bool;
}

impl<F: Fn(Key) -> bool + Send + 'static> KeyDownHandler for F {
    fn on_keydown(&self, key: Key) -> bool {
        self(key)
    }
}

/// SAFETY: `userdata` must be a pointer produced by [`subscribe_keydown`]
/// and still alive (the [`KeyDownSubscription`] hasn't been dropped).
/// `out_handled` must be a non-null pointer to a writable bool (the C++
/// shim guarantees this).
unsafe extern "C" fn keydown_trampoline(userdata: *mut c_void, key: i32, out_handled: *mut bool) {
    crate::panic_guard::guard(|| {
        // Shared `&`: re-entrant handler box (see `KeyDownHandler`).
        let handler = &*userdata.cast::<Box<dyn KeyDownHandler>>();
        let mapped = key_from_raw(key);
        let handled = handler.on_keydown(mapped);
        if !out_handled.is_null() {
            *out_handled = handled;
        }
    })
}

/// Maps a raw `Noesis::Key` ordinal to [`Key`]; unmapped ordinals become
/// [`Key::None`]. New [`Key`] variants need a matching `static_assert` in
/// `noesis_view.cpp`.
fn key_from_raw(raw: i32) -> Key {
    // Not a transmute: an ordinal outside the declared variants would be UB.
    match raw {
        0 => Key::None,
        2 => Key::Back,
        3 => Key::Tab,
        6 => Key::Return,
        7 => Key::Pause,
        8 => Key::CapsLock,
        13 => Key::Escape,
        18 => Key::Space,
        19 => Key::PageUp,
        20 => Key::PageDown,
        21 => Key::End,
        22 => Key::Home,
        23 => Key::Left,
        24 => Key::Up,
        25 => Key::Right,
        26 => Key::Down,
        30 => Key::PrintScreen,
        31 => Key::Insert,
        32 => Key::Delete,
        33 => Key::Help,
        34..=43 => match raw {
            34 => Key::D0,
            35 => Key::D1,
            36 => Key::D2,
            37 => Key::D3,
            38 => Key::D4,
            39 => Key::D5,
            40 => Key::D6,
            41 => Key::D7,
            42 => Key::D8,
            43 => Key::D9,
            _ => Key::None,
        },
        44..=69 => match raw {
            44 => Key::A,
            45 => Key::B,
            46 => Key::C,
            47 => Key::D,
            48 => Key::E,
            49 => Key::F,
            50 => Key::G,
            51 => Key::H,
            52 => Key::I,
            53 => Key::J,
            54 => Key::K,
            55 => Key::L,
            56 => Key::M,
            57 => Key::N,
            58 => Key::O,
            59 => Key::P,
            60 => Key::Q,
            61 => Key::R,
            62 => Key::S,
            63 => Key::T,
            64 => Key::U,
            65 => Key::V,
            66 => Key::W,
            67 => Key::X,
            68 => Key::Y,
            69 => Key::Z,
            _ => Key::None,
        },
        70 => Key::LWin,
        71 => Key::RWin,
        72 => Key::Apps,
        74..=83 => match raw {
            74 => Key::NumPad0,
            75 => Key::NumPad1,
            76 => Key::NumPad2,
            77 => Key::NumPad3,
            78 => Key::NumPad4,
            79 => Key::NumPad5,
            80 => Key::NumPad6,
            81 => Key::NumPad7,
            82 => Key::NumPad8,
            83 => Key::NumPad9,
            _ => Key::None,
        },
        84 => Key::Multiply,
        85 => Key::Add,
        87 => Key::Subtract,
        88 => Key::Decimal,
        89 => Key::Divide,
        90..=113 => match raw {
            90 => Key::F1,
            91 => Key::F2,
            92 => Key::F3,
            93 => Key::F4,
            94 => Key::F5,
            95 => Key::F6,
            96 => Key::F7,
            97 => Key::F8,
            98 => Key::F9,
            99 => Key::F10,
            100 => Key::F11,
            101 => Key::F12,
            102 => Key::F13,
            103 => Key::F14,
            104 => Key::F15,
            105 => Key::F16,
            106 => Key::F17,
            107 => Key::F18,
            108 => Key::F19,
            109 => Key::F20,
            110 => Key::F21,
            111 => Key::F22,
            112 => Key::F23,
            113 => Key::F24,
            _ => Key::None,
        },
        114 => Key::NumLock,
        115 => Key::ScrollLock,
        116 => Key::LeftShift,
        117 => Key::RightShift,
        118 => Key::LeftCtrl,
        119 => Key::RightCtrl,
        120 => Key::LeftAlt,
        121 => Key::RightAlt,
        140 => Key::OemSemicolon,
        141 => Key::OemPlus,
        142 => Key::OemComma,
        143 => Key::OemMinus,
        144 => Key::OemPeriod,
        145 => Key::OemSlash,
        146 => Key::OemTilde,
        149 => Key::OemOpenBrackets,
        150 => Key::OemPipe,
        151 => Key::OemCloseBrackets,
        152 => Key::OemQuotes,
        175 => Key::GamepadLeft,
        176 => Key::GamepadUp,
        177 => Key::GamepadRight,
        178 => Key::GamepadDown,
        179 => Key::GamepadAccept,
        180 => Key::GamepadCancel,
        181 => Key::GamepadMenu,
        182 => Key::GamepadView,
        183 => Key::GamepadPageUp,
        184 => Key::GamepadPageDown,
        185 => Key::GamepadPageLeft,
        186 => Key::GamepadPageRight,
        187 => Key::GamepadContext1,
        188 => Key::GamepadContext2,
        189 => Key::GamepadContext3,
        190 => Key::GamepadContext4,
        _ => Key::None,
    }
}

/// Keeps a [`subscribe_keydown`] handler installed. Drop it to unsubscribe.
/// Holds a `+1` ref on the element.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct KeyDownSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for KeyDownSubscription {}

impl Drop for KeyDownSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by subscribe_keydown; unsubscribe frees the
        // donated box exactly once (deferred if dropping from inside the
        // callback).
        unsafe { noesis_unsubscribe_keydown(self.token.as_ptr()) }
    }
}

/// Subscribes `handler` to `UIElement::KeyDown` on `element`.
///
/// The handler returns `true` to mark the key handled (see
/// [`KeyDownHandler::on_keydown`]). It stays installed while the returned
/// [`KeyDownSubscription`] lives. Returns `None` if `element` is not a
/// `UIElement`. For `KeyUp` or the `Preview*` key events, use
/// [`subscribe_event`].
pub fn subscribe_keydown<H: KeyDownHandler>(
    element: &FrameworkElement,
    handler: H,
) -> Option<KeyDownSubscription> {
    let outer: Box<Box<dyn KeyDownHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked and donated
    // to the C++ handler; the element pointer is borrowed for the call only.
    let token = unsafe {
        noesis_subscribe_keydown(
            element.raw(),
            keydown_trampoline,
            userdata.cast(),
            free_donated::<Box<dyn KeyDownHandler>>,
        )
    };

    if let Some(token) = NonNull::new(token) {
        Some(KeyDownSubscription { token })
    } else {
        // C++ took no ownership on failure.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing
        // else ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

/// The arguments of a routed event, lent to a [`RoutedEventHandler`] for one
/// callback.
///
/// Each accessor returns `None` (or `false`) unless the event carries that kind
/// of data, so one handler can probe several. Which accessors apply depends on
/// the [`RoutedEvent`]: mouse events have [`position`](Self::position), key
/// events [`key`](Self::key), drag events [`drag`](Self::drag), and so on.
/// Events subscribed by a name outside [`RoutedEvent`] only support
/// [`source_ptr`](Self::source_ptr) and
/// [`source_data_context_u64`](Self::source_data_context_u64).
///
/// The underlying C++ args live on Noesis's stack and are valid only during
/// the callback. Don't keep raw pointers read from them past the call.
pub struct EventArgs {
    raw: *const c_void,
    _not_send: PhantomData<*const c_void>,
}

impl EventArgs {
    /// Wraps the opaque args handle the C++ shim passes to an event callback,
    /// for test harnesses and custom C trampolines.
    ///
    /// # Safety
    ///
    /// `raw` must be an args handle from the C++ shim, and the returned value
    /// must not outlive the callback that received it.
    #[doc(hidden)]
    pub unsafe fn from_raw(raw: *const c_void) -> Self {
        EventArgs {
            raw,
            _not_send: PhantomData,
        }
    }

    /// Pointer position in view (screen) coordinates, not relative to the
    /// element, for mouse, mouse-button and mouse-wheel events. `None` for
    /// other events.
    pub fn position(&self) -> Option<(f32, f32)> {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: `raw` is the opaque handle the trampoline received; the
        // accessor validates the arg kind and writes only on a match.
        let ok = unsafe { noesis_mouse_args_position(self.raw, &mut x, &mut y) };
        ok.then_some((x, y))
    }

    /// The button that changed, for a mouse-button event. `None` otherwise.
    pub fn mouse_button(&self) -> Option<MouseButton> {
        // SAFETY: opaque handle; accessor returns -1 unless it's a button event.
        let raw = unsafe { noesis_mouse_button_args_button(self.raw) };
        match raw {
            0 => Some(MouseButton::Left),
            1 => Some(MouseButton::Right),
            2 => Some(MouseButton::Middle),
            3 => Some(MouseButton::XButton1),
            4 => Some(MouseButton::XButton2),
            _ => None,
        }
    }

    /// Wheel rotation for a mouse-wheel event, signed, typically 120 per
    /// notch. A wheel event with no rotation returns `Some(0)`; any other
    /// event returns `None`.
    pub fn wheel_delta(&self) -> Option<i32> {
        if !self.is_wheel() {
            return None;
        }
        // SAFETY: opaque handle; accessor returns 0 unless it's a wheel event.
        Some(unsafe { noesis_mouse_wheel_args_delta(self.raw) })
    }

    /// The [`arg_kind`] discriminant, or `-1` for a null handle.
    fn kind(&self) -> i32 {
        // SAFETY: opaque handle; the accessor reads the carried discriminant.
        unsafe { noesis_event_args_kind(self.raw) }
    }

    fn is_wheel(&self) -> bool {
        self.kind() == arg_kind::MOUSE_WHEEL
    }

    /// The key for a key event. `None` for other events. Keys outside the
    /// [`Key`] enum return `Some(Key::None)`.
    pub fn key(&self) -> Option<Key> {
        // SAFETY: opaque handle; accessor returns -1 unless it's a key event.
        let raw = unsafe { noesis_key_args_key(self.raw) };
        (raw >= 0).then(|| key_from_raw(raw))
    }

    /// The typed character for a `TextInput` event. `None` for other events or
    /// an invalid code point.
    pub fn text_char(&self) -> Option<char> {
        // SAFETY: opaque handle; accessor returns -1 unless it's text input.
        let raw = unsafe { noesis_text_args_ch(self.raw) };
        if raw < 0 {
            return None;
        }
        char::from_u32(raw as u32)
    }

    /// The new `(width, height)` for a `SizeChanged` event, in DIPs. `None`
    /// otherwise.
    pub fn new_size(&self) -> Option<(f32, f32)> {
        let mut w = 0.0f32;
        let mut h = 0.0f32;
        // SAFETY: opaque handle; accessor validates the kind and writes on match.
        let ok = unsafe { noesis_size_changed_args_new_size(self.raw, &mut w, &mut h) };
        ok.then_some((w, h))
    }

    /// Raw pointer to the element that raised the event
    /// (`RoutedEventArgs::source`). `None` if there is no source.
    ///
    /// The pointer is borrowed, not ref-counted, and valid only during the
    /// callback. Don't wrap it in a [`FrameworkElement`]; that would
    /// over-release it.
    pub fn source_ptr(&self) -> Option<*mut c_void> {
        // SAFETY: opaque handle; returns a borrowed pointer or null.
        let p = unsafe { noesis_routed_args_source(self.raw) };
        (!p.is_null()).then_some(p)
    }

    /// Reads the `u64` property `prop_name` from the `DataContext` (inherited
    /// if not set locally) of the element that raised the event.
    ///
    /// Use it to identify the row in a templated list: subscribe once on the
    /// `ItemsControl`, and read an id stored on each row's view model from the
    /// clicked element, with no per-row subscription.
    ///
    /// Returns `None` if the event has no source, the source is not a
    /// `FrameworkElement`, it has no `DataContext`, or the context has no
    /// `u64` property of that name. See
    /// [`FrameworkElement::data_context_u64`](crate::view::FrameworkElement::data_context_u64)
    /// for the field-resolution rules.
    ///
    /// # Panics
    ///
    /// Panics if `prop_name` contains an interior NUL byte.
    #[must_use]
    pub fn source_data_context_u64(&self, prop_name: &str) -> Option<u64> {
        let source = self.source_ptr()?;
        let c = CString::new(prop_name).expect("property name contained interior NUL");
        let mut out: u64 = 0;
        // SAFETY: `source` is the borrowed live event source for the callback
        // duration; the C side borrows its DataContext and writes `out` only on a
        // hit. We never retain `source` past this call.
        let ok = unsafe { noesis_element_datacontext_get_u64(source, c.as_ptr(), &mut out) };
        ok.then_some(out)
    }

    /// Raw pointer to the element that had focus before
    /// (`KeyboardFocusChangedEventArgs::oldFocus`), for `GotKeyboardFocus`,
    /// `LostKeyboardFocus` and their `Preview*` variants. `None` for other
    /// events or when nothing had focus.
    ///
    /// Borrowed; same contract as [`source_ptr`](Self::source_ptr).
    pub fn focus_old_ptr(&self) -> Option<*mut c_void> {
        // SAFETY: opaque handle; returns a borrowed pointer or null.
        let p = unsafe { noesis_routed_events_focus_old(self.raw) };
        (!p.is_null()).then_some(p)
    }

    /// Raw pointer to the element receiving focus
    /// (`KeyboardFocusChangedEventArgs::newFocus`), for the keyboard-focus
    /// events. `None` for other events or when nothing receives focus.
    ///
    /// Borrowed; same contract as [`source_ptr`](Self::source_ptr).
    pub fn focus_new_ptr(&self) -> Option<*mut c_void> {
        // SAFETY: opaque handle; returns a borrowed pointer or null.
        let p = unsafe { noesis_routed_events_focus_new(self.raw) };
        (!p.is_null()).then_some(p)
    }

    /// Effects and key state for a drag event (`DragEnter`, `DragOver`,
    /// `DragLeave`, `Drop` and their `Preview*` variants). `None` for other
    /// events.
    pub fn drag(&self) -> Option<DragInfo> {
        let mut effects = 0u32;
        let mut allowed = 0u32;
        let mut key_states = 0u32;
        // SAFETY: opaque handle; accessor validates the kind and writes on match.
        let ok = unsafe {
            noesis_routed_events_drag_effects(self.raw, &mut effects, &mut allowed, &mut key_states)
        };
        ok.then_some(DragInfo {
            effects: DragEffects(effects),
            allowed_effects: DragEffects(allowed),
            key_states: DragKeyStates(key_states),
        })
    }

    /// Sets `DragEventArgs::effects`, the result a `DragOver` or `Drop` handler
    /// reports back to the drag source. Returns `false`, writing nothing, if
    /// this is not a drag event.
    #[must_use = "a false return means the effect was not set because the live args are not a drag event"]
    pub fn set_drag_effects(&self, effects: DragEffects) -> bool {
        // SAFETY: opaque handle; accessor validates the kind before writing.
        unsafe { noesis_routed_events_drag_set_effects(self.raw, effects.bits()) }
    }

    /// Raw pointer to the dragged data (`DragEventArgs::data`), the object
    /// passed to [`do_drag_drop`]. `None` for other events or when no data is
    /// carried.
    ///
    /// Borrowed; same contract as [`source_ptr`](Self::source_ptr).
    pub fn drag_data_ptr(&self) -> Option<*mut c_void> {
        // SAFETY: opaque handle; returns a borrowed pointer or null.
        let p = unsafe { noesis_routed_events_drag_data(self.raw) };
        (!p.is_null()).then_some(p)
    }

    /// The pointer position during a drag, in `relative_to`'s coordinate space
    /// (`DragEventArgs::GetPosition`). `None` for other events.
    pub fn drag_position(&self, relative_to: &FrameworkElement) -> Option<(f32, f32)> {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: opaque handle + a borrowed live element pointer; accessor
        // validates the kind and writes on match.
        let ok = unsafe {
            noesis_routed_events_drag_position(self.raw, relative_to.raw(), &mut x, &mut y)
        };
        ok.then_some((x, y))
    }

    /// The manipulation origin (`manipulationOrigin`) for `ManipulationStarted`,
    /// `ManipulationDelta`, `ManipulationCompleted` and
    /// `ManipulationInertiaStarting`. `None` for other events.
    pub fn manip_origin(&self) -> Option<(f32, f32)> {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: opaque handle; accessor validates the kind and writes on match.
        let ok = unsafe { noesis_routed_events_manip_origin(self.raw, &mut x, &mut y) };
        ok.then_some((x, y))
    }

    /// The change since the last `ManipulationDelta` (`deltaManipulation`), or
    /// the total on `ManipulationCompleted` (`totalManipulation`). `None` for
    /// other events.
    pub fn manip_delta(&self) -> Option<ManipulationDelta> {
        let mut d = ManipulationDelta::default();
        // SAFETY: opaque handle; accessor validates the kind and writes on match.
        let ok = unsafe {
            noesis_routed_events_manip_delta(
                self.raw,
                &mut d.translation.0,
                &mut d.translation.1,
                &mut d.scale,
                &mut d.rotation,
                &mut d.expansion.0,
                &mut d.expansion.1,
            )
        };
        ok.then_some(d)
    }

    /// The change since the manipulation started (`cumulativeManipulation`),
    /// on `ManipulationDelta`. `None` for other events.
    pub fn manip_cumulative(&self) -> Option<ManipulationDelta> {
        let mut d = ManipulationDelta::default();
        // SAFETY: opaque handle; accessor validates the kind and writes on match.
        let ok = unsafe {
            noesis_routed_events_manip_cumulative(
                self.raw,
                &mut d.translation.0,
                &mut d.translation.1,
                &mut d.scale,
                &mut d.rotation,
                &mut d.expansion.0,
                &mut d.expansion.1,
            )
        };
        ok.then_some(d)
    }

    /// Velocities on `ManipulationDelta` (`velocities`), `ManipulationCompleted`
    /// (`finalVelocities`) or `ManipulationInertiaStarting`
    /// (`initialVelocities`). `None` for other events.
    pub fn manip_velocities(&self) -> Option<ManipulationVelocities> {
        let mut v = ManipulationVelocities::default();
        // SAFETY: opaque handle; accessor validates the kind and writes on match.
        let ok = unsafe {
            noesis_routed_events_manip_velocities(
                self.raw,
                &mut v.angular,
                &mut v.linear.0,
                &mut v.linear.1,
                &mut v.expansion.0,
                &mut v.expansion.1,
            )
        };
        ok.then_some(v)
    }

    /// Whether a `ManipulationDelta` or `ManipulationCompleted` event happened
    /// during inertia (`isInertial`). `None` for other events.
    pub fn manip_is_inertial(&self) -> Option<bool> {
        // SAFETY: opaque handle; accessor returns -1 unless it's a delta/completed event.
        match unsafe { noesis_routed_events_manip_is_inertial(self.raw) } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

/// `Noesis::DragDropEffects` flags: the operations a drag allows or a drop
/// target accepts. Combine with `|`, [`Self::with`] or `collect()`, and test
/// with [`Self::contains`].
///
/// ```
/// use noesis_runtime::events::DragEffects;
/// let e = DragEffects::COPY.with(DragEffects::MOVE);
/// assert!(e.contains(DragEffects::COPY));
/// assert!(!e.contains(DragEffects::LINK));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DragEffects(pub u32);

impl DragEffects {
    /// The drag-and-drop operation transfers no data.
    pub const NONE: Self = Self(0);
    /// The data is copied.
    pub const COPY: Self = Self(1);
    /// The data is moved.
    pub const MOVE: Self = Self(2);
    /// The data is linked.
    pub const LINK: Self = Self(4);
    /// Scrolling is about to start or is occurring in the target.
    pub const SCROLL: Self = Self(0x8000_0000);
    /// `COPY | MOVE | SCROLL`.
    pub const ALL: Self = Self(Self::COPY.0 | Self::MOVE.0 | Self::SCROLL.0);

    /// Wrap a raw `Noesis::DragDropEffects` bitmask.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The raw bitmask Noesis uses.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// A copy of this set with `other`'s bits added.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every flag in `other` is set. Always `true` for [`Self::NONE`].
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no effects are set.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitOr for DragEffects {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl FromIterator<DragEffects> for DragEffects {
    fn from_iter<I: IntoIterator<Item = DragEffects>>(iter: I) -> Self {
        let mut acc = 0;
        for e in iter {
            acc |= e.0;
        }
        Self(acc)
    }
}

/// `Noesis::DragDropKeyStates` flags (`DragEventArgs::keyStates`): the
/// modifier keys and mouse buttons held during a drag.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DragKeyStates(pub u32);

impl DragKeyStates {
    /// No modifier keys or mouse buttons are pressed.
    pub const NONE: Self = Self(0);
    /// The left mouse button is pressed.
    pub const LEFT_MOUSE_BUTTON: Self = Self(1);
    /// The right mouse button is pressed.
    pub const RIGHT_MOUSE_BUTTON: Self = Self(2);
    /// The Shift key is pressed.
    pub const SHIFT_KEY: Self = Self(4);
    /// The Ctrl key is pressed.
    pub const CONTROL_KEY: Self = Self(8);
    /// The middle mouse button is pressed.
    pub const MIDDLE_MOUSE_BUTTON: Self = Self(16);
    /// The Alt key is pressed.
    pub const ALT_KEY: Self = Self(32);

    /// Wrap a raw `Noesis::DragDropKeyStates` bitmask.
    #[must_use]
    pub const fn from_bits(bits: u32) -> Self {
        Self(bits)
    }

    /// The raw bitmask Noesis uses.
    #[must_use]
    pub const fn bits(self) -> u32 {
        self.0
    }

    /// A copy of this set with `other`'s bits added.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Whether every flag in `other` is set. Always `true` for [`Self::NONE`].
    #[must_use]
    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Whether no keys/buttons are held.
    #[must_use]
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

impl core::ops::BitOr for DragKeyStates {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}

impl FromIterator<DragKeyStates> for DragKeyStates {
    fn from_iter<I: IntoIterator<Item = DragKeyStates>>(iter: I) -> Self {
        let mut acc = 0;
        for k in iter {
            acc |= k.0;
        }
        Self(acc)
    }
}

/// Drag state returned by [`EventArgs::drag`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragInfo {
    /// The effect currently reported by the drop target.
    pub effects: DragEffects,
    /// The effects the drag source allows.
    pub allowed_effects: DragEffects,
    /// Modifier keys and mouse buttons held.
    pub key_states: DragKeyStates,
}

/// A manipulation transform (`Noesis::ManipulationDelta`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ManipulationDelta {
    /// `(x, y)` translation in pixels.
    pub translation: (f32, f32),
    /// Scale factor; `1.0` is no change.
    pub scale: f32,
    /// Rotation in degrees.
    pub rotation: f32,
    /// `(x, y)` expansion in pixels.
    pub expansion: (f32, f32),
}

/// Manipulation velocities (`Noesis::ManipulationVelocities`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ManipulationVelocities {
    /// Degrees per millisecond.
    pub angular: f32,
    /// `(x, y)` pixels per millisecond.
    pub linear: (f32, f32),
    /// `(x, y)` pixels per millisecond.
    pub expansion: (f32, f32),
}

/// Handler for [`subscribe_event`] and [`subscribe_event_by_name`]. Any
/// `Fn(&EventArgs) -> bool + Send + 'static` closure implements it.
///
/// Return `true` to mark the event handled. That stops it routing to other
/// elements and skips later handlers on the same element subscribed with
/// `handled_too = false`. Returning `false` leaves the flag as it was.
///
/// May be re-entered (see the module docs on threading).
pub trait RoutedEventHandler: Send + 'static {
    fn on_event(&self, args: &EventArgs) -> bool;
}

impl<F: Fn(&EventArgs) -> bool + Send + 'static> RoutedEventHandler for F {
    fn on_event(&self, args: &EventArgs) -> bool {
        self(args)
    }
}

/// SAFETY: `userdata` must be a pointer produced by [`subscribe_event`] and
/// still alive (the [`EventSubscription`] hasn't been dropped). `args` is the
/// opaque handle the C++ shim passes; it is valid only for this call.
/// `out_handled` must be a non-null pointer to a writable bool.
unsafe extern "C" fn event_trampoline(
    userdata: *mut c_void,
    args: *const c_void,
    out_handled: *mut bool,
) {
    crate::panic_guard::guard(|| {
        // Shared `&`: re-entrant handler box (see `RoutedEventHandler`).
        let handler = &*userdata.cast::<Box<dyn RoutedEventHandler>>();
        let ev = EventArgs {
            raw: args,
            _not_send: PhantomData,
        };
        let handled = handler.on_event(&ev);
        if !out_handled.is_null() {
            *out_handled = handled;
        }
    })
}

/// Keeps a [`subscribe_event`] handler installed. Drop it to unsubscribe.
/// Holds a `+1` ref on the element.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct EventSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for EventSubscription {}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by subscribe_event; unsubscribe frees the
        // donated box exactly once (deferred if dropping from inside the
        // callback).
        unsafe { noesis_unsubscribe_event(self.token.as_ptr()) }
    }
}

/// A routed event for [`subscribe_event`]. Handlers get the [`EventArgs`]
/// accessors that match the event's argument type. For other events, use
/// [`subscribe_event_by_name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum RoutedEvent {
    /// `UIElement.MouseEnter`.
    MouseEnter,
    /// `UIElement.MouseLeave`.
    MouseLeave,
    /// `UIElement.MouseMove`.
    MouseMove,
    /// `UIElement.PreviewMouseMove`.
    PreviewMouseMove,
    /// `UIElement.GotMouseCapture`.
    GotMouseCapture,
    /// `UIElement.LostMouseCapture`.
    LostMouseCapture,
    /// `UIElement.MouseDown`.
    MouseDown,
    /// `UIElement.MouseUp`.
    MouseUp,
    /// `UIElement.MouseLeftButtonDown`.
    MouseLeftButtonDown,
    /// `UIElement.MouseLeftButtonUp`.
    MouseLeftButtonUp,
    /// `UIElement.MouseRightButtonDown`.
    MouseRightButtonDown,
    /// `UIElement.MouseRightButtonUp`.
    MouseRightButtonUp,
    /// `UIElement.PreviewMouseDown`.
    PreviewMouseDown,
    /// `UIElement.PreviewMouseUp`.
    PreviewMouseUp,
    /// `UIElement.PreviewMouseLeftButtonDown`.
    PreviewMouseLeftButtonDown,
    /// `UIElement.PreviewMouseLeftButtonUp`.
    PreviewMouseLeftButtonUp,
    /// `UIElement.PreviewMouseRightButtonDown`.
    PreviewMouseRightButtonDown,
    /// `UIElement.PreviewMouseRightButtonUp`.
    PreviewMouseRightButtonUp,
    /// `UIElement.MouseWheel`.
    MouseWheel,
    /// `UIElement.PreviewMouseWheel`.
    PreviewMouseWheel,
    /// `UIElement.KeyDown`.
    KeyDown,
    /// `UIElement.KeyUp`.
    KeyUp,
    /// `UIElement.PreviewKeyDown`.
    PreviewKeyDown,
    /// `UIElement.PreviewKeyUp`.
    PreviewKeyUp,
    /// `UIElement.TextInput`.
    TextInput,
    /// `UIElement.PreviewTextInput`.
    PreviewTextInput,
    /// `UIElement.GotFocus`.
    GotFocus,
    /// `UIElement.LostFocus`.
    LostFocus,
    /// `UIElement.GotKeyboardFocus`.
    GotKeyboardFocus,
    /// `UIElement.LostKeyboardFocus`.
    LostKeyboardFocus,
    /// `UIElement.PreviewGotKeyboardFocus`.
    PreviewGotKeyboardFocus,
    /// `UIElement.PreviewLostKeyboardFocus`.
    PreviewLostKeyboardFocus,
    /// `FrameworkElement.Loaded`.
    Loaded,
    /// `FrameworkElement.Unloaded`.
    Unloaded,
    /// `FrameworkElement.SizeChanged`.
    SizeChanged,
    /// `UIElement.TouchDown`.
    TouchDown,
    /// `UIElement.TouchMove`.
    TouchMove,
    /// `UIElement.TouchUp`.
    TouchUp,
    /// `UIElement.TouchEnter`.
    TouchEnter,
    /// `UIElement.TouchLeave`.
    TouchLeave,
    /// `UIElement.Tapped`.
    Tapped,
    /// `UIElement.DoubleTapped`.
    DoubleTapped,
    /// `UIElement.Holding`.
    Holding,
    /// `UIElement.RightTapped`.
    RightTapped,
    /// `UIElement.ManipulationStarting`.
    ManipulationStarting,
    /// `UIElement.ManipulationStarted`.
    ManipulationStarted,
    /// `UIElement.ManipulationDelta`.
    ManipulationDelta,
    /// `UIElement.ManipulationInertiaStarting`.
    ManipulationInertiaStarting,
    /// `UIElement.ManipulationCompleted`.
    ManipulationCompleted,
    /// `UIElement.DragEnter`.
    DragEnter,
    /// `UIElement.DragOver`.
    DragOver,
    /// `UIElement.DragLeave`.
    DragLeave,
    /// `UIElement.Drop`.
    Drop,
    /// `UIElement.PreviewDragEnter`.
    PreviewDragEnter,
    /// `UIElement.PreviewDragOver`.
    PreviewDragOver,
    /// `UIElement.PreviewDragLeave`.
    PreviewDragLeave,
    /// `UIElement.PreviewDrop`.
    PreviewDrop,
}

impl RoutedEvent {
    /// The event's Noesis name, as accepted by [`subscribe_event_by_name`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::MouseEnter => "MouseEnter",
            Self::MouseLeave => "MouseLeave",
            Self::MouseMove => "MouseMove",
            Self::PreviewMouseMove => "PreviewMouseMove",
            Self::GotMouseCapture => "GotMouseCapture",
            Self::LostMouseCapture => "LostMouseCapture",
            Self::MouseDown => "MouseDown",
            Self::MouseUp => "MouseUp",
            Self::MouseLeftButtonDown => "MouseLeftButtonDown",
            Self::MouseLeftButtonUp => "MouseLeftButtonUp",
            Self::MouseRightButtonDown => "MouseRightButtonDown",
            Self::MouseRightButtonUp => "MouseRightButtonUp",
            Self::PreviewMouseDown => "PreviewMouseDown",
            Self::PreviewMouseUp => "PreviewMouseUp",
            Self::PreviewMouseLeftButtonDown => "PreviewMouseLeftButtonDown",
            Self::PreviewMouseLeftButtonUp => "PreviewMouseLeftButtonUp",
            Self::PreviewMouseRightButtonDown => "PreviewMouseRightButtonDown",
            Self::PreviewMouseRightButtonUp => "PreviewMouseRightButtonUp",
            Self::MouseWheel => "MouseWheel",
            Self::PreviewMouseWheel => "PreviewMouseWheel",
            Self::KeyDown => "KeyDown",
            Self::KeyUp => "KeyUp",
            Self::PreviewKeyDown => "PreviewKeyDown",
            Self::PreviewKeyUp => "PreviewKeyUp",
            Self::TextInput => "TextInput",
            Self::PreviewTextInput => "PreviewTextInput",
            Self::GotFocus => "GotFocus",
            Self::LostFocus => "LostFocus",
            Self::GotKeyboardFocus => "GotKeyboardFocus",
            Self::LostKeyboardFocus => "LostKeyboardFocus",
            Self::PreviewGotKeyboardFocus => "PreviewGotKeyboardFocus",
            Self::PreviewLostKeyboardFocus => "PreviewLostKeyboardFocus",
            Self::Loaded => "Loaded",
            Self::Unloaded => "Unloaded",
            Self::SizeChanged => "SizeChanged",
            Self::TouchDown => "TouchDown",
            Self::TouchMove => "TouchMove",
            Self::TouchUp => "TouchUp",
            Self::TouchEnter => "TouchEnter",
            Self::TouchLeave => "TouchLeave",
            Self::Tapped => "Tapped",
            Self::DoubleTapped => "DoubleTapped",
            Self::Holding => "Holding",
            Self::RightTapped => "RightTapped",
            Self::ManipulationStarting => "ManipulationStarting",
            Self::ManipulationStarted => "ManipulationStarted",
            Self::ManipulationDelta => "ManipulationDelta",
            Self::ManipulationInertiaStarting => "ManipulationInertiaStarting",
            Self::ManipulationCompleted => "ManipulationCompleted",
            Self::DragEnter => "DragEnter",
            Self::DragOver => "DragOver",
            Self::DragLeave => "DragLeave",
            Self::Drop => "Drop",
            Self::PreviewDragEnter => "PreviewDragEnter",
            Self::PreviewDragOver => "PreviewDragOver",
            Self::PreviewDragLeave => "PreviewDragLeave",
            Self::PreviewDrop => "PreviewDrop",
        }
    }
}

/// Subscribes `handler` to the routed `event` on `element`.
///
/// With `handled_too == false`, the handler is skipped when an earlier handler
/// on the same element already marked the event handled. Events handled on
/// another element never reach this one, whatever the flag: Noesis's
/// `AddHandler` has no `handledEventsToo` parameter.
///
/// Returns `None` if `element` is not a `UIElement`. The handler stays
/// installed while the returned [`EventSubscription`] lives.
pub fn subscribe_event<H: RoutedEventHandler>(
    element: &FrameworkElement,
    event: RoutedEvent,
    handled_too: bool,
    handler: H,
) -> Option<EventSubscription> {
    subscribe_event_by_name(element, event.as_str(), handled_too, handler)
}

/// Subscribes `handler` to the routed event named `event_name` on `element`.
///
/// The names in [`RoutedEvent::as_str`] get the full typed [`EventArgs`]. Any
/// other routed event registered on the element's class (a control's own
/// event, or a custom one) is found by name through Noesis reflection; its
/// handler only gets [`EventArgs::source_ptr`] and
/// [`EventArgs::source_data_context_u64`].
///
/// `handled_too` works as in [`subscribe_event`]. Returns `None` if `element`
/// is not a `UIElement`, or `event_name` is unknown or contains a NUL byte.
pub fn subscribe_event_by_name<H: RoutedEventHandler>(
    element: &FrameworkElement,
    event_name: &str,
    handled_too: bool,
    handler: H,
) -> Option<EventSubscription> {
    let cname = CString::new(event_name).ok()?;

    let outer: Box<Box<dyn RoutedEventHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked and donated
    // to the C++ handler; the element + name pointers are borrowed for the call
    // duration only.
    let token = unsafe {
        noesis_subscribe_event(
            element.raw(),
            cname.as_ptr(),
            handled_too,
            event_trampoline,
            userdata.cast(),
            free_donated::<Box<dyn RoutedEventHandler>>,
        )
    };

    if let Some(token) = NonNull::new(token) {
        Some(EventSubscription { token })
    } else {
        // C++ took no ownership on failure.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

/// Handler for [`subscribe_lifecycle`], called with no arguments. Any
/// `Fn() + Send + 'static` closure implements it.
///
/// May be re-entered: a handler that changes its element (for example by
/// re-parenting it) can trigger the event again before it returns.
pub trait LifecycleHandler: Send + 'static {
    fn on_event(&self);
}

impl<F: Fn() + Send + 'static> LifecycleHandler for F {
    fn on_event(&self) {
        self();
    }
}

/// SAFETY: `userdata` must be a pointer produced by [`subscribe_lifecycle`] and
/// still alive (the [`LifecycleSubscription`] hasn't been dropped).
unsafe extern "C" fn lifecycle_trampoline(userdata: *mut c_void) {
    crate::panic_guard::guard(|| {
        // Shared `&`: re-entrant handler box (see `LifecycleHandler`).
        let handler = &*userdata.cast::<Box<dyn LifecycleHandler>>();
        handler.on_event();
    })
}

/// Keeps a [`subscribe_lifecycle`] handler installed. Drop it to unsubscribe.
/// Holds a `+1` ref on the element.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct LifecycleSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for LifecycleSubscription {}

impl Drop for LifecycleSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by subscribe_lifecycle; unsubscribe frees the
        // donated box exactly once (deferred if dropping from inside the
        // callback).
        unsafe { noesis_unsubscribe_lifecycle(self.token.as_ptr()) }
    }
}

/// A non-routed element notification for [`subscribe_lifecycle`].
///
/// These are plain .NET-style events, not routed events, so they don't bubble
/// and carry no arguments here. This enum lists every supported event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LifecycleEvent {
    /// `FrameworkElement.Initialized`.
    Initialized,
    /// `FrameworkElement.LayoutUpdated`.
    LayoutUpdated,
    /// `FrameworkElement.DataContextChanged`.
    DataContextChanged,
    /// `UIElement.IsEnabledChanged`.
    IsEnabledChanged,
    /// `UIElement.IsVisibleChanged`.
    IsVisibleChanged,
    /// `UIElement.IsHitTestVisibleChanged`.
    IsHitTestVisibleChanged,
    /// `UIElement.IsKeyboardFocusedChanged`.
    IsKeyboardFocusedChanged,
    /// `UIElement.IsKeyboardFocusWithinChanged`.
    IsKeyboardFocusWithinChanged,
    /// `UIElement.IsMouseCapturedChanged`.
    IsMouseCapturedChanged,
    /// `UIElement.IsMouseCaptureWithinChanged`.
    IsMouseCaptureWithinChanged,
    /// `UIElement.IsMouseDirectlyOverChanged`.
    IsMouseDirectlyOverChanged,
    /// `UIElement.FocusableChanged`.
    FocusableChanged,
}

impl LifecycleEvent {
    /// The event's Noesis name, as accepted by [`subscribe_lifecycle_by_name`].
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Initialized => "Initialized",
            Self::LayoutUpdated => "LayoutUpdated",
            Self::DataContextChanged => "DataContextChanged",
            Self::IsEnabledChanged => "IsEnabledChanged",
            Self::IsVisibleChanged => "IsVisibleChanged",
            Self::IsHitTestVisibleChanged => "IsHitTestVisibleChanged",
            Self::IsKeyboardFocusedChanged => "IsKeyboardFocusedChanged",
            Self::IsKeyboardFocusWithinChanged => "IsKeyboardFocusWithinChanged",
            Self::IsMouseCapturedChanged => "IsMouseCapturedChanged",
            Self::IsMouseCaptureWithinChanged => "IsMouseCaptureWithinChanged",
            Self::IsMouseDirectlyOverChanged => "IsMouseDirectlyOverChanged",
            Self::FocusableChanged => "FocusableChanged",
        }
    }
}

/// Subscribes `handler` to the lifecycle `event` on `element`.
///
/// The handler stays installed while the returned [`LifecycleSubscription`]
/// lives. Returns `None` if `element` is not a `FrameworkElement` (a handle can
/// also wrap a plain `Visual`).
pub fn subscribe_lifecycle<H: LifecycleHandler>(
    element: &FrameworkElement,
    event: LifecycleEvent,
    handler: H,
) -> Option<LifecycleSubscription> {
    subscribe_lifecycle_by_name(element, event.as_str(), handler)
}

/// Subscribes `handler` to the lifecycle event named `name` on `element`.
///
/// Only the names in [`LifecycleEvent::as_str`] are supported. Returns `None`
/// if `name` is not one of them or contains a NUL byte, or if `element` is not
/// a `FrameworkElement`.
pub fn subscribe_lifecycle_by_name<H: LifecycleHandler>(
    element: &FrameworkElement,
    name: &str,
    handler: H,
) -> Option<LifecycleSubscription> {
    let cname = CString::new(name).ok()?;

    let outer: Box<Box<dyn LifecycleHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked and donated
    // to the C++ handler; the element + name pointers are borrowed for the call
    // duration only.
    let token = unsafe {
        noesis_subscribe_lifecycle(
            element.raw(),
            cname.as_ptr(),
            lifecycle_trampoline,
            userdata.cast(),
            free_donated::<Box<dyn LifecycleHandler>>,
        )
    };

    if let Some(token) = NonNull::new(token) {
        Some(LifecycleSubscription { token })
    } else {
        // C++ took no ownership on failure.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

/// Starts a drag from `source` carrying `data`, offering `allowed_effects`
/// (`Noesis::DragDrop::DoDragDrop`).
///
/// Returns immediately. The drag then follows the pointer input you feed the
/// view, and drop targets see `data` through [`EventArgs::drag_data_ptr`].
/// This crate has no `DataObject` builder, so the payload is an element.
///
/// Returns `false` if `source` is not a `DependencyObject`.
pub fn do_drag_drop(
    source: &FrameworkElement,
    data: &FrameworkElement,
    allowed_effects: DragEffects,
) -> bool {
    // SAFETY: both pointers are borrowed live elements; DoDragDrop copies what
    // it needs and does not retain the raw pointers past the call we make here.
    unsafe { noesis_routed_events_do_drag_drop(source.raw(), data.raw(), allowed_effects.bits()) }
}

/// Handler for [`subscribe_data_object`]. Any
/// `Fn(Option<*mut c_void>, bool) -> bool + Send + 'static` closure implements
/// it.
pub trait DataObjectHandler: Send + 'static {
    /// Called before a copy or paste. `data_object` is a raw pointer to the
    /// `DataObject`, borrowed for this call only. `is_drag_drop` is `true`
    /// when the transfer is a drag-drop rather than the clipboard.
    ///
    /// The return value becomes the event's cancel flag: `true` cancels, and
    /// `false` clears a cancel set by an earlier handler.
    fn on_data_object(&self, data_object: Option<*mut c_void>, is_drag_drop: bool) -> bool;
}

impl<F: Fn(Option<*mut c_void>, bool) -> bool + Send + 'static> DataObjectHandler for F {
    fn on_data_object(&self, data_object: Option<*mut c_void>, is_drag_drop: bool) -> bool {
        self(data_object, is_drag_drop)
    }
}

/// SAFETY: `userdata` must be a pointer produced by a `subscribe_data_object_*`
/// call and still alive (its [`DataObjectSubscription`] hasn't been dropped).
/// `out_cancel` must be a non-null pointer to a writable bool.
unsafe extern "C" fn data_object_trampoline(
    userdata: *mut c_void,
    data_object: *mut c_void,
    is_drag_drop: bool,
    out_cancel: *mut bool,
) {
    crate::panic_guard::guard(|| {
        // Shared `&`: re-entrant handler box (see `DataObjectHandler`).
        let handler = &*userdata.cast::<Box<dyn DataObjectHandler>>();
        let data = (!data_object.is_null()).then_some(data_object);
        let cancel = handler.on_data_object(data, is_drag_drop);
        if !out_cancel.is_null() {
            *out_cancel = cancel;
        }
    })
}

/// Keeps a [`subscribe_data_object`] handler installed. Drop it to
/// unsubscribe. Holds a `+1` ref on the element.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct DataObjectSubscription {
    token: NonNull<c_void>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for DataObjectSubscription {}

impl Drop for DataObjectSubscription {
    fn drop(&mut self) {
        // SAFETY: token produced by a subscribe call; remove frees the donated
        // box exactly once (deferred if dropping from inside the callback).
        unsafe { noesis_routed_events_remove_data_object_handler(self.token.as_ptr()) }
    }
}

/// The `DataObject` event for [`subscribe_data_object`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataObjectEvent {
    /// `DataObject.Copying`: raised before data is copied, for example by
    /// `Ctrl+C` in a `TextBox`.
    Copying,
    /// `DataObject.Pasting`: raised before data is pasted, for example by
    /// `Ctrl+V`.
    Pasting,
}

/// Subscribes `handler` to `DataObject.Copying` or `DataObject.Pasting` on
/// `element`, typically a `TextBox`.
///
/// The handler stays installed while the returned [`DataObjectSubscription`]
/// lives. Returns `None` if `element` is not a `UIElement`.
pub fn subscribe_data_object<H: DataObjectHandler>(
    element: &FrameworkElement,
    event: DataObjectEvent,
    handler: H,
) -> Option<DataObjectSubscription> {
    let outer: Box<Box<dyn DataObjectHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked and donated
    // to the C++ handler; the element pointer is borrowed for the call only.
    let token = unsafe {
        match event {
            DataObjectEvent::Copying => noesis_routed_events_add_copying_handler(
                element.raw(),
                data_object_trampoline,
                userdata.cast(),
                free_donated::<Box<dyn DataObjectHandler>>,
            ),
            DataObjectEvent::Pasting => noesis_routed_events_add_pasting_handler(
                element.raw(),
                data_object_trampoline,
                userdata.cast(),
                free_donated::<Box<dyn DataObjectHandler>>,
            ),
        }
    };

    if let Some(token) = NonNull::new(token) {
        Some(DataObjectSubscription { token })
    } else {
        // C++ took no ownership on failure.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}
