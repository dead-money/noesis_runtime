//! Subscribe Rust callbacks to Noesis routed events.
//!
//! Start with [`subscribe_click`] for `BaseButton::Click` and
//! [`subscribe_keydown`] for `UIElement::KeyDown`. For any other routed
//! event reach for the generic [`subscribe_event`] (typed by [`RoutedEvent`])
//! or its `&str` escape hatch [`subscribe_event_by_name`]. Non-routed
//! lifecycle notifications (`Loaded`, `IsEnabledChanged`, and friends) go
//! through [`subscribe_lifecycle`]. Every subscription returns an RAII token:
//! a heap-allocated handler that owns its registration and holds a `+1` ref on
//! the source element, so dropping the token unsubscribes.
//!
//! # Threading
//!
//! Click callbacks fire from inside Noesis's input pump (typically
//! `IView::MouseButtonUp` or `IView::Update`), on whatever thread is driving
//! the view. The callback signature has no `Send` bound at the FFI level —
//! the safe wrapper enforces it on the Rust side via the trait. Keep work
//! in the callback small: push to a queue / channel and process from a
//! regular Bevy system step if you need anything heavier than a flag flip.
//!
//! # Lifetime
//!
//! [`ClickSubscription`] is RAII: while alive, the registered handler stays
//! on the button's `Click` event. Drop the subscription to unsubscribe.
//! The subscription holds a `+1` ref on the button so the handler list
//! stays valid even if the only other reference to the element was the
//! [`crate::view::FrameworkElement`] you used to subscribe.

#![allow(unsafe_op_in_unsafe_fn)] // thin FFI surface — explicit blocks add noise

use core::marker::PhantomData;
use core::ptr::NonNull;
use std::ffi::{CString, c_void};

use crate::ffi::{
    noesis_key_args_key, noesis_mouse_args_position, noesis_mouse_button_args_button,
    noesis_mouse_wheel_args_delta, noesis_routed_args_source,
    noesis_routed_events_add_copying_handler, noesis_routed_events_add_pasting_handler,
    noesis_routed_events_do_drag_drop, noesis_routed_events_drag_data,
    noesis_routed_events_drag_effects, noesis_routed_events_drag_position,
    noesis_routed_events_drag_set_effects, noesis_routed_events_focus_new,
    noesis_routed_events_focus_old, noesis_routed_events_manip_cumulative,
    noesis_routed_events_manip_delta, noesis_routed_events_manip_is_inertial,
    noesis_routed_events_manip_origin, noesis_routed_events_manip_velocities,
    noesis_routed_events_remove_data_object_handler, noesis_size_changed_args_new_size,
    noesis_subscribe_click, noesis_subscribe_event, noesis_subscribe_keydown,
    noesis_subscribe_lifecycle, noesis_text_args_ch, noesis_unsubscribe_click,
    noesis_unsubscribe_event, noesis_unsubscribe_keydown, noesis_unsubscribe_lifecycle,
};
use crate::view::{FrameworkElement, Key, MouseButton};

/// Rust-side click handler. Implementors receive a single `()` notification
/// per fired click; if you need the sender or event args, subscribe through
/// the generic [`subscribe_event`] / [`RoutedEventHandler`] instead.
///
/// The `Send + 'static` bounds let the handler live inside a Bevy
/// `Resource` or be moved onto the render thread.
/// Takes `&self` (re-entrant: a handler may re-raise the subscribed event on
/// the same element via [`crate::reflection::raise_event`], re-entering this
/// same box; use interior mutability for handler state).
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

/// RAII subscription token. Drop to unsubscribe and free the boxed handler.
///
/// Holds a `+1` ref on the underlying button (managed C++-side); dropping
/// this releases that ref and removes the handler from the routed-event
/// list. Drop before [`crate::shutdown`] like every other owning handle in
/// this crate.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct ClickSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn ClickHandler>>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for ClickSubscription {}

impl Drop for ClickSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe_click;
        // freed exactly once here.
        unsafe {
            noesis_unsubscribe_click(self.token.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Subscribe `handler` to `BaseButton::Click` on `element`. Returns `None`
/// if the element is not castable to `BaseButton` (e.g. it's a plain
/// `ContentControl` or a `UserControl` whose root isn't a button).
///
/// The returned [`ClickSubscription`] keeps the handler installed for as
/// long as it lives; drop it (or replace it) to unsubscribe.
///
/// # Panics
///
/// Panics only on internal logic errors — specifically if `Box::into_raw`
/// returns null (it cannot, but the wrapper is `NonNull` to keep the
/// invariant explicit at the type level).
pub fn subscribe_click<H: ClickHandler>(
    element: &FrameworkElement,
    handler: H,
) -> Option<ClickSubscription> {
    // Double-Box gives a stable thin pointer for the C ABI userdata, same
    // pattern as the providers.
    let outer: Box<Box<dyn ClickHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked; the
    // element pointer is borrowed for the call duration only — Noesis copies
    // whatever it needs into the routed-event handler list.
    let token = unsafe { noesis_subscribe_click(element.raw(), click_trampoline, userdata.cast()) };

    if let Some(token) = NonNull::new(token) {
        Some(ClickSubscription {
            token,
            userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
        })
    } else {
        // Subscription failed (e.g. element wasn't a button). Free the
        // userdata we leaked above so we don't leak the handler.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

/// Rust-side keydown handler. Receives the pressed key plus a writable flag;
/// setting the flag to `true` marks the routed event handled, stopping
/// propagation (e.g. prevents the backtick keystroke that opens the console
/// from also being typed into a focused `TextBox`).
///
/// The `Send + 'static` bounds let the handler live inside a Bevy
/// `Resource` or be moved onto the render thread.
pub trait KeyDownHandler: Send + 'static {
    /// Called once per `KeyDown` event on the subscribed element. Return
    /// value: `true` to mark the routed event handled, `false` to let it
    /// continue propagating.
    ///
    /// Takes `&self` (re-entrant per [`ClickHandler`]; use interior mutability
    /// for handler state).
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
        // Best-effort map of the raw ordinal back to our safe `Key` mirror.
        // Anything outside the mirrored set arrives as `Key::None` — callers
        // can still observe the event and choose to ignore unmapped keys.
        let mapped = key_from_raw(key);
        let handled = handler.on_keydown(mapped);
        if !out_handled.is_null() {
            *out_handled = handled;
        }
    })
}

/// Convert a raw `Noesis::Key` ordinal back into the safe [`Key`] mirror.
/// Unmapped ordinals collapse to [`Key::None`]; the caller's handler can
/// still match on the value but won't be able to distinguish *which*
/// unmapped key fired. Add variants to [`Key`] (and the C++ `static_assert`s
/// in `noesis_view.cpp`) when a missing key earns it.
fn key_from_raw(raw: i32) -> Key {
    // Match table rather than transmute: transmute would be UB for an ordinal
    // outside the declared variants. Order mirrors the `Key` enum in src/view.rs.
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
        _ => Key::None,
    }
}

/// RAII subscription token for [`subscribe_keydown`]. Drop to unsubscribe
/// and free the boxed handler. Mirrors [`ClickSubscription`].
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct KeyDownSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn KeyDownHandler>>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for KeyDownSubscription {}

impl Drop for KeyDownSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe_keydown;
        // freed exactly once here.
        unsafe {
            noesis_unsubscribe_keydown(self.token.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Subscribe `handler` to `UIElement::KeyDown` on `element`. Returns
/// `None` if the element is not a `UIElement` (rare — essentially every
/// visual element is, but the cast is included so callers don't have to
/// trust the FFI blindly).
///
/// The returned [`KeyDownSubscription`] keeps the handler installed for
/// as long as it lives; drop it (or replace it) to unsubscribe.
///
/// Setting the handler's return value to `true` marks the routed event
/// handled — useful for swallowing the backtick that opens the console
/// so it doesn't get typed into a focused `TextBox`.
///
/// # Panics
///
/// Panics only on internal logic errors — specifically if `Box::into_raw`
/// returns null (it cannot, but the wrapper is `NonNull` to keep the
/// invariant explicit at the type level).
pub fn subscribe_keydown<H: KeyDownHandler>(
    element: &FrameworkElement,
    handler: H,
) -> Option<KeyDownSubscription> {
    let outer: Box<Box<dyn KeyDownHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked; the
    // element pointer is borrowed for the call duration only — Noesis copies
    // whatever it needs into the routed-event handler list.
    let token =
        unsafe { noesis_subscribe_keydown(element.raw(), keydown_trampoline, userdata.cast()) };

    if let Some(token) = NonNull::new(token) {
        Some(KeyDownSubscription {
            token,
            userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
        })
    } else {
        // Subscription failed (e.g. element wasn't a UIElement). Free the
        // userdata we leaked above so we don't leak the handler.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing
        // else ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

/// Borrowed view over a routed event's arguments, handed to a
/// [`RoutedEventHandler`] **by reference** for the duration of one callback.
/// Backed by the opaque C++ `args` pointer; the typed accessors read whichever
/// concrete arg struct actually fired (a generic callback can probe several and
/// act on the one that returns `Some`).
///
/// The handler receives `&EventArgs`, never an owned value. The underlying C++
/// args live on the stack of the Noesis input pump and are valid only while the
/// callback runs — do not stash the borrow or the `source_ptr` beyond the call.
/// (The type deliberately carries no lifetime parameter: a generic-lifetime
/// arg type defeats closure HRTB inference, so the borrow is expressed through
/// the `&EventArgs` the handler is handed instead.)
pub struct EventArgs {
    raw: *const c_void,
    _not_send: PhantomData<*const c_void>,
}

impl EventArgs {
    /// Wrap a borrowed live-args pointer (the opaque handle a
    /// [`RoutedEventHandler`] receives) in an [`EventArgs`] view. Hidden from
    /// the public docs: it exists so test harnesses and advanced callers that
    /// dispatch through a custom C trampoline can reuse the typed accessors.
    ///
    /// # Safety
    ///
    /// `raw` must be a valid args handle produced by the C++ shim and alive for
    /// the lifetime of the returned value (i.e. only for the duration of the
    /// callback that handed it over). The returned `EventArgs` must not outlive
    /// that callback.
    #[doc(hidden)]
    pub unsafe fn from_raw(raw: *const c_void) -> Self {
        EventArgs {
            raw,
            _not_send: PhantomData,
        }
    }

    /// Pointer position in the source element's coordinate space, for mouse,
    /// mouse-button and mouse-wheel events. `None` for other event kinds.
    pub fn position(&self) -> Option<(f32, f32)> {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: `raw` is the opaque handle the trampoline received; the
        // accessor validates the arg kind and writes only on a match.
        let ok = unsafe { noesis_mouse_args_position(self.raw, &mut x, &mut y) };
        ok.then_some((x, y))
    }

    /// Changed mouse button for a mouse-button event; `None` otherwise.
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

    /// Wheel rotation delta for a mouse-wheel event (signed, ~120 per notch).
    /// `None` for non-wheel events. The kind check happens inside the C
    /// accessor, so only genuine wheel events yield `Some`.
    pub fn wheel_delta(&self) -> Option<i32> {
        // Only mouse-class events carry a position; a wheel event always does.
        // Combined with the accessor's kind gate, this disambiguates the
        // 0-delta sentinel from "not a wheel event".
        if !self.is_wheel() {
            return None;
        }
        // SAFETY: opaque handle; accessor returns 0 unless it's a wheel event.
        Some(unsafe { noesis_mouse_wheel_args_delta(self.raw) })
    }

    /// Whether the live args are a mouse-wheel event. A wheel event is the only
    /// mouse-class event that reports a position but no changed button, so we
    /// classify on that pair rather than the ambiguous 0-delta sentinel.
    fn is_wheel(&self) -> bool {
        self.position().is_some() && self.mouse_button().is_none()
    }

    /// Pressed/released key for a key event, mapped to the safe [`Key`] mirror.
    /// `None` for non-key events. Keys outside the mirrored set arrive as
    /// `Some(Key::None)`.
    pub fn key(&self) -> Option<Key> {
        // SAFETY: opaque handle; accessor returns -1 unless it's a key event.
        let raw = unsafe { noesis_key_args_key(self.raw) };
        (raw >= 0).then(|| key_from_raw(raw))
    }

    /// Input character (UTF-32 code point) for a `TextInput` event; `None`
    /// otherwise.
    pub fn text_char(&self) -> Option<char> {
        // SAFETY: opaque handle; accessor returns -1 unless it's text input.
        let raw = unsafe { noesis_text_args_ch(self.raw) };
        if raw < 0 {
            return None;
        }
        char::from_u32(raw as u32)
    }

    /// New size for a `SizeChanged` event (DIPs); `None` otherwise.
    pub fn new_size(&self) -> Option<(f32, f32)> {
        let mut w = 0.0f32;
        let mut h = 0.0f32;
        // SAFETY: opaque handle; accessor validates the kind and writes on match.
        let ok = unsafe { noesis_size_changed_args_new_size(self.raw, &mut w, &mut h) };
        ok.then_some((w, h))
    }

    /// Borrowed raw pointer to the event's originating element
    /// (`RoutedEventArgs::source`). `None` if there is no source.
    ///
    /// The pointer is NOT reference-counted and is valid only for the callback
    /// duration — do not wrap it in a [`FrameworkElement`] (that would
    /// over-release) and do not let it escape the handler.
    pub fn source_ptr(&self) -> Option<*mut c_void> {
        // SAFETY: opaque handle; returns a borrowed pointer or null.
        let p = unsafe { noesis_routed_args_source(self.raw) };
        (!p.is_null()).then_some(p)
    }

    /// Borrowed pointer to the element that previously had focus
    /// (`KeyboardFocusChangedEventArgs::oldFocus`), for the `GotKeyboardFocus` /
    /// `LostKeyboardFocus` events (and their `Preview*` variants). `None` for
    /// other event kinds, or when there was no previously-focused element.
    ///
    /// Not reference-counted; valid only for the callback duration (same
    /// contract as [`source_ptr`](Self::source_ptr)).
    pub fn focus_old_ptr(&self) -> Option<*mut c_void> {
        // SAFETY: opaque handle; returns a borrowed pointer or null.
        let p = unsafe { noesis_routed_events_focus_old(self.raw) };
        (!p.is_null()).then_some(p)
    }

    /// Borrowed pointer to the element focus moved to
    /// (`KeyboardFocusChangedEventArgs::newFocus`), for the keyboard-focus
    /// events. `None` for other kinds / when there is no new focus. Not
    /// reference-counted; valid only for the callback duration.
    pub fn focus_new_ptr(&self) -> Option<*mut c_void> {
        // SAFETY: opaque handle; returns a borrowed pointer or null.
        let p = unsafe { noesis_routed_events_focus_new(self.raw) };
        (!p.is_null()).then_some(p)
    }

    /// Drag effect / allowed-effect / key-state bitsets for a drag event
    /// (`DragEnter` / `DragOver` / `DragLeave` / `Drop` and their `Preview*`
    /// variants). `None` for non-drag events. See [`DragEffects`] /
    /// [`DragKeyStates`].
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

    /// Set the drop result (`DragEventArgs::effects`) a `Drop` / `DragOver`
    /// handler reports back to the drag source. Returns `true` if written (i.e.
    /// the live args are a drag event).
    #[must_use = "a false return means the effect was not set because the live args are not a drag event"]
    pub fn set_drag_effects(&self, effects: DragEffects) -> bool {
        // SAFETY: opaque handle; accessor validates the kind before writing.
        unsafe { noesis_routed_events_drag_set_effects(self.raw, effects.bits()) }
    }

    /// Borrowed pointer to the dragged data object (`DragEventArgs::data`).
    /// `None` for non-drag events or when no data is carried. Not
    /// reference-counted; valid only for the callback duration.
    pub fn drag_data_ptr(&self) -> Option<*mut c_void> {
        // SAFETY: opaque handle; returns a borrowed pointer or null.
        let p = unsafe { noesis_routed_events_drag_data(self.raw) };
        (!p.is_null()).then_some(p)
    }

    /// Drop point in `relative_to`'s coordinate space
    /// (`DragEventArgs::GetPosition`). `None` for non-drag events. `relative_to`
    /// must be a live element.
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

    /// Manipulation origin point (`manipulationOrigin`), present on the
    /// `ManipulationStarted` / `Delta` / `Completed` / `InertiaStarting`
    /// events. `None` for other kinds.
    pub fn manip_origin(&self) -> Option<(f32, f32)> {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: opaque handle; accessor validates the kind and writes on match.
        let ok = unsafe { noesis_routed_events_manip_origin(self.raw, &mut x, &mut y) };
        ok.then_some((x, y))
    }

    /// The most-recent manipulation transform — `deltaManipulation` on a
    /// `ManipulationDelta` event, `totalManipulation` on a
    /// `ManipulationCompleted` event. `None` for other kinds.
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

    /// The cumulative manipulation transform (`cumulativeManipulation`) on a
    /// `ManipulationDelta` event. `None` for other kinds.
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

    /// Manipulation velocities — `velocities` (Delta), `finalVelocities`
    /// (Completed) or `initialVelocities` (`InertiaStarting`). `None` for other
    /// kinds.
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

    /// Whether a `ManipulationDelta` / `ManipulationCompleted` event occurred
    /// during the inertia phase (`isInertial`). `None` for other kinds.
    pub fn manip_is_inertial(&self) -> Option<bool> {
        // SAFETY: opaque handle; accessor returns -1 unless it's a delta/completed event.
        match unsafe { noesis_routed_events_manip_is_inertial(self.raw) } {
            0 => Some(false),
            1 => Some(true),
            _ => None,
        }
    }
}

/// A typed bitset of `Noesis::DragDropEffects` (`DragEventArgs` effects /
/// allowed-effects) — the operations a drag offers or reports. Compose with
/// [`Self::with`] / [`FromIterator`] and test with [`Self::contains`]; convert
/// to/from the raw bitmask Noesis uses with [`Self::bits`] / [`Self::from_bits`].
/// Modeled on [`crate::input::ModifierKeys`] / [`crate::view::RenderFlags`].
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

    /// Whether every bit of `other` is present (with [`Self::NONE`], always
    /// `true`).
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

/// A typed bitset of `Noesis::DragDropKeyStates` (`DragEventArgs::keyStates`) —
/// the modifier-key / mouse-button state during a drag. Compose and test like
/// [`DragEffects`].
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

    /// Whether every bit of `other` is present (with [`Self::NONE`], always
    /// `true`).
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

/// Drag bitmask snapshot read from a [`DragEventArgs`](EventArgs::drag).
/// `effects` is the current/result effect, `allowed_effects` the operations the
/// source permits, `key_states` the modifier/button state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DragInfo {
    pub effects: DragEffects,
    pub allowed_effects: DragEffects,
    pub key_states: DragKeyStates,
}

/// Accumulated manipulation transform (`Noesis::ManipulationDelta`). Translation
/// in pixels, `scale` as a multiplier, `rotation` in degrees, `expansion` in
/// pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ManipulationDelta {
    pub translation: (f32, f32),
    pub scale: f32,
    pub rotation: f32,
    pub expansion: (f32, f32),
}

/// Manipulation velocities (`Noesis::ManipulationVelocities`). `angular` in
/// degrees/ms, `linear` and `expansion` in pixels/ms.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ManipulationVelocities {
    pub angular: f32,
    pub linear: (f32, f32),
    pub expansion: (f32, f32),
}

/// Rust-side handler for the generic routed-event path. Receives a borrowed
/// [`EventArgs`] and returns `true` to mark the routed event handled (stops
/// same-element handlers that opted out of `handled_too`, plus cross-element
/// bubbling/tunneling).
///
/// The `Send + 'static` bounds let the handler live inside a Bevy `Resource`
/// or be moved onto the render thread.
/// Takes `&self` (re-entrant: a handler may re-raise the same event via
/// [`crate::reflection::raise_event`], re-entering this box; use interior
/// mutability for handler state).
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

/// RAII subscription token for [`subscribe_event`]. Drop to unsubscribe and
/// free the boxed handler. Mirrors [`ClickSubscription`] / [`KeyDownSubscription`].
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct EventSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn RoutedEventHandler>>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for EventSubscription {}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe_event;
        // freed exactly once here.
        unsafe {
            noesis_unsubscribe_event(self.token.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// A documented `Noesis::RoutedEvent` accepted by [`subscribe_event`]. Each
/// variant maps to a curated entry in the C++ event table (`noesis_events.cpp`),
/// so the typed accessors on [`EventArgs`] know which concrete arg struct fired.
/// Use [`subscribe_event_by_name`] for arbitrary/custom events not listed here.
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
    /// The WPF/Noesis event name this variant maps to (the string the C++ event
    /// table keys on).
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

/// Subscribe `handler` to the typed routed `event` on `element`.
///
/// `handled_too`: when `false`, the handler is skipped if a prior handler on
/// the same element already marked the event handled. (This SDK's `AddHandler`
/// has no `handledEventsToo` parameter, so already-handled events are never
/// re-routed across elements regardless; the flag governs the per-element
/// handler chain.)
///
/// Returns `None` if `element` is not a `UIElement` or the C++ subscription
/// fails. The returned [`EventSubscription`] keeps the handler installed until
/// dropped. For arbitrary/custom events use [`subscribe_event_by_name`].
///
/// # Panics
///
/// Panics only on internal logic errors — specifically if `Box::into_raw`
/// returns null (it cannot, but the wrapper is `NonNull` to keep the invariant
/// explicit at the type level).
pub fn subscribe_event<H: RoutedEventHandler>(
    element: &FrameworkElement,
    event: RoutedEvent,
    handled_too: bool,
    handler: H,
) -> Option<EventSubscription> {
    subscribe_event_by_name(element, event.as_str(), handled_too, handler)
}

/// Subscribe `handler` to the routed event named `event_name` on `element` — the
/// `&str` escape hatch behind the typed [`subscribe_event`], for custom or
/// not-yet-enumerated events.
///
/// `event_name` uses the WPF/Noesis event names — `"MouseMove"`,
/// `"MouseLeftButtonDown"`, `"MouseWheel"`, `"KeyDown"`, `"KeyUp"`,
/// `"GotFocus"`, `"LostFocus"`, `"Loaded"`, `"Unloaded"`, `"SizeChanged"`,
/// `"TextInput"`, `"Drop"`, `"Tapped"`, and the `Preview*` variants, among
/// others. Unknown-but-reflected names fall back to the SDK's `FindRoutedEvent`
/// lookup (only [`EventArgs::source_ptr`] applies to those).
///
/// `handled_too`: when `false`, the handler is skipped if a prior handler on
/// the same element already marked the event handled. (This SDK's `AddHandler`
/// has no `handledEventsToo` parameter, so already-handled events are never
/// re-routed across elements regardless; the flag governs the per-element
/// handler chain.)
///
/// Returns `None` if `element` is not a `UIElement`, `event_name` is unknown
/// or contains an interior NUL, or the C++ subscription fails. The returned
/// [`EventSubscription`] keeps the handler installed until dropped.
///
/// # Panics
///
/// Panics only on internal logic errors — specifically if `Box::into_raw`
/// returns null (it cannot, but the wrapper is `NonNull` to keep the invariant
/// explicit at the type level).
pub fn subscribe_event_by_name<H: RoutedEventHandler>(
    element: &FrameworkElement,
    event_name: &str,
    handled_too: bool,
    handler: H,
) -> Option<EventSubscription> {
    let cname = CString::new(event_name).ok()?;

    let outer: Box<Box<dyn RoutedEventHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked; the
    // element + name pointers are borrowed for the call duration only.
    let token = unsafe {
        noesis_subscribe_event(
            element.raw(),
            cname.as_ptr(),
            handled_too,
            event_trampoline,
            userdata.cast(),
        )
    };

    if let Some(token) = NonNull::new(token) {
        Some(EventSubscription {
            token,
            userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
        })
    } else {
        // Subscription failed (unknown event / not a UIElement). Free the
        // userdata we leaked above so we don't leak the handler.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

// `Initialized`, `LayoutUpdated`, `DataContextChanged` and the `Is*Changed`
// notifications are NOT routed events — they ride Noesis's `Event_<T>`
// mechanism (`AddEventHandler(Symbol, EventHandler)`), so they go through a
// separate name-keyed entrypoint rather than the routed `subscribe_event` path.
// They carry no arguments we surface, so the handler is a bare `Fn()`.

/// Rust-side handler for a non-routed lifecycle event. These notifications
/// carry no arguments we surface, so the callback takes none.
///
/// The `Send + 'static` bounds let the handler live inside a Bevy `Resource`
/// or be moved onto the render thread.
/// Takes `&self` (re-entrant: a lifecycle handler that re-parents its element
/// can trigger another lifecycle event synchronously on the same box; use
/// interior mutability for handler state).
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

/// RAII subscription token for [`subscribe_lifecycle`]. Drop to unsubscribe and
/// free the boxed handler. Mirrors [`ClickSubscription`].
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct LifecycleSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn LifecycleHandler>>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for LifecycleSubscription {}

impl Drop for LifecycleSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe_lifecycle;
        // freed exactly once here.
        unsafe {
            noesis_unsubscribe_lifecycle(self.token.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// A documented non-routed lifecycle event accepted by [`subscribe_lifecycle`].
/// Each variant maps to an entry in the C++ `ApplyLifecycle` table
/// (`noesis_events.cpp`). Use [`subscribe_lifecycle_by_name`] for any name not
/// enumerated here.
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
    /// The event name this variant maps to (the string the C++ lifecycle table
    /// keys on).
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

/// Subscribe `handler` to the typed non-routed lifecycle `event` on `element`.
///
/// Returns `None` if `element` is not a `FrameworkElement` or the C++
/// subscription fails. The returned [`LifecycleSubscription`] keeps the handler
/// installed until dropped; it holds a `+1` ref on the element so the
/// subscription survives the caller dropping every other handle. For any name
/// not enumerated by [`LifecycleEvent`] use [`subscribe_lifecycle_by_name`].
///
/// # Panics
///
/// Panics only on internal logic errors — specifically if `Box::into_raw`
/// returns null (it cannot, but the wrapper is `NonNull` to keep the invariant
/// explicit at the type level).
pub fn subscribe_lifecycle<H: LifecycleHandler>(
    element: &FrameworkElement,
    event: LifecycleEvent,
    handler: H,
) -> Option<LifecycleSubscription> {
    subscribe_lifecycle_by_name(element, event.as_str(), handler)
}

/// Subscribe `handler` to the non-routed lifecycle event named `name` on
/// `element` — the `&str` escape hatch behind the typed [`subscribe_lifecycle`].
///
/// Supported names: `"Initialized"`, `"LayoutUpdated"`, `"DataContextChanged"`,
/// `"IsEnabledChanged"`, `"IsVisibleChanged"`, `"IsHitTestVisibleChanged"`,
/// `"IsKeyboardFocusedChanged"`, `"IsKeyboardFocusWithinChanged"`,
/// `"IsMouseCapturedChanged"`, `"IsMouseCaptureWithinChanged"`,
/// `"IsMouseDirectlyOverChanged"`, `"FocusableChanged"`.
///
/// Returns `None` if `element` is not a `FrameworkElement`, `name` is unknown
/// or contains an interior NUL, or the C++ subscription fails. The returned
/// [`LifecycleSubscription`] keeps the handler installed until dropped; it holds
/// a `+1` ref on the element so the subscription survives the caller dropping
/// every other handle.
///
/// # Panics
///
/// Panics only on internal logic errors — specifically if `Box::into_raw`
/// returns null (it cannot, but the wrapper is `NonNull` to keep the invariant
/// explicit at the type level).
pub fn subscribe_lifecycle_by_name<H: LifecycleHandler>(
    element: &FrameworkElement,
    name: &str,
    handler: H,
) -> Option<LifecycleSubscription> {
    let cname = CString::new(name).ok()?;

    let outer: Box<Box<dyn LifecycleHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked; the
    // element + name pointers are borrowed for the call duration only.
    let token = unsafe {
        noesis_subscribe_lifecycle(
            element.raw(),
            cname.as_ptr(),
            lifecycle_trampoline,
            userdata.cast(),
        )
    };

    if let Some(token) = NonNull::new(token) {
        Some(LifecycleSubscription {
            token,
            userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
        })
    } else {
        // Subscription failed (unknown name / not a FrameworkElement). Free the
        // userdata we leaked above so we don't leak the handler.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}

/// Initiate a drag-and-drop operation from `source`, carrying `data` as the
/// drag payload and advertising `allowed_effects` (a [`DragEffects`] set).
///
/// Wraps `Noesis::DragDrop::DoDragDrop`. The drag is subsequently driven by the
/// host's pointer/drag input; there is no synchronous result and no headless
/// completion. `data` may be any element used as
/// the transferred payload (this SDK exposes no `DataObject` *builder*, so an
/// element stands in for the data object).
///
/// Returns `false` if `source` is not a `DependencyObject` (it always is for a
/// `FrameworkElement`, so this is effectively infallible for live elements).
pub fn do_drag_drop(
    source: &FrameworkElement,
    data: &FrameworkElement,
    allowed_effects: DragEffects,
) -> bool {
    // SAFETY: both pointers are borrowed live elements; DoDragDrop copies what
    // it needs and does not retain the raw pointers past the call we make here.
    unsafe { noesis_routed_events_do_drag_drop(source.raw(), data.raw(), allowed_effects.bits()) }
}

/// Rust-side handler for the `DataObject.Copying` / `.Pasting` attached events.
/// Receives a borrowed pointer to the clipboard data object (`None` when none
/// is carried), whether the operation originates from a drag-drop, and returns
/// `true` to cancel the copy/paste.
///
/// The `Send + 'static` bounds let the handler live inside a Bevy `Resource`
/// or be moved onto the render thread.
pub trait DataObjectHandler: Send + 'static {
    /// Called when the copy/paste fires. `data_object` is borrowed (valid only
    /// for the call); `is_drag_drop` distinguishes a drag-drop transfer from a
    /// clipboard one. Return `true` to cancel.
    ///
    /// Takes `&self` (re-entrant per [`ClickHandler`]; use interior mutability
    /// for handler state).
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

/// RAII subscription token for a `DataObject.Copying` / `.Pasting` handler.
/// Drop to detach the handler and free the boxed closure. Mirrors
/// [`EventSubscription`]; holds a `+1` ref on the element.
#[must_use = "dropping the subscription immediately unsubscribes the handler"]
pub struct DataObjectSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn DataObjectHandler>>,
}

// SAFETY: Send-only (NOT Sync); see the crate-level "Thread affinity" docs.
unsafe impl Send for DataObjectSubscription {}

impl Drop for DataObjectSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by a subscribe call; freed
        // exactly once here.
        unsafe {
            noesis_routed_events_remove_data_object_handler(self.token.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Which `DataObject` attached event a [`subscribe_data_object`] call targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum DataObjectEvent {
    /// `DataObject.Copying` — raised before data is placed on the clipboard
    /// (e.g. by `Ctrl+C` in a `TextBox`).
    Copying,
    /// `DataObject.Pasting` — raised before clipboard data is consumed (e.g. by
    /// `Ctrl+V`).
    Pasting,
}

/// Attach `handler` to the `DataObject.Copying` or `.Pasting` attached event on
/// `element`. Returns `None` if `element` is not a `UIElement` or the C++
/// subscription fails. The returned [`DataObjectSubscription`] keeps the handler
/// installed until dropped.
///
/// # Panics
///
/// Panics only on internal logic errors — specifically if `Box::into_raw`
/// returns null (it cannot, but the wrapper is `NonNull` to keep the invariant
/// explicit at the type level).
pub fn subscribe_data_object<H: DataObjectHandler>(
    element: &FrameworkElement,
    event: DataObjectEvent,
    handler: H,
) -> Option<DataObjectSubscription> {
    let outer: Box<Box<dyn DataObjectHandler>> = Box::new(Box::new(handler));
    let userdata = Box::into_raw(outer);

    // SAFETY: trampoline is `extern "C"`; userdata is freshly leaked; the
    // element pointer is borrowed for the call duration only.
    let token = unsafe {
        match event {
            DataObjectEvent::Copying => noesis_routed_events_add_copying_handler(
                element.raw(),
                data_object_trampoline,
                userdata.cast(),
            ),
            DataObjectEvent::Pasting => noesis_routed_events_add_pasting_handler(
                element.raw(),
                data_object_trampoline,
                userdata.cast(),
            ),
        }
    };

    if let Some(token) = NonNull::new(token) {
        Some(DataObjectSubscription {
            token,
            userdata: NonNull::new(userdata).expect("Box::into_raw returned null"),
        })
    } else {
        // Subscription failed (not a UIElement). Free the userdata we leaked.
        // SAFETY: userdata came from Box::into_raw moments ago; nothing else
        // ever saw the pointer.
        unsafe { drop(Box::from_raw(userdata)) };
        None
    }
}
