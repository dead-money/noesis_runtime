//! Code-side element-tree construction: `StackPanel` children, `Grid` row/column
//! definitions, and `Border` child, mutated from Rust and round-tripped by identity.

use noesis_runtime::element_tree::{
    ColumnDefinition, GridLength, GridUnitType, RowDefinition, column_definitions, panel_children,
    row_definitions,
};
use noesis_runtime::view::FrameworkElement;

const NS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

#[test]
fn element_tree_build_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let panel = FrameworkElement::parse(&format!("<StackPanel {NS}/>")).expect("parse panel");
        let mut children = panel_children(&panel).expect("StackPanel children");
        assert_eq!(children.count(), 0, "fresh StackPanel has no children");

        let b0 = FrameworkElement::parse(&format!("<Button {NS} Content=\"a\"/>")).expect("b0");
        let b1 = FrameworkElement::parse(&format!("<Button {NS} Content=\"b\"/>")).expect("b1");
        let b2 = FrameworkElement::parse(&format!("<Button {NS} Content=\"c\"/>")).expect("b2");

        assert_eq!(children.add(&b0), Some(0), "first child at index 0");
        assert_eq!(children.add(&b2), Some(1), "second child at index 1");
        assert_eq!(children.count(), 2, "count grows after add");

        assert!(children.insert(1, &b1), "insert b1 at index 1");
        assert_eq!(children.count(), 3, "count grows after insert");
        assert_eq!(children.get_raw(0), b0.raw(), "child[0] is b0");
        assert_eq!(children.get_raw(1), b1.raw(), "child[1] is the inserted b1");
        assert_eq!(children.get_raw(2), b2.raw(), "child[2] is b2");

        let got = children.get(1).expect("get child 1");
        assert_eq!(got.raw(), b1.raw(), "owning get returns the same object");
        drop(got);

        assert!(children.remove_at(0), "remove child[0]");
        assert_eq!(children.count(), 2, "count shrinks after remove");
        assert_eq!(children.get_raw(0), b1.raw(), "b1 shifted to index 0");

        let children2 = panel_children(&panel).expect("children again");
        assert_eq!(children2.count(), 2, "live collection, not a fresh one");
        assert_eq!(children2.get_raw(0), b1.raw(), "b1 survives via live coll");

        assert!(children.clear(), "clear children");
        assert_eq!(children.count(), 0, "empty after clear");

        let tb = FrameworkElement::parse(&format!("<TextBlock {NS}/>")).expect("parse tb");
        assert!(panel_children(&tb).is_none(), "TextBlock is not a Panel");

        let mut border = FrameworkElement::parse(&format!("<Border {NS}/>")).expect("parse border");
        assert!(
            border.decorator_child().is_none(),
            "fresh Border has no child"
        );
        let inner =
            FrameworkElement::parse(&format!("<Button {NS} Content=\"x\"/>")).expect("inner");
        assert!(border.set_decorator_child(&inner), "set Border child");
        let child = border.decorator_child().expect("Border child after set");
        assert_eq!(child.raw(), inner.raw(), "Border.Child is the exact Button");
        drop(child);
        assert!(border.clear_decorator_child(), "clear Border child");
        assert!(border.decorator_child().is_none(), "Border child cleared");
        assert!(
            !tb.clone_ref().set_decorator_child(&inner),
            "TextBlock is not a Decorator"
        );

        let grid = FrameworkElement::parse(&format!("<Grid {NS}/>")).expect("parse grid");
        let mut rows = row_definitions(&grid).expect("grid rows");
        let mut cols = column_definitions(&grid).expect("grid cols");
        assert_eq!(rows.count(), 0, "fresh Grid has no row defs");
        assert_eq!(cols.count(), 0, "fresh Grid has no column defs");

        let mut r0 = RowDefinition::new();
        // Default height is 1* (per the SDK).
        assert_eq!(
            r0.length(),
            Some(GridLength::star(1.0)),
            "default RowDefinition height is 1*"
        );
        assert!(r0.set_length(GridLength::pixels(42.0)), "set row height px");
        assert_eq!(
            r0.length(),
            Some(GridLength {
                value: 42.0,
                unit: GridUnitType::Pixel
            }),
            "row height reads back as 42px"
        );

        let mut r1 = RowDefinition::new();
        assert!(r1.set_length(GridLength::auto()), "set row height auto");
        assert_eq!(
            r1.length().map(|l| l.unit),
            Some(GridUnitType::Auto),
            "row height reads back as Auto"
        );

        assert_eq!(rows.add(&r0), Some(0), "add r0 at index 0");
        assert_eq!(rows.add(&r1), Some(1), "add r1 at index 1");
        assert_eq!(rows.count(), 2, "two row defs");
        assert_eq!(rows.get_raw(0), r0.raw(), "row[0] is r0 by identity");
        assert_eq!(rows.get_raw(1), r1.raw(), "row[1] is r1 by identity");

        let mut c0 = ColumnDefinition::new();
        assert!(c0.set_length(GridLength::star(2.0)), "set col width 2*");
        assert_eq!(
            c0.length(),
            Some(GridLength::star(2.0)),
            "col width reads back as 2*"
        );
        assert_eq!(cols.add(&c0), Some(0), "add c0");
        assert_eq!(cols.count(), 1, "one column def");
        assert_eq!(cols.get_raw(0), c0.raw(), "col[0] is c0 by identity");

        let rows2 = row_definitions(&grid).expect("rows again");
        assert_eq!(rows2.count(), 2, "live row collection persists");

        assert!(rows.remove_at(0), "remove row[0]");
        assert_eq!(rows.count(), 1, "one row after remove");
        assert_eq!(rows.get_raw(0), r1.raw(), "r1 shifted to row[0]");
        assert!(rows.clear(), "clear rows");
        assert_eq!(rows.count(), 0, "no rows after clear");

        assert!(
            row_definitions(&panel).is_none(),
            "StackPanel has no row defs"
        );
        assert!(
            column_definitions(&panel).is_none(),
            "StackPanel has no col defs"
        );
    }

    noesis_runtime::shutdown();
}
