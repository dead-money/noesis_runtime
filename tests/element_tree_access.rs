//! Filtered hit testing, standalone `NameScope` operations, and `RenderTransform`
//! get/set on `FrameworkElement`.

use std::collections::HashMap;

use noesis_runtime::name_scope::NameScope;
use noesis_runtime::transforms::ScaleTransform;
use noesis_runtime::view::{FrameworkElement, HitTestFilterBehavior, HitTestResultBehavior, View};
use noesis_runtime::xaml_provider::XamlProvider;

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
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0), "layout must run before hit testing");

        let root = view.content().expect("View::content returned None");

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
        // Topmost-first: Inner (last-drawn) is reported before Outer.
        let inner_pos = all_names.iter().position(|n| n == "Inner");
        let outer_pos = all_names.iter().position(|n| n == "Outer");
        assert!(
            inner_pos < outer_pos,
            "topmost (Inner) should be reported before Outer, got {all_names:?}"
        );

        // filter=Stop immediately yields zero results; a no-op bridge would collect every hit instead.
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

        // result=Stop after the first hit yields exactly one: the topmost (Inner).
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

        // Skipping the Inner subtree excludes Inner but keeps Outer: per-visual filter selectivity.
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

        let found = scope.find_name("alpha").expect("find_name(alpha)");
        assert_eq!(
            found.raw(),
            alpha.raw(),
            "find_name should return the alpha object"
        );
        assert!(scope.find_name("missing").is_none(), "absent name -> None");

        assert_eq!(
            scope.find_object(&beta).as_deref(),
            Some("beta"),
            "find_object(beta) should be \"beta\""
        );

        let mut enumerated: Vec<String> = Vec::new();
        scope.for_each(|name, _obj| enumerated.push(name.to_string()));
        enumerated.sort();
        assert_eq!(enumerated, vec!["alpha".to_string(), "beta".to_string()]);

        scope.unregister_name("alpha");
        assert!(scope.find_name("alpha").is_none(), "unregistered name gone");
        assert!(scope.find_name("beta").is_some(), "beta still registered");

        // A parsed XAML root already carries a namescope; set_on must REPLACE it.
        let mut host = FrameworkElement::parse(
            "<Border xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"/>",
        )
        .expect("parse host");
        let pre = NameScope::of(&host).map(|s| s.raw());
        // Assert pre.is_some() so the replacement check below isn't a trivial Some != None.
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

        // RenderTransform defaults to the non-null Identity, so a fresh element returns Some.
        // Pointer identity proves set_render_transform crossed the FFI.
        let mut t1 = FrameworkElement::parse(
            "<Border xmlns=\"http://schemas.microsoft.com/winfx/2006/xaml/presentation\"/>",
        )
        .expect("parse t1");
        let identity = t1.render_transform().map(|t| t.raw());
        // Default is non-null Identity (Some); assert it so the assert_ne! below isn't trivially Some != None.
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

        // Default origin (0,0) could pass a constant-zero stub; two distinct values confirm the getter reads real state.
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

    noesis_runtime::shutdown();
}
