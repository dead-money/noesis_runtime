//! TODO §15 — scheme-/assembly-scoped XAML providers.
//!
//! Installs three XAML providers at once — a global one, a scheme-scoped one
//! (`set_scheme_xaml_provider("myassets", ...)`), and an assembly-scoped one
//! (`set_assembly_xaml_provider("App", ...)`) — each recording the URIs it is
//! asked for. The test then drives three loads and asserts Noesis routed each
//! to exactly the right provider:
//!
//!   * `myassets:///main.xaml`                       → the **scheme** provider
//!   * `pack://application:,,,/App;component/...`    → the **assembly** provider
//!   * `plain.xaml`                                  → the **global** provider
//!
//! Both scoped loads are also confirmed end-to-end (the named child of the
//! served XAML is reachable through the loaded element), and the global
//! provider is asserted NOT to have been consulted for the scoped URIs — so a
//! broken scope-routing setter fails the test rather than silently falling
//! back to the global provider.
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p dm_noesis_runtime --test xaml_scoped_providers -- --nocapture`

use std::sync::{Arc, Mutex};

use dm_noesis_runtime::view::FrameworkElement;
use dm_noesis_runtime::xaml_provider::{
    XamlProvider, set_assembly_xaml_provider, set_scheme_xaml_provider, set_xaml_provider,
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
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let log: Arc<Mutex<Vec<(&'static str, String)>>> = Arc::default();

        // Global provider: serves nothing, just records. If scope routing were
        // broken, scoped URIs would fall back here and fail to load.
        let _global = set_xaml_provider(Recorder {
            label: "global",
            bytes: None,
            log: Arc::clone(&log),
        });

        // Scheme provider under the custom "myassets" scheme.
        let _scheme = set_scheme_xaml_provider(
            "myassets",
            Recorder {
                label: "scheme",
                bytes: Some(SCHEME_XAML.as_bytes().to_vec()),
                log: Arc::clone(&log),
            },
        );

        // Assembly provider for the "App" assembly referenced in pack URIs.
        let _assembly = set_assembly_xaml_provider(
            "App",
            Recorder {
                label: "assembly",
                bytes: Some(ASSEMBLY_XAML.as_bytes().to_vec()),
                log: Arc::clone(&log),
            },
        );

        // ── Scheme routing ──────────────────────────────────────────────────
        let schemed = FrameworkElement::load("myassets:///main.xaml")
            .expect("scheme load returned None — scheme provider not consulted");
        assert!(
            schemed.find_name("SCHEMED").is_some(),
            "named child from the scheme-served XAML not reachable; routing failed"
        );
        drop(schemed);

        // ── Assembly routing (pack URI) ─────────────────────────────────────
        let assembled = FrameworkElement::load("pack://application:,,,/App;component/main.xaml")
            .expect("assembly load returned None — assembly provider not consulted");
        assert!(
            assembled.find_name("ASSEMBLED").is_some(),
            "named child from the assembly-served XAML not reachable; routing failed"
        );
        drop(assembled);

        // ── Unscoped routing → global ───────────────────────────────────────
        // The global provider serves nothing, so this load fails — but the
        // *attempt* must be recorded against the global provider.
        assert!(
            FrameworkElement::load("plain.xaml").is_none(),
            "global provider serves nothing, so plain.xaml must not load"
        );

        let entries = log.lock().unwrap().clone();

        // Scheme URI went to the scheme provider, carrying the full scheme URI.
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "scheme" && u.contains("myassets") && u.contains("main.xaml")),
            "scheme provider was not asked for the myassets URI; log = {entries:?}"
        );
        // Pack/assembly URI went to the assembly provider.
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "assembly" && u.contains("main.xaml")),
            "assembly provider was not asked for the pack URI; log = {entries:?}"
        );
        // Unscoped URI went to the global provider.
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "global" && u == "plain.xaml"),
            "global provider was not asked for the unscoped URI; log = {entries:?}"
        );

        // The global provider must NOT have been consulted for either scoped
        // URI — scope routing is exclusive, not a global fallback.
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "global" && (u.contains("myassets") || u.contains("App"))),
            "global provider was consulted for a scoped URI — scope routing broke; \
             log = {entries:?}"
        );
        // Symmetrically, the scheme provider must not have seen the pack URI,
        // nor the assembly provider the scheme URI.
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "scheme" && u.contains("App")),
            "scheme provider saw the assembly URI; log = {entries:?}"
        );
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "assembly" && u.contains("myassets")),
            "assembly provider saw the scheme URI; log = {entries:?}"
        );
    }

    dm_noesis_runtime::shutdown();
}
