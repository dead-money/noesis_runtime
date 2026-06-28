//! Round-trip proofs for the Section 14 system-integration callbacks
//! (`src/integration.rs`).
//!
//! Single `#[test]` per the harness convention (one Noesis init per
//! process); every owning guard drops inside an inner scope before
//! `shutdown()`.
//!
//! What is a genuine end-to-end round trip headlessly:
//!   - `open_url` / `play_audio` invoke the registered Rust closure
//!     *synchronously* through both trampoline layers (Rust → C++ → Noesis
//!     → C++ trampoline → Rust closure). We assert the closure observed the
//!     exact url / uri / volume, and that the values survive the
//!     register/unregister lifecycle.
//!   - `set_culture` / `get_culture` round-trips the BCP-47 name through
//!     Noesis's own `CultureInfo` storage.
//!
//! What cannot be driven headlessly:
//!   - The cursor and software-keyboard callbacks fire from input / focus
//!     handling inside a live view's event pump. There is no headless API to
//!     synthesise that, so we prove only that registration + unregistration
//!     cross the FFI cleanly (no crash, guards drop). See "Known SDK
//!     limitations" in TODO.md.

use std::sync::{Arc, Mutex};

use dm_noesis_runtime::integration::{
    CursorType, get_culture, open_url, play_audio, set_culture, set_cursor_callback,
    set_open_url_callback, set_play_audio_callback, set_software_keyboard_callback,
};

#[test]
fn integration_callbacks_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    // ── OpenUrl: synchronous end-to-end round trip ──────────────────────────
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

        // After the guard drops, the callback is unregistered: open_url must
        // become a no-op (the closure is freed, so it must NOT be called).
        drop(guard);
        open_url("https://example.com/after-drop");
        assert_eq!(
            seen.lock().unwrap().len(),
            2,
            "open_url fired after the guard was dropped (callback not unregistered)",
        );
    }

    // ── PlayAudio: synchronous end-to-end round trip ────────────────────────
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

    // ── Culture: set → get round trip + multiple mutations ──────────────────
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

    // ── Cursor + software keyboard: registration / unregistration only ──────
    // These fire from live input/focus handling, unreachable headlessly. We
    // prove the FFI crossing (register + drop-to-unregister) is sound: the
    // closures capture shared cells so a future live-view test could assert
    // on them, but here we only require no crash through the full lifecycle.
    {
        let cursor_hits: Arc<Mutex<Vec<CursorType>>> = Arc::default();
        let csink = Arc::clone(&cursor_hits);
        let cursor_guard = set_cursor_callback(move |_view, ty| csink.lock().unwrap().push(ty));

        let kbd_hits: Arc<Mutex<Vec<bool>>> = Arc::default();
        let ksink = Arc::clone(&kbd_hits);
        let kbd_guard =
            set_software_keyboard_callback(move |_focused, open| ksink.lock().unwrap().push(open));

        // Re-register (replaces the slot) to exercise the non-null set path
        // a second time, then drop both guards to exercise unregister (null).
        let cursor_guard2 = set_cursor_callback(|_view, _ty| {});
        drop(cursor_guard);
        drop(cursor_guard2);
        drop(kbd_guard);

        // Nothing should have fired headlessly.
        assert!(cursor_hits.lock().unwrap().is_empty());
        assert!(kbd_hits.lock().unwrap().is_empty());
    }

    // ── CursorType::from_raw mapping sanity ─────────────────────────────────
    assert_eq!(CursorType::from_raw(0), CursorType::None);
    assert_eq!(CursorType::from_raw(2), CursorType::Arrow);
    assert_eq!(CursorType::from_raw(14), CursorType::Hand);
    assert_eq!(CursorType::from_raw(28), CursorType::Custom);
    // Out-of-range / Count sentinel maps to None.
    assert_eq!(CursorType::from_raw(29), CursorType::None);
    assert_eq!(CursorType::from_raw(-1), CursorType::None);

    dm_noesis_runtime::shutdown();
}
