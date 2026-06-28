//! TODO §2.A — visual / logical tree traversal + hit testing.
//!
//! Exercises the tree-walk accessors on [`FrameworkElement`]:
//!   * `logical_children_count` / `logical_child` — exact count + XAML order.
//!   * `visual_children_count` / `visual_child`   — reachability of the same
//!     children through the visual tree.
//!   * `logical_parent` / `visual_parent`         — child→parent round-trips,
//!     identity confirmed via the parent's distinguishing `x:Name`.
//!   * `hit_test`                                 — a point inside a known,
//!     explicitly-sized child returns that child; a point in empty space
//!     returns the documented sentinel.
//!   * `template_child`                           — negative path (no template
//!     part exists on a plain panel without an applied `ControlTemplate`).
//!
//! The fixture is a `Grid` with three explicitly-placed `Border`s whose
//! `x:Name`s AND `Width`s are all distinct, so every traversal can be
//! cross-checked two independent ways (name + width) — a stubbed impl that
//! handed back the wrong element, or the root, would be caught.
//!
//!   `cargo test -p dm_noesis_runtime --test element_tree -- --nocapture`

use std::collections::HashMap;

use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

// Root Grid "Root" with three Borders. Each Border has a Background (so it is
// hit-testable) and a distinct Width so identity is checkable independently of
// the name. Positions (via alignment + margin) are non-overlapping:
//   Left   (w=100,h=50) HAlign=Left  VAlign=Top    Margin 0,0      → x[0,100]   y[0,50]
//   Middle (w=120,h=50) HAlign=Left  VAlign=Top    Margin 150,75   → x[150,270] y[75,125]
//   Right  (w=140,h=50) HAlign=Right VAlign=Bottom               → x[260,400] y[150,200]
// The Grid itself has NO Background, so empty space is not hit-testable.
const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid x:Name="Root"
      xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="400" Height="200">
  <Border x:Name="Left" Width="100" Height="50" Background="#FFFF0000"
          HorizontalAlignment="Left" VerticalAlignment="Top" Margin="0,0,0,0"/>
  <Border x:Name="Middle" Width="120" Height="50" Background="#FF00FF00"
          HorizontalAlignment="Left" VerticalAlignment="Top" Margin="150,75,0,0"/>
  <Border x:Name="Right" Width="140" Height="50" Background="#FF0000FF"
          HorizontalAlignment="Right" VerticalAlignment="Bottom" Margin="0,0,0,0"/>
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
fn element_tree_traversal() {
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
        view.set_size(400, 200);
        view.activate();
        // Layout MUST run before hit_test / actual_width assertions.
        assert!(view.update(0.0));

        let root = view.content().expect("View::content returned None");
        assert_eq!(root.name().as_deref(), Some("Root"), "root name");

        // ── Logical tree: exact count + XAML order ───────────────────────────
        assert_eq!(
            root.logical_children_count(),
            3,
            "Grid should have exactly 3 logical children",
        );
        let lc0 = root.logical_child(0).expect("logical_child(0)");
        let lc1 = root.logical_child(1).expect("logical_child(1)");
        let lc2 = root.logical_child(2).expect("logical_child(2)");
        // Logical order follows XAML declaration order.
        assert_eq!(lc0.name().as_deref(), Some("Left"), "logical_child(0) name");
        assert_eq!(
            lc1.name().as_deref(),
            Some("Middle"),
            "logical_child(1) name",
        );
        assert_eq!(
            lc2.name().as_deref(),
            Some("Right"),
            "logical_child(2) name"
        );
        // Cross-check identity via the independent Width property.
        assert_eq!(lc0.get_f32("Width"), Some(100.0), "Left width");
        assert_eq!(lc1.get_f32("Width"), Some(120.0), "Middle width");
        assert_eq!(lc2.get_f32("Width"), Some(140.0), "Right width");

        // Out-of-range logical child → None.
        assert!(
            root.logical_child(9999).is_none(),
            "logical_child(9999) should be None",
        );

        // ── Visual tree: same three children reachable ───────────────────────
        // A plain Grid inserts no wrapper visuals, so visual children == logical
        // children here. (Assert >= as the robust invariant, then the exact
        // equality that actually holds for a Panel.)
        let vcount = root.visual_children_count();
        assert!(
            vcount >= 3,
            "visual_children_count {vcount} should be >= logical count 3",
        );
        assert_eq!(
            vcount, 3,
            "a plain Grid's visual children should equal its 3 logical children",
        );
        let vc1 = root.visual_child(1).expect("visual_child(1)");
        // Visual order also follows the children collection order.
        assert_eq!(
            vc1.name().as_deref(),
            Some("Middle"),
            "visual_child(1) name"
        );
        assert_eq!(vc1.get_f32("Width"), Some(120.0), "visual_child(1) width");

        // Out-of-range visual child → None.
        assert!(
            root.visual_child(9999).is_none(),
            "visual_child(9999) should be None",
        );

        // ── Parent round-trips: child → parent identity == Root ──────────────
        let middle = root.find_name("Middle").expect("find_name(Middle)");
        let lp = middle.logical_parent().expect("Middle.logical_parent()");
        assert_eq!(
            lp.name().as_deref(),
            Some("Root"),
            "logical_parent(Middle) should be Root",
        );
        let vp = middle.visual_parent().expect("Middle.visual_parent()");
        assert_eq!(
            vp.name().as_deref(),
            Some("Root"),
            "visual_parent(Middle) should be Root",
        );

        // Root's parents: OBSERVED — the root content is NOT the top of either
        // tree. `View::create` reparents it under an internal, unnamed root
        // container, so both `logical_parent` and `visual_parent` of the root
        // are `Some` with an empty `x:Name` (i.e. not one of our named Borders).
        // We assert that observed shape rather than the naive `None`.
        let root_lp = root
            .logical_parent()
            .expect("root logical_parent is the View's internal container (Some)");
        assert_eq!(
            root_lp.name().as_deref(),
            Some(""),
            "root's logical parent is the unnamed View container",
        );
        let root_vp = root
            .visual_parent()
            .expect("root visual_parent is the View's internal container (Some)");
        assert_eq!(
            root_vp.name().as_deref(),
            Some(""),
            "root's visual parent is the unnamed View container",
        );

        // ── Hit testing (layout has settled) ─────────────────────────────────
        // Confirm layout really ran before trusting hit geometry.
        assert!(
            middle.actual_width().unwrap() > 0.0,
            "Middle should have a laid-out width after update()",
        );

        // A point inside Middle's bounds (x[150,270], y[75,125]) hits Middle.
        let hit = root
            .hit_test(200.0, 100.0)
            .expect("hit_test inside Middle should hit something");
        assert_eq!(
            hit.name().as_deref(),
            Some("Middle"),
            "hit_test(200,100) should land on Middle",
        );
        assert_eq!(
            hit.get_f32("Width"),
            Some(120.0),
            "hit element width cross-check (Middle)",
        );

        // A point inside Left's bounds (x[0,100], y[0,50]) hits Left — a second
        // distinct target rules out a stub that always returns the same node.
        let hit_left = root
            .hit_test(50.0, 25.0)
            .expect("hit_test inside Left should hit something");
        assert_eq!(
            hit_left.name().as_deref(),
            Some("Left"),
            "hit_test(50,25) should land on Left",
        );

        // A point in empty Grid space (no Border, Grid has no Background) →
        // nothing hit. Observed: None (the transparent Grid is not hit-testable).
        assert!(
            root.hit_test(120.0, 30.0).is_none(),
            "hit_test on empty space should be None",
        );

        // ── template_child: negative path ───────────────────────────────────
        // A plain Border has no applied ControlTemplate, so no template part of
        // any name exists. Positive template-part coverage would require a
        // templated control with a theme/ControlTemplate supplying named parts
        // (not available without app resources here), so we assert the
        // documented None and don't fake a pass.
        assert!(
            middle.template_child("PART_DoesNotExist").is_none(),
            "template_child on a non-templated Border should be None",
        );
        assert!(
            root.template_child("PART_DoesNotExist").is_none(),
            "template_child for an unknown part on the Grid should be None",
        );

        // ── Ordered teardown — drop every handle before shutdown ─────────────
        drop(hit_left);
        drop(hit);
        drop(root_vp);
        drop(root_lp);
        drop(vp);
        drop(lp);
        drop(middle);
        drop(vc1);
        drop(lc2);
        drop(lc1);
        drop(lc0);
        drop(root);
        view.deactivate();
        drop(view);
        drop(_registered);
    }

    dm_noesis_runtime::shutdown();
}
