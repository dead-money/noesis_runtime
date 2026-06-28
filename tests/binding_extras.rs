//! TODO §3 — code-built bindings + Rust value converters.
//!
//! One headless `#[test]` (the "init once per process" rule). It proves the two
//! new surfaces end-to-end against a Rust-backed view model, reading the bound
//! values back through the *target* elements (not the VM) so the assertions
//! catch a stubbed/wrong converter or a binding that never propagated:
//!
//!   * **Converter (Convert).** A `TextBlock.Text` is bound — via a *code-built*
//!     `Binding` carrying a Rust `IValueConverter` — to the VM's `Count` i32 DP.
//!     The converter maps the integer to `"EVEN"`/`"ODD"`; the `TextBlock` must
//!     show the *converted* word, not the raw number, and must re-convert when
//!     `Count` changes.
//!   * **`StringFormat`.** A second `TextBlock.Text` is bound to `Count` with a
//!     code-built `StringFormat` (`"Count is {0}"`) — proving the code path
//!     matches XAML `{Binding Count, StringFormat=...}` authoring.
//!   * **Converter (`ConvertBack`).** A `TextBox.Text` is bound `TwoWay`
//!     (`PropertyChanged`) to `Count` through an invertible i32⇄string converter.
//!     Writing text into the `TextBox` must flow *back* through `ConvertBack` and
//!     land on the VM's `Count` (read via the instance getter).
//!   * **Lifetime.** A converter created and dropped WITHOUT binding runs its
//!     free handler exactly once (a Drop counter). A `set_binding` against an
//!     unknown DP name returns `false`.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use dm_noesis_runtime::binding::{Binding, BindingMode, UpdateSourceTrigger, set_binding};
use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::converters::{ConvertArg, Converted, Converter, ValueConverter};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Width="240" Height="140">
  <StackPanel>
    <TextBlock x:Name="Converted"/>
    <TextBlock x:Name="Formatted"/>
    <TextBox   x:Name="Editor"/>
  </StackPanel>
</Grid>"##;

struct InMem(HashMap<String, Vec<u8>>);
impl XamlProvider for InMem {
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
    fn load_xaml(&mut self, uri: &str) -> Option<&[u8]> {
        self.0.get(uri).map(Vec::as_slice)
    }
}

// VM class registration needs a property-change handler; this one is inert.
struct NoopHandler;
impl PropertyChangeHandler for NoopHandler {
    fn on_changed(&mut self, _i: Instance, _idx: u32, _v: PropertyValue<'_>) {}
}

// i32 -> "EVEN"/"ODD". A deliberately non-identity mapping: if the binding
// delivered the raw source instead of the converted value, the TextBlock would
// read "4"/"5", not the word.
struct Parity;
impl ValueConverter for Parity {
    fn convert(&self, value: &ConvertArg, _p: &ConvertArg) -> Option<Converted> {
        let n = value.as_i32()?;
        Some(Converted::String(
            if n % 2 == 0 { "EVEN" } else { "ODD" }.to_string(),
        ))
    }
}

// Invertible i32 <-> decimal string, for the TwoWay / ConvertBack path.
struct Decimal;
impl ValueConverter for Decimal {
    fn convert(&self, value: &ConvertArg, _p: &ConvertArg) -> Option<Converted> {
        Some(Converted::String(value.as_i32()?.to_string()))
    }
    fn convert_back(&self, value: &ConvertArg, _p: &ConvertArg) -> Option<Converted> {
        value
            .as_str()?
            .trim()
            .parse::<i32>()
            .ok()
            .map(Converted::Int32)
    }
}

// Drop-counting converter for the lifetime assertion.
struct DropProbe(Arc<AtomicU32>);
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl ValueConverter for DropProbe {
    fn convert(&self, _v: &ConvertArg, _p: &ConvertArg) -> Option<Converted> {
        None
    }
}

#[test]
fn code_built_bindings_and_converters() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let drop_count = Arc::new(AtomicU32::new(0));

    {
        // ── Lifetime: a converter built and dropped WITHOUT binding must run its
        // free handler exactly once. ───────────────────────────────────────────
        assert_eq!(drop_count.load(Ordering::SeqCst), 0);
        {
            let probe = Converter::new(DropProbe(Arc::clone(&drop_count)));
            assert!(!probe.raw().is_null());
            assert_eq!(
                drop_count.load(Ordering::SeqCst),
                0,
                "free handler ran before the converter was dropped"
            );
        }
        assert_eq!(
            drop_count.load(Ordering::SeqCst),
            1,
            "unbound converter's free handler must run exactly once on drop"
        );

        // ── View model: one i32 DP, `Count`. ───────────────────────────────────
        let mut builder =
            ClassBuilder::new("Sample.ExtrasVM", ClassBase::ContentControl, NoopHandler);
        let count_idx = builder.add_property("Count", PropType::Int32);
        assert_eq!(count_idx, 0);
        let registration = builder.register().expect("VM class registration failed");
        let vm = registration
            .create_instance()
            .expect("create_instance returned None");
        vm.handle().set_int32(count_idx, 4);

        // ── Wire the scene. ─────────────────────────────────────────────────────
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(240, 140);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        // SAFETY: vm is alive for the rest of this scope; Noesis stores its own ref.
        assert!(
            content.set_data_context(&vm),
            "set_data_context returned false"
        );

        let converted = content.find_name("Converted").expect("find Converted");
        let formatted = content.find_name("Formatted").expect("find Formatted");
        let mut editor = content.find_name("Editor").expect("find Editor");

        // Keep the converters alive for the lifetime of the bindings.
        let parity = Converter::new(Parity);
        let decimal = Converter::new(Decimal);

        // 1. Converted.Text = Parity(Count) — code-built Binding + Converter.
        let b1 = Binding::new("Count").converter(&parity);
        assert!(
            set_binding(&converted, "Text", &b1),
            "set_binding onto Converted.Text failed"
        );

        // 2. Formatted.Text = StringFormat(Count) — code-built Binding + StringFormat.
        let b2 = Binding::new("Count").string_format("Count is {0}");
        assert!(
            set_binding(&formatted, "Text", &b2),
            "set_binding onto Formatted.Text failed"
        );

        // 3. Editor.Text <-> Count, TwoWay through an invertible converter.
        let b3 = Binding::new("Count")
            .mode(BindingMode::TwoWay)
            .update_source_trigger(UpdateSourceTrigger::PropertyChanged)
            .converter(&decimal);
        assert!(
            set_binding(&editor, "Text", &b3),
            "set_binding onto Editor.Text failed"
        );

        // Negative path: an unknown DP name must be rejected.
        let bad = Binding::new("Count");
        assert!(
            !set_binding(&converted, "NoSuchProperty", &bad),
            "set_binding should reject an unknown DP name"
        );

        // First layout settles every binding.
        assert!(view.update(0.0));

        // Convert: the CONVERTED word lands, not the raw "4".
        assert_eq!(
            converted.text().as_deref(),
            Some("EVEN"),
            "converter did not deliver EVEN for Count=4"
        );
        // StringFormat code path.
        assert_eq!(
            formatted.text().as_deref(),
            Some("Count is 4"),
            "code-built StringFormat did not apply"
        );
        // TwoWay convert (source -> target).
        assert_eq!(
            editor.text().as_deref(),
            Some("4"),
            "two-way converter did not project Count onto the TextBox"
        );

        // Drive the source: the converter must re-run on the next update.
        vm.handle().set_int32(count_idx, 5);
        assert!(view.update(0.0));
        assert_eq!(
            converted.text().as_deref(),
            Some("ODD"),
            "converter did not re-run after the source changed"
        );
        assert_eq!(
            editor.text().as_deref(),
            Some("5"),
            "two-way converter did not reproject the changed source"
        );

        // ConvertBack: writing the TextBox text must flow back to the source via
        // ConvertBack (string -> i32). Read the VM's Count to confirm.
        assert!(editor.set_text("42"), "set_text on Editor failed");
        assert!(view.update(0.0));
        assert_eq!(
            vm.handle().get_int32(count_idx),
            Some(42),
            "ConvertBack did not push the edited TextBox value onto the source"
        );

        // ── Teardown (drop every Noesis handle before shutdown). ───────────────
        drop(b1);
        drop(b2);
        drop(b3);
        drop(bad);
        drop(converted);
        drop(formatted);
        drop(editor);
        drop(content);
        view.deactivate();
        drop(view);
        drop(parity);
        drop(decimal);
        drop(vm);
        drop(registration);
        drop(_guard);
    }

    dm_noesis_runtime::shutdown();

    assert_eq!(
        drop_count.load(Ordering::SeqCst),
        1,
        "unbound converter free handler must have fired exactly once overall"
    );
}
