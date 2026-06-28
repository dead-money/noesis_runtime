//! Keyboard state, modifier keys, and `KeyboardNavigation` attached properties — end-to-end across the FFI.

use std::collections::HashMap;

use noesis_runtime::input::{KeyStates, KeyboardNavigation, KeyboardNavigationMode, ModifierKeys};
use noesis_runtime::view::{FrameworkElement, Key, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <TextBox x:Name="Edit" Width="160" Height="30"
           HorizontalAlignment="Center" VerticalAlignment="Center"/>
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

// One test per file: Noesis initialises once per process; both behaviours run sequentially.
#[test]
fn keyboard_state_modifiers_and_navigation() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE.as_bytes().to_vec());
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let root = FrameworkElement::load("scene.xaml").expect("load scene");
        let mut edit = root.find_name("Edit").expect("find Edit");

        let mut view = View::create(root);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0), "first update builds tree");

        // Focus the TextBox so key events route to it and the Keyboard's
        // focused-element wiring is live.
        assert!(edit.focus(), "TextBox accepts focus");
        let _ = view.update(0.016);
        assert!(edit.is_keyboard_focused(), "TextBox keyboard-focused");

        assert!(edit.is_key_up(Key::A), "A is up before pressing");
        assert!(!edit.is_key_down(Key::A));

        let _ = view.key_down(Key::A);
        let _ = view.update(0.032);
        assert!(edit.is_key_down(Key::A), "Keyboard::IsKeyDown(A) is true");
        let states = edit.key_states(Key::A).expect("key_states for A");
        assert!(
            states.contains(KeyStates::DOWN),
            "GetKeyStates(A) contains Down (raw bits {})",
            states.bits()
        );

        let _ = view.key_up(Key::A);
        let _ = view.update(0.048);
        assert!(edit.is_key_up(Key::A), "Keyboard::IsKeyUp(A) after release");
        assert!(!edit.is_key_down(Key::A), "no longer down");

        let before = edit.modifiers().expect("modifiers readable");
        assert!(
            !before.contains(ModifierKeys::SHIFT),
            "Shift not held initially"
        );
        let _ = view.key_down(Key::LeftShift);
        let _ = view.update(0.064);
        let held = edit.modifiers().expect("modifiers readable");
        assert!(
            held.contains(ModifierKeys::SHIFT),
            "GetModifiers() contains Shift while held (raw bits {})",
            held.bits()
        );
        let _ = view.key_up(Key::LeftShift);
        let _ = view.update(0.08);
        assert!(
            !edit
                .modifiers()
                .expect("modifiers")
                .contains(ModifierKeys::SHIFT),
            "Shift cleared after release"
        );

        assert!(
            !edit.is_key_toggled(Key::CapsLock),
            "CapsLock not toggled before pressing"
        );
        let _ = view.key_down(Key::CapsLock);
        let _ = view.update(0.096);
        assert!(
            edit.is_key_toggled(Key::CapsLock),
            "Keyboard::IsKeyToggled(CapsLock) true after press"
        );
        let caps_states = edit.key_states(Key::CapsLock).expect("CapsLock key_states");
        assert!(
            caps_states.contains(KeyStates::TOGGLED),
            "GetKeyStates(CapsLock) contains Toggled (raw bits {})",
            caps_states.bits()
        );
        let _ = view.key_up(Key::CapsLock);
        let _ = view.update(0.104);

        drop(view);

        let mut el = FrameworkElement::parse(
            r#"<Button xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" Content="X"/>"#,
        )
        .expect("parse button");

        assert!(KeyboardNavigation::set_tab_index(&el, 7));
        assert_eq!(KeyboardNavigation::tab_index(&el), Some(7));
        assert!(KeyboardNavigation::set_tab_index(&el, -3));
        assert_eq!(KeyboardNavigation::tab_index(&el), Some(-3));

        assert!(KeyboardNavigation::set_is_tab_stop(&el, false));
        assert_eq!(KeyboardNavigation::is_tab_stop(&el), Some(false));
        assert!(KeyboardNavigation::set_is_tab_stop(&el, true));
        assert_eq!(KeyboardNavigation::is_tab_stop(&el), Some(true));

        assert!(KeyboardNavigation::set_tab_navigation(
            &el,
            KeyboardNavigationMode::Cycle
        ));
        assert_eq!(
            KeyboardNavigation::tab_navigation(&el),
            Some(KeyboardNavigationMode::Cycle)
        );
        assert!(KeyboardNavigation::set_tab_navigation(
            &el,
            KeyboardNavigationMode::Contained
        ));
        assert_eq!(
            KeyboardNavigation::tab_navigation(&el),
            Some(KeyboardNavigationMode::Contained)
        );

        assert!(KeyboardNavigation::set_control_tab_navigation(
            &el,
            KeyboardNavigationMode::Once
        ));
        assert_eq!(
            KeyboardNavigation::control_tab_navigation(&el),
            Some(KeyboardNavigationMode::Once)
        );

        assert!(KeyboardNavigation::set_directional_navigation(
            &el,
            KeyboardNavigationMode::Local
        ));
        assert_eq!(
            KeyboardNavigation::directional_navigation(&el),
            Some(KeyboardNavigationMode::Local)
        );

        assert!(KeyboardNavigation::set_accepts_return(&el, true));
        assert_eq!(KeyboardNavigation::accepts_return(&el), Some(true));

        // Touch `el` mutably so the `mut` binding isn't flagged.
        let _ = el.focus();

        drop(el);
    }

    noesis_runtime::shutdown();
}
