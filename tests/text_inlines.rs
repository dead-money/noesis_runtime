//! Code-built `TextBlock` inline content round-trips: Run, `LineBreak`, Span subclasses,
//! Hyperlink, and `InlineUIContainer` via live Noesis objects.

use noesis_runtime::text_inlines::{
    Bold, Hyperlink, Inline, InlineUIContainer, Italic, LineBreak, Run, Span, TextDecorations,
    Underline, text_block_inlines,
};
use noesis_runtime::view::FrameworkElement;

const NS: &str = r#"xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml""#;

#[test]
fn text_inlines_round_trip() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let mut run = Run::new("hello");
        assert_eq!(run.text().as_deref(), Some("hello"), "Run text from ctor");
        assert!(run.set_text("world"), "Run::set_text");
        assert_eq!(
            run.text().as_deref(),
            Some("world"),
            "Run text after set_text"
        );

        let tb_xaml = format!("<TextBlock {NS}/>");
        let tb = FrameworkElement::parse(&tb_xaml).expect("parse TextBlock");
        let mut inlines = text_block_inlines(&tb).expect("TextBlock inlines");
        assert_eq!(inlines.count(), 0, "fresh TextBlock has no inlines");

        let run0 = Run::new("first");
        let i0 = inlines.add(&run0).expect("add run0");
        assert_eq!(i0, 0, "first inline added at index 0");
        assert_eq!(inlines.count(), 1, "count grows after add");

        let br = LineBreak::new();
        inlines.add(&br).expect("add line break");
        assert_eq!(inlines.count(), 2, "count grows for LineBreak");

        // Pointer identity: the inline read back is the one we added.
        assert_eq!(
            inlines.get_raw(0),
            run0.raw(),
            "TextBlock inline[0] is the Run we added"
        );

        let border = FrameworkElement::parse(&format!("<Border {NS}/>")).expect("parse Border");
        assert!(
            text_block_inlines(&border).is_none(),
            "Border is not a TextBlock"
        );

        let span = Span::new();
        let inner_run = Run::new("nested");
        {
            let mut span_inlines = span.inlines().expect("Span inlines");
            assert_eq!(span_inlines.count(), 0, "fresh Span has no inlines");
            span_inlines.add(&inner_run).expect("add inner run");
            assert_eq!(span_inlines.count(), 1, "Span inline count after add");
            assert_eq!(
                span_inlines.get_raw(0),
                inner_run.raw(),
                "Span inline[0] is the nested Run"
            );
        }
        let span_idx = inlines.add(&span).expect("add span to TextBlock");
        assert_eq!(span_idx, 2, "Span is the third top-level inline");
        assert_eq!(inlines.count(), 3, "TextBlock now has three inlines");
        assert_eq!(
            inlines.get_raw(2),
            span.raw(),
            "TextBlock inline[2] is the Span"
        );
        // Re-reading the Span's inlines still shows the nested Run by identity:
        // proves GetInlines returns the live nested collection, not a fresh one.
        let span_inlines2 = span.inlines().expect("Span inlines again");
        assert_eq!(span_inlines2.count(), 1, "Span still has its nested Run");
        assert_eq!(
            span_inlines2.get_raw(0),
            inner_run.raw(),
            "nested Run survives via the live Span collection"
        );

        let bold = Bold::new();
        let italic = Italic::new();
        let underline = Underline::new();
        {
            let mut bi = bold.inlines().expect("Bold inlines");
            let r = Run::new("strong");
            bi.add(&r).expect("add to bold");
            assert_eq!(bi.count(), 1, "Bold hosts its child Run");
            assert_eq!(bi.get_raw(0), r.raw(), "Bold inline[0] identity");
        }
        assert!(italic.inlines().is_some(), "Italic exposes inlines");
        assert!(underline.inlines().is_some(), "Underline exposes inlines");

        let mut link = Hyperlink::new();
        assert_eq!(link.navigate_uri(), None, "fresh Hyperlink has no URI");
        assert!(
            link.set_navigate_uri("https://noesisengine.com/"),
            "Hyperlink::set_navigate_uri"
        );
        assert_eq!(
            link.navigate_uri().as_deref(),
            Some("https://noesisengine.com/"),
            "Hyperlink NavigateUri read back from Noesis"
        );
        {
            let mut li = link.inlines().expect("Hyperlink inlines");
            let label = Run::new("click me");
            li.add(&label).expect("add label to hyperlink");
            assert_eq!(li.count(), 1, "Hyperlink hosts its label Run");
        }

        let deco_run = Run::new("decorated");
        assert_eq!(
            deco_run.text_decorations(),
            Some(TextDecorations::None),
            "default decoration is None"
        );
        assert!(
            deco_run.set_text_decorations(TextDecorations::Strikethrough),
            "set TextDecorations"
        );
        assert_eq!(
            deco_run.text_decorations(),
            Some(TextDecorations::Strikethrough),
            "TextDecorations read back from live Inline"
        );
        assert!(bold.set_text_decorations(TextDecorations::Underline));
        assert_eq!(
            bold.text_decorations(),
            Some(TextDecorations::Underline),
            "Bold TextDecorations round-trip"
        );

        let mut container = InlineUIContainer::new();
        assert!(!container.has_child(), "fresh container has no child");
        let button = FrameworkElement::parse(&format!("<Button {NS} Content=\"Go\"/>"))
            .expect("parse Button");
        assert!(container.set_child(&button), "set InlineUIContainer child");
        assert!(container.has_child(), "container reports a child");
        assert_eq!(
            container.child_raw(),
            button.raw(),
            "InlineUIContainer.Child is the exact Button we set"
        );
        let cidx = inlines.add(&container).expect("add container to TextBlock");
        assert_eq!(cidx, 3, "container is the fourth top-level inline");
        assert_eq!(inlines.count(), 4, "TextBlock now has four inlines");
    }

    noesis_runtime::shutdown();
}
