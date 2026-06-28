//! Subscribe Rust callbacks to Noesis routed events (Phase 5.B).
//!
//! Exposes [`subscribe_click`] for `BaseButton::Click` and
//! [`subscribe_keydown`] for `UIElement::KeyDown`. The shape generalizes —
//! every routed event is a `Delegate<void(BaseComponent*, const
//! RoutedEventArgs&)>` on the C++ side, and the FFI pattern
//! (heap-allocated handler that owns its registration via RAII, holding a
//! +1 ref on the source element) repeats. Add sibling functions when
//! other events earn the surface.
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
    dm_noesis_key_args_key, dm_noesis_mouse_args_position, dm_noesis_mouse_button_args_button,
    dm_noesis_mouse_wheel_args_delta, dm_noesis_routed_args_source,
    dm_noesis_size_changed_args_new_size, dm_noesis_subscribe_click, dm_noesis_subscribe_event,
    dm_noesis_subscribe_keydown, dm_noesis_text_args_ch, dm_noesis_unsubscribe_click,
    dm_noesis_unsubscribe_event, dm_noesis_unsubscribe_keydown,
};
use crate::view::{FrameworkElement, Key, MouseButton};

/// Rust-side click handler. Implementors receive a single `()` notification
/// per fired click; if you need the sender / event args, extend the FFI
/// before adding a richer trait method here.
///
/// The `Send + 'static` bounds let the handler live inside a Bevy
/// `Resource` or be moved onto the render thread.
pub trait ClickHandler: Send + 'static {
    fn on_click(&mut self);
}

impl<F: FnMut() + Send + 'static> ClickHandler for F {
    fn on_click(&mut self) {
        self();
    }
}

/// SAFETY: `userdata` must be a pointer produced by [`subscribe_click`] and
/// still alive (the [`ClickSubscription`] hasn't been dropped).
unsafe extern "C" fn click_trampoline(userdata: *mut c_void) {
    let handler = &mut *userdata.cast::<Box<dyn ClickHandler>>();
    handler.on_click();
}

/// RAII subscription token. Drop to unsubscribe and free the boxed handler.
///
/// Holds a `+1` ref on the underlying button (managed C++-side); dropping
/// this releases that ref and removes the handler from the routed-event
/// list. Drop before [`crate::shutdown`] like every other owning handle in
/// this crate.
pub struct ClickSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn ClickHandler>>,
}

// SAFETY: matches the Registered guards on the providers — every Box<dyn
// ClickHandler> is `Send`, and the C++ subscription is bound to a single
// button whose access is serialized by Noesis. Sync is safe for the same
// reason: there are no `&self` methods that touch Noesis state.
unsafe impl Send for ClickSubscription {}
unsafe impl Sync for ClickSubscription {}

impl Drop for ClickSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe_click;
        // freed exactly once here.
        unsafe {
            dm_noesis_unsubscribe_click(self.token.as_ptr());
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
    let token =
        unsafe { dm_noesis_subscribe_click(element.raw(), click_trampoline, userdata.cast()) };

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

// ── KeyDown subscription ────────────────────────────────────────────────────

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
    fn on_keydown(&mut self, key: Key) -> bool;
}

impl<F: FnMut(Key) -> bool + Send + 'static> KeyDownHandler for F {
    fn on_keydown(&mut self, key: Key) -> bool {
        self(key)
    }
}

/// SAFETY: `userdata` must be a pointer produced by [`subscribe_keydown`]
/// and still alive (the [`KeyDownSubscription`] hasn't been dropped).
/// `out_handled` must be a non-null pointer to a writable bool (the C++
/// shim guarantees this).
unsafe extern "C" fn keydown_trampoline(userdata: *mut c_void, key: i32, out_handled: *mut bool) {
    let handler = &mut *userdata.cast::<Box<dyn KeyDownHandler>>();
    // Best-effort map of the raw ordinal back to our safe `Key` mirror.
    // Anything outside the mirrored set arrives as `Key::None` — callers
    // can still observe the event and choose to ignore unmapped keys.
    let mapped = key_from_raw(key);
    let handled = handler.on_keydown(mapped);
    if !out_handled.is_null() {
        *out_handled = handled;
    }
}

/// Convert a raw `Noesis::Key` ordinal back into the safe [`Key`] mirror.
/// Unmapped ordinals collapse to [`Key::None`]; the caller's handler can
/// still match on the value but won't be able to distinguish *which*
/// unmapped key fired. Add variants to [`Key`] (and the C++ `static_assert`s
/// in `noesis_view.cpp`) when a missing key earns it.
fn key_from_raw(raw: i32) -> Key {
    // Roundtrip the explicit-discriminant enum through transmute would be
    // sound but brittle (UB if `raw` falls outside the declared variants).
    // A match table is verbose but safe; the compiler folds it into a
    // jump table for the common path. Order mirrors the Key enum's
    // declaration in src/view.rs for ease of audit.
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
pub struct KeyDownSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn KeyDownHandler>>,
}

// SAFETY: matches the Send/Sync rationale on [`ClickSubscription`] —
// every Box<dyn KeyDownHandler> is `Send`, and the C++ subscription is
// bound to a single element whose access is serialised by Noesis.
unsafe impl Send for KeyDownSubscription {}
unsafe impl Sync for KeyDownSubscription {}

impl Drop for KeyDownSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe_keydown;
        // freed exactly once here.
        unsafe {
            dm_noesis_unsubscribe_keydown(self.token.as_ptr());
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
        unsafe { dm_noesis_subscribe_keydown(element.raw(), keydown_trampoline, userdata.cast()) };

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

// ── Generic routed-event subscription (TODO §5) ─────────────────────────────

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
    /// Pointer position in the source element's coordinate space, for mouse,
    /// mouse-button and mouse-wheel events. `None` for other event kinds.
    pub fn position(&self) -> Option<(f32, f32)> {
        let mut x = 0.0f32;
        let mut y = 0.0f32;
        // SAFETY: `raw` is the opaque handle the trampoline received; the
        // accessor validates the arg kind and writes only on a match.
        let ok = unsafe { dm_noesis_mouse_args_position(self.raw, &mut x, &mut y) };
        ok.then_some((x, y))
    }

    /// Changed mouse button for a mouse-button event; `None` otherwise.
    pub fn mouse_button(&self) -> Option<MouseButton> {
        // SAFETY: opaque handle; accessor returns -1 unless it's a button event.
        let raw = unsafe { dm_noesis_mouse_button_args_button(self.raw) };
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
        Some(unsafe { dm_noesis_mouse_wheel_args_delta(self.raw) })
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
        let raw = unsafe { dm_noesis_key_args_key(self.raw) };
        (raw >= 0).then(|| key_from_raw(raw))
    }

    /// Input character (UTF-32 code point) for a `TextInput` event; `None`
    /// otherwise.
    pub fn text_char(&self) -> Option<char> {
        // SAFETY: opaque handle; accessor returns -1 unless it's text input.
        let raw = unsafe { dm_noesis_text_args_ch(self.raw) };
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
        let ok = unsafe { dm_noesis_size_changed_args_new_size(self.raw, &mut w, &mut h) };
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
        let p = unsafe { dm_noesis_routed_args_source(self.raw) };
        (!p.is_null()).then_some(p)
    }
}

/// Rust-side handler for the generic routed-event path. Receives a borrowed
/// [`EventArgs`] and returns `true` to mark the routed event handled (stops
/// same-element handlers that opted out of `handled_too`, plus cross-element
/// bubbling/tunneling).
///
/// The `Send + 'static` bounds let the handler live inside a Bevy `Resource`
/// or be moved onto the render thread.
pub trait RoutedEventHandler: Send + 'static {
    fn on_event(&mut self, args: &EventArgs) -> bool;
}

impl<F: FnMut(&EventArgs) -> bool + Send + 'static> RoutedEventHandler for F {
    fn on_event(&mut self, args: &EventArgs) -> bool {
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
    let handler = &mut *userdata.cast::<Box<dyn RoutedEventHandler>>();
    let ev = EventArgs {
        raw: args,
        _not_send: PhantomData,
    };
    let handled = handler.on_event(&ev);
    if !out_handled.is_null() {
        *out_handled = handled;
    }
}

/// RAII subscription token for [`subscribe_event`]. Drop to unsubscribe and
/// free the boxed handler. Mirrors [`ClickSubscription`] / [`KeyDownSubscription`].
pub struct EventSubscription {
    token: NonNull<c_void>,
    userdata: NonNull<Box<dyn RoutedEventHandler>>,
}

// SAFETY: matches the Send/Sync rationale on [`ClickSubscription`] — every
// Box<dyn RoutedEventHandler> is `Send`, and the C++ subscription is bound to a
// single element whose access is serialised by Noesis.
unsafe impl Send for EventSubscription {}
unsafe impl Sync for EventSubscription {}

impl Drop for EventSubscription {
    fn drop(&mut self) {
        // SAFETY: token + userdata produced together by subscribe_event;
        // freed exactly once here.
        unsafe {
            dm_noesis_unsubscribe_event(self.token.as_ptr());
            drop(Box::from_raw(self.userdata.as_ptr()));
        }
    }
}

/// Subscribe `handler` to the routed event named `event_name` on `element`.
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
pub fn subscribe_event<H: RoutedEventHandler>(
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
        dm_noesis_subscribe_event(
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
