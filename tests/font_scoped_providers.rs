//! Routing test for scoped font providers: verifies each of the four setters
//! (`SetSchemeFontProvider` / `SetAssemblyFontProvider` / `SetSchemeAssemblyFontProvider`)
//! routes to a distinct Noesis call — a compile-only test can't catch a wiring swap.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p noesis_runtime --test font_scoped_providers -- --nocapture`

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use noesis_runtime::font_provider::{
    FontProvider, set_assembly_font_provider, set_font_provider, set_scheme_assembly_font_provider,
    set_scheme_font_provider,
};
use noesis_runtime::view::{FrameworkElement, View};

// One TextBlock per scope. The FontFamily prefix selects the provider:
//   * myassets:///Fonts/#Bitter                              → scheme
//   * pack://application:,,,/App;component/Fonts/#Bitter     → assembly
//   * packs://application:,,,/Skin;component/Fonts/#Bitter   → scheme+assembly
//   * Fonts/#Bitter                                          → global
const FONT_XAML: &str = r##"<StackPanel xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
            xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <TextBlock x:Name="TbScheme"   Text="AAA" FontFamily="myassets:///Fonts/#Bitter"/>
  <TextBlock x:Name="TbAssembly" Text="BBB" FontFamily="pack://application:,,,/App;component/Fonts/#Bitter"/>
  <TextBlock x:Name="TbBoth"     Text="CCC" FontFamily="packs://application:,,,/Skin;component/Fonts/#Bitter"/>
  <TextBlock x:Name="TbGlobal"   Text="DDD" FontFamily="Fonts/#Bitter"/>
</StackPanel>"##;

// Logs (label, op, folder_uri) triples for scan_folder/open_font calls.
struct FontRecorder {
    label: &'static str,
    bytes: Vec<u8>,
    // keeps opened bytes alive across the &[u8] borrow
    current: Option<Vec<u8>>,
    log: Arc<Mutex<Vec<(&'static str, &'static str, String)>>>,
}
impl FontProvider for FontRecorder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn scan_folder(&mut self, folder_uri: &str, register: &mut dyn FnMut(&str)) {
        self.log
            .lock()
            .unwrap()
            .push((self.label, "scan", folder_uri.to_string()));
        register("Bitter-Regular.ttf");
    }
    fn open_font(&mut self, folder_uri: &str, _filename: &str) -> Option<&[u8]> {
        self.log
            .lock()
            .unwrap()
            .push((self.label, "open", folder_uri.to_string()));
        self.current = Some(self.bytes.clone());
        self.current.as_deref()
    }
}

#[test]
fn font_scoped_providers_route_by_scheme_and_assembly() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let sdk_dir = std::env::var("NOESIS_SDK_DIR")
            .expect("NOESIS_SDK_DIR not set; required for this test");
        let mut bitter = PathBuf::from(sdk_dir);
        bitter.push("Data/Fonts/Bitter-Regular.ttf");
        let bytes =
            std::fs::read(&bitter).unwrap_or_else(|_| panic!("read failed: {}", bitter.display()));

        let log: Arc<Mutex<Vec<(&'static str, &'static str, String)>>> = Arc::default();
        let make = |label: &'static str| FontRecorder {
            label,
            bytes: bytes.clone(),
            current: None,
            log: Arc::clone(&log),
        };

        let _global = set_font_provider(make("global"));
        let _scheme = set_scheme_font_provider("myassets", make("scheme"));
        let _assembly = set_assembly_font_provider("App", make("assembly"));
        let _both = set_scheme_assembly_font_provider("packs", "Skin", make("both"));

        // ScanFolder/OpenFont fire during text measure to resolve #Bitter.
        let root = FrameworkElement::parse(FONT_XAML).expect("parse TextBlock stack");
        let mut view = View::create(root);
        view.set_size(640, 480);
        view.update(0.0);
        view.update(0.016);

        let entries = log.lock().unwrap().clone();
        assert!(
            !entries.is_empty(),
            "no font provider was consulted during the measure pass; \
             ScanFolder/OpenFont never fired — log = {entries:?}"
        );

        assert!(
            entries
                .iter()
                .any(|(l, _, f)| *l == "scheme" && f.contains("myassets")),
            "scheme provider was not asked for the myassets font folder; log = {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(l, _, f)| *l == "assembly" && f.contains("App")),
            "assembly provider was not asked for the App font folder; log = {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(l, _, f)| *l == "both" && f.contains("Skin")),
            "scheme+assembly provider was not asked for the Skin font folder; log = {entries:?}"
        );
        // The global provider gets the unscoped "Fonts" folder (no scheme/asm
        // token). Identify it by label + the absence of any scoped token.
        assert!(
            entries
                .iter()
                .any(|(l, _, f)| *l == "global" && f == "Fonts" && !f.contains("myassets")),
            "global provider was not asked for the unscoped Fonts folder; log = {entries:?}"
        );

        // "App" (capital) appears only in the assembly URI — "application"
        // (lowercase) in the pack/packs URIs does NOT match.
        assert!(
            !entries.iter().any(|(l, _, f)| *l == "global"
                && (f.contains("myassets") || f.contains("App") || f.contains("Skin"))),
            "global provider was consulted for a scoped font folder; log = {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|(l, _, f)| *l == "scheme" && (f.contains("App") || f.contains("Skin"))),
            "scheme provider saw an out-of-scope font folder; log = {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|(l, _, f)| *l == "assembly" && (f.contains("Skin") || f.contains("myassets"))),
            "assembly provider saw an out-of-scope font folder; log = {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|(l, _, f)| *l == "both" && (f.contains("App") || f.contains("myassets"))),
            "scheme+assembly provider saw an out-of-scope font folder; log = {entries:?}"
        );

        drop(view);
    }

    noesis_runtime::shutdown();
}
