//! Integration test for `FrameworkElement::set_visibility`: flipping an overlay
//! between Visible and Collapsed gates whether clicks reach the element below.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::events::subscribe_click;
use noesis_runtime::view::{FrameworkElement, MouseButton, View};
use noesis_runtime::xaml_provider::XamlProvider;

const VISIBILITY_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Button x:Name="HitButton"
          Content="Below"
          Width="100" Height="40"
          HorizontalAlignment="Center"
          VerticalAlignment="Center"/>
  <!-- Drawn last → on top in z-order. When Visible the Border covers
       the button and intercepts hit-tests; when Collapsed it leaves
       the visual tree entirely so clicks reach the button below. -->
  <Border x:Name="Overlay"
          Background="#80FF0000"
          Visibility="Collapsed"/>
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
fn set_visibility_toggles_overlay_and_blocks_hit_test() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let click_count = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert(
            "scene.xaml".to_string(),
            VISIBILITY_XAML.as_bytes().to_vec(),
        );
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");

        let button = content.find_name("HitButton").expect("find_name HitButton");
        let counter_in_handler = Arc::clone(&click_count);
        let _click_sub = subscribe_click(&button, move || {
            counter_in_handler.fetch_add(1, Ordering::SeqCst);
        })
        .expect("subscribe_click returned None : HitButton not a button?");

        // First update is required before hit-testing works.
        assert!(view.update(0.0), "first Update should report change");

        let _ = view.mouse_move(100, 100);
        let _ = view.update(0.016);
        let _ = view.mouse_button_down(100, 100, MouseButton::Left);
        let _ = view.update(0.032);
        let _ = view.mouse_button_up(100, 100, MouseButton::Left);
        let _ = view.update(0.048);
        assert_eq!(
            click_count.load(Ordering::SeqCst),
            1,
            "with overlay Collapsed, click should reach the button"
        );

        let mut overlay = content.find_name("Overlay").expect("find_name Overlay");
        overlay.set_visibility(true);
        let _ = view.update(0.064);
        let _ = view.mouse_button_down(100, 100, MouseButton::Left);
        let _ = view.update(0.080);
        let _ = view.mouse_button_up(100, 100, MouseButton::Left);
        let _ = view.update(0.096);
        assert_eq!(
            click_count.load(Ordering::SeqCst),
            1,
            "with overlay Visible, click should NOT reach the button"
        );

        overlay.set_visibility(false);
        let _ = view.update(0.112);
        let _ = view.mouse_button_down(100, 100, MouseButton::Left);
        let _ = view.update(0.128);
        let _ = view.mouse_button_up(100, 100, MouseButton::Left);
        let _ = view.update(0.144);
        assert_eq!(
            click_count.load(Ordering::SeqCst),
            2,
            "with overlay Collapsed again, click should reach the button"
        );

        drop(_click_sub);
        drop(overlay);
        view.deactivate();
        drop(view);
    }

    noesis_runtime::shutdown();
}
