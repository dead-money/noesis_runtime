//! Visual and logical tree traversal, parent round-trips, hit testing, and
//! template-child negative path on a three-Border Grid fixture.

use std::collections::HashMap;

use noesis_runtime::view::{FrameworkElement, View};
use noesis_runtime::xaml_provider::XamlProvider;

// Non-overlapping layout; the Grid has no Background (empty space is not hit-testable):
//   Left   (w=100,h=50) HAlign=Left  VAlign=Top    Margin 0,0    → x[0,100]   y[0,50]
//   Middle (w=120,h=50) HAlign=Left  VAlign=Top    Margin 150,75 → x[150,270] y[75,125]
//   Right  (w=140,h=50) HAlign=Right VAlign=Bottom               → x[260,400] y[150,200]
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
        // Layout MUST run before hit_test / actual_width assertions.
        assert!(view.update(0.0));

        let root = view.content().expect("View::content returned None");
        assert_eq!(root.name().as_deref(), Some("Root"), "root name");

        assert_eq!(
            root.logical_children_count(),
            3,
            "Grid should have exactly 3 logical children",
        );
        let lc0 = root.logical_child(0).expect("logical_child(0)");
        let lc1 = root.logical_child(1).expect("logical_child(1)");
        let lc2 = root.logical_child(2).expect("logical_child(2)");
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

        assert!(
            root.logical_child(9999).is_none(),
            "logical_child(9999) should be None",
        );

        // A plain Grid inserts no wrapper visuals, so visual count == logical count.
        // Assert >= first (robust), then the exact equality that holds for a Panel.
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
        assert_eq!(
            vc1.name().as_deref(),
            Some("Middle"),
            "visual_child(1) name"
        );
        assert_eq!(vc1.get_f32("Width"), Some(120.0), "visual_child(1) width");

        assert!(
            root.visual_child(9999).is_none(),
            "visual_child(9999) should be None",
        );

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

        // View::create reparents the content under an internal unnamed container,
        // so the root's logical/visual parent is Some("") rather than None.
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

        assert!(
            middle.actual_width().unwrap() > 0.0,
            "Middle should have a laid-out width after update()",
        );

        // x[150,270], y[75,125] → Middle.
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

        // x[0,100], y[0,50] → Left; a second distinct target rules out a stub that returns one fixed element.
        let hit_left = root
            .hit_test(50.0, 25.0)
            .expect("hit_test inside Left should hit something");
        assert_eq!(
            hit_left.name().as_deref(),
            Some("Left"),
            "hit_test(50,25) should land on Left",
        );

        // Empty Grid space (no Background) is not hit-testable → None.
        assert!(
            root.hit_test(120.0, 30.0).is_none(),
            "hit_test on empty space should be None",
        );

        // No ControlTemplate applied, so template_child returns None.
        // Positive coverage would require theme resources not available in this test context.
        assert!(
            middle.template_child("PART_DoesNotExist").is_none(),
            "template_child on a non-templated Border should be None",
        );
        assert!(
            root.template_child("PART_DoesNotExist").is_none(),
            "template_child for an unknown part on the Grid should be None",
        );

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

    noesis_runtime::shutdown();
}
