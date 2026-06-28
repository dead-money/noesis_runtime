//! TODO §15 — scheme-/assembly-scoped **texture** providers (routing test).
//!
//! Companion to `xaml_scoped_providers.rs`, but for the texture-provider
//! triple. Each of the three scoped texture setters maps to a DISTINCT Noesis
//! call (`SetSchemeTextureProvider` / `SetAssemblyTextureProvider` /
//! `SetSchemeAssemblyTextureProvider`), so a copy-paste bug routing one setter
//! to the wrong Noesis function would not be caught by a compile-only test.
//!
//! The test installs four texture providers at once — a global one, a
//! scheme-scoped one (`myassets`), an assembly-scoped one (`App`), and a
//! scheme+assembly one (`packs` + `Skin`) — each a `Recorder` logging every
//! URI its `info` (Noesis `GetTextureInfo`) callback is asked for. It then
//! drives a single measure pass over a `<Grid>` of four `<Image Source=.../>`
//! whose sources span the four scopes, and asserts Noesis routed each URI to
//! exactly the right provider — and to NO other.
//!
//! `GetTextureInfo` is consulted during the measure pass to size the `Image`
//! before any pixels are decoded, so this is fully headless: no GPU / render
//! device is required (we never bind one to the `View`).
//!
//! Run with `NOESIS_SDK_DIR` set:
//!   `cargo test -p noesis_runtime --test texture_scoped_providers -- --nocapture`

use std::sync::{Arc, Mutex};

use noesis_runtime::texture_provider::{
    ImageData, TextureInfo, TextureProvider, set_assembly_texture_provider,
    set_scheme_assembly_texture_provider, set_scheme_texture_provider, set_texture_provider,
};
use noesis_runtime::view::{FrameworkElement, View};

// Four <Image> sources, one per scope:
//   * myassets:///tex.png                                  → scheme provider
//   * pack://application:,,,/App;component/tex.png         → assembly provider
//   * packs://application:,,,/Skin;component/tex.png       → scheme+assembly provider
//   * plain.png                                            → global provider
const TEX_XAML: &str = r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Image x:Name="ImgScheme"   Source="myassets:///tex.png"/>
  <Image x:Name="ImgAssembly" Source="pack://application:,,,/App;component/tex.png"/>
  <Image x:Name="ImgBoth"     Source="packs://application:,,,/Skin;component/tex.png"/>
  <Image x:Name="ImgGlobal"   Source="plain.png"/>
</Grid>"##;

/// Logs every URI its `info` callback is asked for, tagged with `label`, and
/// reports `dims` so Noesis can size the `Image`. `load` is never expected to
/// fire headless (no render device), but is recorded too for diagnostics.
struct TexRecorder {
    label: &'static str,
    dims: (u32, u32),
    log: Arc<Mutex<Vec<(&'static str, String)>>>,
}
impl TextureProvider for TexRecorder {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn info(&mut self, uri: &str) -> Option<TextureInfo> {
        self.log.lock().unwrap().push((self.label, uri.to_string()));
        Some(TextureInfo::new(self.dims.0, self.dims.1))
    }
    fn load(&mut self, _uri: &str) -> Option<ImageData<'_>> {
        None
    }
}

#[test]
fn texture_scoped_providers_route_by_scheme_and_assembly() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let log: Arc<Mutex<Vec<(&'static str, String)>>> = Arc::default();

        let _global = set_texture_provider(TexRecorder {
            label: "global",
            dims: (10, 10),
            log: Arc::clone(&log),
        });
        let _scheme = set_scheme_texture_provider(
            "myassets",
            TexRecorder {
                label: "scheme",
                dims: (20, 20),
                log: Arc::clone(&log),
            },
        );
        let _assembly = set_assembly_texture_provider(
            "App",
            TexRecorder {
                label: "assembly",
                dims: (30, 30),
                log: Arc::clone(&log),
            },
        );
        let _both = set_scheme_assembly_texture_provider(
            "packs",
            "Skin",
            TexRecorder {
                label: "both",
                dims: (40, 40),
                log: Arc::clone(&log),
            },
        );

        // Parse the four-Image tree and drive a measure pass through a View.
        // GetTextureInfo fires during layout to size each Image.
        let root = FrameworkElement::parse(TEX_XAML).expect("parse Image grid");
        let mut view = View::create(root);
        view.set_size(640, 480);
        // A couple of updates: the first kicks off source resolution, the
        // second guarantees layout settled. Neither needs a render device.
        view.update(0.0);
        view.update(0.016);

        let entries = log.lock().unwrap().clone();
        assert!(
            !entries.is_empty(),
            "no texture provider was consulted during the measure pass; \
             GetTextureInfo never fired — log = {entries:?}"
        );

        // ── Each scoped URI landed on its own provider ──────────────────────
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "scheme" && u.contains("myassets") && u.contains("tex.png")),
            "scheme provider was not asked for the myassets URI; log = {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "assembly" && u.contains("App") && u.contains("tex.png")),
            "assembly provider was not asked for the pack/App URI; log = {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "both" && u.contains("Skin") && u.contains("tex.png")),
            "scheme+assembly provider was not asked for the packs/Skin URI; log = {entries:?}"
        );
        assert!(
            entries
                .iter()
                .any(|(l, u)| *l == "global" && u.contains("plain.png")),
            "global provider was not asked for the unscoped URI; log = {entries:?}"
        );

        // ── Exclusivity: no provider saw a URI outside its scope ────────────
        // The global provider must never be consulted for a scoped URI (no
        // silent fallback through the global registration).
        assert!(
            !entries.iter().any(|(l, u)| *l == "global"
                && (u.contains("myassets") || u.contains("App") || u.contains("Skin"))),
            "global provider was consulted for a scoped URI — routing leaked; log = {entries:?}"
        );
        // The scheme provider must only ever see myassets URIs.
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "scheme" && (u.contains("App") || u.contains("Skin"))),
            "scheme provider saw an out-of-scope URI; log = {entries:?}"
        );
        // The assembly provider must only ever see App URIs (not Skin/myassets).
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "assembly" && (u.contains("Skin") || u.contains("myassets"))),
            "assembly provider saw an out-of-scope URI; log = {entries:?}"
        );
        // The scheme+assembly provider must only ever see Skin URIs.
        assert!(
            !entries
                .iter()
                .any(|(l, u)| *l == "both" && (u.contains("App") || u.contains("myassets"))),
            "scheme+assembly provider saw an out-of-scope URI; log = {entries:?}"
        );

        drop(view);
    }

    noesis_runtime::shutdown();
}
