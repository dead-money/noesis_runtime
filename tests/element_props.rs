//! Integration tests for the `FrameworkElement` convenience methods added in
//! TODO §2.E/F/G:
//!
//! * **§E** — typed scalar sugar (`width`/`height`/`opacity`,
//!   `is_enabled`/`focusable`, `tag`) plus the bespoke alignment path
//!   (`horizontal_alignment` / `vertical_alignment` ↔ `HAlign`/`VAlign`) and
//!   the laid-out `actual_width` / `actual_height` read-backs.
//! * **§F** — namescope `register_name` / `unregister_name`, cross-checked
//!   through `find_name`.
//! * **§G** — thread affinity (`check_access` / `thread_id`).
//!
//! Single `#[test]` per the harness convention (one Noesis init per process).
//!
//! Run with `NOESIS_SDK_DIR` set (trial mode is fine):
//!   `cargo test -p noesis_runtime --test element_props -- --nocapture`

use std::collections::HashMap;

use noesis_runtime::view::{FrameworkElement, HAlign, VAlign, View};
use noesis_runtime::xaml_provider::XamlProvider;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid x:Name="Root"
      xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="400" Height="200">
  <!-- Explicitly sized + Left/Top aligned so it does NOT stretch: its
       ActualWidth/Height must collapse onto the declared Width/Height. -->
  <TextBlock x:Name="Sized" Text="sized"
             Width="120" Height="40"
             HorizontalAlignment="Left" VerticalAlignment="Top"/>
  <!-- XAML-declared scalars + alignment, asserted via the getters BEFORE any
       setter runs (proves XAML → getter agreement). -->
  <TextBlock x:Name="Aligned" Text="aligned"
             Opacity="0.25" IsEnabled="False" Focusable="True"
             HorizontalAlignment="Right" VerticalAlignment="Bottom"/>
  <!-- A distinct named element used as the namescope/tag payload. -->
  <TextBlock x:Name="Anchor" Text="anchor"
             HorizontalAlignment="Center" VerticalAlignment="Center"/>
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
fn element_props_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let _registered = noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(400, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");

        // ── §E ActualSize. ───────────────────────────────────────────────────
        // OBSERVED: Noesis lays the tree out eagerly (on content()/set_size),
        // so `actual_width()` already reports the laid-out 120.0 here, BEFORE
        // our explicit update(0.0). The "actual is ~0 before update" probe from
        // the brief is therefore not observable in this runtime — record it and
        // move on rather than asserting a falsehood.
        let mut sized = content.find_name("Sized").expect("Sized not found");
        if let Some(w) = sized.actual_width() {
            // Never larger than the declared Width for a non-stretched element.
            assert!(
                w <= 120.0,
                "ActualWidth pre-update should not exceed Width, got {w}"
            );
        }

        assert!(view.update(0.0), "first update should report a change");

        // `width()` is the *requested* Width; `actual_width()` is the laid-out
        // width. For a Left/Top-aligned element with an explicit size they
        // coincide.
        assert_eq!(sized.width(), Some(120.0), "requested Width mismatch");
        assert_eq!(sized.height(), Some(40.0), "requested Height mismatch");
        assert_eq!(
            sized.actual_width(),
            Some(120.0),
            "ActualWidth should equal laid-out Width after update",
        );
        assert_eq!(
            sized.actual_height(),
            Some(40.0),
            "ActualHeight should equal laid-out Height after update",
        );

        // ── §E XAML → getter agreement (BEFORE any setter touches it) ────────
        let aligned = content.find_name("Aligned").expect("Aligned not found");
        assert_eq!(
            aligned.horizontal_alignment(),
            Some(HAlign::Right),
            "XAML HorizontalAlignment=\"Right\" should read back as HAlign::Right",
        );
        assert_eq!(
            aligned.vertical_alignment(),
            Some(VAlign::Bottom),
            "XAML VerticalAlignment=\"Bottom\" should read back as VAlign::Bottom",
        );
        assert_eq!(aligned.opacity(), Some(0.25), "XAML Opacity mismatch");
        assert_eq!(
            aligned.is_enabled(),
            Some(false),
            "XAML IsEnabled=\"False\" mismatch",
        );
        assert_eq!(
            aligned.focusable(),
            Some(true),
            "XAML Focusable=\"True\" mismatch",
        );

        // ── §E Alignment round-trip: every ordinal, both axes. Catches an
        //    off-by-one in the ordinal ↔ variant mapping. ─────────────────────
        let haligns = [
            (0, HAlign::Left),
            (1, HAlign::Center),
            (2, HAlign::Right),
            (3, HAlign::Stretch),
        ];
        for (ord, h) in haligns {
            sized.set_horizontal_alignment(h);
            assert_eq!(
                sized.horizontal_alignment(),
                Some(h),
                "HAlign ordinal {ord} did not round-trip",
            );
            assert_eq!(h as i32, ord, "HAlign discriminant drifted from ordinal");
        }
        let valigns = [
            (0, VAlign::Top),
            (1, VAlign::Center),
            (2, VAlign::Bottom),
            (3, VAlign::Stretch),
        ];
        for (ord, v) in valigns {
            sized.set_vertical_alignment(v);
            assert_eq!(
                sized.vertical_alignment(),
                Some(v),
                "VAlign ordinal {ord} did not round-trip",
            );
            assert_eq!(v as i32, ord, "VAlign discriminant drifted from ordinal");
        }

        // ── §E Scalar set→get round-trips with concrete values ───────────────
        // 0.5 and 0.25 are exactly representable in f32, so exact equality is
        // safe here; if a future value isn't, switch to a tolerance.
        assert!(sized.set_width(256.0), "set_width failed");
        assert_eq!(sized.width(), Some(256.0), "Width did not round-trip");

        assert!(sized.set_height(64.0), "set_height failed");
        assert_eq!(sized.height(), Some(64.0), "Height did not round-trip");

        assert!(sized.set_opacity(0.5), "set_opacity failed");
        assert_eq!(sized.opacity(), Some(0.5), "Opacity did not round-trip");

        assert!(sized.set_enabled(false), "set_enabled(false) failed");
        assert_eq!(
            sized.is_enabled(),
            Some(false),
            "IsEnabled did not round-trip to false",
        );
        assert!(sized.set_enabled(true), "set_enabled(true) failed");
        assert_eq!(
            sized.is_enabled(),
            Some(true),
            "IsEnabled back to true failed"
        );

        // TextBlock.Focusable defaults to false; round-trip both directions.
        assert!(sized.set_focusable(true), "set_focusable(true) failed");
        assert_eq!(
            sized.focusable(),
            Some(true),
            "Focusable did not round-trip to true",
        );
        assert!(sized.set_focusable(false), "set_focusable(false) failed");
        assert_eq!(
            sized.focusable(),
            Some(false),
            "Focusable did not round-trip to false",
        );

        // ── §E Tag: absence → None, then set → Some. No pointer-eq is exposed,
        //    so presence/absence is the strongest available check. ────────────
        let anchor = content.find_name("Anchor").expect("Anchor not found");
        assert!(
            aligned.tag().is_none(),
            "an element with no Tag should report None",
        );
        assert!(sized.set_tag(&anchor), "set_tag(&Anchor) returned false");
        assert!(
            sized.tag().is_some(),
            "Tag should resolve to a component after set_tag",
        );

        // ── §F Namescope register / unregister, cross-checked via find_name.
        //    Register the Anchor element under a FRESH key on the content root
        //    (which hosts the scope). find_name resolves the registered object;
        //    we distinguish it by its x:Name (\"Anchor\"). ─────────────────────
        let mut root = content; // the view content root hosts the namescope
        assert!(
            root.find_name("Dynamic").is_none(),
            "fresh key must not resolve before registration",
        );
        assert!(
            root.register_name("Dynamic", &anchor),
            "register_name on the content root returned false",
        );
        let resolved = root
            .find_name("Dynamic")
            .expect("find_name(\"Dynamic\") should resolve after register_name");
        assert_eq!(
            resolved.name().as_deref(),
            Some("Anchor"),
            "namescope resolved the wrong object",
        );
        drop(resolved);
        assert!(
            root.unregister_name("Dynamic"),
            "unregister_name returned false",
        );
        assert!(
            root.find_name("Dynamic").is_none(),
            "find_name should stop resolving after unregister_name",
        );

        // ── §G Thread affinity. The test thread created the view + elements,
        //    so check_access() is true here and thread_id() is a stable
        //    non-MAX value shared across elements of the same view. ───────────
        assert!(
            root.check_access(),
            "check_access() should be true on owner thread"
        );
        assert!(
            sized.check_access(),
            "check_access() should be true on owner thread"
        );

        let root_tid = root.thread_id();
        let sized_tid = sized.thread_id();
        assert_ne!(
            root_tid,
            u32::MAX,
            "attached element thread_id should not be MAX"
        );
        assert_eq!(
            root_tid, sized_tid,
            "two elements from the same view should share a thread id",
        );

        // FrameworkElement is Send but NOT Sync (Noesis objects are
        // thread-affine — see the crate-level "Thread affinity" docs). So we
        // *move* ownership to another thread (not share `&root`), call the
        // owner-query check_access() there — it must observe a DIFFERENT owner
        // thread and return false — then move the element back for teardown.
        let (root, off_access) = std::thread::spawn(move || {
            let access = root.check_access();
            (root, access)
        })
        .join()
        .unwrap();
        assert!(
            !off_access,
            "check_access() must be false on a non-owner thread",
        );

        // ORDERED teardown — drop every handle before shutdown.
        drop(anchor);
        drop(aligned);
        drop(sized);
        drop(root);
        view.deactivate();
        drop(view);
        drop(_registered);
    }

    noesis_runtime::shutdown();
}
