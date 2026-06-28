//! TODO §4 — `ICommand` from Rust: XAML `Command="{Binding ...}"` invokes Rust.
//!
//! A single headless `#[test]` (one per file: the "init once per process"
//! rule). It proves the command bridge end-to-end through a real XAML-bound
//! Button click, plus the enable/disable + lifecycle behaviours:
//!
//!   * A `<Button Command="{Binding Go}">` bound (via a Rust view-model
//!     `DataContext`) to a Rust [`Command`]. A synthetic click increments a
//!     shared counter from the Rust `execute` — and the command parameter
//!     (`CommandParameter="42"`, a boxed string) arrives non-null.
//!   * `can_execute=false` + `raise_can_execute_changed` disables the button
//!     (`is_enabled() == Some(false)`) and suppresses `execute` on click.
//!     Flipping it back re-enables and lets the click through again.
//!   * Lifecycle: a command created and dropped WITHOUT ever binding it runs
//!     its free handler exactly once (a Drop counter), and a null vtable can't
//!     even be constructed here (that path is unit-covered by the FFI null
//!     guard — `Command::new` never passes a null vtable).

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use dm_noesis_runtime::classes::{ClassBuilder, Instance, PropertyChangeHandler, PropertyValue};
use dm_noesis_runtime::commands::{Command, CommandHandler, CommandParameter};
use dm_noesis_runtime::ffi::{ClassBase, PropType};
use dm_noesis_runtime::view::{FrameworkElement, MouseButton, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

// A Button bound to the VM's `Go` command, with a constant CommandParameter so
// we can assert the parameter reaches Rust. Centered 100x40 in a 200x200 grid.
const XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200">
  <Button x:Name="GoButton" Content="Go" Width="100" Height="40"
          HorizontalAlignment="Center" VerticalAlignment="Center"
          Command="{Binding Go}" CommandParameter="42"/>
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

// A controllable command: counts executes, records whether a parameter
// arrived, and gates on a shared `enabled` flag.
struct Counting {
    executes: Arc<AtomicU32>,
    saw_param: Arc<AtomicU32>,
    enabled: Arc<AtomicU32>, // 1 = can_execute true, 0 = false
}
impl CommandHandler for Counting {
    fn can_execute(&self, _param: CommandParameter) -> bool {
        self.enabled.load(Ordering::SeqCst) != 0
    }
    fn execute(&mut self, param: CommandParameter) {
        if param.is_some() {
            self.saw_param.fetch_add(1, Ordering::SeqCst);
        }
        self.executes.fetch_add(1, Ordering::SeqCst);
    }
}

// Drop-counting handler for the lifecycle assertion. Increments a shared
// counter exactly when the boxed handler is dropped (i.e. the free trampoline
// ran).
struct DropProbe(Arc<AtomicU32>);
impl Drop for DropProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
impl CommandHandler for DropProbe {
    fn execute(&mut self, _param: CommandParameter) {}
}

#[test]
fn rust_command_drives_button() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    let executes = Arc::new(AtomicU32::new(0));
    let saw_param = Arc::new(AtomicU32::new(0));
    let enabled = Arc::new(AtomicU32::new(1));
    let drop_count = Arc::new(AtomicU32::new(0));

    {
        // ── Lifecycle: a command built and dropped WITHOUT binding must run
        // its free handler exactly once. ──────────────────────────────────
        assert_eq!(drop_count.load(Ordering::SeqCst), 0);
        {
            let probe = Command::new(DropProbe(Arc::clone(&drop_count)));
            // Touch raw() so the command is genuinely live, not optimized away.
            assert!(!probe.raw().is_null());
            assert_eq!(
                drop_count.load(Ordering::SeqCst),
                0,
                "free handler ran before the command was dropped"
            );
        }
        assert_eq!(
            drop_count.load(Ordering::SeqCst),
            1,
            "unbound command's free handler must run exactly once on drop"
        );

        // ── View-model exposing the command as a bindable BaseComponent DP. ─
        let mut builder =
            ClassBuilder::new("Sample.CommandVM", ClassBase::ContentControl, NoopHandler);
        let go_idx = builder.add_property("Go", PropType::BaseComponent);
        assert_eq!(go_idx, 0);
        let registration = builder.register().expect("VM class registration failed");
        let vm = registration
            .create_instance()
            .expect("create_instance returned None");

        // The bound command.
        let command = Command::new(Counting {
            executes: Arc::clone(&executes),
            saw_param: Arc::clone(&saw_param),
            enabled: Arc::clone(&enabled),
        });
        // Hand the command to the VM's `Go` property; the binding resolves
        // `{Binding Go}` against it.
        // SAFETY: command is alive for the rest of this scope; raw() is a live
        // BaseComponent*. The VM stores its own reference.
        unsafe { vm.handle().set_component(go_idx, command.raw()) };

        // ── Wire the scene. ─────────────────────────────────────────────────
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), XAML.as_bytes().to_vec());
        let _guard = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem(bytes));

        let element = FrameworkElement::load("scene.xaml").expect("load_xaml returned None");
        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        let mut content = view.content().expect("View::content returned None");
        // SAFETY: vm is alive for the rest of this scope.
        assert!(
            content.set_data_context(&vm),
            "set_data_context returned false"
        );

        assert!(view.update(0.0), "first update should report change");

        let button = content
            .find_name("GoButton")
            .expect("find_name(GoButton) failed");
        // With can_execute=true, the command-bound button is enabled.
        assert_eq!(
            button.is_enabled(),
            Some(true),
            "button bound to an executable command should be enabled"
        );

        // ── Click #1: command runs, parameter arrives. ──────────────────────
        click(&mut view, 100, 100);
        assert_eq!(
            executes.load(Ordering::SeqCst),
            1,
            "click did not invoke the Rust command's execute"
        );
        assert_eq!(
            saw_param.load(Ordering::SeqCst),
            1,
            "command did not receive the CommandParameter"
        );

        // ── Disable: can_execute=false + raise → button disabled, click is a
        // no-op. ───────────────────────────────────────────────────────────
        enabled.store(0, Ordering::SeqCst);
        command.raise_can_execute_changed();
        let _ = view.update(0.0);
        assert_eq!(
            button.is_enabled(),
            Some(false),
            "raising CanExecuteChanged with can_execute=false must disable the button"
        );

        click(&mut view, 100, 100);
        assert_eq!(
            executes.load(Ordering::SeqCst),
            1,
            "execute ran while the command reported can_execute=false"
        );

        // ── Re-enable: click works again. ───────────────────────────────────
        enabled.store(1, Ordering::SeqCst);
        command.raise_can_execute_changed();
        let _ = view.update(0.0);
        assert_eq!(button.is_enabled(), Some(true), "button should re-enable");

        click(&mut view, 100, 100);
        assert_eq!(
            executes.load(Ordering::SeqCst),
            2,
            "execute should run again after re-enabling"
        );

        // ── Teardown (drop every Noesis handle before shutdown). ───────────
        drop(button);
        drop(content);
        view.deactivate();
        drop(view);
        drop(command);
        drop(vm);
        drop(registration);
        drop(_guard);
    }

    dm_noesis_runtime::shutdown();

    // The bound command's free handler is covered by the no-leak/no-double-free
    // refcount contract; the explicit Drop-counter assertion above proves the
    // free path fires exactly once for the unbound command.
    assert_eq!(drop_count.load(Ordering::SeqCst), 1);
}

// Synthetic click at (x, y): Noesis needs a move at the press point first.
fn click(view: &mut View, x: i32, y: i32) {
    let _ = view.mouse_move(x, y);
    let _ = view.update(0.0);
    let _ = view.mouse_button_down(x, y, MouseButton::Left);
    let _ = view.update(0.0);
    let _ = view.mouse_button_up(x, y, MouseButton::Left);
    let _ = view.update(0.0);
}
