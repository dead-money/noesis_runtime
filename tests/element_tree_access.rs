//! TODO §2 — the remaining element-tree-access surface:
//!   * **Filtered hit testing** (`hit_test_filtered` / `hit_test_all`): multi-hit
//!     collection, plus the filter/result behaviours that steer and stop the walk.
//!   * **Standalone `NameScope`**: create / attach / look up / reverse-look-up /
//!     enumerate / unregister.
//!   * **`RenderTransform` read-back** (`render_transform`) and
//!     **render-transform-origin** get/set.
//!
//! Single `#[test]` (Noesis can't be re-init'd in a process); all owning
//! wrappers drop before `shutdown()`. Hit testing needs a laid-out view; the
//! `NameScope` / transform parts work on detached elements.
//!
//!   `cargo test -p dm_noesis_runtime --test element_tree_access -- --nocapture`

use std::collections::HashMap;

use dm_noesis_runtime::name_scope::NameScope;
use dm_noesis_runtime::transforms::ScaleTransform;
use dm_noesis_runtime::view::{
    FrameworkElement, HitTestFilterBehavior, HitTestResultBehavior, View,
};
use dm_noesis_runtime::xaml_provider::XamlProvider;

// Two concentric, both-hit-testable Borders so a point at the centre is hit by
// BOTH (multi-hit), with distinct x:Names for identity. The inner Border is the
// topmost (declared as the outer's child, drawn last over the same point).
const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid x:Name="Root"
      xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="200" Height="200">
  <Border x:Name="Outer" Background="#FF202020" Width="200" Height="200">
    <Border x:Name="Inner" Background="#FF00FF00" Width="100" Height="100"
            HorizontalAlignment="Center" VerticalAlignment="Center"/>
  </Border>
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

fn names(hits: &[FrameworkElement]) -> Vec<String> {
    hits.iter().filter_map(FrameworkElement::name).collect()
}

#[test]
fn filtered_hit_test_namescope_and_render_transform() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let _registered = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0), "layout must run before hit testing");

        let root = view.content().expect("View::content returned None");

        // ── (3) Filtered hit testing ─────────────────────────────────────────
        // (a) hit_test_all collects EVERY hit at the centre — both Borders.
        let all = root.hit_test_all(100.0, 100.0);
        let all_names = names(&all);
        assert!(
            all.len() >= 2,
            "centre is covered by both Borders; expected >= 2 hits, got {} ({all_names:?})",
            all.len()
        );
        assert!(
            all_names.iter().any(|n| n == "Inner") && all_names.iter().any(|n| n == "Outer"),
            "hit_test_all should include Inner and Outer, got {all_names:?}"
        );
        // Topmost-first: the inner (last-drawn) Border is reported before the outer.
        let inner_pos = all_names.iter().position(|n| n == "Inner");
        let outer_pos = all_names.iter().position(|n| n == "Outer");
        assert!(
            inner_pos < outer_pos,
            "topmost (Inner) should be reported before Outer, got {all_names:?}"
        );

        // (b) A filter that returns Stop immediately must yield ZERO results —
        //     proves the filter return value is honoured (a no-op bridge would
        //     instead collect every hit).
        let mut stop_hits = 0usize;
        root.hit_test_filtered(
            100.0,
            100.0,
            |_| HitTestFilterBehavior::Stop,
            |_| {
                stop_hits += 1;
                HitTestResultBehavior::Continue
            },
        );
        assert_eq!(stop_hits, 0, "filter=Stop must prevent any result callback");

        // (c) A result callback that returns Stop after the first hit must yield
        //     EXACTLY one — and it must be the topmost (Inner).
        let mut first: Vec<String> = Vec::new();
        root.hit_test_filtered(
            100.0,
            100.0,
            |_| HitTestFilterBehavior::Continue,
            |hit| {
                if let Some(n) = hit.name() {
                    first.push(n);
                }
                HitTestResultBehavior::Stop
            },
        );
        assert_eq!(
            first,
            vec!["Inner".to_string()],
            "result=Stop after first hit"
        );

        // (d) A filter that skips the Inner subtree must exclude Inner but keep
        //     Outer — proves per-visual filter selectivity.
        let mut filtered: Vec<String> = Vec::new();
        root.hit_test_filtered(
            100.0,
            100.0,
            |v| {
                if v.name().as_deref() == Some("Inner") {
                    HitTestFilterBehavior::ContinueSkipSelfAndChildren
                } else {
                    HitTestFilterBehavior::Continue
                }
            },
            |hit| {
                if let Some(n) = hit.name() {
                    filtered.push(n);
                }
                HitTestResultBehavior::Continue
            },
        );
        assert!(
            !filtered.iter().any(|n| n == "Inner"),
            "filtered-out Inner must not appear, got {filtered:?}"
        );
        assert!(
            filtered.iter().any(|n| n == "Outer"),
            "Outer should still be hit, got {filtered:?}"
        );

        // ── (4) Standalone NameScope ─────────────────────────────────────────
        let alpha = FrameworkElement::parse(
            "<Border xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"/>",
        )
        .expect("parse alpha");
        let beta = FrameworkElement::parse(
            "<Border xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"/>",
        )
        .expect("parse beta");

        let mut scope = NameScope::new();
        scope.register_name("alpha", &alpha);
        scope.register_name("beta", &beta);

        // Forward lookup returns the same underlying object.
        let found = scope.find_name("alpha").expect("find_name(alpha)");
        assert_eq!(
            found.raw(),
            alpha.raw(),
            "find_name should return the alpha object"
        );
        assert!(scope.find_name("missing").is_none(), "absent name -> None");

        // Reverse lookup.
        assert_eq!(
            scope.find_object(&beta).as_deref(),
            Some("beta"),
            "find_object(beta) should be \"beta\""
        );

        // Enumeration sees both pairs.
        let mut enumerated: Vec<String> = Vec::new();
        scope.for_each(|name, _obj| enumerated.push(name.to_string()));
        enumerated.sort();
        assert_eq!(enumerated, vec!["alpha".to_string(), "beta".to_string()]);

        // Unregister removes only that name.
        scope.unregister_name("alpha");
        assert!(scope.find_name("alpha").is_none(), "unregistered name gone");
        assert!(scope.find_name("beta").is_some(), "beta still registered");

        // Attach / read back the scope on an element (round-trips by identity).
        // Note: a parsed XAML root already carries its own (XAML) namescope, so
        // we don't assert None up front — set_on must REPLACE whatever is there
        // and of() must hand back exactly the scope we set.
        let mut host = FrameworkElement::parse(
            "<Border xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"/>",
        )
        .expect("parse host");
        let pre = NameScope::of(&host).map(|s| s.raw());
        // A parsed XAML root carries its own namescope, so `pre` is Some here —
        // assert it so the replacement check below isn't a trivial Some != None.
        assert!(
            pre.is_some(),
            "a parsed XAML root should already carry a namescope"
        );
        assert!(
            NameScope::set_on(&mut host, Some(&scope)),
            "set_on should succeed on a FrameworkElement"
        );
        let read_back = NameScope::of(&host).expect("of() after set_on");
        assert_eq!(
            read_back.raw(),
            scope.raw(),
            "GetNameScope should return the same scope object we set"
        );
        assert_ne!(
            Some(read_back.raw()),
            pre,
            "set_on should have replaced the pre-existing namescope"
        );

        // ── (2) RenderTransform read-back + origin ───────────────────────────
        // Note: RenderTransform defaults to the (non-null) Identity transform,
        // so a fresh element reads back Some(identity). We prove the round-trip
        // by pointer identity: render_transform() must hand back exactly the
        // transform object we set, and it must differ from the prior identity.
        let mut t1 = FrameworkElement::parse(
            "<Border xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"/>",
        )
        .expect("parse t1");
        let identity = t1.render_transform().map(|t| t.raw());
        // The default is the non-null Identity transform, so `identity` is Some
        // — assert it so the `assert_ne!` below isn't a trivial Some != None.
        assert!(
            identity.is_some(),
            "default RenderTransform should be the non-null Identity"
        );

        let scale = ScaleTransform::new(2.0, 3.0, 0.0, 0.0);
        let scale_raw = scale.raw();
        assert!(
            t1.set_render_transform(&scale),
            "set_render_transform should succeed on a UIElement"
        );
        let read = t1.render_transform().expect("render_transform after set");
        assert_eq!(
            read.raw(),
            scale_raw,
            "render_transform should hand back exactly the transform we set"
        );
        assert_ne!(
            Some(read.raw()),
            identity,
            "the set transform must differ from the default Identity"
        );

        // The read-back handle is itself a Transform: re-apply it to another
        // element and confirm that element now reports the SAME transform.
        let mut t2 = FrameworkElement::parse(
            "<Border xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"/>",
        )
        .expect("parse t2");
        assert!(
            t2.set_render_transform(&read),
            "the read-back AnyTransform should be re-applicable"
        );
        assert_eq!(
            t2.render_transform().map(|t| t.raw()),
            Some(scale_raw),
            "t2 should report the same re-applied transform object"
        );

        // Render-transform-origin round-trips numerically. The default is
        // (0,0), but that alone can't tell a working getter from a constant-0
        // stub — so we set TWO distinct non-default values and confirm the
        // getter tracks each, proving it reads real state.
        assert_eq!(
            t1.render_transform_origin(),
            (0.0, 0.0),
            "default origin is (0,0)"
        );
        assert!(t1.set_render_transform_origin(0.25, 0.75));
        let (ox, oy) = t1.render_transform_origin();
        assert!(
            (ox - 0.25).abs() < 1e-6 && (oy - 0.75).abs() < 1e-6,
            "origin should round-trip, got ({ox}, {oy})"
        );
        assert!(t1.set_render_transform_origin(0.5, 0.1));
        let (ox2, oy2) = t1.render_transform_origin();
        assert!(
            (ox2 - 0.5).abs() < 1e-6 && (oy2 - 0.1).abs() < 1e-6,
            "a second distinct origin should also round-trip, got ({ox2}, {oy2})"
        );

        view.deactivate();
        drop(view);
    }

    dm_noesis_runtime::shutdown();
}
