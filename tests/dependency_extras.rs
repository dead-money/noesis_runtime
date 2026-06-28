//! Integration tests for the dependency-property extras added in TODO §2:
//!
//! * **B. Attached properties** — `get/set_attached_{i32,f32,bool}` resolving a
//!   `DependencyProperty` registered on an owner type (`Grid.Row`,
//!   `Canvas.Left`).
//! * **C. Clear / `CurrentValue` / `BaseValue`** — `clear_value`,
//!   `set_current_{...}`, `get_base_{...}`.
//! * **D. Dynamic tag inference** — `property_tag`, `get_dynamic`.
//!
//! One `#[test]` per integration binary (Noesis must `init` exactly once per
//! process); every Noesis handle is dropped before `shutdown()`.
//!
//! Run with `NOESIS_LICENSE_*` set (trial mode is fine):
//!   `cargo test --test dependency_extras -- --nocapture`

use std::collections::HashMap;

use noesis_runtime::ffi::PropType;
use noesis_runtime::view::{DynValue, FrameworkElement, View};
use noesis_runtime::xaml_provider::{XamlProvider, set_xaml_provider};

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

// Root Grid declares three rows; `RowChild` sits in row 2 via the `Grid.Row`
// attached property. The `Canvas` hosts a child positioned with `Canvas.Left` /
// `Canvas.Top`. `ClearChild` carries a local `Width` and `Text` for the
// clear / current / base sections. Because the XAML names `<Grid>` and
// `<Canvas>`, those owner types are reflected and attached-property resolution
// works.
const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="400" Height="300">
  <Grid.RowDefinitions>
    <RowDefinition Height="*"/>
    <RowDefinition Height="*"/>
    <RowDefinition Height="*"/>
  </Grid.RowDefinitions>

  <TextBlock x:Name="RowChild" Grid.Row="2" Panel.ZIndex="2" Text="row"
             Foreground="White"
             VerticalAlignment="Top" HorizontalAlignment="Left"/>

  <Canvas x:Name="Canv" Grid.Row="0">
    <TextBlock x:Name="CanvChild" Canvas.Left="40" Canvas.Top="15"
               Text="canv" Foreground="White"/>
  </Canvas>

  <TextBlock x:Name="ClearChild" Grid.Row="1"
             Text="base-text" Width="120" Margin="3,5,7,9"
             Foreground="White"
             VerticalAlignment="Top" HorizontalAlignment="Left"/>
</Grid>"##;

#[test]
fn dependency_extras() {
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
        let _registered = set_xaml_provider(InMem { bytes });

        let element = FrameworkElement::load("scene.xaml").expect("XAML parse");
        let mut view = View::create(element);
        view.set_size(400, 300);
        view.activate();
        view.update(0.0);

        let content = view.content().expect("root");
        let mut row_child = content.find_name("RowChild").expect("RowChild");
        let mut canv_child = content.find_name("CanvChild").expect("CanvChild");
        let mut clear_child = content.find_name("ClearChild").expect("ClearChild");

        // ── Section B: attached properties ──────────────────────────────────
        //
        // FINDING: Noesis declares Grid.Row / Grid.Column / RowSpan / ColumnSpan
        // as `uint32_t` (NsGui/Grid.h: `static uint32_t GetRow(...)`). The FFI's
        // Int32 tag validates against `TypeOf<int32_t>()` and so cannot reach
        // them, but the UInt32 tag validates against `TypeOf<uint32_t>()` and
        // does — see the Grid.Row coverage below. We exercise the positive
        // int32 round-trip on `Panel.ZIndex` (declared `int32_t`) and the
        // positive uint32 round-trip on `Grid.Row`, plus the strict negative
        // that the Int32 tag still rejects the uint32 Grid.Row.

        // Panel.ZIndex is an Int32 attached property; XAML set it to 2.
        assert_eq!(
            row_child.get_attached_i32("Panel", "ZIndex"),
            Some(2),
            "Panel.ZIndex should read back the XAML value 2",
        );
        assert!(
            row_child.set_attached_i32("Panel", "ZIndex", 1),
            "set Panel.ZIndex=1 should succeed",
        );
        assert_eq!(
            row_child.get_attached_i32("Panel", "ZIndex"),
            Some(1),
            "Panel.ZIndex should now be 1",
        );

        // Grid.Row is a *uint32* attached property → reachable through the
        // UInt32 tag. XAML set it to 2; round-trip it down to 1.
        assert_eq!(
            row_child.get_attached_u32("Grid", "Row"),
            Some(2),
            "Grid.Row should read back the XAML value 2 via the UInt32 tag",
        );
        assert!(
            row_child.set_attached_u32("Grid", "Row", 1),
            "set Grid.Row=1 via UInt32 tag should succeed",
        );
        assert_eq!(
            row_child.get_attached_u32("Grid", "Row"),
            Some(1),
            "Grid.Row should now be 1",
        );

        // Strict-tag negative: the Int32 tag genuinely mismatches the uint32
        // Grid.Row type, so the i32 path must still reject it cleanly. (Grid.Row
        // is an attached property registered on Grid, not on the child's own
        // class, so there is no plain — non-attached — `property_tag`/
        // `get_dynamic` query for it; that path stays exercised via Margin /
        // Width / IsEnabled in Section D below.)
        assert_eq!(
            row_child.get_attached_i32("Grid", "Row"),
            None,
            "Grid.Row is uint32 in Noesis; the Int32 FFI tag must reject it",
        );

        // Bool attached round-trip on Grid.IsSharedSizeScope (declared `bool`).
        assert_eq!(
            row_child.get_attached_bool("Grid", "IsSharedSizeScope"),
            Some(false),
            "IsSharedSizeScope default is false",
        );
        assert!(
            row_child.set_attached_bool("Grid", "IsSharedSizeScope", true),
            "set Grid.IsSharedSizeScope=true should succeed",
        );
        assert_eq!(
            row_child.get_attached_bool("Grid", "IsSharedSizeScope"),
            Some(true),
            "Grid.IsSharedSizeScope should now be true",
        );

        // Canvas.Left / Canvas.Top are Float attached properties.
        assert_eq!(
            canv_child.get_attached_f32("Canvas", "Left"),
            Some(40.0),
            "Canvas.Left should read back 40",
        );
        assert_eq!(
            canv_child.get_attached_f32("Canvas", "Top"),
            Some(15.0),
            "Canvas.Top should read back 15",
        );
        assert!(
            canv_child.set_attached_f32("Canvas", "Left", 88.0),
            "set Canvas.Left=88 should succeed",
        );
        assert_eq!(
            canv_child.get_attached_f32("Canvas", "Left"),
            Some(88.0),
            "Canvas.Left should now be 88",
        );

        // Negatives: unknown owner type, unknown property, and tag mismatch.
        assert!(
            !row_child.set_attached_i32("NotAType", "ZIndex", 0),
            "unknown owner type should fail",
        );
        assert_eq!(
            row_child.get_attached_i32("NotAType", "ZIndex"),
            None,
            "unknown owner type get should be None",
        );
        assert!(
            !row_child.set_attached_i32("Panel", "Nope", 0),
            "unknown attached property should fail",
        );
        assert_eq!(
            row_child.get_attached_i32("Panel", "Nope"),
            None,
            "unknown attached property get should be None",
        );
        // Tag mismatch: Panel.ZIndex resolves but is Int32, so the Float setter
        // and getter must be rejected (the property is found; only the tag is
        // wrong).
        assert!(
            !row_child.set_attached_f32("Panel", "ZIndex", 1.0),
            "tag mismatch (Panel.ZIndex is Int32, not Float) should fail",
        );
        assert_eq!(
            row_child.get_attached_f32("Panel", "ZIndex"),
            None,
            "tag-mismatched get should be None",
        );

        // ── Section C: clear / current / base value ─────────────────────────

        // Width is a local value (120 from XAML; reassert by setting fresh).
        assert!(clear_child.set_f32("Width", 250.0), "set Width=250");
        assert_eq!(clear_child.get_f32("Width"), Some(250.0), "Width is 250");

        // clear_value reverts Width to its default. In Noesis (as in WPF) the
        // FrameworkElement.Width default is NaN ("Auto"), so the post-clear
        // read is Some(NaN) rather than a finite number — assert that shape.
        assert!(clear_child.clear_value("Width"), "clear_value(Width) ok");
        let after = clear_child.get_f32("Width");
        assert!(
            after.is_some_and(f32::is_nan),
            "Width after clear should be the NaN 'Auto' default, got {after:?}",
        );

        // clear_value on a read-only property and on an unknown property fail.
        assert!(
            !clear_child.clear_value("ActualWidth"),
            "clear_value on read-only ActualWidth should fail",
        );
        assert!(
            !clear_child.clear_value("NotARealProperty"),
            "clear_value on unknown property should fail",
        );

        // SetCurrentValue vs base value. Establish a LOCAL base via set_f32,
        // then override the effective value via set_current_f32. Noesis keeps
        // the local value as the *base* while the effective getter returns the
        // current value.
        assert!(clear_child.set_f32("Width", 200.0), "set base Width=200");
        assert!(
            clear_child.set_current_f32("Width", 123.0),
            "set_current Width=123",
        );
        assert_eq!(
            clear_child.get_f32("Width"),
            Some(123.0),
            "effective Width reflects SetCurrentValue (123)",
        );
        assert_eq!(
            clear_child.get_base_f32("Width"),
            Some(200.0),
            "base Width remains the local value (200), unaffected by SetCurrentValue",
        );

        // String SetCurrentValue on a TextBlock's Text, read back via text().
        assert_eq!(
            clear_child.get_base_string("Text").as_deref(),
            Some("base-text"),
            "base Text is the XAML/local value",
        );
        assert!(
            clear_child.set_current_string("Text", "current-text"),
            "set_current_string(Text)",
        );
        assert_eq!(
            clear_child.text().as_deref(),
            Some("current-text"),
            "text() reflects SetCurrentValue",
        );
        assert_eq!(
            clear_child.get_string("Text").as_deref(),
            Some("current-text"),
            "get_string(Text) agrees with text()",
        );
        // Base Text is unchanged by SetCurrentValue.
        assert_eq!(
            clear_child.get_base_string("Text").as_deref(),
            Some("base-text"),
            "base Text unaffected by SetCurrentValue",
        );

        // Note: there is no get_base_component — component (BaseComponent) tags
        // are not supported by the base-value FFI, so that path is unreachable
        // from Rust by construction; nothing to assert here.

        // ── Section D: dynamic tag inference ────────────────────────────────

        // Width is `float` in Noesis (NOT WPF's double) — the existing
        // dependency_property test round-trips it via get_f32, so the reflected
        // type is float => PropType::Float.
        assert_eq!(
            clear_child.property_tag("Width"),
            Some(PropType::Float),
            "Width tag is Float in Noesis",
        );
        assert_eq!(
            clear_child.property_tag("IsEnabled"),
            Some(PropType::Bool),
            "IsEnabled tag is Bool",
        );
        assert_eq!(
            clear_child.property_tag("Margin"),
            Some(PropType::Thickness),
            "Margin tag is Thickness",
        );
        // Background is a Brush (a BaseComponent, not an ImageSource).
        assert_eq!(
            content.property_tag("Background"),
            Some(PropType::BaseComponent),
            "Background (Brush) tag is BaseComponent",
        );
        assert_eq!(
            clear_child.property_tag("NotARealProperty"),
            None,
            "unknown property has no tag",
        );

        // get_dynamic dispatches via the inferred tag. Width is currently 123
        // (SetCurrentValue effective above).
        match clear_child.get_dynamic("Width") {
            Some(DynValue::F32(v)) => {
                assert_eq!(v, 123.0, "dynamic Width value");
                // Cross-check against the typed getter.
                assert_eq!(clear_child.get_f32("Width"), Some(v));
            }
            other => panic!("expected DynValue::F32 for Width, got {other:?}"),
        }

        // Bool property via dynamic dispatch, cross-checked against get_bool.
        match clear_child.get_dynamic("IsEnabled") {
            Some(DynValue::Bool(v)) => {
                assert_eq!(clear_child.get_bool("IsEnabled"), Some(v));
                assert!(v, "IsEnabled defaults to true");
            }
            other => panic!("expected DynValue::Bool for IsEnabled, got {other:?}"),
        }

        // Thickness via dynamic dispatch, cross-checked against get_thickness.
        match clear_child.get_dynamic("Margin") {
            Some(DynValue::Thickness(m)) => {
                assert_eq!(m, [3.0, 5.0, 7.0, 9.0], "Margin from XAML");
                assert_eq!(clear_child.get_thickness("Margin"), Some(m));
            }
            other => panic!("expected DynValue::Thickness for Margin, got {other:?}"),
        }

        // Unknown property yields no dynamic value.
        assert!(
            clear_child.get_dynamic("NotARealProperty").is_none(),
            "unknown property has no dynamic value",
        );

        // ── ordered teardown ────────────────────────────────────────────────
        drop(clear_child);
        drop(canv_child);
        drop(row_child);
        drop(content);
        view.deactivate();
        drop(view);
        drop(_registered);
    }

    noesis_runtime::shutdown();
}
