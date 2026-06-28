//! TODO §16 — mouse capture + focus-state, end-to-end across the FFI.
//!
//! Builds a real `View` over a two-button scene, then drives Noesis-computed
//! state and asserts on it:
//!   * `capture_mouse()` flips `is_mouse_captured()` false→true→false and
//!     `Mouse::GetCaptured` (via `mouse_captured()`) points at the captured
//!     element while held, then null after release.
//!   * `focus()` makes `is_keyboard_focused()` true and `Keyboard::GetFocused`
//!     (via `keyboard_focused()`) point at the focused element.
//!   * `FocusManager` focus-scope set/get round-trips, and `focused_element`
//!     reports the focused button.
//!   * `capture_touch` binds an active touch device (and is refused with none),
//!     and `predict_focus(Down)` resolves to the stacked sibling button.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   cargo test --test `input_capture_focus` -- --nocapture

use std::collections::HashMap;

use dm_noesis_runtime::input::{CaptureMode, FocusManager, FocusNavigationDirection};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <StackPanel x:Name="Root">
    <Button x:Name="One"  Content="One"  Width="120" Height="40"/>
    <Button x:Name="Two"  Content="Two"  Width="120" Height="40"/>
  </StackPanel>
</Grid>"##;

struct InMem {
    bytes: HashMap<String, Vec<u8>>,
}

impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.bytes.get(uri).map(Vec::as_slice)
    }
}

#[test]
fn capture_focus_roundtrip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
        let _registered = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let root = FrameworkElement::load("scene.xaml").expect("load scene");
        // Grab the buttons before handing the root to the View (find_name works
        // on the loaded tree's namescope).
        let mut one = root.find_name("One").expect("find One");
        let mut two = root.find_name("Two").expect("find Two");

        let mut view = View::create(root);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0), "first update builds tree");
        let _ = view.update(0.016);

        // ── Mouse capture ───────────────────────────────────────────────────
        assert!(!one.is_mouse_captured(), "nothing captured initially");
        assert!(one.mouse_captured().is_none(), "Mouse::GetCaptured null");

        assert!(
            one.capture_mouse(),
            "capture_mouse should succeed in a live View"
        );
        let _ = view.update(0.032);
        assert!(one.is_mouse_captured(), "is_mouse_captured flips true");

        // Mouse::GetCaptured must point at exactly the element we captured.
        let captured = one.mouse_captured().expect("a captured element");
        assert_eq!(
            captured.as_ptr(),
            one.raw(),
            "Mouse::GetCaptured points at the capturing element"
        );
        // And the OTHER button is not the captured one.
        assert_ne!(captured.as_ptr(), two.raw());

        one.release_mouse_capture();
        let _ = view.update(0.048);
        assert!(!one.is_mouse_captured(), "release flips it back to false");
        assert!(
            one.mouse_captured().is_none(),
            "nothing captured after release"
        );

        // SubTree capture mode through Mouse::Capture, then release via mode None.
        assert!(
            two.capture_mouse_mode(CaptureMode::SubTree),
            "capture with SubTree mode"
        );
        let _ = view.update(0.064);
        assert!(two.is_mouse_captured(), "two now captured");
        assert_eq!(
            two.mouse_captured().expect("captured").as_ptr(),
            two.raw(),
            "captured element is two"
        );
        assert!(two.capture_mouse_mode(CaptureMode::None) || !two.is_mouse_captured());
        // Belt-and-suspenders explicit release.
        two.release_mouse_capture();
        let _ = view.update(0.08);

        // ── Keyboard focus ──────────────────────────────────────────────────
        assert!(!one.is_keyboard_focused(), "not focused before focus()");
        assert!(one.focus(), "Button accepts focus");
        let _ = view.update(0.096);
        assert!(
            one.is_keyboard_focused(),
            "is_keyboard_focused true after focus()"
        );
        assert!(one.is_focused(), "logical focus too");

        // Keyboard::GetFocused must be exactly `one`.
        let focused = one.keyboard_focused().expect("a focused element");
        assert_eq!(
            focused.as_ptr(),
            one.raw(),
            "Keyboard::GetFocused points at the focused button"
        );

        // Focus-within: the StackPanel parent should report focus within it.
        let panel = two.logical_parent().expect("button has a logical parent");
        assert!(
            panel.is_keyboard_focus_within(),
            "ancestor reports keyboard focus within"
        );

        // Move focus to `two` and confirm the flip.
        assert!(two.focus(), "two accepts focus");
        let _ = view.update(0.112);
        assert!(two.is_keyboard_focused(), "two now keyboard-focused");
        assert!(!one.is_keyboard_focused(), "one lost keyboard focus");

        // ── FocusManager focus scope round-trip ─────────────────────────────
        // Buttons aren't focus scopes by default; mark one and read it back.
        assert!(
            !FocusManager::is_focus_scope(&one),
            "not a scope by default"
        );
        assert!(
            FocusManager::set_is_focus_scope(&one, true),
            "set IsFocusScope"
        );
        assert!(
            FocusManager::is_focus_scope(&one),
            "IsFocusScope round-trips true"
        );
        assert!(FocusManager::set_is_focus_scope(&one, false));
        assert!(!FocusManager::is_focus_scope(&one), "and back to false");

        // Make the StackPanel a focus scope, set its FocusedElement to `one`,
        // and read it back through FocusManager — a full get/set round-trip.
        assert!(
            FocusManager::set_is_focus_scope(&panel, true),
            "panel is a scope"
        );
        assert!(
            FocusManager::set_focused_element(&panel, Some(&one)),
            "set FocusedElement"
        );
        let focused_in_scope =
            FocusManager::focused_element(&panel).expect("a focused element in the scope");
        assert_eq!(
            focused_in_scope.as_ptr(),
            one.raw(),
            "FocusManager.FocusedElement round-trips to `one`"
        );
        // GetFocusScope from inside the scope resolves back to the panel.
        let scope = FocusManager::focus_scope(&one).expect("an enclosing focus scope");
        assert_eq!(
            scope.as_ptr(),
            panel.raw(),
            "GetFocusScope resolves to the marked panel"
        );

        // ── Focus traversal: MoveFocus + focus engagement ───────────────────
        // Re-focus `one`, then Tab-style MoveFocus(Next) should leave it.
        assert!(one.focus());
        let _ = view.update(0.128);
        assert!(one.is_keyboard_focused());
        let moved = one.move_focus(FocusNavigationDirection::Next, true);
        let _ = view.update(0.144);
        // MoveFocus returns whether focus moved; with two tab stops it should.
        assert!(moved, "MoveFocus(Next) reports it moved focus");
        assert!(
            !one.is_keyboard_focused(),
            "focus left `one` after MoveFocus(Next)"
        );

        // focus_engage(false) is the 1-arg Focus(bool) knob — focuses `one`.
        assert!(one.focus_engage(false), "focus_engage focuses the element");
        let _ = view.update(0.16);
        assert!(one.is_keyboard_focused(), "engage path focused `one`");

        // ── Touch capture (UIElement::CaptureTouch) ─────────────────────────
        // CaptureTouch binds an *active* touch device id to the element. With no
        // active touch, an arbitrary id can't be captured — a real negative.
        assert!(
            !one.capture_touch(99),
            "no active touch device #99 to capture"
        );
        // Bring a touch device live over `one`, then capture it. touch_down's
        // own bool is just whether an element handled the press (no Click
        // handler ⇒ false); the device is registered regardless, so the capture
        // must now succeed.
        let _ = view.touch_down(60, 100, 7);
        let _ = view.update(0.176);
        assert!(
            one.capture_touch(7),
            "CaptureTouch(7) succeeds once the touch device is active"
        );
        // Headless note: touch capture is tracked per-TouchDevice, distinct from
        // Mouse capture, so it does NOT surface through GetIsMouseCaptured /
        // Mouse::GetCaptured — those stay clear (the observable, asserted fact).
        let _ = view.update(0.184);
        assert!(
            !one.is_mouse_captured(),
            "touch capture is separate from mouse capture"
        );
        let _ = view.touch_up(60, 100, 7);
        let _ = view.update(0.192);

        // ── PredictFocus (directional) ──────────────────────────────────────
        // `one` is focused; the only sibling below it is `two`, so directional
        // PredictFocus(Down) resolves to `two` without moving focus. (The
        // tab-order directions Next/Previous/First/Last are unsupported and
        // return None — asserted here too.)
        assert!(one.focus());
        let _ = view.update(0.2);
        let predicted = one
            .predict_focus(FocusNavigationDirection::Down)
            .expect("PredictFocus(Down) resolves to a candidate");
        assert_eq!(
            predicted.as_ptr(),
            two.raw(),
            "PredictFocus(Down) points at the button below"
        );
        assert!(
            one.predict_focus(FocusNavigationDirection::Next).is_none(),
            "PredictFocus does not support tab-order Next"
        );

        drop(view);
    }

    dm_noesis_runtime::shutdown();
}
