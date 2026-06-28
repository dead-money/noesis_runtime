//! Text, caret, focus, and `KeyDown` subscription round-trip on `TextBox` and `TextBlock`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use noesis_runtime::events::subscribe_keydown;
use noesis_runtime::view::{FrameworkElement, Key, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="400" Height="200">
  <Grid.RowDefinitions>
    <RowDefinition Height="*"/>
    <RowDefinition Height="Auto"/>
  </Grid.RowDefinitions>
  <TextBlock x:Name="LogText" Grid.Row="0"
             Text="initial log" Foreground="White"
             VerticalAlignment="Top" HorizontalAlignment="Left"/>
  <TextBox x:Name="CommandInput" Grid.Row="1"
           Text="initial input" Margin="10"
           HorizontalAlignment="Stretch" VerticalAlignment="Bottom"/>
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
fn text_keydown_focus_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    let return_count = Arc::new(AtomicU32::new(0));
    let tilde_count = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(400, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");

        let mut log = content
            .find_name("LogText")
            .expect("LogText not found in scene");
        assert_eq!(
            log.text().as_deref(),
            Some("initial log"),
            "TextBlock initial Text mismatch",
        );
        assert!(
            log.set_text("rewritten log\nline two"),
            "set_text(TextBlock) failed"
        );
        assert_eq!(
            log.text().as_deref(),
            Some("rewritten log\nline two"),
            "TextBlock Text didn't update",
        );

        let mut input = content
            .find_name("CommandInput")
            .expect("CommandInput not found in scene");
        assert_eq!(
            input.text().as_deref(),
            Some("initial input"),
            "TextBox initial Text mismatch",
        );
        assert!(input.set_text("typed command"), "set_text(TextBox) failed");
        assert_eq!(
            input.text().as_deref(),
            Some("typed command"),
            "TextBox Text didn't update",
        );

        // set_caret_to_end must short-circuit gracefully on non-TextBox elements.
        assert!(input.set_caret_to_end(), "set_caret_to_end(TextBox) failed");
        assert!(
            !log.set_caret_to_end(),
            "set_caret_to_end(TextBlock) should fail cleanly"
        );

        // Focus the TextBox so KeyDowns route to it; post-Activate the focus is on
        // the View root, not on the TextBox we subscribed against.
        let focused = input.focus();
        assert!(focused, "TextBox refused focus");

        let return_in_handler = Arc::clone(&return_count);
        let tilde_in_handler = Arc::clone(&tilde_count);
        let sub = subscribe_keydown(&input, move |key| match key {
            Key::Return => {
                return_in_handler.fetch_add(1, Ordering::SeqCst);
                true
            }
            Key::OemTilde => {
                tilde_in_handler.fetch_add(1, Ordering::SeqCst);
                false
            }
            _ => false,
        })
        .expect("subscribe_keydown returned None — TextBox not a UIElement?");

        assert!(view.update(0.0), "first Update should report change");

        let _ = view.key_down(Key::Return);
        let _ = view.update(0.016);
        let _ = view.key_down(Key::OemTilde);
        let _ = view.update(0.032);

        drop(sub);
        view.deactivate();
        drop(input);
        drop(log);
        drop(content);
        drop(view);
    }

    noesis_runtime::shutdown();

    assert_eq!(
        return_count.load(Ordering::SeqCst),
        1,
        "Return KeyDown handler should have fired once",
    );
    assert_eq!(
        tilde_count.load(Ordering::SeqCst),
        1,
        "OemTilde KeyDown handler should have fired once",
    );
}
