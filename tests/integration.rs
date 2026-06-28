//! Round-trip proofs for the system-integration callbacks (`src/integration.rs`).
//!
//! The software-keyboard callback cannot be driven headlessly; only its
//! register/unregister lifecycle is tested. See LIMITATIONS.md.

use std::sync::{Arc, Mutex};

use noesis_runtime::integration::{
    CursorType, get_culture, open_url, play_audio, set_culture, set_cursor_callback,
    set_open_url_callback, set_play_audio_callback, set_software_keyboard_callback,
};
use noesis_runtime::view::{FrameworkElement, View};

/// Root sets a non-default `Cursor` so a mouse-move over it must drive the
/// global cursor callback. `Background` makes the `Grid` hit-testable.
const CURSOR_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200" Cursor="Hand"/>"##;

#[test]
fn integration_callbacks_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    // No callback registered: the C++ trampoline guards against null, so these must be safe no-ops.
    open_url("https://example.com/before-any-callback");
    play_audio("none.wav", 0.25);

    {
        let seen: Arc<Mutex<Vec<String>>> = Arc::default();
        let sink = Arc::clone(&seen);
        let guard = set_open_url_callback(move |url| sink.lock().unwrap().push(url.to_string()));

        open_url("https://www.noesisengine.com/docs/");
        open_url(""); // edge: empty string must cross cleanly

        let observed = seen.lock().unwrap().clone();
        assert_eq!(
            observed,
            vec![
                "https://www.noesisengine.com/docs/".to_string(),
                String::new()
            ],
            "open_url must invoke the registered callback synchronously with the exact url",
        );

        drop(guard);
        open_url("https://example.com/after-drop");
        assert_eq!(
            seen.lock().unwrap().len(),
            2,
            "open_url fired after the guard was dropped (callback not unregistered)",
        );
    }

    {
        let seen: Arc<Mutex<Vec<(String, f32)>>> = Arc::default();
        let sink = Arc::clone(&seen);
        let guard = set_play_audio_callback(move |uri, volume| {
            sink.lock().unwrap().push((uri.to_string(), volume));
        });

        play_audio("click.wav", 0.5);
        play_audio("ambient.ogg", 1.0);

        let observed = seen.lock().unwrap().clone();
        assert_eq!(observed.len(), 2, "play_audio must fire once per call");
        assert_eq!(observed[0].0, "click.wav");
        assert!(
            (observed[0].1 - 0.5).abs() < f32::EPSILON,
            "volume must round-trip"
        );
        assert_eq!(observed[1].0, "ambient.ogg");
        assert!((observed[1].1 - 1.0).abs() < f32::EPSILON);

        drop(guard);
        play_audio("after.wav", 0.1);
        assert_eq!(
            seen.lock().unwrap().len(),
            2,
            "play_audio fired after the guard was dropped",
        );
    }

    {
        // Default before any set is "en-US" (CultureInfo's struct default).
        assert_eq!(get_culture(), "en-US", "default culture should be en-US");

        for name in ["fr-FR", "ja-JP", "de-DE", "en-US"] {
            set_culture(name);
            assert_eq!(
                get_culture(),
                name,
                "culture must round-trip through Noesis storage",
            );
        }
    }

    {
        let cursor_hits: Arc<Mutex<Vec<CursorType>>> = Arc::default();
        let csink = Arc::clone(&cursor_hits);
        let cursor_guard = set_cursor_callback(move |_view, ty| csink.lock().unwrap().push(ty));

        let root = FrameworkElement::parse(CURSOR_XAML).expect("parse cursor XAML");
        let mut view = View::create(root);
        view.set_size(200, 200);
        view.activate();
        // Build the render tree before hit-testing the pointer.
        let _ = view.update(0.0);
        let _ = view.mouse_move(100, 100);
        let _ = view.update(0.016);

        let observed = cursor_hits.lock().unwrap().clone();
        assert!(
            observed.contains(&CursorType::Hand),
            "cursor callback should fire with Hand for a mouse-move over a \
             Cursor=\"Hand\" element; observed {observed:?}",
        );

        view.deactivate();
        drop(view);
        drop(cursor_guard);
    }

    // The software-keyboard callback requires a real platform virtual keyboard; not driveable headlessly.
    {
        let kbd_hits: Arc<Mutex<Vec<bool>>> = Arc::default();
        let ksink = Arc::clone(&kbd_hits);
        let kbd_guard =
            set_software_keyboard_callback(move |_focused, open| ksink.lock().unwrap().push(open));

        drop(kbd_guard);

        assert!(kbd_hits.lock().unwrap().is_empty());
    }

    assert_eq!(CursorType::from_raw(0), CursorType::None);
    assert_eq!(CursorType::from_raw(2), CursorType::Arrow);
    assert_eq!(CursorType::from_raw(14), CursorType::Hand);
    assert_eq!(CursorType::from_raw(28), CursorType::Custom);
    // Out-of-range / Count sentinel maps to None.
    assert_eq!(CursorType::from_raw(29), CursorType::None);
    assert_eq!(CursorType::from_raw(-1), CursorType::None);

    noesis_runtime::shutdown();
}

/// Verifies the documented panic for an interior NUL byte; fires before the FFI so no Noesis init is needed.
#[test]
#[should_panic(expected = "interior NUL")]
fn open_url_interior_nul_panics() {
    open_url("https://example.com/\0evil");
}
