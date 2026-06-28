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
//! What the cursor hook proves:
//!   - The cursor callback *does* fire headlessly: a live `View` over XAML
//!     whose root sets `Cursor="Hand"` invokes the global cursor callback with
//!     [`CursorType::Hand`] when a synthesised mouse-move lands over the
//!     element. This is a genuine end-to-end firing (Noesis input → `QueryCursor`
//!     → global callback → C++ trampoline → Rust closure).
//!
//! What cannot be driven headlessly:
//!   - The software-keyboard callback fires only when a virtual-keyboard-
//!     enabled element gains focus on a platform that requests the on-screen
//!     keyboard; there is no headless API to synthesise that, so for it we
//!     prove only that registration + unregistration cross the FFI cleanly
//!     (no crash, guard drops). See "Known SDK limitations" in TODO.md.

use std::sync::{Arc, Mutex};

use dm_noesis_runtime::integration::{
    CursorType, get_culture, open_url, play_audio, set_culture, set_cursor_callback,
    set_open_url_callback, set_play_audio_callback, set_software_keyboard_callback,
};
use dm_noesis_runtime::view::{FrameworkElement, View};

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
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    // ── No callback registered: triggers must be safe no-ops ────────────────
    // At fresh state no open-URL / play-audio callback is registered. The
    // triggers must cross the FFI and return without panicking or crashing
    // (the C++ trampoline guards against the null slot).
    open_url("https://example.com/before-any-callback");
    play_audio("none.wav", 0.25);

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

    // ── Cursor: genuine end-to-end firing through a live View ───────────────
    // A headless View over XAML whose root sets `Cursor="Hand"`. Registering
    // the global cursor callback and then synthesising a mouse-move over the
    // element drives Noesis's QueryCursor path, which invokes our callback with
    // the element's cursor. This is a real dispatch proof, not just an FFI
    // crossing.
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

    // ── Software keyboard: registration / unregistration FFI crossing only ──
    // The on-screen-keyboard callback only fires when a virtual-keyboard-
    // enabled element gains focus on a platform that requests it; that path
    // can't be synthesised headlessly. We prove ONLY that registering and
    // dropping the guard (which unregisters) crosses the FFI cleanly — this is
    // not a dispatch test.
    {
        let kbd_hits: Arc<Mutex<Vec<bool>>> = Arc::default();
        let ksink = Arc::clone(&kbd_hits);
        let kbd_guard =
            set_software_keyboard_callback(move |_focused, open| ksink.lock().unwrap().push(open));

        drop(kbd_guard);

        // Nothing can fire headlessly, so the closure must never have run.
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

/// `open_url` documents that it panics on an interior NUL byte. The panic
/// happens in `CString::new(...).expect(...)` *before* any FFI crossing, so
/// this test needs no Noesis init and is safe to run in its own process slot.
#[test]
#[should_panic(expected = "interior NUL")]
fn open_url_interior_nul_panics() {
    open_url("https://example.com/\0evil");
}
