//! `Registered::register_font` smoke test — verifies that an eagerly
//! registered face round-trips through the C++ `RustFontProvider` and
//! ends up in the underlying `CachedFontProvider`'s cache, by observing
//! that the provider's `open_font` callback fires for the eagerly
//! registered `(folder, filename)` pair (Noesis opens the stream to scan
//! face metadata as part of `RegisterFont`).
//!
//! Without the FFI under test, the only path into the cache is the lazy
//! `ScanFolder` callback — and `ScanFolder` runs at most once per folder,
//! caches its result, so any face missing at that moment is invisible
//! forever. The eager-register path makes scan timing irrelevant.
//!
//! Requires `NOESIS_SDK_DIR` (`Data/Fonts/Bitter-Regular.ttf` is read at
//! test time).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use dm_noesis_runtime::font_provider::{FontProvider, set_font_provider};

/// Records every `(folder, filename)` pair `open_font` was asked for. Faked
/// `scan_folder` returns nothing — so the only way an entry shows up here
/// is if `RegisterFont` was invoked from outside the scan loop.
struct ObservedProvider {
    /// `(folder, filename)` → bytes.
    bytes: std::collections::HashMap<(String, String), Vec<u8>>,
    /// Pairs handed to `open_font`. Shared with the test body so the
    /// assertion can read it after the FFI call returns.
    opens: Arc<Mutex<Vec<(String, String)>>>,
    /// Pairs handed to `scan_folder` — for diagnostics only.
    #[allow(dead_code)]
    scans: Arc<Mutex<Vec<String>>>,
    /// Holds the bytes of the most recently opened file alive across the
    /// borrow `open_font` returns (same pattern as `BevyFontProvider`).
    current: Option<Vec<u8>>,
}

impl FontProvider for ObservedProvider {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn scan_folder(&mut self, folder_uri: &str, _register: &mut dyn FnMut(&str)) {
        // Deliberately register nothing — this is the failure mode the
        // FFI under test exists to work around. The real BevyFontProvider
        // would register every loaded font here; faking an empty scan
        // proves eager-register works *without* a healthy scan_folder.
        self.scans.lock().unwrap().push(folder_uri.to_string());
    }

    fn open_font(&mut self, folder_uri: &str, filename: &str) -> Option<&[u8]> {
        self.opens
            .lock()
            .unwrap()
            .push((folder_uri.to_string(), filename.to_string()));
        let bytes = self
            .bytes
            .get(&(folder_uri.to_string(), filename.to_string()))?
            .clone();
        self.current = Some(bytes);
        self.current.as_deref()
    }
}

#[test]
fn register_font_round_trips_through_open_font() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let sdk_dir =
        std::env::var("NOESIS_SDK_DIR").expect("NOESIS_SDK_DIR not set; required for this test");
    let mut bitter_path = PathBuf::from(sdk_dir);
    bitter_path.push("Data/Fonts/Bitter-Regular.ttf");
    let bitter_bytes = std::fs::read(&bitter_path)
        .unwrap_or_else(|_| panic!("read failed: {}", bitter_path.display()));

    let opens: Arc<Mutex<Vec<(String, String)>>> = Arc::default();
    let scans: Arc<Mutex<Vec<String>>> = Arc::default();

    let mut bytes_map = std::collections::HashMap::new();
    bytes_map.insert(
        ("Fonts".to_string(), "Bitter-Regular.ttf".to_string()),
        bitter_bytes,
    );
    let provider = ObservedProvider {
        bytes: bytes_map,
        opens: Arc::clone(&opens),
        scans: Arc::clone(&scans),
        current: None,
    };

    {
        let registered = set_font_provider(provider);

        // Sanity: nothing has called open_font yet. Noesis hasn't been
        // asked to resolve any font.
        assert!(
            opens.lock().unwrap().is_empty(),
            "open_font fired before any font lookup",
        );

        // Eagerly register Bitter without going through ScanFolder.
        // CachedFontProvider::RegisterFont opens the file synchronously
        // to scan face metadata, so this must trigger an `open_font`
        // call for the same pair.
        registered.register_font("Fonts", "Bitter-Regular.ttf");

        let observed_opens = opens.lock().unwrap().clone();
        assert!(
            observed_opens
                .iter()
                .any(|(f, n)| f == "Fonts" && n == "Bitter-Regular.ttf"),
            "register_font should have triggered open_font(\"Fonts\", \"Bitter-Regular.ttf\"); \
             observed opens = {observed_opens:?}",
        );

        drop(registered);
    }

    dm_noesis_runtime::shutdown();
}
