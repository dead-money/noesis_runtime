//! `FrameworkElement::num_references` (`BaseRefCounted::GetNumReferences`).
//!
//! The absolute refcount is a Noesis internal, so the test pins deltas:
//! a live handle reports `>= 1`; `clone_ref` bumps it `+1`; dropping the clone drops it `-1`.

use noesis_runtime::view::FrameworkElement;

const XAML: &str = r##"<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml">
  <Button x:Name="B" Content="Hi"/>
</Grid>"##;

#[test]
fn num_references_tracks_add_and_release() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        noesis_runtime::set_license(&name, &key);
    }
    noesis_runtime::init();

    {
        let el = FrameworkElement::parse(XAML).expect("parse failed");

        let base = el.num_references();
        assert!(
            base >= 1,
            "a live owned handle must report >= 1 reference (got {base})"
        );

        let clone = el.clone_ref();
        let after_clone = el.num_references();
        assert_eq!(
            after_clone,
            base + 1,
            "clone_ref must bump the count by exactly 1 ({base} -> {after_clone})"
        );
        // The clone observes the same shared count.
        assert_eq!(
            clone.num_references(),
            after_clone,
            "both handles point at the same component → same refcount"
        );

        drop(clone);
        let after_drop = el.num_references();
        assert_eq!(
            after_drop, base,
            "dropping the clone must drop the count by exactly 1 ({after_clone} -> {after_drop})"
        );
    }

    noesis_runtime::shutdown();
}
