//! Integration test for the text-property + keydown-subscription + focus
//! FFI surface added on the `text-keydown-focus-ffi` branch.
//!
//! Loads a XAML with a named `<TextBox>` + a named `<TextBlock>`, exercises
//! every helper:
//!
//! 1. `set_text` / `text()` round-trip on both element types.
//! 2. `set_caret_to_end` on the TextBox.
//! 3. `focus()` on the TextBox.
//! 4. `subscribe_keydown` — callback receives a synthetic `Key::Return`
//!    plus a synthetic `Key::OemTilde`, mirrors AoR's command-input flow.
//!
//! Run with `NOESIS_SDK_DIR` set (and ideally a license, though trial mode
//! is fine for a smoke test):
//!   `cargo test -p dm_noesis_runtime --test text_and_keys -- --nocapture`

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use dm_noesis_runtime::events::subscribe_keydown;
use dm_noesis_runtime::view::{FrameworkElement, Key, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

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
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let return_count = Arc::new(AtomicU32::new(0));
    let tilde_count = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let provider = InMem { bytes };
        let _registered = dm_noesis_runtime::xaml_provider::set_xaml_provider(provider);

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(400, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");

        // ── Text round-trip on the TextBlock ──────────────────────────────
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

        // ── Text round-trip on the TextBox ────────────────────────────────
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

        // ── Caret nudge — only meaningful on TextBox; the TextBlock helper
        //    must short-circuit gracefully.
        assert!(input.set_caret_to_end(), "set_caret_to_end(TextBox) failed");
        assert!(
            !log.set_caret_to_end(),
            "set_caret_to_end(TextBlock) should fail cleanly"
        );

        // ── Focus the input box so subsequent KeyDowns route to it. The
        //    `key_down` event below would otherwise route to the focused
        //    element which, post-Activate, is the View root — not the
        //    TextBox we subscribed against.
        let focused = input.focus();
        assert!(focused, "TextBox refused focus");

        // ── Subscribe to KeyDown. Mark `Return` handled (mirrors AoR's
        //    flow: swallow Enter so the line break doesn't get inserted),
        //    let `OemTilde` propagate.
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

        // Sanity: subscribing on a non-UIElement should fail. Every
        // FrameworkElement we have at hand IS a UIElement (TextBox /
        // TextBlock / Grid all derive), so we can't easily synthesize a
        // negative case here without a ResourceDictionary lookup. Skipped
        // until the markup-extension pipeline lands a non-UIElement
        // reference into FFI reach.

        // First Update builds the initial render tree.
        assert!(view.update(0.0), "first Update should report change");

        // Drive the keys through the View's input pump.
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

    dm_noesis_runtime::shutdown();

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
