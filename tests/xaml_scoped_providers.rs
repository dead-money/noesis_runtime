//! Integration tests for scheme-, assembly-, and scheme+assembly-scoped XAML
//! providers: verifies each URI is routed to its provider exclusively.

use std::sync::{Arc, Mutex};

use noesis_runtime::view::FrameworkElement;
use noesis_runtime::xaml_provider::{
    XamlProvider, set_assembly_xaml_provider, set_scheme_assembly_xaml_provider,
    set_scheme_xaml_provider, set_xaml_provider,
};

const SCHEME_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Button x:Name="SCHEMED" Content="from scheme provider"/>
</Grid>"##;

const ASSEMBLY_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Button x:Name="ASSEMBLED" Content="from assembly provider"/>
</Grid>"##;

const BOTH_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Button x:Name="SCHEME_ASSEMBLED" Content="from scheme+assembly provider"/>
</Grid>"##;

/// Serves `bytes` for any URI it is asked for, recording each requested
/// `(label, uri)` in the shared log so the test can confirm which provider
/// Noesis routed to.
struct Recorder {
    label: &'static str,
    bytes: Option<Vec<u8>>,
    log: Arc<Mutex<Vec<(&'static str, String)>>>,
}
impl XamlProvider for Recorder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.log.lock().unwrap().push((self.label, uri.to_string()));
        self.bytes.as_deref()
    }
}

#[test]
fn scoped_providers_route_by_scheme_and_assembly() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let log: Arc<Mutex<Vec<(&'static str, String)>>> = Arc::default();

        // Serves nothing; a scope-routing bug would cause scoped URIs to fall
        // back here and fail to load, which is how this catches the breakage.
        let _global = set_xaml_provider(Recorder {
            label: "global",
            bytes: None,
            log: Arc::clone(&log),
        });

        let _scheme = set_scheme_xaml_provider(
            "myassets",
            Recorder {
                label: "scheme",
                bytes: Some(SCHEME_XAML.as_bytes().to_vec()),
                log: Arc::clone(&log),
            },
        );

        let _assembly = set_assembly_xaml_provider(
            "App",
            Recorder {
                label: "assembly",
                bytes: Some(ASSEMBLY_XAML.as_bytes().to_vec()),
                log: Arc::clone(&log),
            },
        );

        // Routes through the third distinct Noesis call (SetSchemeAssemblyXamlProvider),
        // so a mis-wired setter the other two providers would not catch is caught here.
        let _both = set_scheme_assembly_xaml_provider(
            "packs",
            "Skin",
            Recorder {
                label: "both",
                bytes: Some(BOTH_XAML.as_bytes().to_vec()),
                log: Arc::clone(&log),
            },
        );

        let schemed = FrameworkElement::load("myassets:///main.xaml")
            .expect("scheme load returned None — scheme provider not consulted");
        assert!(
            schemed.find_name("SCHEMED").is_some(),
            "named child from the scheme-served XAML not reachable; routing failed"
        );
        drop(schemed);

        let assembled = FrameworkElement::load("pack://application:,,,/App;component/main.xaml")
            .expect("assembly load returned None — assembly provider not consulted");
        assert!(
            assembled.find_name("ASSEMBLED").is_some(),
            "named child from the assembly-served XAML not reachable; routing failed"
        );
        drop(assembled);

        let both = FrameworkElement::load("packs://application:,,,/Skin;component/main.xaml")
            .expect("scheme+assembly load returned None — combined provider not consulted");
        assert!(
            both.find_name("SCHEME_ASSEMBLED").is_some(),
            "named child from the scheme+assembly-served XAML not reachable; routing failed"
        );
        drop(both);

        // The global provider serves nothing, so this fails — but the attempt
        // must be recorded against the global provider.
        assert!(
            FrameworkElement::load("plain.xaml").is_none(),
            "global provider serves nothing, so plain.xaml must not load"
        );

        let entries = log.lock().unwrap().clone();

        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "scheme" && u.contains("myassets") && u.contains("main.xaml")),
            "scheme provider was not asked for the myassets URI; log = {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "assembly" && u.contains("App") && u.contains("main.xaml")),
            "assembly provider was not asked for the App pack URI; log = {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "both" && u.contains("Skin") && u.contains("main.xaml")),
            "scheme+assembly provider was not asked for the packs/Skin URI; log = {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "global" && u == "plain.xaml"),
            "global provider was not asked for the unscoped URI; log = {entries:?}"
        );

        // Scope routing is exclusive: the global provider must not be consulted
        // for any scoped URI.
        assert!(
            !entries.iter().any(|(l, u)| *l == "global"
                && (u.contains("myassets") || u.contains("App") || u.contains("Skin"))),
            "global provider was consulted for a scoped URI — scope routing broke; \
             log = {entries:?}"
        );
        // "App" is capitalized so the lowercase "application" in pack URIs does
        // not accidentally match the assembly-provider filter.
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "scheme" && (u.contains("App") || u.contains("Skin"))),
            "scheme provider saw an out-of-scope URI; log = {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "assembly" && (u.contains("myassets") || u.contains("Skin"))),
            "assembly provider saw an out-of-scope URI; log = {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "both" && (u.contains("myassets") || u.contains("App"))),
            "scheme+assembly provider saw an out-of-scope URI; log = {entries:?}"
        );
    }

    noesis_runtime::shutdown();
}
