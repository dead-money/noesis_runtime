//! TODO §5 — richer typed routed-event arg accessors + `DragDrop` source side.
//!
//! Three groups, all in one process (Noesis inits once per test binary):
//!
//!   * **Focus-changed** — drive real keyboard focus through the headless View
//!     and assert `KeyboardFocusChangedEventArgs::oldFocus` / `newFocus` read
//!     back the elements focus moved between.
//!   * **Drag + manipulation** (`--features test-utils`) — a real
//!     `DragEventArgs` / `Manipulation*EventArgs` is constructed C++-side with
//!     known field values and dispatched exactly as the live pump would; the
//!     production accessors read every typed field back. A stubbed accessor
//!     returning a sentinel would fail these. (Drag/manipulation events can't be
//!     synthesised headlessly — a drag needs an OS drag loop, manipulation needs
//!     a multi-frame touch stream under a live render pass.)
//!   * **`DragDrop` source side + `DataObject` handlers** — `DoDragDrop` crosses
//!     the FFI without crashing; `DataObject.Copying` / `.Pasting` handlers
//!     attach and detach cleanly (a stub returning a null token fails `expect`).

use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::{collections::HashMap, ffi::c_void};

use dm_noesis_runtime::events::{
    DataObjectEvent, DragEffects, EventArgs, RoutedEvent, do_drag_drop, subscribe_data_object,
    subscribe_event,
};
use dm_noesis_runtime::view::{FrameworkElement, View};
use dm_noesis_runtime::xaml_provider::XamlProvider;

const SCENE_XAML: &str = r##"<?xml version="1.0" encoding="utf-8"?>
<StackPanel xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation"
      xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml"
      Background="#FF202020">
  <TextBox x:Name="A" Width="120" Height="30"/>
  <TextBox x:Name="B" Width="120" Height="30"/>
</StackPanel>"##;

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
fn typed_args_focus_drag_manipulation() {
    if let (Ok(name), Ok(key)) = (
        std::env::var("NOESIS_LICENSE_NAME"),
        std::env::var("NOESIS_LICENSE_KEY"),
    ) {
        dm_noesis_runtime::set_license(&name, &key);
    }
    dm_noesis_runtime::init();

    // Last GotKeyboardFocus newFocus / LostKeyboardFocus oldFocus pointers,
    // stored as `usize` (a raw `*mut c_void` is not `Send` so can't ride the
    // capture into a `RoutedEventHandler`). Compared by identity only.
    let got_focus: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
    let lost_focus: Arc<Mutex<Option<usize>>> = Arc::new(Mutex::new(None));
    let focus_fires = Arc::new(AtomicU32::new(0));

    {
        let mut bytes = HashMap::new();
        bytes.insert("scene.xaml".to_string(), SCENE_XAML.as_bytes().to_vec());
        let _registered = dm_noesis_runtime::xaml_provider::set_xaml_provider(InMem { bytes });

        let element =
            FrameworkElement::load("scene.xaml").expect("load_xaml returned None for scene.xaml");

        let mut view = View::create(element);
        view.set_size(200, 200);
        view.activate();

        let content = view.content().expect("View::content returned None");

        // ── Focus-changed: capture old/new focus as focus moves A → B ──
        let gf = Arc::clone(&got_focus);
        let ff = Arc::clone(&focus_fires);
        let got_sub = subscribe_event(
            &content,
            RoutedEvent::GotKeyboardFocus,
            true,
            move |args: &EventArgs| {
                if let Some(p) = args.focus_new_ptr() {
                    *gf.lock().unwrap() = Some(p as usize);
                    ff.fetch_add(1, Ordering::SeqCst);
                }
                false
            },
        )
        .expect("subscribe GotKeyboardFocus returned None");

        let lf = Arc::clone(&lost_focus);
        let lost_sub = subscribe_event(
            &content,
            RoutedEvent::LostKeyboardFocus,
            true,
            move |args: &EventArgs| {
                if let Some(p) = args.focus_old_ptr() {
                    *lf.lock().unwrap() = Some(p as usize);
                }
                false
            },
        )
        .expect("subscribe LostKeyboardFocus returned None");

        assert!(view.update(0.0), "first Update should report change");
        let _ = view.update(0.016);

        let mut a = content.find_name("A").expect("TextBox 'A' not found");
        let mut b = content.find_name("B").expect("TextBox 'B' not found");
        let a_ptr = a.raw();
        let b_ptr = b.raw();

        assert!(a.focus(), "TextBox A should accept focus");
        let _ = view.update(0.032);
        assert!(b.focus(), "TextBox B should accept focus");
        let _ = view.update(0.048);

        // The headless View runs the keyboard-focus machinery, so the focus
        // events genuinely fire — assert the typed old/new focus fields read
        // back the elements focus moved between (a stub returning null fails).
        assert!(
            focus_fires.load(Ordering::SeqCst) > 0,
            "GotKeyboardFocus should fire as focus moves"
        );
        assert_eq!(
            *got_focus.lock().unwrap(),
            Some(b_ptr as usize),
            "last GotKeyboardFocus newFocus should be TextBox B"
        );
        // oldFocus on the A→B transition should be A.
        assert_eq!(
            *lost_focus.lock().unwrap(),
            Some(a_ptr as usize),
            "LostKeyboardFocus oldFocus should be TextBox A"
        );

        // ── DragDrop source side: crosses the FFI without crashing ──
        // No headless completion (no OS drag loop) — we assert the call returns
        // true for live elements (a stub returning false on the null-guard path
        // would still pass here, but the null path is covered by the C guard).
        assert!(
            do_drag_drop(&content, &content, DragEffects::ALL),
            "DoDragDrop on a live element should cross the FFI"
        );

        // ── DataObject copy/paste handlers attach + detach ──
        let copying = subscribe_data_object(
            &content,
            DataObjectEvent::Copying,
            |_data: Option<*mut c_void>, _is_dnd: bool| false,
        )
        .expect("subscribe DataObject.Copying returned None");
        let pasting = subscribe_data_object(
            &content,
            DataObjectEvent::Pasting,
            |_data: Option<*mut c_void>, _is_dnd: bool| false,
        )
        .expect("subscribe DataObject.Pasting returned None");

        // ── Drag + manipulation typed accessors (test-utils raisers) ──
        #[cfg(feature = "test-utils")]
        {
            use dm_noesis_runtime::events::DragKeyStates;
            use dm_noesis_runtime::ffi::{
                RoutedEventFn, dm_noesis_routed_events_test_raise_drag,
                dm_noesis_routed_events_test_raise_manip_completed,
                dm_noesis_routed_events_test_raise_manip_delta,
            };

            // Trampoline that re-exposes the borrowed args to a Rust closure,
            // matching the production `RoutedEventFn` shape. `userdata` is a
            // `*mut &mut dyn FnMut(&EventArgs)`.
            unsafe extern "C" fn raise_tramp(
                userdata: *mut c_void,
                args: *const c_void,
                out_handled: *mut bool,
            ) {
                // SAFETY: userdata points at the trait-object ref we pass below;
                // args is the borrowed live arg handle valid for this call only.
                let f = unsafe { &mut *userdata.cast::<&mut dyn FnMut(&EventArgs)>() };
                let ev = unsafe { EventArgs::from_raw(args) };
                f(&ev);
                if !out_handled.is_null() {
                    unsafe { *out_handled = false };
                }
            }

            // Invoke a C++ raiser synchronously, routing the constructed args
            // into `f`. Borrows (never moves) the surrounding state, so the same
            // `content` can be reused across raisers and at teardown.
            let run_raise =
                |raise: unsafe extern "C" fn(*mut c_void, RoutedEventFn, *mut c_void),
                 mut f: &mut dyn FnMut(&EventArgs)| {
                    let ud = (&mut f) as *mut &mut dyn FnMut(&EventArgs);
                    // SAFETY: raise calls raise_tramp synchronously with ud,
                    // which outlives the call.
                    unsafe { raise(content.raw(), raise_tramp, ud.cast()) };
                };

            // ---- Drag ----
            let mut drag_seen = 0u32;
            run_raise(dm_noesis_routed_events_test_raise_drag, &mut |args| {
                let info = args.drag().expect("drag() should be Some for a drag event");
                assert_eq!(info.effects, DragEffects::COPY, "effects");
                assert_eq!(info.allowed_effects, DragEffects::ALL, "allowedEffects");
                assert_eq!(info.key_states, DragKeyStates::CONTROL_KEY, "keyStates");
                assert_eq!(
                    args.drag_data_ptr(),
                    Some(content.raw()),
                    "drag data should be the element we passed"
                );
                let pos = args
                    .drag_position(&content)
                    .expect("drag_position should be Some");
                assert!(
                    (pos.0 - 12.0).abs() < 0.01 && (pos.1 - 34.0).abs() < 0.01,
                    "drop point {pos:?} should be (12, 34)"
                );
                // Mutating the result effect round-trips through the live args.
                assert!(args.set_drag_effects(DragEffects::MOVE));
                assert_eq!(
                    args.drag().unwrap().effects,
                    DragEffects::MOVE,
                    "set_drag_effects should round-trip"
                );
                drag_seen += 1;
            });
            assert_eq!(drag_seen, 1, "drag handler fired once");

            // ---- Manipulation delta ----
            let mut md_seen = 0u32;
            run_raise(
                dm_noesis_routed_events_test_raise_manip_delta,
                &mut |args| {
                    assert_eq!(args.manip_origin(), Some((100.0, 200.0)), "origin");
                    let d = args.manip_delta().expect("manip_delta Some");
                    assert_eq!(d.translation, (5.0, 7.0), "delta translation");
                    assert_eq!(d.scale, 2.0, "delta scale");
                    assert_eq!(d.rotation, 15.0, "delta rotation");
                    assert_eq!(d.expansion, (3.0, 4.0), "delta expansion");
                    let c = args.manip_cumulative().expect("manip_cumulative Some");
                    assert_eq!(c.translation, (50.0, 70.0), "cumulative translation");
                    assert_eq!(c.scale, 4.0, "cumulative scale");
                    let v = args.manip_velocities().expect("manip_velocities Some");
                    assert_eq!(v.angular, 1.5, "angular velocity");
                    assert_eq!(v.linear, (0.5, 0.6), "linear velocity");
                    assert_eq!(v.expansion, (0.1, 0.2), "expansion velocity");
                    assert_eq!(args.manip_is_inertial(), Some(true), "isInertial");
                    md_seen += 1;
                },
            );
            assert_eq!(md_seen, 1, "manip-delta fired once");

            // ---- Manipulation completed ----
            let mut mc_seen = 0u32;
            run_raise(
                dm_noesis_routed_events_test_raise_manip_completed,
                &mut |args| {
                    assert_eq!(args.manip_origin(), Some((100.0, 200.0)), "origin");
                    let d = args.manip_delta().expect("total Some");
                    assert_eq!(d.translation, (11.0, 13.0), "total translation");
                    assert_eq!(d.scale, 3.0, "total scale");
                    assert_eq!(d.rotation, 45.0, "total rotation");
                    assert_eq!(d.expansion, (1.0, 2.0), "total expansion");
                    // cumulative is delta-only → None on a Completed event.
                    assert!(
                        args.manip_cumulative().is_none(),
                        "completed has no cumulative"
                    );
                    let v = args.manip_velocities().expect("final velocities Some");
                    assert_eq!(v.angular, 2.5, "final angular");
                    assert_eq!(v.linear, (1.5, 1.6), "final linear");
                    assert_eq!(args.manip_is_inertial(), Some(false), "isInertial false");
                    mc_seen += 1;
                },
            );
            assert_eq!(mc_seen, 1, "manip-completed fired once");
        }

        // Ordered teardown.
        drop(copying);
        drop(pasting);
        drop(got_sub);
        drop(lost_sub);
        drop(a);
        drop(b);
        drop(content);
        view.deactivate();
        drop(view);
        drop(_registered);
    }

    dm_noesis_runtime::shutdown();
}
