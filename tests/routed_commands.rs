//! TODO §4 — the routed-command system: `RoutedCommand` / `RoutedUICommand`,
//! `CommandBinding` (`Executed` / `CanExecute` Rust handlers attached to an
//! element), and the built-in `ApplicationCommands` / `ComponentCommands`
//! libraries. Complements `tests/commands.rs` (the Rust `ICommand` base).
//!
//! Single headless `#[test]` (init once per process); all owning wrappers drop
//! before `shutdown()`. Routed commands need a laid-out view (they route
//! through the element tree to a matching `CommandBinding`).
//!
//!   `cargo test -p dm_noesis_runtime --test routed_commands -- --nocapture`

use std::collections::HashMap;
use std::ptr::NonNull;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use dm_noesis_runtime::commands::{
    ApplicationCommand, CommandBinding, CommandBindingHandler, CommandParameter, ComponentCommand,
    RoutedCommand, RoutedUICommand,
};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<Grid x:Name="Root"
      xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020" Width="200" Height="200"/>"##;

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

// A CommandBinding handler that counts executions, records whether the last
// invocation carried a non-null parameter, and gates execution on a shared flag.
struct Probe {
    executed: Arc<AtomicU32>,
    saw_param: Arc<AtomicBool>,
    can: Arc<AtomicBool>,
}

impl CommandBindingHandler for Probe {
    fn can_execute(&self, _param: CommandParameter) -> bool {
        self.can.load(Ordering::SeqCst)
    }
    fn execute(&mut self, param: CommandParameter) {
        self.saw_param.store(param.is_some(), Ordering::SeqCst);
        self.executed.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn routed_commands_bindings_and_builtin_libraries() {
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
        view.set_size(200, 200);
        view.activate();
        assert!(view.update(0.0), "layout must run before command routing");
        let root = view.content().expect("View::content returned None");

        // ── RoutedCommand + CommandBinding: Execute routes to the handler ─────
        let cmd = RoutedCommand::new("DoThing", "UIElement").expect("create RoutedCommand");
        assert_eq!(cmd.name().as_deref(), Some("DoThing"), "GetName round-trip");

        // Negative control: with no CommandBinding attached, the command cannot
        // execute (nothing in the tree handles it).
        assert!(
            !cmd.can_execute(None, &root),
            "an unbound routed command must not be executable"
        );

        let executed = Arc::new(AtomicU32::new(0));
        let saw_param = Arc::new(AtomicBool::new(false));
        let can = Arc::new(AtomicBool::new(true));
        let binding = CommandBinding::new(
            &cmd,
            Probe {
                executed: Arc::clone(&executed),
                saw_param: Arc::clone(&saw_param),
                can: Arc::clone(&can),
            },
        )
        .expect("create CommandBinding");
        assert!(binding.attach(&root), "attach to the root UIElement");

        // Now it routes: CanExecute reflects the handler, Execute fires it.
        assert!(
            cmd.can_execute(None, &root),
            "bound command should be executable when the handler allows it"
        );
        cmd.execute(None, &root);
        assert_eq!(
            executed.load(Ordering::SeqCst),
            1,
            "Execute should fire the handler once"
        );
        assert!(
            !saw_param.load(Ordering::SeqCst),
            "no parameter was passed, so the handler should see None"
        );

        // Parameter plumbing: pass a non-null BaseComponent* (the root itself).
        let param = NonNull::new(root.raw());
        cmd.execute(param, &root);
        assert_eq!(
            executed.load(Ordering::SeqCst),
            2,
            "second Execute fires again"
        );
        assert!(
            saw_param.load(Ordering::SeqCst),
            "a non-null parameter should arrive as Some in the handler"
        );

        // CanExecute gating reflects the shared flag, both ways.
        can.store(false, Ordering::SeqCst);
        assert!(
            !cmd.can_execute(None, &root),
            "CanExecute should now report false (handler gates it)"
        );
        can.store(true, Ordering::SeqCst);
        assert!(
            cmd.can_execute(None, &root),
            "flipping the flag back re-enables CanExecute"
        );

        // ── RAII: dropping the binding detaches its handlers ─────────────────
        drop(binding);
        let before = executed.load(Ordering::SeqCst);
        cmd.execute(None, &root);
        assert_eq!(
            executed.load(Ordering::SeqCst),
            before,
            "after the CommandBinding is dropped, Execute must no longer fire the handler"
        );
        assert!(
            !cmd.can_execute(None, &root),
            "after drop, the command is unhandled again"
        );

        // ── RoutedUICommand: Text + Name ─────────────────────────────────────
        let mut uic =
            RoutedUICommand::new("Save", "Save File", "UIElement").expect("create RoutedUICommand");
        assert_eq!(uic.name().as_deref(), Some("Save"));
        assert_eq!(uic.text().as_deref(), Some("Save File"));
        uic.set_text("Saved");
        assert_eq!(uic.text().as_deref(), Some("Saved"), "SetText round-trip");

        // ── Built-in libraries: identity, names, singleton stability ─────────
        let copy = ApplicationCommand::Copy.command();
        assert_eq!(
            copy.name().as_deref(),
            Some("Copy"),
            "ApplicationCommands.Copy name"
        );
        assert!(
            copy.text().is_some_and(|t| !t.is_empty()),
            "Copy has display text"
        );
        assert_eq!(
            ApplicationCommand::Paste.command().name().as_deref(),
            Some("Paste")
        );
        assert_eq!(
            ApplicationCommand::Undo.command().name().as_deref(),
            Some("Undo")
        );
        assert_eq!(
            ApplicationCommand::SelectAll.command().name().as_deref(),
            Some("SelectAll")
        );
        assert_eq!(
            ComponentCommand::MoveDown.command().name().as_deref(),
            Some("MoveDown"),
            "ComponentCommands.MoveDown name"
        );

        // The same library entry resolves to the same framework singleton; two
        // different entries are distinct objects.
        assert_eq!(
            ApplicationCommand::Copy.command().raw(),
            ApplicationCommand::Copy.command().raw(),
            "a built-in command is a stable singleton"
        );
        assert_ne!(
            ApplicationCommand::Copy.command().raw(),
            ApplicationCommand::Cut.command().raw(),
            "distinct built-ins are distinct objects"
        );

        // ── Built-in command through a CommandBinding (integration) ──────────
        let copy_runs = Arc::new(AtomicU32::new(0));
        let cr = Arc::clone(&copy_runs);
        let copy_binding = CommandBinding::new(&copy, move |_param: CommandParameter| {
            cr.fetch_add(1, Ordering::SeqCst);
        })
        .expect("create CommandBinding for a built-in");
        assert!(copy_binding.attach(&root));
        assert!(
            copy.can_execute(None, &root),
            "the built-in Copy should be executable once a binding handles it"
        );
        copy.execute(None, &root);
        assert_eq!(
            copy_runs.load(Ordering::SeqCst),
            1,
            "invoking the built-in Copy should reach the CommandBinding handler"
        );

        // Bad owner type -> None (reflection can't resolve it).
        assert!(
            RoutedCommand::new("X", "NoSuchType_ZZZ").is_none(),
            "an unresolvable owner type should yield None"
        );

        drop(copy_binding);
        view.deactivate();
        drop(view);
    }

    dm_noesis_runtime::shutdown();
}
