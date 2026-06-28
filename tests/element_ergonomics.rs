//! Element-creation ergonomics: set/clear `Content`, add `Panel` children, and assign
//! `Command`s from Rust code, then round-trip every value from the live Noesis object.

use noesis_runtime::commands::Command;
use noesis_runtime::view::FrameworkElement;

const NS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

#[test]
fn element_ergonomics_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut cc = FrameworkElement::parse(&format!("<ContentControl {NS}/>")).expect("parse cc");
        assert!(
            cc.content().is_none(),
            "fresh ContentControl has no content"
        );

        let inner =
            FrameworkElement::parse(&format!("<Button {NS} Content=\"hi\"/>")).expect("inner");
        assert!(cc.set_content(&inner), "set Content from code");
        let got = cc.content().expect("Content after set");
        assert_eq!(got.raw(), inner.raw(), "Content is the exact Button");
        drop(got);

        assert!(cc.clear_content(), "clear Content");
        assert!(cc.content().is_none(), "Content cleared");

        let mut panel =
            FrameworkElement::parse(&format!("<StackPanel {NS}/>")).expect("parse panel");
        assert!(
            !panel.set_content(&inner),
            "StackPanel has no Content dependency property"
        );

        let c0 = FrameworkElement::parse(&format!("<Button {NS} Content=\"a\"/>")).expect("c0");
        let c1 = FrameworkElement::parse(&format!("<Button {NS} Content=\"b\"/>")).expect("c1");
        assert!(panel.add_child(&c0), "add first child");
        assert!(panel.add_child(&c1), "add second child");

        let children =
            noesis_runtime::element_tree::panel_children(&panel).expect("panel children");
        assert_eq!(children.count(), 2, "add_child grew the panel to 2");
        assert_eq!(children.get_raw(0), c0.raw(), "child[0] is c0 by identity");
        assert_eq!(children.get_raw(1), c1.raw(), "child[1] is c1 by identity");
        drop(children);

        let mut tb = FrameworkElement::parse(&format!("<TextBlock {NS}/>")).expect("parse tb");
        assert!(!tb.add_child(&c0), "TextBlock is not a Panel");

        // `Tag` is a BaseComponent-typed DP on every FrameworkElement, so a Command
        // (also a BaseComponent at runtime) can round-trip through it by pointer identity.
        let command = Command::new(|_param| {});
        assert!(
            tb.set_command("Tag", &command),
            "assign command to a BaseComponent DP"
        );
        let stored = tb.get_component("Tag").expect("Tag holds the command");
        assert_eq!(
            stored.as_ptr(),
            command.raw(),
            "Tag holds the exact command object"
        );

        assert!(
            !tb.set_command("NoSuchProperty", &command),
            "unknown DP name is rejected"
        );

        let mut button =
            FrameworkElement::parse(&format!("<Button {NS} Content=\"go\"/>")).expect("button");
        assert!(
            button.set_command("Command", &command),
            "assign command to Button.Command from code"
        );
        let on_button = button.get_component("Command").expect("Button.Command set");
        assert_eq!(
            on_button.as_ptr(),
            command.raw(),
            "Button.Command holds the exact command object"
        );

        drop(command);
    }

    noesis_runtime::shutdown();
}
