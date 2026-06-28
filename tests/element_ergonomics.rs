//! Integration test for first-class element-creation ergonomics (Phase 2):
//! build content / panel children / a command assignment from Rust code — no
//! XAML-string content literals and no caller-side `unsafe` — then read every
//! value BACK out of the live Noesis object.
//!
//! Each assertion is written to FAIL against a stub: a `set_content` that didn't
//! reach the `Content` DP, an `add_child` that didn't grow the panel, or a
//! `set_command` that dropped the command pointer would all break a round-trip
//! here. Structure proofs use pointer identity between the object handed in and
//! the borrowed pointer read back.
//!
//! Single `#[test]` per the harness convention (one Noesis init per process);
//! all owning handles drop inside the inner scope before `shutdown()`.

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
        // ── ContentControl.Content: set / read / clear from code ──────────────
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

        // A non-ContentControl element has no Content DP.
        let mut panel =
            FrameworkElement::parse(&format!("<StackPanel {NS}/>")).expect("parse panel");
        assert!(
            !panel.set_content(&inner),
            "StackPanel has no Content dependency property"
        );

        // ── Panel.add_child: typed convenience over Panel.Children ────────────
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

        // A non-Panel element rejects add_child.
        let mut tb = FrameworkElement::parse(&format!("<TextBlock {NS}/>")).expect("parse tb");
        assert!(!tb.add_child(&c0), "TextBlock is not a Panel");

        // ── FrameworkElement::set_command: safe AsCommand → BaseComponent DP ───
        // `Tag` is a BaseComponent-typed DP on every FrameworkElement, so the
        // by-name component-DP path accepts a command (an ICommand that is a
        // BaseComponent at runtime). Pointer identity proves the FFI crossing.
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

        // An unknown DP name is rejected (no panic, just false).
        assert!(
            !tb.set_command("NoSuchProperty", &command),
            "unknown DP name is rejected"
        );

        // A built-in control's `Command` DP is reachable directly: assigning a
        // command to a Button's `Command` from code round-trips by identity.
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
