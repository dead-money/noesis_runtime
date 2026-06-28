//! `Registered::register_font` smoke test — verifies eager `RegisterFont`
//! triggers `open_font` without `ScanFolder` (which caches after one call;
//! faces absent at scan time are invisible forever without eager register).
//!
//! Requires `NOESIS_SDK_DIR` (`Data/Fonts/Bitter-Regular.ttf` is read at test time).

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use noesis_runtime::font_provider::{FontProvider, set_font_provider};

// Records open_font pairs; scan_folder returns nothing, so opens only
// accumulate if RegisterFont triggers open_font outside the scan loop.
struct ObservedProvider {
    bytes: std::collections::HashMap<(String, String), Vec<u8>>,
    opens: Arc<Mutex<Vec<(String, String)>>>, // shared with test body
    #[allow(dead_code)]
    scans: Arc<Mutex<Vec<String>>>,
    // keeps the most recently opened bytes alive across the &[u8] borrow
    current: Option<Vec<u8>>,
}

impl FontProvider for ObservedProvider {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }

    fn scan_folder(&mut self, folder_uri: &str, _register: &mut dyn FnMut(&str)) {
        // deliberately empty — proves eager RegisterFont works without a healthy scan_folder
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
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

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

        assert!(
            opens.lock().unwrap().is_empty(),
            "open_font fired before any font lookup",
        );

        // CachedFontProvider::RegisterFont scans face metadata synchronously,
        // so this triggers open_font for the same (folder, filename) pair.
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

    noesis_runtime::shutdown();
}
